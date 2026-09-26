//! 应用编排集成测试：计划 → CAS → 原子提交 → 等待重载 → 恢复。
//!
//! 覆盖的关键不变量（来自 [配置生命周期](../../docs/architecture/02-configuration-lifecycle.md)）：
//! - 计划阶段绝不修改 Codex 配置。
//! - 提交前必须 CAS；外部修改后拒绝写入并转入冲突。
//! - 提交成功最多到“等待重载”，绝不自行宣称宿主已加载。
//! - 恢复只撤销本工具写入且未被外部修改的字段。
//! - 上游 Key 永不写入 config.toml。

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use switch_core::{
    application::{
        ApplyService, Clock, GatewayLayout, ModelDraft, ProviderDraft, WorkspaceService,
    },
    codex::{
        config,
        detect::{legacy_instance_id, CodexInstance, StartupMode},
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
    protocols::CHAT_COMPLETIONS_V1,
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
    /// 用例据此检查「失败之后路由里还剩什么」。
    router: Arc<GatewayRouter>,
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
        protocol_override: None,
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
    let router = Arc::new(GatewayRouter::new());
    let service = ApplyService::new(
        repository,
        store.clone(),
        router.clone(),
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
        router,
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

    /// 应用数据目录：目录文件与路由快照都挂在它下面。
    fn layout_dir(&self) -> PathBuf {
        self.config_path.parent().unwrap().join("app-data")
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

/// 宿主升级换了 bundle 内 CLI 位置之后，历史写入记录挂在**旧口径**的实例 ID 上
/// （旧口径把 CLI 路径算进了身份）。还原必须仍然找得到它们，否则界面报
/// 「本工具尚未写入过该实例的配置，无需还原」——而配置里明明还留着我们写的字段。
/// 2026-09-26 的 ChatGPT 26.924 在真机上就是这条故障。
#[test]
fn restore_finds_records_left_under_a_pre_upgrade_instance_id() {
    const APP_PATH: &str = "/Applications/ChatGPT.app";
    let mut harness = Harness::with_ready_model(None);

    // 升级前那次应用：身份是旧的算法，记录挂在旧 ID 下。
    let config_root = PathBuf::from(harness.instance.config_root.clone());
    harness.instance.app_path = Some(APP_PATH.to_owned());
    harness.instance.id = legacy_instance_id(
        &config_root,
        Path::new(APP_PATH),
        "Contents/Resources/codex",
    );
    let pre_upgrade_id = harness.instance.id.clone();
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-pre-upgrade")
        .unwrap();
    harness.service.confirm_reload(&operation_id, true).unwrap();

    // 升级后：同一个安装（同配置根、同 bundle 路径），但 CLI 挪了位置，身份随之改变。
    harness.instance.cli_path = Some(format!("{APP_PATH}/Contents/Resources/codex-cli/bin/codex"));
    harness.instance.id = InstanceId::new("inst_after_cli_moved");
    assert_ne!(harness.instance.id, pre_upgrade_id);

    // 还原必须按旧 ID 找回那份记录，而不是当成「从没写过」。
    let restore = harness
        .service
        .plan_restore(&harness.instance)
        .expect("换了 CLI 位置也必须找回升级前的写入记录");
    harness
        .service
        .execute_restore(
            restore.id.as_str(),
            &restore.plan_hash,
            "idem-post-upgrade-restore",
        )
        .unwrap();

    let text = harness.read_config();
    assert!(
        !text.contains("model_provider"),
        "还原后不该再有本工具写的 provider，实际：{text}"
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

/// 写盘失败不能留下一个「配置里并不存在」的已发布目录版本。
///
/// 回归：`router.publish` 过去排在 `write_atomic` 之前，而写入失败只是把错误冒出去——
/// 刚发布的版本留在内存路由器里继续服务，下次应用的版本号又与它对不上。
/// 表现是网关为一个没人指向的版本服务，而界面说「保存失败」。
#[test]
fn a_failed_write_takes_back_the_publication_it_just_made() {
    let harness = Harness::with_ready_model(Some("model = \"gpt-5.6-sol\"\n"));
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();

    // 让写入失败：目录去掉写权限，`File::create` 就建不出临时文件。
    // 恢复权限用 guard，保证断言失败也不会把临时目录留在只读状态。
    let dir = harness.config_path.parent().unwrap().to_path_buf();
    let original = std::fs::metadata(&dir).unwrap().permissions();
    let mut read_only = original.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&dir, read_only).unwrap();
    let result =
        harness
            .service
            .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-write-fail");
    std::fs::set_permissions(&dir, original).unwrap();

    assert!(result.is_err(), "写入失败必须报错，不能假装提交成功");
    assert!(
        !harness.router.revisions().contains(&plan.catalog_revision),
        "写入失败后，刚发布的目录版本必须被收回，否则网关会一直为一个配置里不存在的版本服务：{:?}",
        harness.router.revisions()
    );
    // 用户配置保持原样。
    assert_eq!(
        std::fs::read_to_string(&harness.config_path).unwrap(),
        "model = \"gpt-5.6-sol\"\n"
    );
}

/// 旧目录版本必须被回收。
///
/// 回归：生产路径上 `retain`/`release`/`retire` 从来没被调用过，每应用一次就永久多一份
/// 目录快照与一份磁盘文件（本机实测堆了 3 份）。保留代数是有意的策略：留下「当前 + 上一代」，
/// 因为宿主只在启动时读配置，刚应用完它还带着旧前缀在跑，旧快照要服务到它真的重启为止。
#[test]
fn apply_reclaims_catalog_revisions_beyond_the_retention_window() {
    let harness = Harness::with_ready_model(None);
    let mut revisions = Vec::new();
    for round in 0..4 {
        // 每轮换一个 Key：目录内容不变，但来源摘要变，于是得到真正的新版本号。
        let provider = harness.workspace.list_providers().unwrap().remove(0);
        let credential = harness
            .workspace
            .add_credential(
                provider.id.as_str(),
                &format!("第{round}次"),
                format!("synthetic-secret-round{round}-0123456789"),
            )
            .unwrap();
        harness
            .workspace
            .select_credential(provider.id.as_str(), credential.id.as_str())
            .unwrap();
        let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
        harness
            .service
            .execute_apply(plan.id.as_str(), &plan.plan_hash, &format!("idem-{round}"))
            .unwrap();
        revisions.push(plan.catalog_revision);
    }

    let last = revisions.last().unwrap().clone();
    let mut distinct: Vec<String> = revisions.clone();
    distinct.dedup();
    assert!(distinct.len() >= 3, "本用例需要至少 3 个不同版本号才有意义");

    let live = harness.router.revisions();
    assert!(live.contains(&last), "当前版本必须还在：{live:?}");
    assert!(
        live.len() <= 2,
        "只应保留当前版本与上一代，实际留下 {live:?}"
    );
    let oldest = &revisions[0];
    assert!(!live.contains(oldest), "最早那代必须被回收");
    assert!(
        !harness.layout_dir().join("catalogs").join(oldest).exists(),
        "内存里回收了，磁盘上的目录也要删掉"
    );
}

/// 仍被引用的版本不能被回收——这是「旧宿主带旧前缀继续工作」的保障。
#[test]
fn a_referenced_revision_survives_reclamation() {
    let harness = Harness::with_ready_model(None);
    let first = harness.service.plan_apply(&harness.instance, None).unwrap();
    harness
        .service
        .execute_apply(first.id.as_str(), &first.plan_hash, "idem-a")
        .unwrap();
    // 模拟「还有一个在途请求 / 续接绑定挂在这个版本上」。
    harness.router.retain(&first.catalog_revision, 0);

    for round in 0..4 {
        let provider = harness.workspace.list_providers().unwrap().remove(0);
        let credential = harness
            .workspace
            .add_credential(
                provider.id.as_str(),
                &format!("轮{round}"),
                format!("synthetic-secret-ref{round}-0123456789"),
            )
            .unwrap();
        harness
            .workspace
            .select_credential(provider.id.as_str(), credential.id.as_str())
            .unwrap();
        let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
        harness
            .service
            .execute_apply(plan.id.as_str(), &plan.plan_hash, &format!("idem-r{round}"))
            .unwrap();
    }

    assert!(
        harness.router.revisions().contains(&first.catalog_revision),
        "仍有引用的版本不能被回收：{:?}",
        harness.router.revisions()
    );

    // 引用释放之后，下一次应用就该把它收走。
    harness.router.release(&first.catalog_revision, 0);
    let provider = harness.workspace.list_providers().unwrap().remove(0);
    let credential = harness
        .workspace
        .add_credential(
            provider.id.as_str(),
            "收尾",
            "synthetic-secret-final-0123456789".to_string(),
        )
        .unwrap();
    harness
        .workspace
        .select_credential(provider.id.as_str(), credential.id.as_str())
        .unwrap();
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-final")
        .unwrap();
    assert!(
        !harness.router.revisions().contains(&first.catalog_revision),
        "引用归零后应当被回收：{:?}",
        harness.router.revisions()
    );
}

/// 应用是**替换** Codex 的模型菜单，不是往里追加。
///
/// 这条不是「实现细节」，而是用户最容易误解的行为：`model_catalog_json` 指向的目录里
/// 只有本工具发布的别名，Codex 自带的模型会整批从选择器里消失，直到「还原为原生 Codex」。
/// 真机实测 `model/list` 只剩 1 条。以前界面和文档都没说过这件事，于是「加一个模型」
/// 的预期与「原来能用的都不见了」的结果对不上。
///
/// 这里锁定的是核心侧的事实：写进去的目录**只含本次发布的别名**，一个原生模型都不带。
#[test]
fn applying_replaces_the_host_menu_with_exactly_the_published_aliases() {
    let harness = Harness::with_ready_model(Some(
        "model = \"gpt-5.6-sol\"\nmodel_provider = \"openai\"\n",
    ));
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-catalog")
        .unwrap();

    let catalog_text = std::fs::read_to_string(
        harness
            .layout_dir()
            .join("catalogs")
            .join(&plan.catalog_revision)
            .join("models.json"),
    )
    .unwrap();
    let catalog: serde_json::Value = serde_json::from_str(&catalog_text).unwrap();
    let published: Vec<String> = catalog["models"]
        .as_array()
        .expect("目录里有 models 数组")
        .iter()
        .map(|entry| entry["slug"].as_str().unwrap().to_owned())
        .collect();

    assert_eq!(
        published, plan.catalog_aliases,
        "目录内容必须与计划声明的别名逐一对应"
    );
    // 原生模型不会出现在这里——它们没有被合并进去，这就是「替换」的确切含义。
    for native in ["gpt-5.6-sol", "gpt-6-astra", "gpt-5"] {
        assert!(
            !catalog_text.contains(native),
            "目录里不该出现原生模型 {native}；它是替换而不是追加"
        );
    }
    // 配置确实指向这份目录（宿主据此换掉整份菜单）。
    assert!(harness.read_config().contains("model_catalog_json"));
    assert!(harness.read_config().contains(&plan.catalog_revision));
}

/// 模型级协议覆盖必须真的改变路由。
///
/// 回归：`Model.protocol_override` 早就在域模型里，`build_routes` 也按它选协议
/// （`model.protocol_override.unwrap_or(provider.protocol)`），但 `ModelDraft` 没有这个字段，
/// 于是从界面永远设不上——P0 要求「独立模型编辑：显示名、上游 ID、协议…」里的协议是个死字段。
#[test]
fn a_model_level_protocol_override_reaches_the_route() {
    let harness = Harness::with_ready_model(None);
    let provider = harness.workspace.list_providers().unwrap().remove(0);
    assert_eq!(
        provider.protocol,
        Protocol::Responses,
        "前提：供应商本身是 Responses"
    );

    // 同一个供应商下的这个模型单独走 chat/completions。
    // 改**已有**那个模型：新建第二个会得到两个 alias，而断言按 alias 取路由，取到谁不确定。
    let existing = harness.workspace.list_models().unwrap().remove(0);
    let model = harness
        .workspace
        .save_model(
            ModelDraft {
                id: Some(existing.id.as_str().to_owned()),
                protocol_override: Some(Protocol::ChatCompletions),
                ..ready_model(provider.id.as_str())
            },
            existing.version,
        )
        .unwrap();
    assert_eq!(model.protocol_override, Some(Protocol::ChatCompletions));

    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-protocol")
        .unwrap();

    let snapshot = harness
        .router
        .admission(
            &plan.catalog_revision,
            &plan.catalog_aliases[0],
            &harness.instance.id,
        )
        .expect("目录里应当有这个 alias");
    assert_eq!(
        snapshot.route.protocol_id, CHAT_COMPLETIONS_V1,
        "模型级协议要覆盖供应商的协议"
    );

    // 目标是「模型级覆盖能生效」，不是「覆盖能清掉」——清掉覆盖走的是同一段代码，
    // 再写一遍只会和上面的唯一约束打架（同一个 supplier+upstream 的第二次身份登记）。
}

/// Chat Completions 不再带「实验状态」警告：工具调用已经在真实上游上验证过。
///
/// 以前这里锁的是相反的结论——核心层把 CC 适配标成「未通过工具调用门禁」，于是每次发布
/// 只要有一个模型走 CC，差异里就会多一条黄色警告。2026-09-24 实测该断言的前提不成立
/// （见 `docs/architecture/03-gateway-and-protocols.md` 的兼容表），警告已撤。
/// 这条用例留着是为了挡住「无意间又把那条泛化警告加回来」。
#[test]
fn chat_completions_models_are_no_longer_flagged_as_experimental() {
    let harness = Harness::with_ready_model(None);
    let provider = harness.workspace.list_providers().unwrap().remove(0);
    let model = harness.workspace.list_models().unwrap().remove(0);
    harness
        .workspace
        .save_model(
            ModelDraft {
                id: Some(model.id.as_str().to_owned()),
                protocol_override: Some(Protocol::ChatCompletions),
                ..ready_model(provider.id.as_str())
            },
            model.version,
        )
        .unwrap();

    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    assert!(
        !plan
            .warnings
            .iter()
            .any(|warning| warning.contains("chatAdapterExperimental")),
        "CC 适配的工具调用已经验证过，不该再有实验状态警告：{:?}",
        plan.warnings
    );
}

/// 宿主重启的事实要被记成回执。
///
/// 回归：以前只有配置页那两个按钮会写回执，而「应用并重启 Codex」自己重启了宿主却不记账，
/// 于是配置早已生效、Codex 也真的重新读过了，界面却永远显示「等待重载」、待应用条一直在。
/// 真机上就是这么踩到的：事务停在 awaiting_reload，模型停在 awaiting_reload。
#[test]
fn a_host_started_after_the_publication_is_recorded_as_a_receipt() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-receipt")
        .unwrap();
    assert_eq!(harness.stage(&operation_id), ApplyStage::AwaitingReload);

    // 测试时钟的发布时间是 2026-09-18；1_900_000_000 是 2030 年，晚于它。
    let confirmed = harness
        .service
        .reconcile_host_reload(Some(1_900_000_000))
        .unwrap();
    assert_eq!(confirmed, vec![operation_id.clone()], "应记下这次回执");
    assert_eq!(harness.stage(&operation_id), ApplyStage::Verified);
    assert_eq!(harness.host_states(), vec![HostState::Loaded]);

    // 幂等：再跑一次不会重复确认，也不会报错。
    assert!(harness
        .service
        .reconcile_host_reload(Some(1_900_000_000))
        .unwrap()
        .is_empty());
}

/// 比发布更早起来的宿主不是回执——那份配置它还没读过。
#[test]
fn a_host_running_since_before_the_publication_is_not_a_receipt() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-stale-host")
        .unwrap();

    // 1_600_000_000 是 2020 年：宿主一直活着，配置改了也不会被它读到。
    assert!(harness
        .service
        .reconcile_host_reload(Some(1_600_000_000))
        .unwrap()
        .is_empty());
    assert_eq!(harness.stage(&operation_id), ApplyStage::AwaitingReload);
    assert_eq!(harness.host_states(), vec![HostState::AwaitingReload]);
}

/// 查不到宿主启动时间时不许猜：停在原处等人工确认，不用「大概重启过了」顶替回执。
#[test]
fn an_unknown_host_start_time_never_confirms_by_itself() {
    let harness = Harness::with_ready_model(None);
    let plan = harness.service.plan_apply(&harness.instance, None).unwrap();
    let operation_id = harness
        .service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "idem-no-host")
        .unwrap();

    assert!(harness
        .service
        .reconcile_host_reload(None)
        .unwrap()
        .is_empty());
    assert_eq!(harness.stage(&operation_id), ApplyStage::AwaitingReload);
    assert_eq!(harness.host_states(), vec![HostState::AwaitingReload]);
}

/// 共存模式：产物只落进应用数据目录里的托管 profile，用户真实的 config.toml 一个字节不动。
///
/// 这条锁的是共存模式的**全部承诺**：官方模型走原生那根（它的配置必须原样），
/// 我们的模型走托管那根（配置写在那儿）。写错一边，用户要么丢了原生配置，要么菜单里
/// 出现的是一个指向别人家的路由。
#[test]
fn coexist_writes_the_managed_profile_and_leaves_the_native_config_alone() {
    // 用户自己的配置里带着与路由无关的设置：托管 profile 要继承它们（插件、项目信任
    // 都是他 Codex 体验的一部分），但不能继承路由——那是原生那根的。
    // 注意 TOML 的表头规则：表头之后的行属于那张表。路由字段写在最前面，
    // 否则它们会变成 `[projects...]` 的成员，测的就不是「剥离受管字段」了。
    let native = r#"# 用户自己的设置
approval_policy = "on-request"
model = "gpt-5.6-sol"
model_provider = "openai"

[projects."/Users/me/work"]
trust_level = "trusted"
"#;
    let harness = Harness::with_ready_model(Some(native));

    let plan = harness
        .service
        .plan_coexist(&harness.instance, None)
        .unwrap();
    let managed_config = harness.layout_dir().join("codex-home").join("config.toml");
    assert_eq!(
        plan.config_path,
        managed_config.display().to_string(),
        "计划必须落在托管 profile 上"
    );

    let operation = plan.id.as_str().to_owned();
    harness
        .service
        .execute_apply(&operation, &plan.plan_hash, "coexist-key")
        .unwrap();

    let managed = std::fs::read_to_string(&managed_config).unwrap();
    assert!(
        managed.contains("model_providers.gptswitch"),
        "托管 profile 要带上网关 provider：{managed}"
    );
    assert!(
        managed.contains("model_catalog_json"),
        "托管 profile 要指向我们发布的目录"
    );
    assert!(
        managed.contains("trust_level") && managed.contains("on-request"),
        "与路由无关的用户设置要继承过来：{managed}"
    );
    assert!(
        !managed.contains("model_provider = \"openai\""),
        "原生那份路由不能被带进托管 profile（它要写给自己的）：{managed}"
    );

    assert_eq!(
        harness.read_config(),
        native,
        "用户真实的 config.toml 必须一个字节都没变"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&managed_config)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "托管 profile 里有用户的设置，不能给别人读");
    }
}

/// 共存模式的开关是**意图**：记下来，但不能拿它当「已经生效」。
#[test]
fn coexist_switch_is_intent_not_evidence() {
    let harness = Harness::with_ready_model(None);
    assert!(!harness.service.coexist_enabled().unwrap());
    harness.service.set_coexist_enabled(true).unwrap();
    assert!(harness.service.coexist_enabled().unwrap());
    harness.service.set_coexist_enabled(false).unwrap();
    assert!(!harness.service.coexist_enabled().unwrap());
}
