#![allow(clippy::field_reassign_with_default)] // 夹具按「先默认值再逐项赋值」写更好读

//! G0 实机探针：用真实的计划 → CAS 提交链路产出 `config.toml` 与模型目录，
//! 再交给真实 Codex app-server 校验目录契约。
//!
//! 用法：`cargo run -p switch-core --example g0_apply_pipeline -- <工作目录>`
//!
//! 与 `scripts/g0/probe-catalog.mjs` 的区别：那个探针喂给 Codex 的是手写目录，
//! 这里喂的是 `CatalogCompiler` 与 `apply_managed` 的真实产物。所有输入都是合成的，
//! 工作目录与用户现有 `CODEX_HOME` 完全隔离，不读取任何真实凭据。

use std::{path::PathBuf, sync::Arc};
use switch_core::{
    application::{
        ApplyService, GatewayLayout, ModelDraft, ProviderDraft, SystemClock, WorkspaceService,
    },
    codex::detect::{CodexInstance, StartupMode},
    credentials::MemoryVault,
    domain::{
        capability::{InputCapability, InputKind, Support, Verification},
        ids::InstanceId,
        model::ModelPolicy,
        provider::{AuthKind, Protocol},
        reasoning::ReasoningPolicy,
        tokens::TokenCount,
        version::{CompatibilityStatus, VersionFingerprint},
    },
    gateway::{self, GatewayRouter},
    storage::{MemoryOperationStore, Repository, SqliteRepository},
};

/// 合成令牌：探针里 Codex 会通过 auth helper 取得它，上游 mock 用它校验请求。
const SYNTHETIC_TOKEN: &str = "synthetic-g0-apply-token";
const SYNTHETIC_SECRET: &str = "synthetic-upstream-secret-not-a-real-key";

fn main() {
    let workdir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("用法：g0_apply_pipeline <工作目录>"),
    );
    let codex_home = workdir.join("codex-home");
    let auth_helper = workdir.join("bin").join("gptswitch-auth-helper");
    for directory in [
        &workdir,
        &codex_home,
        &auth_helper.parent().unwrap().to_path_buf(),
    ] {
        std::fs::create_dir_all(directory).expect("无法创建工作目录");
    }
    write_auth_helper(&auth_helper);

    let config_path = codex_home.join("config.toml");
    let db_path = workdir.join("metadata.sqlite");
    let repository: Arc<dyn Repository> = Arc::new(SqliteRepository::open(&db_path).unwrap());
    let vault = Arc::new(MemoryVault::new());
    let workspace = WorkspaceService::new(repository.clone(), vault);
    let operations = Arc::new(MemoryOperationStore::new());
    let service = ApplyService::new(
        repository,
        operations,
        Arc::new(GatewayRouter::new()),
        GatewayLayout {
            app_data_dir: workdir.join("app-data"),
            port: gateway::DEFAULT_PORT,
            auth_helper: auth_helper.display().to_string(),
            base_instructions: "You are a coding assistant. Follow the user instructions."
                .to_owned(),
        },
        Arc::new(SystemClock),
    );

    let instance = CodexInstance {
        id: InstanceId::new("inst_g0"),
        app_path: None,
        cli_path: None,
        desktop_version: None,
        cli_version: None,
        config_root: codex_home.display().to_string(),
        config_file: config_path.display().to_string(),
        config_exists: config_path.exists(),
        startup_mode: StartupMode::NotRunning,
        compatibility: CompatibilityStatus::Unverified,
        fingerprint: VersionFingerprint::unknown(),
        conflicting_managers: Vec::new(),
        blocked_reason_key: None,
    };

    let provider = workspace
        .save_provider(
            ProviderDraft {
                id: None,
                name: "G0 合成供应商".into(),
                endpoint: "http://127.0.0.1:1/v1".into(),
                protocol: Protocol::Responses,
                auth_kind: AuthKind::ApiKey,
                preset_id: None,
                notes: None,
                enabled: true,
            },
            0,
        )
        .unwrap();
    let credential = workspace
        .add_credential(provider.id.as_str(), "G0", SYNTHETIC_SECRET.into())
        .unwrap();
    workspace
        .select_credential(provider.id.as_str(), credential.id.as_str())
        .unwrap();

    let text_model = workspace
        .save_model(text_only_model(provider.id.as_str()), 0)
        .unwrap();
    let vision_model = workspace
        .save_model(vision_model(provider.id.as_str()), 0)
        .unwrap();

    let plan = service.plan_apply(&instance, None).unwrap();
    let operation_id = service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "g0-idem")
        .unwrap();
    // 故意不调用 confirm_reload：宿主是否真的加载由后续真实 Codex 校验决定。
    let stage = service.status(&operation_id).unwrap().operation.stage;
    let config_text = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        !config_text.contains(SYNTHETIC_SECRET),
        "上游 Key 绝不能写入 config.toml"
    );

    let catalog_path = service.layout().catalog_path(&plan.catalog_revision);
    let catalog_json = std::fs::read_to_string(&catalog_path).unwrap();

    println!(
        "{}",
        serde_json::json!({
            "workdir": workdir.display().to_string(),
            "codexHome": codex_home.display().to_string(),
            "configPath": config_path.display().to_string(),
            "catalogPath": catalog_path.display().to_string(),
            "authHelper": auth_helper.display().to_string(),
            "authToken": SYNTHETIC_TOKEN,
            "operationId": operation_id,
            "stage": format!("{stage:?}"),
            "planHash": plan.plan_hash,
            "expectedAliases": [text_model.catalog_alias.as_str(), vision_model.catalog_alias.as_str()],
            "configText": config_text,
            "catalog": serde_json::from_str::<serde_json::Value>(&catalog_json).unwrap(),
        })
    );
}

