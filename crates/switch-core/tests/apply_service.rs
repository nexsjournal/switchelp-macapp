//! 应用编排集成测试：计划 → CAS → 原子提交 → 等待重载 → 恢复。
//!
//! 覆盖的关键不变量（来自 [配置生命周期](../../docs/architecture/02-configuration-lifecycle.md)）：
//! - 计划阶段绝不修改 Codex 配置。
//! - 提交前必须 CAS；外部修改后拒绝写入并转入冲突。
//! - 提交成功最多到“等待重载”，绝不自行宣称宿主已加载。
//! - 恢复只撤销本工具写入且未被外部修改的字段。
//! - 上游 Key 永不写入 config.toml。

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use switch_core::{
    application::{
        ApplyService, Clock, GatewayLayout, ModelDraft, ProviderDraft, WorkspaceService,
    },
    codex::{
        config,
        detect::{CodexInstance, StartupMode},
        plan::{ApplyStage, RecoveryDecision},
    },
    credentials::MemoryVault,
    domain::{
        error::ErrorCode,
        ids::InstanceId,
        model::{HostState, ModelPolicy},
        provider::{AuthKind, Protocol},
        tokens::TokenCount,
        version::{CompatibilityStatus, VersionFingerprint},
    },
    gateway::{self, GatewayRouter},
    storage::{MemoryOperationStore, OperationStore, Repository, SqliteRepository},
};

const SYNTHETIC_SECRET: &str = "synthetic-upstream-secret-0123456789";

/// 可注入时钟：测试可把时间调到计划过期之后。
struct TestClock {
    now: Mutex<i64>,
}

impl TestClock {
    fn new(now: i64) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    fn advance_to(&self, now: i64) {
        *self.now.lock().expect("锁未被污染") = now;
    }
}

impl Clock for TestClock {
    fn now_unix(&self) -> i64 {
        *self.now.lock().expect("锁未被污染")
    }

