use std::sync::Arc;
use switch_core::{
    application::{ModelDraft, ProviderDraft, WorkspaceService},
    credentials::{MemoryVault, SecretVault},
    domain::{
        capability::{InputKind, InputPath, Support, Verification},
        error::ErrorCode,
        ids::ProviderId,
        model::{HostState, ModelPolicy},
        provider::{AuthKind, Protocol},
    },
    storage::{Repository, SqliteRepository},
};

fn provider_draft() -> ProviderDraft {
    ProviderDraft {
        id: None,
        name: "测试供应商".into(),
        endpoint: "https://example.test/v1".into(),
        protocol: Protocol::Responses,
        auth_kind: AuthKind::ApiKey,
        preset_id: None,
        notes: None,
        enabled: true,
    }
}
fn ready_model(provider_id: &str, upstream_id: &str, display_name: &str) -> ModelDraft {
    let policy = ModelPolicy {
        context_limit: Some(switch_core::domain::tokens::TokenCount::new(128_000).unwrap()),
        output_limit: Some(switch_core::domain::tokens::TokenCount::new(8_192).unwrap()),
        ..Default::default()
    };
    ModelDraft {
        id: None,
        provider_id: provider_id.to_owned(),
        upstream_id: upstream_id.to_owned(),
        catalog_alias: String::new(),
        display_name: display_name.to_owned(),
        policy,
        in_catalog: true,
        display_name_overridden: true,
    }
}

fn setup() -> (WorkspaceService, Arc<MemoryVault>, Arc<SqliteRepository>) {
    let repo = Arc::new(SqliteRepository::in_memory().unwrap());
    let vault = Arc::new(MemoryVault::new());
    (
        WorkspaceService::new(repo.clone(), vault.clone()),
        vault,
        repo,
    )
}