/// 写一个只输出本机令牌的合成 helper，形状与 G3 的真实 helper 一致。
///
/// 每次调用都会追加一行到 `auth-helper-invocations.log`：宿主到底有没有真的
/// 调用 `auth.command`、带了什么参数，只能从调用记录判断。
fn write_auth_helper(path: &PathBuf) {
    let script = format!(
        "#!/bin/sh\nprintf '%s %s\\n' \"$(date +%s)\" \"$*\" >> \"$(dirname \"$0\")/auth-helper-invocations.log\"\necho {SYNTHETIC_TOKEN}\n"
    );
    std::fs::write(path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

/// 手工声明的输入能力只描述上游；网关与宿主两层由 `WorkspaceService` 重算。
fn policy(context: u64, output: u64, vision: bool, reasoning: ReasoningPolicy) -> ModelPolicy {
    let mut policy = ModelPolicy::default();
    policy.context_limit = Some(TokenCount::new(context).unwrap());
    policy.output_limit = Some(TokenCount::new(output).unwrap());
    policy.inputs = InputKind::ALL
        .iter()
        .map(|kind| {
            let upstream = match kind {
                InputKind::Text => Support::Supported,
                InputKind::Image if vision => Support::Supported,
                _ => Support::Unsupported,
            };
            InputCapability::new(
                *kind,
                upstream,
                Support::Unknown,
                Support::Unknown,
                Verification::Declared,
            )
        })
        .collect();
    policy.reasoning = reasoning;
    policy
}

fn text_only_model(provider_id: &str) -> ModelDraft {
    let mut draft = base_model(provider_id, "synthetic/text-model-a", "G0 文本模型");
    draft.policy = policy(
        128_000,
        8_192,
        false,
        ReasoningPolicy::effort(vec!["low".into(), "high".into()], Some("low".into())),
    );
    draft
}

fn vision_model(provider_id: &str) -> ModelDraft {
    let mut draft = base_model(provider_id, "synthetic/vision-model-b", "G0 视觉模型");
    draft.policy = policy(
        200_000,
        16_384,
        true,
        ReasoningPolicy::effort(vec!["medium".into()], Some("medium".into())),
    );
    draft
}

fn base_model(provider_id: &str, upstream_id: &str, display_name: &str) -> ModelDraft {
    ModelDraft {
        id: None,
        provider_id: provider_id.to_owned(),
        upstream_id: upstream_id.to_owned(),
        catalog_alias: String::new(),
        display_name: display_name.to_owned(),
        policy: ModelPolicy::default(),
        in_catalog: true,
        display_name_overridden: true,
        protocol_override: None,
    }
}