    fn now(&self) -> String {
        "2026-09-18T00:00:00Z".to_owned()
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    instance: CodexInstance,
    config_path: PathBuf,
    service: ApplyService,
    workspace: WorkspaceService,
    store: Arc<MemoryOperationStore>,
    clock: Arc<TestClock>,
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

/// 声明了上下文的可应用模型；未声明上下文的模型由 `unready_model` 单独构造。
fn ready_model(provider_id: &str) -> ModelDraft {
    let policy = ModelPolicy {
        context_limit: Some(TokenCount::new(128_000).unwrap()),
        output_limit: Some(TokenCount::new(8_192).unwrap()),
        ..Default::default()
    };
    ModelDraft {
        id: None,
        provider_id: provider_id.to_owned(),
        upstream_id: "Vendor/模型-X".into(),
        catalog_alias: String::new(),
        display_name: "我的模型".into(),
        policy,
        in_catalog: true,
        display_name_overridden: true,
    }
}

fn harness(existing_config: Option<&str>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    if let Some(text) = existing_config {
        std::fs::write(&config_path, text).unwrap();
    }
    let db = dir.path().join("metadata.sqlite");
    let repository: Arc<dyn Repository> = Arc::new(SqliteRepository::open(&db).unwrap());
    let vault = Arc::new(MemoryVault::new());
    let workspace = WorkspaceService::new(repository.clone(), vault.clone());
    let store = Arc::new(MemoryOperationStore::new());
    let clock = Arc::new(TestClock::new(1_700_000_000));
    let service = ApplyService::new(
        repository,
        store.clone(),
        Arc::new(GatewayRouter::new()),
        GatewayLayout {
            app_data_dir: dir.path().join("app-data"),
            port: gateway::DEFAULT_PORT,
            auth_helper: "/tmp/gptswitch-auth".to_owned(),
            base_instructions: "测试用基础指令".to_owned(),
        },
        clock.clone(),
    );
    let instance = CodexInstance {
        id: InstanceId::new("inst_test"),
        app_path: None,
        cli_path: None,
        desktop_version: None,
        cli_version: None,
        config_root: dir.path().display().to_string(),
        config_file: config_path.display().to_string(),
        config_exists: config_path.exists(),
        startup_mode: StartupMode::NotRunning,
        compatibility: CompatibilityStatus::Unverified,
        fingerprint: VersionFingerprint::unknown(),
        conflicting_managers: Vec::new(),
        blocked_reason_key: None,
    };
    Harness {
        _dir: dir,
        instance,
        config_path,
        service,
        workspace,
        store,
        clock,
    }
}

impl Harness {
    /// 建立可应用的完整数据：供应商 + 已选 Key + 已声明上下文的模型。
    fn with_ready_model(existing_config: Option<&str>) -> Self {
        let harness = harness(existing_config);
        let provider = harness
            .workspace
            .save_provider(provider_draft(), 0)
            .unwrap();
        let credential = harness
            .workspace
            .add_credential(provider.id.as_str(), "日常", SYNTHETIC_SECRET.into())
            .unwrap();
        harness
            .workspace
            .select_credential(provider.id.as_str(), credential.id.as_str())
            .unwrap();
        harness
            .workspace
            .save_model(ready_model(provider.id.as_str()), 0)
            .unwrap();
        harness
    }

    fn read_config(&self) -> String {
        std::fs::read_to_string(&self.config_path).unwrap()
    }

    fn write_config(&self, text: &str) {
        std::fs::write(&self.config_path, text).unwrap();
    }

    fn operation_id(&self) -> String {
        self.store
            .unfinished()
            .unwrap()
            .last()
            .expect("应有未完成事务")
            .operation
            .id
            .as_str()
            .to_owned()
    }

    fn stage(&self, operation_id: &str) -> ApplyStage {
        self.service.status(operation_id).unwrap().operation.stage
    }

    fn host_states(&self) -> Vec<HostState> {
        self.workspace
            .list_models()
            .unwrap()
            .into_iter()
            .map(|model| model.host_state)
            .collect()
    }
}

/// 换过 Key 之后再应用一次。
///
/// 目录内容没变（版本号相同），但路由快照里的凭据版本变了；而 `router.publish` 的不可变
/// 守卫是按「目录版本」比对的：同一版本、内容不同即拒绝发布。结果就是「换了 Key 再点应用」
/// 静默失败——事务停在 prepared，Codex 配置一行都没改，界面只说闪了一下。
#[test]
fn applying_again_after_rotating_the_key_succeeds() {
    let harness = Harness::with_ready_model(None);

    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let first = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-first")
        .unwrap();
    assert_eq!(harness.stage(&first), ApplyStage::AwaitingReload);

    // 换一个 Key：只动凭据，模型与目录内容都不变。
    let provider = harness.workspace.list_providers().unwrap().remove(0);
    let credential = harness
        .workspace
        .list_credentials(provider.id.as_str())
        .unwrap()
        .remove(0);
    harness
        .workspace
        .replace_credential(
            credential.id.as_str(),
            "synthetic-rotated-secret-0123456789".into(),
            credential.version,
        )
        .unwrap();

    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let second = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-second");
    assert!(
        second.is_ok(),
        "换 Key 后再应用必须成功：{:?}",
        second.err()
    );
    assert_eq!(harness.stage(&second.unwrap()), ApplyStage::AwaitingReload);
}

#[test]
fn plan_stage_never_touches_codex_config_but_writes_the_catalog() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();

    assert!(
        !harness.config_path.exists(),
        "计划阶段不得创建或改写 Codex 配置"
    );
    assert!(!plan.changes.is_empty());
    assert!(!plan.catalog_aliases.is_empty());
    assert!(harness
        .service
        .layout()
        .catalog_path(&plan.catalog_revision)
        .exists());
    assert_eq!(harness.stage(&harness.operation_id()), ApplyStage::Prepared);
}

impl Harness {
    /// 只建立供应商并选中可用 Key，返回 provider id；模型由调用方单独保存。
    fn seed_provider_and_key(&self) -> String {
        let provider = self.workspace.save_provider(provider_draft(), 0).unwrap();
        let credential = self
            .workspace
            .add_credential(provider.id.as_str(), "日常", SYNTHETIC_SECRET.into())
            .unwrap();
        self.workspace
            .select_credential(provider.id.as_str(), credential.id.as_str())
            .unwrap();
        provider.id.as_str().to_owned()
    }
}

#[test]
fn plan_refuses_models_without_declared_context() {
    let harness = harness(None);
    let provider_id = harness.seed_provider_and_key();
    // 未声明 context：应用阶段不得用保守值伪装真实上限。
    harness
        .workspace
        .save_model(
            ModelDraft {
                policy: ModelPolicy::default(),
                ..ready_model(&provider_id)
            },
            0,
        )
        .unwrap();

    let error = harness
        .service
        .plan_apply(&harness.instance, None)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ValidationFailed);
    assert_eq!(error.message_key, "error.contextRequired");
    assert!(!harness.config_path.exists(), "计划被拒绝后不得写入配置");
}

