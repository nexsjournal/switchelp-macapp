use std::sync::Arc;
use switch_core::{
    application::{ModelDraft, ProviderDraft, WorkspaceService},
    credentials::{MemoryVault, SecretVault},
    domain::{
        capability::{InputKind, InputPath, Support, Verification},
        credential::CredentialStatus,
        error::ErrorCode,
        ids::ProviderId,
        model::{HostState, ModelPolicy},
        provider::{AuthKind, Protocol},
    },
    storage::{Repository, SqliteRepository},
};

/// 建一个干净的 service 与一个已保存的供应商。
fn seeded_provider() -> (WorkspaceService, switch_core::domain::provider::Provider) {
    let repository: Arc<dyn Repository> = Arc::new(SqliteRepository::in_memory().unwrap());
    let service = WorkspaceService::new(repository, Arc::new(MemoryVault::new()));
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    (service, provider)
}

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
        protocol_override: None,
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
                protocol_override: None,
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
        Some("测试供应商/上游给的官方名字"),
        "发现值要记在发现层，供界面提示“上游叫这个名字”；带供应商前缀，与菜单里显示的一致"
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
    assert_eq!(after.display_name, "测试供应商/上游给的官方名字");
}

/// 默认显示名带供应商前缀：Codex 的模型菜单是扁平的一份，跨供应商同名时要能区分。
#[test]
fn default_display_name_carries_the_provider_prefix() {
    let (service, _vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let mut draft = ready_model(provider.id.as_str(), "deepseek-v4.1", "deepseek-v4.1");
    draft.display_name_overridden = false;
    let saved = service.save_model(draft, 0).unwrap();
    assert_eq!(saved.display_name, "测试供应商/deepseek-v4.1");
    assert_eq!(
        saved.display_name_layer.discovered.as_deref(),
        Some("测试供应商/deepseek-v4.1"),
        "发现层要和生效值同一个字符串，否则下一次刷新会把前缀冲掉"
    );

    // 用户显式改名：原样用用户的名字，系统不再替他加前缀。
    let mut renamed = ready_model(provider.id.as_str(), "deepseek-v4.1", "ds4");
    renamed.id = Some(saved.id.as_str().to_owned());
    renamed.display_name_overridden = true;
    let renamed = service.save_model(renamed, saved.version).unwrap();
    assert_eq!(renamed.display_name, "ds4");
}

/// 供应商改名：它旗下模型的显示名前缀跟着改，菜单里不会留着旧名字。
#[test]
fn renaming_a_provider_requalifies_its_model_display_names() {
    let (service, _vault, _repo) = setup();
    let provider = service.save_provider(provider_draft(), 0).unwrap();
    let mut draft = ready_model(provider.id.as_str(), "deepseek-v4.1", "deepseek-v4.1");
    draft.display_name_overridden = false;
    draft.in_catalog = true;
    let saved = service.save_model(draft, 0).unwrap();
    assert_eq!(saved.display_name, "测试供应商/deepseek-v4.1");

    let mut renamed = provider_draft();
    renamed.id = Some(provider.id.as_str().to_owned());
    renamed.name = "qiyuan".into();
    service.save_provider(renamed, provider.version).unwrap();

    let after = service.list_models().unwrap().into_iter().next().unwrap();
    assert_eq!(after.display_name, "qiyuan/deepseek-v4.1");
    assert_eq!(
        after.host_state,
        switch_core::domain::model::HostState::PendingApply,
        "菜单里的名字变了，磁盘上的目录就旧了"
    );
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

/// 多 Key 管理：改名、停用、重新启用。
///
/// 回归：核心与数据库一直都支持同一供应商下多个 Key，但**没有任何途径把 Key 置为停用**——
/// `CredentialStatus::Disabled` 只有测试在写，生产代码里没有 setter。于是
/// 「禁用」这条 P0 要求实际上不存在，界面也没有可点的入口。
mod key_pool {
    use super::*;

    #[test]
    fn a_key_can_be_renamed_and_the_version_advances() {
        let (service, provider) = seeded_provider();
        let key = service
            .add_credential(
                provider.id.as_str(),
                "日常",
                "synthetic-secret-0000000001".to_string(),
            )
            .unwrap();

        let renamed = service
            .rename_credential(key.id.as_str(), "  备用通道  ", key.version)
            .unwrap();
        assert_eq!(renamed.label, "备用通道", "备注名要去掉首尾空白");
        assert_eq!(
            renamed.version,
            key.version + 1,
            "版本必须推进，否则并发改写会被放行"
        );

        // 拿旧版本再改一次必须被拒——这是「重读再写」的依据。
        assert_eq!(
            service
                .rename_credential(key.id.as_str(), "另一个名字", key.version)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );

        // 空名字与超长名字都要拒绝。
        let current = service
            .list_credentials(provider.id.as_str())
            .unwrap()
            .remove(0);
        assert!(service
            .rename_credential(key.id.as_str(), "   ", current.version)
            .is_err());
    }

    #[test]
    fn a_key_can_be_disabled_and_enabled_again() {
        let (service, provider) = seeded_provider();
        let first = service
            .add_credential(
                provider.id.as_str(),
                "日常",
                "synthetic-secret-0000000002".to_string(),
            )
            .unwrap();
        let second = service
            .add_credential(
                provider.id.as_str(),
                "备用",
                "synthetic-secret-0000000003".to_string(),
            )
            .unwrap();
        service
            .select_credential(provider.id.as_str(), first.id.as_str())
            .unwrap();

        // 停用**非当前**的 Key：允许。
        let off = service
            .set_credential_disabled(second.id.as_str(), true, second.version)
            .unwrap();
        assert_eq!(off.status, CredentialStatus::Disabled);
        assert!(!off.status.is_selectable());

        // 停用的 Key 不能被设为当前。
        assert!(service
            .select_credential(provider.id.as_str(), second.id.as_str())
            .is_err());

        // 重新启用回到「已保存、未检测」，不把停用前的验证结果带回来。
        let on = service
            .set_credential_disabled(second.id.as_str(), false, off.version)
            .unwrap();
        assert_eq!(on.status, CredentialStatus::Saved);
    }

    /// 停用当前 Key 必须被拒：路由已经指向它，停用之后每次新请求都会失败，
    /// 而界面上看起来只是「关掉了一个开关」。
    #[test]
    fn the_active_key_cannot_be_disabled() {
        let (service, provider) = seeded_provider();
        let key = service
            .add_credential(
                provider.id.as_str(),
                "日常",
                "synthetic-secret-0000000004".to_string(),
            )
            .unwrap();
        service
            .select_credential(provider.id.as_str(), key.id.as_str())
            .unwrap();
        let current = service
            .list_credentials(provider.id.as_str())
            .unwrap()
            .remove(0);

        let error = service
            .set_credential_disabled(key.id.as_str(), true, current.version)
            .unwrap_err();
        assert_eq!(error.message_key, "error.activeCredentialCannotBeDisabled");
        assert_eq!(error.code, ErrorCode::Conflict);
    }
}

/// 换当前 Key 必须让「待应用」重新出现。
///
/// 网关服务的是发布时冻结的路由快照——里面写死了 credential_id 与凭据版本。
/// 只改选中项、不重新发布，新请求仍然走旧 Key。以前这件事完全不可见：
/// 换完 Key 页面上没有任何提示，用户以为已经生效了。
#[test]
fn switching_the_active_key_puts_the_models_back_to_pending() {
    let (service, provider) = seeded_provider();
    let first = service
        .add_credential(
            provider.id.as_str(),
            "日常",
            "synthetic-secret-switch-1".to_string(),
        )
        .unwrap();
    let second = service
        .add_credential(
            provider.id.as_str(),
            "备用",
            "synthetic-secret-switch-2".to_string(),
        )
        .unwrap();
    service
        .select_credential(provider.id.as_str(), first.id.as_str())
        .unwrap();
    let model = service
        .save_model(ready_model(provider.id.as_str(), "vendor/a", "模型甲"), 0)
        .unwrap();
    // 应用一次，让它落到「已加载」。
    let loaded = {
        let mut next = model.clone();
        next.host_state = HostState::Loaded;
        service.repository.save_model(next, model.version).unwrap()
    };
    assert_eq!(loaded.host_state, HostState::Loaded);

    service
        .select_credential(provider.id.as_str(), second.id.as_str())
        .unwrap();

    let after = service.list_models().unwrap().remove(0);
    assert_eq!(
        after.host_state,
        HostState::PendingApply,
        "换 Key 之后模型必须回到待应用，否则界面完全看不出「还没生效」"
    );
}

/// 把一个已发布的模型落到「已加载」，返回写回后的行（版本已前移）。
fn mark_loaded(
    service: &WorkspaceService,
    model: &switch_core::domain::model::Model,
) -> switch_core::domain::model::Model {
    let mut next = model.clone();
    next.host_state = HostState::Loaded;
    service.repository.save_model(next, model.version).unwrap()
}

/// 改模型的协议覆盖必须回到待应用。
///
/// `protocol_override` 决定路由快照里的 `protocol_id`，也就是用哪个适配器：不重新发布
/// 就等于没改。用户的话是「配置更新了但没同步，还得自己重启 Codex」——看得见的那部分是
/// 界面：只要模型回到待应用，网关页顶部那条就会自己出现。
#[test]
fn changing_the_protocol_override_puts_the_model_back_to_pending() {
    let (service, provider) = seeded_provider();
    let model = service
        .save_model(ready_model(provider.id.as_str(), "vendor/a", "模型甲"), 0)
        .unwrap();
    let model = mark_loaded(&service, &model);

    let mut switched = ready_model(provider.id.as_str(), "vendor/a", "模型甲");
    switched.id = Some(model.id.as_str().to_owned());
    switched.catalog_alias = model.catalog_alias.as_str().to_owned();
    switched.protocol_override = Some(Protocol::ChatCompletions);
    let after = service.save_model(switched, model.version).unwrap();

    assert_eq!(
        after.host_state,
        HostState::PendingApply,
        "换适配器不重新发布就等于没换"
    );
}

/// 什么都没改就保存（打开编辑器直接点保存）不该把「已加载」打回待应用。
///
/// 反过来做的话，用户每次翻一遍模型都会被要求再应用一次，那条待应用条就成了噪音。
#[test]
fn saving_a_model_without_changes_leaves_the_host_state_alone() {
    let (service, provider) = seeded_provider();
    let model = service
        .save_model(ready_model(provider.id.as_str(), "vendor/a", "模型甲"), 0)
        .unwrap();
    let model = mark_loaded(&service, &model);

    let mut same = ready_model(provider.id.as_str(), "vendor/a", "模型甲");
    same.id = Some(model.id.as_str().to_owned());
    same.catalog_alias = model.catalog_alias.as_str().to_owned();
    let after = service.save_model(same, model.version).unwrap();

    assert_eq!(after.host_state, HostState::Loaded);
}

/// 改供应商的接口协议：它旗下已纳入目录的模型必须回到待应用。
///
/// 协议决定 `protocol_id`，而它冻结在路由快照里；以前改完协议页面上一句提示都没有，
/// 用户以为已经生效了，实际新请求还在走旧适配器。
#[test]
fn switching_a_providers_protocol_puts_its_models_back_to_pending() {
    let (service, provider) = seeded_provider();
    let model = service
        .save_model(ready_model(provider.id.as_str(), "vendor/a", "模型甲"), 0)
        .unwrap();
    mark_loaded(&service, &model);

    let mut switched = provider_draft();
    switched.id = Some(provider.id.as_str().to_owned());
    switched.protocol = Protocol::ChatCompletions;
    service.save_provider(switched, provider.version).unwrap();

    let after = service.list_models().unwrap().remove(0);
    assert_eq!(
        after.host_state,
        HostState::PendingApply,
        "换协议之后模型必须回到待应用"
    );
}

/// 只改备注这种与路由无关的字段，不该要求重新应用。
#[test]
fn editing_a_providers_notes_does_not_ask_for_a_reapply() {
    let (service, provider) = seeded_provider();
    let model = service
        .save_model(ready_model(provider.id.as_str(), "vendor/a", "模型甲"), 0)
        .unwrap();
    mark_loaded(&service, &model);

    let mut annotated = provider_draft();
    annotated.id = Some(provider.id.as_str().to_owned());
    annotated.notes = Some("备用账号".into());
    service.save_provider(annotated, provider.version).unwrap();

    let after = service.list_models().unwrap().remove(0);
    assert_eq!(after.host_state, HostState::Loaded);
}
