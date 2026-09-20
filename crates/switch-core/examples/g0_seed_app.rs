#![allow(clippy::field_reassign_with_default)] // 夹具按「先默认值再逐项赋值」写更好读

//! 把一次真实的应用事务**写入应用自己的数据库**，供端到端验收使用。
//!
//! 用法：
//! `cargo run -p switch-core --example g0_seed_app -- <应用数据目录> <实例配置路径> <上游地址>`
//!
//! 它做的是与界面完全相同的路径：检测输入之外的全部数据都由 `WorkspaceService`
//! 与 `ApplyService` 生成，`config.toml`、目录文件、操作记录都落在真实位置。
//! 事务停在 `AwaitingReload`，因此下次启动应用时启动恢复会把路由重新发布出来，
//! 不需要任何测试专用的生产代码。
//!
//! 合成上游地址由外部传入：验收脚本先起 mock，再把地址交进来，
//! 这样配置里的 `base_url` 指向的是真实的本机网关，而上游指向 mock。

use std::{path::PathBuf, sync::Arc};
use switch_core::{
    application::{
        ApplyService, GatewayLayout, ModelDraft, ProviderDraft, SystemClock, WorkspaceService,
    },
    codex::detect::{CodexInstance, StartupMode},
    credentials::{MemoryVault, SecretVault},
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
    storage::{OperationStore, Repository, SqliteOperationStore, SqliteRepository},
};

/// 与 `src-tauri` 的 `AUTH_HELPER_INSTANCE` 一致：宿主固定传这个值。
const AUTH_HELPER_INSTANCE: &str = "local-main";
const SYNTHETIC_SECRET: &str = "synthetic-seed-secret-not-a-real-key";

fn main() {
    let mut args = std::env::args().skip(1);
    let app_data = PathBuf::from(args.next().expect("缺少应用数据目录"));
    let instance_config = PathBuf::from(args.next().expect("缺少实例配置路径"));
    let upstream = args.next().expect("缺少上游地址");

    // 目录与配置必须与界面走同一条写入路径，所以这里有真实的密钥库与 SQLite。
    let vault: Arc<dyn SecretVault> = Arc::new(MemoryVault::new());
    let repository: Arc<dyn Repository> =
        Arc::new(SqliteRepository::open(&app_data.join("metadata.sqlite")).unwrap());
    let operations: Arc<dyn OperationStore> =
        Arc::new(SqliteOperationStore::open(&app_data.join("metadata.sqlite")).unwrap());
    let workspace = WorkspaceService::new(repository.clone(), vault);
    let service = ApplyService::new(
        repository,
        operations,
        Arc::new(GatewayRouter::new()),
        GatewayLayout {
            app_data_dir: app_data.clone(),
            port: gateway::DEFAULT_PORT,
            // 必须与应用安装 helper 的位置一致，否则宿主调用一个不存在的文件。
            auth_helper: app_data
                .join("bin")
                .join("gptswitch-auth-helper")
                .display()
                .to_string(),
            base_instructions: "You are a coding assistant. Follow the user instructions."
                .to_owned(),
        },
        Arc::new(SystemClock),
    );

    if let Some(parent) = instance_config.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let instance = CodexInstance {
        id: InstanceId::new("inst_e2e"),
        app_path: None,
        cli_path: None,
        desktop_version: None,
        cli_version: None,
        config_root: instance_config
            .parent()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        config_file: instance_config.display().to_string(),
        config_exists: instance_config.exists(),
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
                name: "E2E 合成供应商".into(),
                endpoint: upstream,
                // 用 chat 协议：这样端到端链路里一定会走 adaptation，而不是纯透传。
                protocol: Protocol::ChatCompletions,
                auth_kind: AuthKind::ApiKey,
                preset_id: None,
                notes: None,
                enabled: true,
            },
            0,
        )
        .unwrap();
    let credential = workspace
        .add_credential(provider.id.as_str(), "E2E", SYNTHETIC_SECRET.into())
        .unwrap();
    workspace
        .select_credential(provider.id.as_str(), credential.id.as_str())
        .unwrap();

    let text_model = workspace
        .save_model(
            model(
                provider.id.as_str(),
                "synthetic/text-model",
                "E2E 文本模型",
                false,
            ),
            0,
        )
        .unwrap();
    let vision_model = workspace
        .save_model(
            model(
                provider.id.as_str(),
                "synthetic/vision-model",
                "E2E 视觉模型",
                true,
            ),
            0,
        )
        .unwrap();

    let plan = service.plan_apply(&instance, None).unwrap();
    let operation_id = service
        .execute_apply(plan.id.as_str(), &plan.plan_hash, "e2e-seed")
        .unwrap();
    // 停在 AwaitingReload：不确认重载，交给应用启动恢复去重新发布路由。
    let stage = service.status(&operation_id).unwrap().operation.stage;

    let config_text = std::fs::read_to_string(&instance_config).unwrap();
    assert!(
        !config_text.contains(SYNTHETIC_SECRET),
        "上游 Key 绝不能写入 config.toml"
    );
    println!(
        "{}",
        serde_json::json!({
            "appData": app_data.display().to_string(),
            "configPath": instance_config.display().to_string(),
            "catalogPath": service.layout().catalog_path(&plan.catalog_revision).display().to_string(),
            "operationId": operation_id,
            // 应用读的是系统凭据库；验收脚本据此写入并可精确删除同一条目。
            "secretRef": credential.secret_ref,
            "stage": format!("{stage:?}"),
            "aliases": [text_model.catalog_alias.as_str(), vision_model.catalog_alias.as_str()],
            "upstreamIds": [text_model.upstream_id, vision_model.upstream_id],
            "configText": config_text,
        })
    );
}

/// 两个模型：一个纯文本，一个带图片能力。
///
/// 手工声明的输入能力只描述上游；网关与宿主两层由 `WorkspaceService` 重算——
/// 这正是要在端到端里验证的部分。
fn model(provider_id: &str, upstream_id: &str, display_name: &str, vision: bool) -> ModelDraft {
    let mut policy = ModelPolicy::default();
    policy.context_limit = Some(TokenCount::new(128_000).unwrap());
    policy.output_limit = Some(TokenCount::new(4_096).unwrap());
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
    policy.reasoning =
        ReasoningPolicy::effort(vec!["low".into(), "high".into()], Some("low".into()));
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

/// 供验收脚本核对：宿主固定传的实例参数。
#[allow(dead_code)]
fn helper_instance_arg() -> &'static str {
    AUTH_HELPER_INSTANCE
}