#[test]
fn plan_refuses_provider_without_a_selected_key() {
    let harness = harness(None);
    let provider = harness
        .workspace
        .save_provider(provider_draft(), 0)
        .unwrap();
    let credential = harness
        .workspace
        .add_credential(provider.id.as_str(), "日常", SYNTHETIC_SECRET.into())
        .unwrap();
    assert!(credential.secret_ref.starts_with("gptswitch/"));
    // 故意不调用 select_credential。
    harness
        .workspace
        .save_model(ready_model(provider.id.as_str()), 0)
        .unwrap();

    let error = harness
        .service
        .plan_apply(&harness.instance, None)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ValidationFailed);
    assert!(error
        .safe_details
        .iter()
        .any(|detail| detail.contains("尚未选择 API Key")));
}

#[test]
fn plan_refuses_a_disabled_provider() {
    let harness = harness(None);
    let mut draft = provider_draft();
    draft.enabled = false;
    let provider = harness.workspace.save_provider(draft, 0).unwrap();
    let credential = harness
        .workspace
        .add_credential(provider.id.as_str(), "日常", SYNTHETIC_SECRET.into())
        .unwrap();
    harness
        .workspace
        .select_credential(provider.id.as_str(), credential.id.as_str())
        .unwrap();
    harness
        .workspace
        .save_model(ready_model(provider.id.as_str()), 0)
        .unwrap();

    let error = harness
        .service
        .plan_apply(&harness.instance, None)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ValidationFailed);
    assert!(error
        .safe_details
        .iter()
        .any(|detail| detail.contains("已停用")));
}

#[test]
fn external_config_change_blocks_the_commit_and_keeps_the_foreign_content() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness.operation_id();

    harness.write_config("# 外部程序写入的内容\n");
    let error = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-conflict")
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::ConfigChanged);
    assert_eq!(error.message_key, "error.configChanged");
    assert_eq!(harness.stage(&operation_id), ApplyStage::Conflict);
    assert_eq!(
        harness.read_config(),
        "# 外部程序写入的内容\n",
        "冲突后不得覆盖外部修改"
    );
}

#[test]
fn commit_awaits_host_reload_and_never_writes_upstream_secrets() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();

    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-ok")
        .unwrap();

    assert_eq!(harness.stage(&operation_id), ApplyStage::AwaitingReload);
    assert_eq!(
        harness.host_states(),
        vec![HostState::AwaitingReload],
        "提交成功最多到“等待重载”，不得自称已加载"
    );

    let text = harness.read_config();
    assert!(
        text.contains("[model_providers.gptswitch]"),
        "必须写入本机网关 provider"
    );
    assert!(
        text.contains("model_catalog_json"),
        "必须指向编译后的目录文件"
    );
    assert!(text.contains("base_url"), "必须指向本机网关地址");
    assert!(
        !text.contains(SYNTHETIC_SECRET),
        "上游 Key 绝不能写入 Codex 配置"
    );
    assert!(
        text.contains(&format!(
            "http://127.0.0.1:18765/i/{}/",
            harness.instance.id.as_str()
        )),
        "base_url 必须带实例前缀，网关按前缀做目录版本准入：\n{text}"
    );

    // 生效摘要里的时间必须是时间，不是事务 id（界面用它显示「当前生效时间」）。
    let summary = harness
        .service
        .applied_summary()
        .unwrap()
        .expect("提交后应有生效摘要");
    assert_eq!(summary.operation_id, operation_id);
    assert_ne!(
        summary.applied_at, summary.operation_id,
        "applied_at 不能是事务 id"
    );
    assert!(
        summary.applied_at.contains('T') && summary.applied_at.ends_with('Z'),
        "applied_at 应是 RFC3339 时刻：{}",
        summary.applied_at
    );
    assert_eq!(
        summary.stage,
        ApplyStage::AwaitingReload,
        "提交成功不等于宿主已加载"
    );

    let publication = harness.service.publication(&operation_id).unwrap();
    let publication = publication.expect("提交后必须发布运行快照");
    assert_eq!(publication.catalog_revision, plan.catalog_revision);
    assert!(publication.endpoint_prefix.starts_with("/i/"));
}