/// 只有一个 Key 时也要能删掉它。
///
/// 回归：过去 `delete_credential` 拒绝删除「当前 Key」，而只有一个 Key 时无从切换，
/// 于是 Key 删不掉、供应商也就永远删不掉——用户被卡死在角落里。
#[test]
fn the_active_key_can_be_deleted_and_clears_the_selection() {
    let (service, vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let key = service
        .add_credential(provider.id.as_str(), "日常", "synthetic-secret".into())
        .unwrap();
    service
        .select_credential(provider.id.as_str(), key.id.as_str())
        .unwrap();

    service.delete_credential(key.id.as_str()).unwrap();

    assert!(service
        .list_credentials(provider.id.as_str())
        .unwrap()
        .is_empty());
    let after = service
        .list_providers()
        .unwrap()
        .into_iter()
        .find(|item| item.id == provider.id)
        .unwrap();
    assert_eq!(
        after.active_credential_id, None,
        "删掉当前 Key 之后不应再指向它"
    );
    assert!(
        vault.load(&key.secret_ref).unwrap().is_none(),
        "安全条目要一并撤销"
    );
}

/// 删除供应商会连同它的 Key 与模型一起删掉。
///
/// 回归：过去有依赖就拒绝删除，叠加上面那个死结，供应商再也删不掉。
#[test]
fn deleting_a_provider_removes_its_keys_and_models() {
    let (service, vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let key = service
        .add_credential(provider.id.as_str(), "日常", "synthetic-secret".into())
        .unwrap();
    service
        .select_credential(provider.id.as_str(), key.id.as_str())
        .unwrap();
    service
        .save_model(
            ready_model(provider.id.as_str(), "vendor/model-x", "模型甲"),
            0,
        )
        .unwrap();

    service.delete_provider(provider.id.as_str()).unwrap();

    assert!(service.list_providers().unwrap().is_empty());
    assert!(service
        .list_credentials(provider.id.as_str())
        .unwrap()
        .is_empty());
    assert!(
        service.list_models().unwrap().is_empty(),
        "它的模型也应一并删除"
    );
    assert!(
        vault.load(&key.secret_ref).unwrap().is_none(),
        "安全条目要一并撤销"
    );
}

#[test]
fn key_rotation_keeps_old_request_secret_and_rejects_stale_edit() {
    let (service, vault, repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let old = service
        .add_credential(provider.id.as_str(), "日常", "synthetic-old-secret".into())
        .unwrap();
    service
        .select_credential(provider.id.as_str(), old.id.as_str())
        .unwrap();
    let new = service
        .replace_credential(old.id.as_str(), "synthetic-new-secret".into(), 1)
        .unwrap();
    assert_ne!(old.secret_ref, new.secret_ref);
    assert_eq!(
        vault.load(&old.secret_ref).unwrap().as_deref(),
        Some("synthetic-old-secret")
    );
    assert_eq!(
        vault.load(&new.secret_ref).unwrap().as_deref(),
        Some("synthetic-new-secret")
    );
    assert_eq!(new.secret_version, 2);
    assert_eq!(
        service
            .replace_credential(old.id.as_str(), "synthetic-stale-secret".into(), 1)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(vault.len(), 2);
    assert_eq!(
        repo.get_provider(&provider.id)
            .unwrap()
            .unwrap()
            .active_credential_id,
        Some(old.id)
    );
    assert!(!serde_json::to_string(&new)
        .unwrap()
        .contains("synthetic-new-secret"));
}

#[test]
fn locked_vault_leaves_no_metadata_and_missing_entry_cannot_be_selected() {
    let (service, vault, repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    vault.set_locked(true);
    let error = service
        .add_credential(provider.id.as_str(), "日常", "synthetic-secret".into())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::KeystoreLocked);
    assert!(service
        .list_credentials(provider.id.as_str())
        .unwrap()
        .is_empty());
    vault.set_locked(false);
    let credential = service
        .add_credential(provider.id.as_str(), "日常", "synthetic-secret".into())
        .unwrap();
    vault.delete(&credential.secret_ref).unwrap();
    assert_eq!(
        service
            .select_credential(provider.id.as_str(), credential.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::CredentialMissing
    );
    assert!(repo
        .get_provider(&provider.id)
        .unwrap()
        .unwrap()
        .active_credential_id
        .is_none());
}

#[test]
fn failed_metadata_commit_removes_only_the_new_vault_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let repo = Arc::new(SqliteRepository::open(&path).unwrap());
    let vault = Arc::new(MemoryVault::new());
    let service = WorkspaceService::new(repo.clone(), vault.clone());
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let credential = service
        .add_credential(provider.id.as_str(), "日常", "synthetic-old".into())
        .unwrap();
    let external = rusqlite::Connection::open(path).unwrap();
    external.execute_batch("CREATE TRIGGER reject_changes BEFORE UPDATE ON credentials BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
    assert!(service
        .replace_credential(credential.id.as_str(), "synthetic-new".into(), 1)
        .is_err());
    assert_eq!(vault.len(), 1);
    assert_eq!(
        vault.load(&credential.secret_ref).unwrap().as_deref(),
        Some("synthetic-old")
    );
    assert_eq!(
        repo.get_credential(&credential.id)
            .unwrap()
            .unwrap()
            .secret_version,
        1
    );
}

#[test]
fn model_save_recomputes_host_capabilities_and_never_claims_loaded() {
    let (service, _, _) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let mut policy = ModelPolicy::default();
    let pdf = policy
        .inputs
        .iter_mut()
        .find(|i| i.kind == InputKind::Pdf)
        .unwrap();
    pdf.upstream = Support::Supported;
    pdf.host = Support::Supported;
    pdf.gateway = Support::Supported;
    pdf.effective_path = InputPath::Native;
    pdf.verification = Verification::Verified;
    let saved = service
        .save_model(
            ModelDraft {
                id: None,
                provider_id: provider.id.to_string(),
                upstream_id: "Vendor/模型-X".into(),
                catalog_alias: String::new(),
                display_name: "我的模型".into(),
                policy,
                in_catalog: true,
                display_name_overridden: true,
            },
            0,
        )
        .unwrap();
    assert_eq!(saved.host_state, HostState::PendingApply);
    assert!(saved.catalog_alias.as_str().starts_with("gs/"));
    let pdf = saved.policy.input(InputKind::Pdf).unwrap();
    assert_eq!(pdf.upstream, Support::Supported);
    assert_eq!(pdf.effective_path, InputPath::Blocked);
    assert_eq!(pdf.verification, Verification::Declared);
    assert!(!saved.policy.catalog_modalities().contains(&"pdf"));
}

#[test]
fn unknown_provider_never_creates_a_vault_entry() {
    let (service, vault, _) = setup();
    assert!(service
        .add_credential(
            ProviderId::new("missing").as_str(),
            "test",
            "synthetic-secret".into()
        )
        .is_err());
    assert!(vault.is_empty());
}

/// MOD-02 的验收点：刷新发现结果不得覆盖用户手工填写的显示名。
#[test]
fn discovery_updates_the_discovered_layer_without_overwriting_user_values() {
    let (service, _vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let saved = service
        .save_model(
            ready_model(provider.id.as_str(), "vendor/model-a", "我自己起的名字"),
            0,
        )
        .unwrap();
    assert!(
        saved.display_name_layer.overridden,
        "用户填写的显示名属于覆盖层"
    );

    let updated = service
        .record_discovery(
            provider.id.as_str(),
            &[
                ("vendor/model-a".to_owned(), "上游给的官方名字".to_owned()),
                ("vendor/model-b".to_owned(), "未保存的模型".to_owned()),
            ],
        )
        .unwrap();

    assert_eq!(updated, 1, "只更新已保存且被发现到的模型");
    let after = service.list_models().unwrap().into_iter().next().unwrap();
    assert_eq!(
        after.display_name, "我自己起的名字",
        "用户值必须保留，发现结果不能覆盖它"
    );
    assert_eq!(
        after.display_name_layer.discovered.as_deref(),
        Some("上游给的官方名字"),
        "发现值要记在发现层，供界面提示“上游叫这个名字”"
    );
}

/// 没有用户覆盖时，发现值应当成为生效值。
#[test]
fn discovery_follows_the_upstream_name_when_the_user_never_renamed_it() {
    let (service, _vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let mut draft = ready_model(provider.id.as_str(), "vendor/model-a", "vendor/model-a");
    draft.display_name_overridden = false;
    service.save_model(draft, 0).unwrap();

    service
        .record_discovery(
            provider.id.as_str(),
            &[("vendor/model-a".to_owned(), "上游给的官方名字".to_owned())],
        )
        .unwrap();

    let after = service.list_models().unwrap().into_iter().next().unwrap();
    assert_eq!(after.display_name, "上游给的官方名字");
}

/// 发现结果里没有的模型，刷新时保持原样。
#[test]
fn discovery_leaves_models_it_did_not_see_untouched() {
    let (service, _vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    service
        .save_model(
            ready_model(provider.id.as_str(), "vendor/removed-model", "已下线模型"),
            0,
        )
        .unwrap();

    let updated = service
        .record_discovery(
            provider.id.as_str(),
            &[("vendor/other".to_owned(), "别的模型".to_owned())],
        )
        .unwrap();

    assert_eq!(updated, 0);
    let after = service.list_models().unwrap().into_iter().next().unwrap();
    assert_eq!(after.display_name, "已下线模型");
    assert!(after.display_name_layer.discovered.is_none());
}

/// 删除模型：纳入目录后必须先移出，不能删掉宿主菜单里挂着的身份。
#[test]
fn deleting_a_model_requires_leaving_the_catalog_first() {
    let (service, _vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let saved = service
        .save_model(ready_model(provider.id.as_str(), "vendor/a", "模型"), 0)
        .unwrap();
    assert!(saved.in_catalog, "默认纳入目录");

    let error = service
        .delete_model(saved.id.as_str(), saved.version)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ValidationFailed);
    assert!(error
        .safe_details
        .iter()
        .any(|detail| detail.contains("先移出目录")));

    // 移出目录后即可删除。
    let mut draft = ready_model(provider.id.as_str(), "vendor/a", "模型");
    draft.id = Some(saved.id.as_str().to_owned());
    draft.in_catalog = false;
    let left = service.save_model(draft, saved.version).unwrap();
    service
        .delete_model(left.id.as_str(), left.version)
        .unwrap();
    assert!(service.list_models().unwrap().is_empty());
}

/// 版本不符时拒绝删除，避免删掉别人刚改过的那一条。
#[test]
fn deleting_a_model_rejects_a_stale_version() {
    let (service, _vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let mut draft = ready_model(provider.id.as_str(), "vendor/a", "模型");
    draft.in_catalog = false;
    let saved = service.save_model(draft, 0).unwrap();

    let error = service
        .delete_model(saved.id.as_str(), saved.version + 1)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(
        service.list_models().unwrap().len(),
        1,
        "拒绝时必须什么都没删"
    );
}

/// 删除 Key：正在使用的不能删；删掉后安全条目必须一起消失。
#[test]
fn deleting_a_credential_revokes_its_secret_and_allows_the_active_one() {
    let (service, vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let active = service
        .add_credential(provider.id.as_str(), "日常", "synthetic-active".into())
        .unwrap();
    let spare = service
        .add_credential(provider.id.as_str(), "备用", "synthetic-spare".into())
        .unwrap();
    service
        .select_credential(provider.id.as_str(), active.id.as_str())
        .unwrap();

    let before = vault.len();
    assert_eq!(before, 2);

    // 当前 Key 也能删：删掉之后供应商没有当前 Key，而不是留下一个走不出去的死结。
    service.delete_credential(active.id.as_str()).unwrap();
    let after_active = service
        .list_providers()
        .unwrap()
        .into_iter()
        .find(|item| item.id == provider.id)
        .unwrap();
    assert_eq!(after_active.active_credential_id, None);
    assert!(
        vault.load(&active.secret_ref).unwrap().is_none(),
        "秘密必须一起撤销"
    );

    service.delete_credential(spare.id.as_str()).unwrap();

    assert!(service
        .list_credentials(provider.id.as_str())
        .unwrap()
        .is_empty());
    assert_eq!(vault.len(), 0, "秘密必须随元数据一起撤销");
    assert!(vault.load(&spare.secret_ref).unwrap().is_none());
}
