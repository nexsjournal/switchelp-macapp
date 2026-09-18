use std::sync::{Arc, Barrier};
use switch_core::{
    domain::{
        credential::Credential,
        error::ErrorCode,
        ids::{CatalogAlias, CredentialId, ModelId, ProviderId},
        model::Model,
        provider::{AuthKind, Protocol, Provider},
    },
    storage::{Repository, SqliteRepository},
};

fn provider(id: &str) -> Provider {
    Provider::draft(
        ProviderId::new(id),
        "测试供应商",
        "https://example.test/v1",
        Protocol::Responses,
        AuthKind::ApiKey,
        "2026-09-18T00:00:00Z",
    )
    .unwrap()
}
fn model(id: &str, upstream: &str, alias: &str) -> Model {
    Model::draft(
        ModelId::new(id),
        ProviderId::new("p"),
        upstream,
        "测试模型",
        CatalogAlias::parse(alias).unwrap(),
        "2026-09-18T00:00:00Z",
    )
    .unwrap()
}
fn credential(id: &str, provider: &str) -> Credential {
    Credential::create(
        CredentialId::new(id),
        ProviderId::new(provider),
        "测试 Key",
        "synthetic-secret-never-in-sqlite",
        "2026-09-18T00:00:00Z",
    )
    .unwrap()
}

#[test]
fn persists_entities_across_reopen_without_secret_material() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metadata.sqlite");
    {
        let repo = SqliteRepository::open(&path).unwrap();
        repo.save_provider(provider("p"), 0).unwrap();
        repo.save_credential(credential("c", "p")).unwrap();
        repo.save_model(model("m", "Vendor/模型-X", "gs/p/m"), 0)
            .unwrap();
    }
    let repo = SqliteRepository::open(&path).unwrap();
    assert_eq!(repo.list_providers().unwrap().len(), 1);
    assert_eq!(repo.list_models().unwrap()[0].upstream_id, "Vendor/模型-X");
    assert_eq!(
        repo.list_credentials(&ProviderId::new("p")).unwrap().len(),
        1
    );
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-secret-never-in-sqlite"));
    }
}

#[test]
fn concurrent_connections_cannot_overwrite_a_winning_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metadata.sqlite");
    let repo = SqliteRepository::open(&path).unwrap();
    let saved = repo.save_provider(provider("p"), 0).unwrap();
    let a = SqliteRepository::open(&path).unwrap();
    let b = SqliteRepository::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = [a, b]
        .into_iter()
        .enumerate()
        .map(|(index, repository)| {
            let barrier = barrier.clone();
            let mut draft = saved.clone();
            std::thread::spawn(move || {
                draft.name = format!("并发编辑 {index}");
                barrier.wait();
                repository.save_provider(draft, 1)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| r.as_ref().is_err_and(|e| e.code == ErrorCode::Conflict))
            .count(),
        1
    );
    assert_eq!(repo.list_providers().unwrap()[0].version, 2);
}

#[test]
fn model_identity_uses_inherited_protocol_and_is_case_sensitive() {
    let repo = SqliteRepository::in_memory().unwrap();
    repo.save_provider(provider("p"), 0).unwrap();
    repo.save_model(model("a", "Vendor/X", "gs/p/a"), 0)
        .unwrap();
    let mut duplicate = model("b", "Vendor/X", "gs/p/b");
    duplicate.protocol_override = Some(Protocol::Responses);
    assert!(repo.save_model(duplicate, 0).is_err());
    assert!(repo.save_model(model("c", "vendor/x", "gs/p/c"), 0).is_ok());
    assert!(repo.save_model(model("d", "other", "gs/p/a"), 0).is_err());
}

#[test]
fn provider_protocol_conflict_rolls_back_provider_and_model_index() {
    let repo = SqliteRepository::in_memory().unwrap();
    let mut p = repo.save_provider(provider("p"), 0).unwrap();
    repo.save_model(model("a", "x", "gs/p/a"), 0).unwrap();
    let mut chat = model("b", "x", "gs/p/b");
    chat.protocol_override = Some(Protocol::ChatCompletions);
    repo.save_model(chat, 0).unwrap();
    p.protocol = Protocol::ChatCompletions;
    assert!(repo.save_provider(p, 1).is_err());
    let unchanged = repo.get_provider(&ProviderId::new("p")).unwrap().unwrap();
    assert_eq!(unchanged.version, 1);
    assert_eq!(unchanged.protocol, Protocol::Responses);
}

#[test]
fn renamed_alias_is_released_and_parent_references_are_protected() {
    let repo = SqliteRepository::in_memory().unwrap();
    assert!(repo
        .save_model(model("missing", "x", "gs/p/missing"), 0)
        .is_err());
    repo.save_provider(provider("p"), 0).unwrap();
    let mut m = repo.save_model(model("a", "x", "gs/p/a"), 0).unwrap();
    m.catalog_alias = CatalogAlias::parse("gs/p/new").unwrap();
    repo.save_model(m, 1).unwrap();
    repo.save_model(model("b", "y", "gs/p/a"), 0).unwrap();
    assert!(repo.delete_provider(&ProviderId::new("p")).is_err());
    assert_eq!(repo.list_models().unwrap().len(), 2);
}

#[test]
fn keys_cannot_cross_providers_or_be_deleted_while_selected() {
    let repo = SqliteRepository::in_memory().unwrap();
    let mut p = repo.save_provider(provider("p"), 0).unwrap();
    repo.save_provider(provider("other"), 0).unwrap();
    repo.save_credential(credential("c", "other")).unwrap();
    p.active_credential_id = Some(CredentialId::new("c"));
    assert!(repo.save_provider(p.clone(), 1).is_err());
    repo.save_credential(credential("mine", "p")).unwrap();
    p.active_credential_id = Some(CredentialId::new("mine"));
    repo.save_provider(p, 1).unwrap();
    assert!(repo.delete_credential(&CredentialId::new("mine")).is_err());
    let mut current = repo
        .get_credential(&CredentialId::new("mine"))
        .unwrap()
        .unwrap();
    current
        .replace_secret("new-synthetic-secret", "now")
        .unwrap();
    repo.save_credential(current.clone()).unwrap();
    assert!(repo.save_credential(current).is_err());
}

#[test]
fn future_schema_is_refused_and_left_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.sqlite");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    assert!(SqliteRepository::open(&path).is_err());
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 999);
}