#[test]
fn host_receipt_promotes_to_verified_only_after_confirmation() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-confirm")
        .unwrap();

    let state = harness.service.confirm_reload(&operation_id, true).unwrap();
    assert_eq!(state.operation.stage, ApplyStage::Verified);
    assert_eq!(harness.host_states(), vec![HostState::Loaded]);
}

#[test]
fn unconfirmed_reload_stops_at_pending_instead_of_claiming_loaded() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-pending")
        .unwrap();

    let state = harness
        .service
        .confirm_reload(&operation_id, false)
        .unwrap();
    assert_eq!(state.operation.stage, ApplyStage::Pending);
    assert_ne!(harness.host_states(), vec![HostState::Loaded]);
}

#[test]
fn repeating_the_same_idempotency_key_reuses_the_same_operation() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();

    let first = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-same")
        .unwrap();
    let after_first = harness.read_config();

    let second = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-same")
        .unwrap();

    assert_eq!(first, second, "同一计划与幂等键必须复用同一事务");
    assert_eq!(
        harness.read_config(),
        after_first,
        "重复执行不得再写一次配置"
    );
    assert_eq!(harness.stage(&first), ApplyStage::AwaitingReload);
}

#[test]
fn an_expired_plan_is_blocked_instead_of_written() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness.operation_id();

    harness
        .clock
        .advance_to(plan.created_at_unix + plan.ttl_secs + 1);
    let error = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-expired")
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::ConfigChanged);
    assert_eq!(error.message_key, "error.planExpired");
    assert_eq!(harness.stage(&operation_id), ApplyStage::Blocked);
    assert!(!harness.config_path.exists(), "过期计划不得写入配置");
}

#[test]
fn a_tampered_plan_hash_is_rejected_before_any_write() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness.operation_id();

    let error = harness
        .service
        .execute_apply(plan.id.as_str(), "bad-hash", "idem-tampered")
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message_key, "error.planHashMismatch");
    assert_eq!(harness.stage(&operation_id), ApplyStage::Prepared);
    assert!(!harness.config_path.exists());
}

#[test]
fn recovery_reports_a_committed_transaction_as_awaiting_host_reload() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-recovery-aware")
        .unwrap();

    let reports = harness.service.startup_recovery().unwrap();
    let report = reports
        .iter()
        .find(|report| report.operation_id == operation_id)
        .expect("已提交的事务必须出现在恢复报告里");
    assert_eq!(report.decision, RecoveryDecision::AwaitHostReload);
    assert!(!report.applied, "等待宿主重载不是需要自动修复的状态");
    assert_eq!(harness.stage(&operation_id), ApplyStage::AwaitingReload);
}

#[test]
fn recovery_records_commit_when_the_file_matches_the_written_hash() {
    let harness = Harness::with_ready_model(None);
    harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness.operation_id();

    // 模拟“文件已替换、DB 尚未记完成”的崩溃点。
    let mut state = harness.store.get(&operation_id).unwrap().unwrap();
    state.operation.stage = ApplyStage::Committing;
    state.operation.written_hash = Some(config::hash("# 已提交但未记账\n"));
    harness.store.save(state).unwrap();
    harness.write_config("# 已提交但未记账\n");

    let reports = harness.service.startup_recovery().unwrap();
    let report = reports
        .iter()
        .find(|r| r.operation_id == operation_id)
        .unwrap();
    assert_eq!(
        report.decision,
        RecoveryDecision::RecordCommitFromFile {
            written_hash: config::hash("# 已提交但未记账\n")
        }
    );
    assert!(report.applied);
    assert_eq!(harness.stage(&operation_id), ApplyStage::AwaitingReload);
}

#[test]
fn recovery_enters_conflict_when_an_external_change_diverges_from_the_plan() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness.operation_id();

    let mut state = harness.store.get(&operation_id).unwrap().unwrap();
    state.operation.stage = ApplyStage::Committing;
    state.operation.written_hash = None;
    harness.store.save(state).unwrap();
    harness.write_config("# 外部程序抢先写入\n");

    let reports = harness.service.startup_recovery().unwrap();
    let report = reports
        .iter()
        .find(|r| r.operation_id == operation_id)
        .unwrap();
    assert_eq!(
        report.decision,
        RecoveryDecision::ConflictWithExternalChange
    );
    assert!(report.applied);
    assert_eq!(harness.stage(&operation_id), ApplyStage::Conflict);
    assert_eq!(
        harness.read_config(),
        "# 外部程序抢先写入\n",
        "恢复不得覆盖外部修改"
    );
    assert!(!plan.expected_config_hash.is_empty());
}

/// 还原后本工具写入的受管字段必须撤销，但被外部改过的那条要保留当前值。
#[test]
fn restore_drops_managed_keys_but_keeps_an_externally_modified_field() {
    let harness =
        Harness::with_ready_model(Some("# 用户自己的注释\napproval_policy = \"on-request\"\n"));
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-restore-setup")
        .unwrap();
    harness.service.confirm_reload(&operation_id, true).unwrap();
    assert_eq!(harness.host_states(), vec![HostState::Loaded]);

    // 外部程序只改了受管字段里的一条：还原必须保留它，而不是覆盖成基线。
    let tampered: String = harness
        .read_config()
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("model = ") {
                "model = \"外部改过的模型\""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        tampered.contains("外部改过的模型"),
        "夹具必须真的改到 model 行"
    );
    harness.write_config(&tampered);

    let restore = harness.service.plan_restore(&harness.instance).unwrap();
    assert!(
        restore
            .warnings
            .iter()
            .any(|warning| warning.contains("已被外部修改，将保留当前值")),
        "还原计划必须标出被外部修改的字段，实际警告：{:?}",
        restore.warnings
    );
    let restore_id = harness
        .store
        .find_by_plan(restore.id.as_str())
        .unwrap()
        .expect("还原计划必须登记事务")
        .operation
        .id
        .as_str()
        .to_owned();

    harness
        .service
        .execute_restore(restore.id.as_str(), &restore.plan_hash, "idem-restore")
        .unwrap();

    let text = harness.read_config();
    assert!(
        text.contains("model = \"外部改过的模型\""),
        "冲突字段必须保留外部值"
    );
    assert!(
        !text.contains("model_catalog_json"),
        "本工具写入的目录字段必须被撤销"
    );
    assert!(
        !text.contains("[model_providers.gptswitch]"),
        "本工具写入的 provider 必须被撤销"
    );
    assert!(
        text.contains("approval_policy = \"on-request\""),
        "无关字段必须原样保留"
    );
    assert!(text.contains("# 用户自己的注释"), "用户注释必须保留");
    assert!(
        !text.contains(SYNTHETIC_SECRET),
        "还原后的配置不得含上游 Key"
    );
    assert_eq!(harness.stage(&restore_id), ApplyStage::Verified);
    assert_eq!(
        harness.host_states(),
        vec![HostState::PendingApply],
        "还原后模型不再属于已生效目录"
    );
}

/// 回归：配置里已经带着本工具早先写下的字段时，还原必须把 Codex 交还给原生，
/// 而不是把我们自己的值当成「用户原值」再写回去。
///
/// 真机上的表现是：用户点了几遍「还原」、也重启了 Codex，可左下角仍然显示本工具，
/// 也回不到账号登录——因为记录下来的基线本身就是我们写的（旧版本装过，或另一个工具
/// 把我们写的行圈进了它的托管块），于是「还原」等于原样写回。
#[test]
fn restore_returns_to_native_when_the_recorded_baseline_was_our_own_write() {
    // 模拟旧版本留下的现场：受管字段已经是我们写的值。
    let leftovers = [
        "model = \"gs/p_old/m_old\"",
        "model_provider = \"gptswitch\"",
        "model_catalog_json = \"/Users/me/Library/Application Support/app.gptswitch.desktop/catalogs/rev_old/models.json\"",
        "model_context_window = 128000",
        "approval_policy = \"on-request\"",
        "",
        "[model_providers.gptswitch]",
        "name = \"Switchelp\"",
        "base_url = \"http://127.0.0.1:18765/i/inst_old/c/rev_old/v1\"",
        "wire_api = \"responses\"",
        "",
    ]
    .join("\n");
    let harness = Harness::with_ready_model(Some(&leftovers));

    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-leftover-apply")
        .unwrap();
    harness.service.confirm_reload(&operation_id, true).unwrap();

    let restore = harness.service.plan_restore(&harness.instance).unwrap();
    harness
        .service
        .execute_restore(
            restore.id.as_str(),
            &restore.plan_hash,
            "idem-leftover-restore",
        )
        .unwrap();

    let text = harness.read_config();
    assert!(
        !text.contains("gptswitch") && !text.contains("gs/"),
        "还原后不得再有本工具的 provider / 别名，实际：\n{text}"
    );
    assert!(
        !text.contains("model_catalog_json"),
        "还原后不得再指向本工具的目录"
    );
    // 上一条 apply 没有记录过 `model_context_window`（它的写入条件是「用户在模型里覆盖了
    // 上下文」），所以还原也不认领它。这是刻意的：同一个值也可能是用户自己在 Codex 里设的，
    // 光看数字分不出作者，删掉它是越权。真正决定路由的是 provider / 别名 / 目录三项，它们
    // 撤销之后 Codex 就已经回到原生登录与原生模型列表。
    assert!(
        text.contains("approval_policy = \"on-request\""),
        "无关字段必须原样保留"
    );
}

/// 首次接管时，配置里已经存在的本工具字段不能被记成「用户原值」。
#[test]
fn taking_over_our_own_leftovers_records_no_baseline() {
    use switch_core::codex::config::{looks_like_our_value, FieldOwnership};

    assert!(looks_like_our_value("model_provider", Some("gptswitch")));
    assert!(looks_like_our_value("model", Some("gs/p_a/m_1")));
    assert!(looks_like_our_value(
        "model_catalog_json",
        Some("/Users/me/Library/Application Support/app.gptswitch.desktop/catalogs/rev_1/models.json")
    ));
    assert!(looks_like_our_value(
        "model_providers.gptswitch",
        Some("name = \"Switchelp\"\n")
    ));
    // 认不出来的一律不当成自己的：泛化数值、原生模型名、别人的目录。
    assert!(!looks_like_our_value(
        "model_context_window",
        Some("128000")
    ));
    assert!(!looks_like_our_value("model", Some("gpt-5.6-sol")));
    assert!(!looks_like_our_value(
        "model_catalog_json",
        Some("/Users/me/Library/Application Support/OpenCodex/custom_model_catalog.json")
    ));
    assert!(!looks_like_our_value("model_provider", Some("openai")));

    let record = |key: &str, baseline: Option<&str>| FieldOwnership {
        key_path: key.to_owned(),
        baseline_presence: baseline.is_some(),
        baseline_value: baseline.map(str::to_owned),
        last_written_value: None,
    };
    // 基线里带着本工具的 provider ID 与 providers 子表 → 判为「本工具自己的作业」。
    assert!(switch_core::codex::config::baseline_is_our_own_work(&[
        record("model_provider", Some("gptswitch")),
        record("model_providers.gptswitch", Some("name = \"Switchelp\"\n")),
    ]));
    // 基线是真·原生配置 → 不是。
    assert!(!switch_core::codex::config::baseline_is_our_own_work(&[
        record("model_provider", Some("openai")),
    ]));
}

/// 从未写入过该实例的配置时，还原必须明确拒绝，而不是生成空计划。
#[test]
fn restore_is_refused_before_anything_was_written() {
    let harness = Harness::with_ready_model(None);

    let error = harness.service.plan_restore(&harness.instance).unwrap_err();

    assert_eq!(error.code, ErrorCode::ValidationFailed);
    assert!(
        error
            .safe_details
            .iter()
            .any(|detail| detail.contains("尚未写入过该实例的配置")),
        "错误详情应说明无需还原，实际：{:?}",
        error.safe_details
    );
}

/// 还原计划生成后配置又被外部改写：同样要 CAS 失败，不得覆盖。
#[test]
fn restore_blocks_when_the_config_changes_after_the_restore_plan() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-restore-cas-setup")
        .unwrap();
    harness.service.confirm_reload(&operation_id, true).unwrap();

    let restore = harness.service.plan_restore(&harness.instance).unwrap();
    let restore_id = harness
        .store
        .find_by_plan(restore.id.as_str())
        .unwrap()
        .expect("还原计划必须登记事务")
        .operation
        .id
        .as_str()
        .to_owned();

    harness.write_config("# 计划生成后外部又改了\n");
    let error = harness
        .service
        .execute_restore(restore.id.as_str(), &restore.plan_hash, "idem-restore-cas")
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::ConfigChanged);
    assert_eq!(error.message_key, "error.configChanged");
    assert_eq!(harness.stage(&restore_id), ApplyStage::Conflict);
    assert_eq!(
        harness.read_config(),
        "# 计划生成后外部又改了\n",
        "冲突后不得覆盖外部修改"
    );
}
