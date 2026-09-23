use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use switch_core::{
    application::{AppliedSummary, ModelDraft, ProviderDraft},
    codex::backup::BackupEntry,
    codex::{
        config::{ConfigSnapshot, MANAGED_KEYS},
        detect::CodexInstance,
        plan::{ApplyPlan, OperationEvent},
    },
    diagnostics::{
        fetch_models, DiagnosticEvent, DiscoveredModel, ExportPreview, LogLevel, ProbePlan,
        ProbeReport, ProbeTarget,
    },
    domain::{
        credential::Credential,
        error::{CoreError, ErrorCode},
        ids::{ModelId, ProviderId},
        model::qualified_display_name,
        model::Model,
        provider::Provider,
    },
};
use tauri::{Emitter, Manager, State, WebviewWindow};
use tauri_plugin_updater::UpdaterExt;

use crate::state::{loopback_bypass_env, DesktopState, SystemProxyReport};

type Desktop<'a> = State<'a, Arc<DesktopState>>;

/// 只读配置检查结果。预览已脱敏，不含任何上游秘密。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectResult {
    pub instance_id: String,
    pub config_path: String,
    pub redacted_preview: String,
    /// 当前配置文件中已存在的受管字段；未出现的不计入。
    pub managed_fields: Vec<String>,
    /// 检测到的其他配置管理工具标记，仅用于提示，不读取它们的凭据。
    pub conflicts: Vec<String>,
}

/// 提交类命令的返回形状：执行与还原共用。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteResult {
    pub operation_id: String,
}

/// 当前已生效的配置摘要；从未应用过时返回 null。
#[tauri::command]
pub async fn apply_summary(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Option<AppliedSummary>, CoreError> {
    run(window, state, |desktop| desktop.apply.applied_summary()).await
}

/// 本机网关状态。未启动时必须带出原因，界面不得显示成“正常”。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayReport {
    pub running: bool,
    /// 是否已暂停接受新请求。
    pub paused: bool,
    pub port: Option<u16>,
    pub served: u64,
    /// 已发布的目录版本；空表示还没应用过任何配置。
    pub revisions: Vec<String>,
    /// 令牌指纹：便于人工核对 helper 指向同一令牌，不泄露令牌本身。
    pub token_fingerprint: String,
    pub error: Option<String>,
    /// 系统代理与回环地址的关系。宿主到本机网关是回环地址，系统代理会把它也代理走，
    /// 用户看到的就是「502 Bad Gateway: Unknown error」——这条事实要能显示出来。
    pub system_proxy: SystemProxyReport,
}

/// 事务状态。`events` 让界面能显示“等待 Codex 重载”这类中间态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyStatus {
    pub operation_id: String,
    pub events: Vec<OperationEvent>,
    /// 是否仍有未完成阶段；窗口重开后先读状态，再决定是否继续等待。
    pub open: bool,
}

fn authorize(window: &WebviewWindow) -> Result<(), CoreError> {
    let url = window
        .url()
        .map_err(|_| CoreError::new(ErrorCode::Unauthorized, "error.unauthorized"))?;
    let packaged = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"));
    let development = cfg!(debug_assertions)
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
        && url.port() == Some(5173);
    if window.label() != "main" || (!packaged && !development) {
        return Err(CoreError::new(
            ErrorCode::Unauthorized,
            "error.unauthorized",
        ));
    }
    Ok(())
}

async fn run<T: Send + 'static>(
    window: WebviewWindow,
    state: Desktop<'_>,
    work: impl FnOnce(&DesktopState) -> Result<T, CoreError> + Send + 'static,
) -> Result<T, CoreError> {
    authorize(&window)?;
    let desktop = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || work(&desktop))
        .await
        .map_err(|_| CoreError::internal("后台操作异常退出"))?
}

#[tauri::command]
pub async fn providers_list(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Vec<Provider>, CoreError> {
    run(window, state, |desktop| desktop.workspace.list_providers()).await
}
#[tauri::command]
pub async fn providers_save(
    window: WebviewWindow,
    state: Desktop<'_>,
    draft: ProviderDraft,
    expected_version: u64,
) -> Result<Provider, CoreError> {
    run(window, state, move |desktop| {
        desktop.workspace.save_provider(draft, expected_version)
    })
    .await
}
#[tauri::command]
pub async fn credentials_list(
    window: WebviewWindow,
    state: Desktop<'_>,
    provider_id: String,
) -> Result<Vec<Credential>, CoreError> {
    run(window, state, move |desktop| {
        desktop.workspace.list_credentials(&provider_id)
    })
    .await
}
#[tauri::command]
pub async fn credentials_add(
    window: WebviewWindow,
    state: Desktop<'_>,
    provider_id: String,
    label: String,
    secret: String,
) -> Result<Credential, CoreError> {
    run(window, state, move |desktop| {
        desktop
            .workspace
            .add_credential(&provider_id, &label, secret)
    })
    .await
}
#[tauri::command]
pub async fn credentials_replace(
    window: WebviewWindow,
    state: Desktop<'_>,
    credential_id: String,
    secret: String,
    expected_version: u64,
) -> Result<Credential, CoreError> {
    run(window, state, move |desktop| {
        desktop
            .workspace
            .replace_credential(&credential_id, secret, expected_version)
    })
    .await
}
#[tauri::command]
pub async fn credentials_select(
    window: WebviewWindow,
    state: Desktop<'_>,
    provider_id: String,
    credential_id: String,
) -> Result<(), CoreError> {
    run(window, state, move |desktop| {
        desktop
            .workspace
            .select_credential(&provider_id, &credential_id)
    })
    .await
}
#[tauri::command]
pub async fn models_list(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Vec<Model>, CoreError> {
    run(window, state, |desktop| desktop.workspace.list_models()).await
}
#[tauri::command]
pub async fn models_save(
    window: WebviewWindow,
    state: Desktop<'_>,
    draft: ModelDraft,
    expected_version: u64,
) -> Result<Model, CoreError> {
    run(window, state, move |desktop| {
        desktop.workspace.save_model(draft, expected_version)
    })
    .await
}

#[tauri::command]
pub async fn providers_delete(
    window: WebviewWindow,
    state: Desktop<'_>,
    provider_id: String,
) -> Result<(), CoreError> {
    run(window, state, move |desktop| {
        desktop.workspace.delete_provider(&provider_id)
    })
    .await
}
#[tauri::command]
pub async fn credentials_rename(
    window: WebviewWindow,
    state: Desktop<'_>,
    credential_id: String,
    label: String,
    expected_version: u64,
) -> Result<Credential, CoreError> {
    run(window, state, move |desktop| {
        desktop
            .workspace
            .rename_credential(&credential_id, &label, expected_version)
    })
    .await
}

/// 停用 / 重新启用一个 Key。停用当前正在用的那个会被核心拒绝，并把原因带回来。
#[tauri::command]
pub async fn credentials_set_disabled(
    window: WebviewWindow,
    state: Desktop<'_>,
    credential_id: String,
    disabled: bool,
    expected_version: u64,
) -> Result<Credential, CoreError> {
    run(window, state, move |desktop| {
        desktop
            .workspace
            .set_credential_disabled(&credential_id, disabled, expected_version)
    })
    .await
}

#[tauri::command]
pub async fn credentials_delete(
    window: WebviewWindow,
    state: Desktop<'_>,
    credential_id: String,
) -> Result<(), CoreError> {
    run(window, state, move |desktop| {
        desktop.workspace.delete_credential(&credential_id)
    })
    .await
}
#[tauri::command]
pub async fn models_delete(
    window: WebviewWindow,
    state: Desktop<'_>,
    model_id: String,
    expected_version: u64,
) -> Result<(), CoreError> {
    run(window, state, move |desktop| {
        desktop.workspace.delete_model(&model_id, expected_version)
    })
    .await
}

/// 只读检测 Codex 实例；不安装、不写入、不读取任何凭据。
#[tauri::command]
pub async fn instances_detect(
    window: WebviewWindow,
    state: Desktop<'_>,
    explicit_path: Option<String>,
) -> Result<Vec<CodexInstance>, CoreError> {
    run(window, state, move |desktop| {
        desktop.detect_instances(explicit_path)
    })
    .await
}

/// 读取并脱敏当前配置，供差异页与冲突提示使用。
#[tauri::command]
pub async fn config_inspect(
    window: WebviewWindow,
    state: Desktop<'_>,
    instance_id: String,
) -> Result<InspectResult, CoreError> {
    run(window, state, move |desktop| {
        let instance = desktop.instance(&instance_id)?;
        let snapshot = ConfigSnapshot::read(&instance.config_file)?;
        let managed_fields = MANAGED_KEYS
            .iter()
            .filter(|key| snapshot.managed_value(key).is_some())
            .map(|key| (*key).to_owned())
            .collect();
        Ok(InspectResult {
            instance_id: instance.id.as_str().to_owned(),
            config_path: instance.config_file,
            redacted_preview: snapshot.redacted_preview(),
            managed_fields,
            conflicts: snapshot.foreign_managers(),
        })
    })
    .await
}

/// 生成应用计划。只写目录草稿，绝不修改 Codex 配置。
#[tauri::command]
pub async fn apply_plan(
    window: WebviewWindow,
    state: Desktop<'_>,
    instance_id: String,
    default_alias: Option<String>,
) -> Result<ApplyPlan, CoreError> {
    run(window, state, move |desktop| {
        let instance = desktop.instance(&instance_id)?;
        // 共存模式下写的是托管 profile（应用数据目录里的第二个 CODEX_HOME），
        // 用户真实的 ~/.codex 不参与——用的是同一条管线，换的只是目标实例。
        if desktop.apply.coexist_enabled()? {
            desktop
                .apply
                .plan_coexist(&instance, default_alias.as_deref())
        } else {
            desktop
                .apply
                .plan_apply(&instance, default_alias.as_deref())
        }
    })
    .await
}

/// 提交应用计划。必须携带计划摘要；CAS 失败即转入冲突。
#[tauri::command]
pub async fn apply_execute(
    window: WebviewWindow,
    state: Desktop<'_>,
    plan_id: String,
    plan_hash: String,
    idempotency_key: String,
) -> Result<ExecuteResult, CoreError> {
    run(window, state, move |desktop| {
        // 本机网关没起来就绝不能写配置。
        //
        // 写进 Codex 的 base_url 是 `127.0.0.1:<port>/i/<实例>/c/<新版本>/v1`；写下去之后
        // Codex 的每一次请求都打在这个前缀上。网关不在时它必然全部失败，而用户看到的是
        // 「已应用」——配置被改坏了，却没人告诉他。
        //
        // 最典型的成因是**开了第二个 Switchelp**：它绑不到端口、网关起不来，但它的 IPC 与
        // 写配置照常，于是它会把 Codex 指向一个只有它自己知道、而它又服务不了的版本。
        // 这里直接拦住，原因交给界面显示。
        if desktop.gateway().is_none() {
            let detail = match desktop.gateway_error() {
                Some(reason) => format!(
                    "本机网关未运行，现在写入会让 Codex 用不了这些模型：{reason}。\
                     最常见的原因是已经开着另一个 Switchelp 实例——一个进程只能占住网关端口。"
                ),
                None => "本机网关未运行，无法应用配置。".to_owned(),
            };
            // 不挂 recovery action：界面目前只渲染 `recompare` 一种恢复项，
            // 给一个不会变成按钮的动作等于承诺一个不存在的出口。
            return Err(
                CoreError::new(ErrorCode::Internal, "error.gatewayRequiredForApply")
                    .with_detail(detail),
            );
        }
        let operation_id = desktop
            .apply
            .execute_apply(&plan_id, &plan_hash, &idempotency_key)?;
        Ok(ExecuteResult { operation_id })
    })
    .await
}

/// 重启宿主的结果。
///
/// 字段以「确认」为准：只有真的观察到进程状态才为 true。不像早先那样只报告
/// 「命令发出去没有」——`osascript` 与 `open` 对不存在的应用都返回 0，退出码不携带信息，
/// 于是界面会在什么都没发生时也报成功。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRestart {
    pub app_path: String,
    /// 确认旧进程已经退出（或本来就没在运行）。
    pub quit_confirmed: bool,
    /// 优雅退出没成、最后是发信号结束的。界面要据此提醒未保存内容可能丢失。
    pub quit_forced: bool,
    /// 确认新进程已经起来。
    pub launched_confirmed: bool,
}

/// 重启探测到的宿主实例。
///
/// Codex 只在启动时读 `config.toml`：写完配置不重启，模型就不会出现在它的模型菜单里。
/// 路径**只从已检测到的实例里取**，渲染层传不了任意路径。
#[tauri::command]
pub async fn host_restart(
    window: WebviewWindow,
    state: Desktop<'_>,
    instance_id: String,
) -> Result<HostRestart, CoreError> {
    run(window, state, move |desktop| {
        restart_host(desktop, &instance_id)
    })
    .await
}

fn restart_host(desktop: &DesktopState, instance_id: &str) -> Result<HostRestart, CoreError> {
    let instance = desktop.instance(instance_id)?;
    let app_path = instance
        .app_path
        .clone()
        .ok_or_else(|| CoreError::validation("这个实例没有可重启的应用路径；请手动重开 Codex"))?;
    let platform = switch_core::platform::Platform::current();
    // 共存模式靠给宿主带上一组环境变量生效（`CODEX_CLI_PATH` 指向 bridge）。
    // 注入不了就**不重启**：普通重启会把宿主拉回纯原生，而界面还显示着共存已启用——
    // 那正是「看着正常、实则全错」。
    let mut env = if desktop.apply.coexist_enabled()? {
        switch_core::codex::coexist::launch_env(&desktop.app_data_dir(), &instance)?
    } else {
        Vec::new()
    };
    // 与共存无关，任何一次启动宿主都要带上。宿主到本机网关走的是回环地址，而它自己的
    // HTTP 客户端会把系统代理套上去（不读代理例外列表），代理到不了 127.0.0.1，
    // 用户看到的就是「502 Bad Gateway: Unknown error」。绕不过去的是客户端，不是我们，
    // 所以只能靠环境变量。
    env.extend(loopback_bypass_env(platform));
    let plan = switch_core::platform::restart_plan_with_env(platform, &app_path, &env);
    let process_name = switch_core::platform::host_process_name(platform, &app_path);
    // 退出与启动都要等到**观察到**结果为止。这条命令会阻塞几秒到二三十秒，
    // 界面那侧显示「重启中」，比立刻返回一个不可信的成功好。
    let outcome = switch_core::platform::restart_host(
        &switch_core::platform::SystemProcessProbe,
        &plan,
        &process_name,
        switch_core::platform::RestartTiming::default(),
    );
    Ok(HostRestart {
        app_path,
        quit_confirmed: outcome.quit_confirmed,
        quit_forced: outcome.quit_forced,
        launched_confirmed: outcome.launched_confirmed,
    })
}

/// 查询事务状态。界面据此显示“等待 Codex 重载”而不是“已加载”。
#[tauri::command]
pub async fn apply_status(
    window: WebviewWindow,
    state: Desktop<'_>,
    operation_id: String,
) -> Result<ApplyStatus, CoreError> {
    run(window, state, move |desktop| {
        status_of(desktop, &operation_id)
    })
    .await
}

/// 用户确认宿主是否已经加载新目录；未确认时停在等待状态。
#[tauri::command]
pub async fn apply_confirm_reload(
    window: WebviewWindow,
    state: Desktop<'_>,
    operation_id: String,
    loaded: bool,
) -> Result<ApplyStatus, CoreError> {
    run(window, state, move |desktop| {
        desktop.apply.confirm_reload(&operation_id, loaded)?;
        status_of(desktop, &operation_id)
    })
    .await
}

/// 自动补记宿主回执的结果。空数组表示「没有可确认的事务」，不是失败。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconciledReload {
    /// 本次被补记的 operation id；界面据此决定要不要重读数据。
    pub confirmed_operation_ids: Vec<String>,
}

/// 宿主进程在这次发布之后重新启动过，就把它记成回执。
///
/// 界面在**应用完成**、**窗口重新获得焦点**、**启动**三个时机各调一次：这三处覆盖了
/// 「宿主可能已经重启过、而我们还没记账」的全部时机。查不到宿主启动时间（没在运行、
/// 平台还不支持）时什么都不做，事务留在等待人工确认上——不用「大概重启过了」顶替回执。
#[tauri::command]
pub async fn apply_reconcile_reload(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<ReconciledReload, CoreError> {
    run(window, state, reconcile_host_reload).await
}

fn reconcile_host_reload(desktop: &DesktopState) -> Result<ReconciledReload, CoreError> {
    use switch_core::platform::{host_process_name, Platform, ProcessProbe, SystemProcessProbe};

    let platform = Platform::current();
    // 取探测结果里的第一个实例，与界面上「重启 Codex」用的是同一条选中规则——
    // 两边指向同一个宿主，否则会出现「重启了 A、按 B 的进程时间记账」。
    let host_started_at_unix = desktop
        .detect_instances(None)?
        .into_iter()
        .next()
        .and_then(|instance| instance.app_path)
        .and_then(|app_path| {
            let process_name = host_process_name(platform, &app_path);
            SystemProcessProbe.started_at_unix(&process_name)
        });
    Ok(ReconciledReload {
        confirmed_operation_ids: desktop.apply.reconcile_host_reload(host_started_at_unix)?,
    })
}

/// 共存模式的状态。
///
/// 「开关」与「事实」分开报：`enabled` 只说明我们记下的意图，`hostUnderBridge` 才是
/// 宿主此刻真的有没有跑在 bridge 上——后者由进程启动时间与 bridge 日志两个可观察事实算出，
/// 拿不到证据时是 `null`（无法确认），不是 `false`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoexistState {
    pub enabled: bool,
    /// bridge 有没有装好。没装好时 `bridgeDetail` 说明原因。
    pub bridge_ready: bool,
    pub bridge_detail: Option<String>,
    pub bridge_path: Option<String>,
    pub managed_home: String,
    pub managed_config_exists: bool,
    /// 宿主此刻是否跑在 bridge 上；`null` 表示无法确认。
    pub host_under_bridge: Option<bool>,
    /// 这个实例具不具备接管前提（有 CLI、没有硬阻塞）。
    pub ready: bool,
    pub blocked_reason: Option<String>,
}

/// 查共存模式状态。
#[tauri::command]
pub async fn coexist_status(
    window: WebviewWindow,
    state: Desktop<'_>,
    instance_id: String,
) -> Result<CoexistState, CoreError> {
    run(window, state, move |desktop| {
        let instance = desktop.instance(&instance_id)?;
        let app_data = desktop.app_data_dir();
        let enabled = desktop.apply.coexist_enabled()?;
        let (bridge_ready, bridge_detail, bridge_path) = match desktop.bridge() {
            Ok(path) => (true, None, Some(path.display().to_string())),
            Err(reason) => (false, Some(reason.to_owned()), None),
        };

        // 事实层：宿主进程什么时候起的，bridge 什么时候起的。
        let platform = switch_core::platform::Platform::current();
        let host_under_bridge = instance.app_path.as_deref().and_then(|app_path| {
            let process = switch_core::platform::host_process_name(platform, app_path);
            let started = switch_core::platform::ProcessProbe::started_at_unix(
                &switch_core::platform::SystemProcessProbe,
                &process,
            );
            switch_core::codex::coexist::running_under_bridge(
                switch_core::codex::coexist::last_start_unix(
                    &switch_core::codex::coexist::bridge_log(&app_data),
                ),
                started,
            )
        });

        let blocked_reason = if platform == switch_core::platform::Platform::Windows {
            Some("error.coexistUnsupportedPlatform".to_owned())
        } else if instance.cli_path.is_none() {
            Some("error.coexistNeedsCli".to_owned())
        } else {
            instance.blocked_reason_key.clone()
        };
        Ok(CoexistState {
            enabled,
            bridge_ready,
            bridge_detail,
            bridge_path,
            managed_home: switch_core::codex::coexist::managed_home(&app_data)
                .display()
                .to_string(),
            managed_config_exists: switch_core::codex::coexist::managed_config_file(&app_data)
                .exists(),
            host_under_bridge,
            ready: blocked_reason.is_none(),
            blocked_reason,
        })
    })
    .await
}

/// 开/关共存模式。
///
/// 打开时先做两件必须做的事，任何一件不成就整体失败（不允许「半个共存」）：
/// 1. 把之前写进原生配置的路由字段还原掉——共存模式下原生那根必须是干净的，
///    否则它自己也会读我们的目录，两根就变成了同一根；
/// 2. 确认随包分发的 bridge 真的装好了。
///
/// 关掉只改意图。托管 profile 留在原地：再打开时不必重新播种，也不会动用户任何东西。
#[tauri::command]
pub async fn coexist_set(
    window: WebviewWindow,
    state: Desktop<'_>,
    instance_id: String,
    enabled: bool,
) -> Result<CoexistState, CoreError> {
    run(window, state, move |desktop| {
        if enabled {
            let instance = desktop.instance(&instance_id)?;
            if switch_core::platform::Platform::current()
                == switch_core::platform::Platform::Windows
            {
                return Err(CoreError::new(
                    ErrorCode::CapabilityUnsupported,
                    "error.coexistUnsupportedPlatform",
                )
                .with_detail(
                    "共存模式目前只有 macOS 装配完整：Windows 上还没有注入路径。".to_owned(),
                ));
            }
            if let Err(reason) = desktop.bridge() {
                return Err(CoreError::new(ErrorCode::Internal, "error.bridgeMissing")
                    .with_detail(reason.to_owned()));
            }
            switch_core::codex::coexist::launch_env(&desktop.app_data_dir(), &instance)?;
            // 原生那份还带着我们的路由？先还原，再开共存。
            restore_native_if_needed(desktop, &instance)?;
        }
        desktop.apply.set_coexist_enabled(enabled)?;
        coexist_state_of(desktop, &instance_id)
    })
    .await
}

/// 用当前的原生配置重建托管 profile 的底子。
///
/// 「底子」指除路由以外的设置（插件、市场、项目信任）：它是开启共存时复制的那一份快照。
/// 用户在原生配置里加了东西之后想让托管那边也带上，就靠这个动作——下一次生成计划时
/// 会重新复制一份。
#[tauri::command]
pub async fn coexist_resync(
    window: WebviewWindow,
    state: Desktop<'_>,
    instance_id: String,
) -> Result<CoexistState, CoreError> {
    run(window, state, move |desktop| {
        if !desktop.apply.coexist_enabled()? {
            return Err(CoreError::validation(
                "共存模式没有开启，没有需要重建的托管 profile",
            ));
        }
        switch_core::codex::coexist::clear_managed_config(&desktop.app_data_dir())?;
        coexist_state_of(desktop, &instance_id)
    })
    .await
}

/// 原生配置里还有我们写的字段时，把它还原干净。
fn restore_native_if_needed(
    desktop: &DesktopState,
    instance: &CodexInstance,
) -> Result<(), CoreError> {
    let plan = desktop.apply.plan_restore(instance)?;
    if plan.changes.is_empty() {
        return Ok(());
    }
    let idempotency = format!("coexist-restore-{}", plan.plan_hash);
    desktop
        .apply
        .execute_restore(plan.id.as_str(), &plan.plan_hash, &idempotency)?;
    Ok(())
}

fn coexist_state_of(desktop: &DesktopState, instance_id: &str) -> Result<CoexistState, CoreError> {
    let instance = desktop.instance(instance_id)?;
    let app_data = desktop.app_data_dir();
    let (bridge_ready, bridge_detail, bridge_path) = match desktop.bridge() {
        Ok(path) => (true, None, Some(path.display().to_string())),
        Err(reason) => (false, Some(reason.to_owned()), None),
    };
    Ok(CoexistState {
        enabled: desktop.apply.coexist_enabled()?,
        bridge_ready,
        bridge_detail,
        bridge_path,
        managed_home: switch_core::codex::coexist::managed_home(&app_data)
            .display()
            .to_string(),
        managed_config_exists: switch_core::codex::coexist::managed_config_file(&app_data).exists(),
        host_under_bridge: None,
        ready: instance.cli_path.is_some(),
        blocked_reason: instance.blocked_reason_key.clone(),
    })
}

/// 生成还原计划。只撤销本工具写入且未被外部修改的字段。
#[tauri::command]
pub async fn restore_plan(
    window: WebviewWindow,
    state: Desktop<'_>,
    instance_id: String,
) -> Result<ApplyPlan, CoreError> {
    run(window, state, move |desktop| {
        let instance = desktop.instance(&instance_id)?;
        if desktop.apply.coexist_enabled()? {
            // 共存模式下我们从来没写过原生配置，没有可还原的东西。
            // 报「没有可还原的字段」而不是静默成功：静默成功会让用户以为原生被清过了。
            return Err(CoreError::new(
                ErrorCode::ValidationFailed,
                "error.coexistRestoreNotApplicable",
            )
            .with_detail(
                "共存模式下原生 config.toml 没有被改动过；要退出共存模式请用「与原生共存」的开关。"
                    .to_owned(),
            ));
        }
        desktop.apply.plan_restore(&instance)
    })
    .await
}

/// 提交还原计划。与应用共用同一套 CAS 与幂等规则。
#[tauri::command]
pub async fn restore_execute(
    window: WebviewWindow,
    state: Desktop<'_>,
    plan_id: String,
    plan_hash: String,
    idempotency_key: String,
) -> Result<ExecuteResult, CoreError> {
    run(window, state, move |desktop| {
        let operation_id = desktop
            .apply
            .execute_restore(&plan_id, &plan_hash, &idempotency_key)?;
        Ok(ExecuteResult { operation_id })
    })
    .await
}

/// 网关状态：本机网关是否在监听、服务了哪个目录版本。
#[tauri::command]
pub async fn gateway_status(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<GatewayReport, CoreError> {
    run(window, state, |desktop| {
        let system_proxy = desktop.system_proxy();
        Ok(match desktop.gateway() {
            Some(gateway) => {
                let status = gateway.status();
                GatewayReport {
                    running: status.running,
                    paused: status.paused,
                    port: status.port,
                    served: status.served,
                    revisions: status.revisions,
                    token_fingerprint: status.token_fingerprint,
                    error: None,
                    system_proxy,
                }
            }
            None => GatewayReport {
                running: false,
                paused: false,
                port: None,
                served: 0,
                revisions: Vec::new(),
                token_fingerprint: String::new(),
                error: desktop.gateway_error().map(str::to_owned),
                system_proxy,
            },
        })
    })
    .await
}

/// 平台与窗口策略，供界面按平台调整布局。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformReport {
    pub platform: String,
    pub titlebar_height: u32,
    pub leading_reserve: u32,
    /// 是否由系统绘制标题栏；为真时界面不需要自绘拖拽区。
    pub system_decorations: bool,
}

/// 备份列表与手动备份。备份可能含其他工具写入的密钥，因此默认只给遮罩预览。
#[tauri::command]
pub async fn backups_list(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Vec<BackupEntry>, CoreError> {
    run(window, state, |desktop| desktop.backups().list()).await
}

/// 手动备份一个实例的配置文件。
#[tauri::command]
pub async fn backups_create(
    window: WebviewWindow,
    state: Desktop<'_>,
    instance_id: String,
) -> Result<BackupEntry, CoreError> {
    run(window, state, move |desktop| {
        let instance = desktop.instance(&instance_id)?;
        let now = switch_core::diagnostics::now_rfc3339();
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or_default();
        let entry =
            desktop
                .backups()
                .create(std::path::Path::new(&instance.config_file), millis, &now)?;
        // 保留策略与自动备份共用：只保留最近若干份。
        let _ = desktop
            .backups()
            .prune(switch_core::codex::backup::DEFAULT_KEEP);
        Ok(entry)
    })
    .await
}

/// 备份的遮罩预览。原始内容不经过 IPC，避免明文密钥进入前端状态。
#[tauri::command]
pub async fn backups_preview(
    window: WebviewWindow,
    state: Desktop<'_>,
    backup_id: String,
) -> Result<String, CoreError> {
    run(window, state, move |desktop| {
        desktop.backups().read_masked(&backup_id)
    })
    .await
}

/// 恢复一份备份。
///
/// 会先把**当前**文件再备份一次，因此恢复本身也可回退。
/// 事务记录不跟着回退——之后可以重新生成差异。
#[tauri::command]
pub async fn backups_restore(
    window: WebviewWindow,
    state: Desktop<'_>,
    backup_id: String,
) -> Result<String, CoreError> {
    run(window, state, move |desktop| {
        let text = desktop.backups().read(&backup_id)?;
        let entry = desktop
            .backups()
            .list()?
            .into_iter()
            .find(|item| item.id == backup_id)
            .ok_or_else(|| CoreError::not_found("备份"))?;
        let target = std::path::PathBuf::from(&entry.source_path);
        let now = switch_core::diagnostics::now_rfc3339();
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or_default();
        // 先给当前状态留退路，再覆盖。
        if target.exists() {
            desktop.backups().create(&target, millis, &now)?;
        }
        switch_core::codex::config::write_atomic(&target, &text)?;
        Ok(target.display().to_string())
    })
    .await
}

// ---------------------------------------------------------------- 应用内更新

/// 更新清单与发布页都从这一个仓库派生。只写一处，免得两处地址漂移。
const UPDATE_REPO: &str = "nexsjournal/switchelp-macapp";
/// 安装完成后的标记文件：下次启动读到它就报一句「已更新到 x.y.z」。
const UPDATE_MARKER: &str = "update-pending.json";
/// 进度事件名。前端在弹窗打开期间监听它。
const UPDATE_PROGRESS_EVENT: &str = "update://progress";

/// 更新检查结果。`latest` 的含义取决于 `has_update`：有更新时是能装的新版本，
/// 没有时就是当前版本。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReport {
    pub current: String,
    pub latest: Option<String>,
    pub has_update: bool,
    /// 新版本的发布说明（Markdown，来自清单里的 notes）。可能缺省。
    pub notes: Option<String>,
    pub release_url: Option<String>,
    /// 发布日期，形如 `2026-09-21`。
    pub published_at: Option<String>,
    /// 查询失败的原因。有值时 `latest` 为空，界面不得显示成“已是最新”。
    pub error: Option<String>,
}

/// 下载进度。`total` 为空表示上游没给长度（进度条退化成不确定态）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    /// `download` 或 `install`。
    pub phase: &'static str,
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// 把插件的错误翻成一句人话。
///
/// 只描述**发生了什么**，不猜原因、不给建议：网络失败就是「连不上更新源」，
/// 而不是「请检查网络设置」。原始错误码附在后面，便于对着日志排查。
fn describe_update_error(error: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match error {
        Error::Reqwest(inner) if inner.is_timeout() => {
            "连不上更新源：请求超时（网络或代理）".to_owned()
        }
        Error::Reqwest(_) => "连不上更新源（网络或代理）".to_owned(),
        Error::ReleaseNotFound => {
            "更新源里没有可读的更新清单（Release 缺少 latest.json）".to_owned()
        }
        Error::TargetNotFound(target) => format!("这次发布没有 {target} 对应的更新包"),
        Error::TargetsNotFound(targets) => format!("这次发布没有适合本机的更新包（{targets:?}）"),
        Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) => {
            "安装包签名校验没通过，已放弃安装".to_owned()
        }
        Error::AuthenticationFailed => "系统授权被取消，安装没有进行".to_owned(),
        Error::EmptyEndpoints => "没有配置更新源".to_owned(),
        Error::UnsupportedArch | Error::UnsupportedOs => "当前平台不支持应用内更新".to_owned(),
        other => format!("更新失败：{other}"),
    }
}

fn update_error(error: &tauri_plugin_updater::Error) -> CoreError {
    // 签名不对不是「重试一下就好」：归为不可重试的校验失败，其余归为可重试的内部错误。
    let code = match error {
        tauri_plugin_updater::Error::Minisign(_)
        | tauri_plugin_updater::Error::Base64(_)
        | tauri_plugin_updater::Error::SignatureUtf8(_) => ErrorCode::ValidationFailed,
        _ => ErrorCode::Internal,
    };
    CoreError::new(code, "error.updateInstallFailed").with_detail(describe_update_error(error))
}

/// 一次更新检查。不下载任何东西。
async fn update_report(window: &WebviewWindow) -> UpdateReport {
    let current = window.package_info().version.to_string();
    let mut report = UpdateReport {
        current: current.clone(),
        latest: None,
        has_update: false,
        notes: None,
        release_url: None,
        published_at: None,
        error: None,
    };
    let updater = match window.updater() {
        Ok(updater) => updater,
        Err(error) => {
            report.error = Some(describe_update_error(&error));
            return report;
        }
    };
    match updater.check().await {
        Ok(Some(update)) => {
            report.has_update = true;
            report.release_url = Some(format!(
                "https://github.com/{UPDATE_REPO}/releases/tag/v{}",
                update.version
            ));
            report.published_at = update.date.map(|date| date.date().to_string());
            report.notes = update.body.clone();
            report.latest = Some(update.version.clone());
        }
        // 没有更新：`latest` 回填当前版本，界面据此说「已是最新」。
        Ok(None) => report.latest = Some(current),
        Err(error) => report.error = Some(describe_update_error(&error)),
    }
    report
}

/// 检查更新。读一次更新源，**不下载、不安装**；下载安装走 `update_install`。
#[tauri::command]
pub async fn update_check(window: WebviewWindow) -> Result<UpdateReport, CoreError> {
    authorize(&window)?;
    Ok(update_report(&window).await)
}

/// 下载并安装更新。
///
/// 成功时**不返回**：装完立刻重启（旧的应用包已经被移走，继续跑一份磁盘上已经不存在的
/// bundle 不是个稳定状态）。前端只需要处理 Err——窗口会在重启时消失。
#[tauri::command]
pub async fn update_install(window: WebviewWindow, state: Desktop<'_>) -> Result<(), CoreError> {
    authorize(&window)?;
    let updater = window.updater().map_err(|error| update_error(&error))?;
    // 重新检查一次：从用户看到「有新版本」到点下按钮之间，更新源可能已经变了。
    let update = updater
        .check()
        .await
        .map_err(|error| update_error(&error))?
        .ok_or_else(|| {
            CoreError::new(ErrorCode::ValidationFailed, "error.updateNotAvailable")
                .with_detail("这次检查没有发现可安装的新版本，可能已经被更新过了".to_owned())
        })?;

    let handle = window.clone();
    let downloaded = Arc::new(AtomicU64::new(0));
    let bytes = {
        let handle = handle.clone();
        let downloaded = downloaded.clone();
        update
            .download(
                move |chunk: usize, total: Option<u64>| {
                    let done = downloaded.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
                    let _ = handle.emit(
                        UPDATE_PROGRESS_EVENT,
                        UpdateProgress {
                            phase: "download",
                            downloaded: done,
                            total,
                        },
                    );
                },
                || {},
            )
            .await
            .map_err(|error| update_error(&error))?
    };

    let total = downloaded.load(Ordering::Relaxed);
    let _ = handle.emit(
        UPDATE_PROGRESS_EVENT,
        UpdateProgress {
            phase: "install",
            downloaded: total,
            total: Some(total),
        },
    );
    // 解包与替换应用包是阻塞的文件操作（在 macOS 上是「移走旧的、放进新的」）。
    let installing = update.clone();
    let version = update.version.clone();
    tauri::async_runtime::spawn_blocking(move || installing.install(bytes))
        .await
        .map_err(|_| CoreError::internal("安装线程异常退出"))?
        .map_err(|error| update_error(&error))?;

    // 记账：重启后读它，告诉用户「已更新到 x.y.z」——安装本身发生在弹窗消失那一刻，
    // 不给一句话，用户只能靠版本号自己确认。
    let marker = state.inner().app_data_dir().join(UPDATE_MARKER);
    let _ = std::fs::write(&marker, serde_json::json!({"version": version}).to_string());

    window.app_handle().restart()
}

/// 取出并清掉「刚更新完」的标记。返回这次启动所对应的新版本号（没有则 null）。
#[tauri::command]
pub async fn update_take_result(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Option<String>, CoreError> {
    authorize(&window)?;
    let marker = state.inner().app_data_dir().join(UPDATE_MARKER);
    let Ok(text) = std::fs::read_to_string(&marker) else {
        return Ok(None);
    };
    // 读完就删：这条提示只该出现一次。
    let _ = std::fs::remove_file(&marker);
    Ok(serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .get("version")
                .and_then(|version| version.as_str())
                .map(str::to_owned)
        }))
}

/// 用系统浏览器打开发布页。安装失败时的兜底路径：用户自己去下载。
///
/// 地址只允许本仓库的发布页——这个命令会交给 shell 执行，放任意 URL 进来等于开了个后门；
/// 调用方也必须先过 authorize（和其余命令一样，非主窗口不得触发）。
#[tauri::command]
pub async fn update_open_release_page(window: WebviewWindow, url: String) -> Result<(), CoreError> {
    authorize(&window)?;
    let prefix = format!("https://github.com/{UPDATE_REPO}/releases");
    if !url.starts_with(&prefix) {
        return Err(
            CoreError::new(ErrorCode::ValidationFailed, "error.updateBadReleaseUrl")
                .with_detail(format!("只允许打开本仓库的发布页，收到的是：{url}")),
        );
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&url)
        .spawn()
        .map_err(|_| CoreError::internal("无法打开浏览器"))?;
    Ok(())
}

/// 平台信息。前端据此设置 `data-platform` 与窗口相关 CSS 变量。
#[tauri::command]
pub async fn platform_info(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<PlatformReport, CoreError> {
    run(window, state, |_desktop| {
        let platform = switch_core::platform::Platform::current();
        let chrome = switch_core::platform::window_chrome(platform);
        Ok(PlatformReport {
            platform: platform.as_str().to_owned(),
            titlebar_height: chrome.titlebar_height,
            leading_reserve: chrome.leading_reserve,
            system_decorations: chrome.system_decorations,
        })
    })
    .await
}

/// 暂停或继续接受新推理请求。在途请求不受影响。
#[tauri::command]
pub async fn gateway_set_paused(
    window: WebviewWindow,
    state: Desktop<'_>,
    paused: bool,
) -> Result<bool, CoreError> {
    run(window, state, move |desktop| {
        let gateway = desktop
            .gateway()
            .ok_or_else(|| CoreError::internal("本机网关未启动"))?;
        gateway.set_paused(paused);
        Ok(gateway.is_paused())
    })
    .await
}

/// 诊断事件列表。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticList {
    pub items: Vec<DiagnosticEvent>,
    pub next_cursor: Option<String>,
}

/// 诊断包导出结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub saved_path: String,
    pub bytes: usize,
}

/// 列出诊断事件。`level` 为空表示不过滤。
#[tauri::command]
pub async fn diagnostics_list(
    window: WebviewWindow,
    state: Desktop<'_>,
    level: Option<String>,
) -> Result<DiagnosticList, CoreError> {
    run(window, state, move |desktop| {
        let level = match level.as_deref() {
            Some("warning") => Some(LogLevel::Warning),
            Some("error") => Some(LogLevel::Error),
            Some("info") => Some(LogLevel::Info),
            _ => None,
        };
        Ok(DiagnosticList {
            items: desktop.diagnostics().list(level),
            next_cursor: None,
        })
    })
    .await
}

/// 清空本工具自己的诊断事件。不影响 Codex 历史与配置事务记录。
#[tauri::command]
pub async fn diagnostics_clear(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<usize, CoreError> {
    run(window, state, |desktop| Ok(desktop.diagnostics().clear())).await
}

/// 诊断包预览：列出包含项、排除项与准确体积，保存前先让用户看清楚。
#[tauri::command]
pub async fn diagnostics_preview(
    window: WebviewWindow,
    state: Desktop<'_>,
    scopes: Vec<String>,
) -> Result<ExportPreview, CoreError> {
    run(window, state, move |desktop| {
        switch_core::diagnostics::preview_export(
            &desktop.diagnostics(),
            &scopes,
            env!("CARGO_PKG_VERSION"),
        )
    })
    .await
}

/// 导出诊断包到应用数据目录，返回真实保存路径。
#[tauri::command]
pub async fn diagnostics_export(
    window: WebviewWindow,
    state: Desktop<'_>,
    scopes: Vec<String>,
) -> Result<ExportResult, CoreError> {
    run(window, state, move |desktop| {
        let stamp = switch_core::diagnostics::now_rfc3339().replace(':', "-");
        let path = desktop
            .exports_dir()
            .join(format!("diagnostics-{stamp}.json"));
        let (saved_path, bytes) = switch_core::diagnostics::write_export(
            &desktop.diagnostics(),
            &scopes,
            env!("CARGO_PKG_VERSION"),
            &path,
        )?;
        Ok(ExportResult {
            saved_path: saved_path.display().to_string(),
            bytes,
        })
    })
    .await
}

/// 读取上游模型列表。结果只作为发现值，不覆盖用户手工填写的显示名。
#[tauri::command]
pub async fn models_discover(
    window: WebviewWindow,
    state: Desktop<'_>,
    provider_id: String,
    credential_id: String,
) -> Result<Vec<DiscoveredModel>, CoreError> {
    run(window, state, move |desktop| {
        let repository = desktop.workspace.repository.clone();
        let provider = repository
            .get_provider(&ProviderId::new(&provider_id))?
            .ok_or_else(|| CoreError::not_found("供应商"))?;
        let saved: Vec<String> = repository
            .list_models()?
            .into_iter()
            .filter(|model| model.provider_id.as_str() == provider_id)
            .map(|model| model.upstream_id)
            .collect();
        let secret = desktop.workspace.resolve_secret(&credential_id)?;
        let models = fetch_models(&provider.endpoint, secret.expose(), &saved)?;
        // 弹窗里预览的名字就是添加之后菜单里显示的名字：前缀规则只写在域层一处。
        let models: Vec<DiscoveredModel> = models
            .into_iter()
            .map(|model| DiscoveredModel {
                display_name: qualified_display_name(&provider.name, &model.display_name),
                ..model
            })
            .collect();

        // 记入发现层：用户覆盖过的显示名保持不变（R07）。
        let pairs: Vec<(String, String)> = models
            .iter()
            .map(|model| (model.upstream_id.clone(), model.display_name.clone()))
            .collect();
        desktop.workspace.record_discovery(&provider_id, &pairs)?;
        desktop.diagnostics().record(
            DiagnosticEvent::new(
                switch_core::diagnostics::now_rfc3339(),
                LogLevel::Info,
                "discovery",
                provider.name.clone(),
                "result.modelsDiscovered",
            )
            .with_metadata("provider_id", provider_id.clone())
            .with_metadata("byte_size", models.len().to_string()),
        );
        Ok(models)
    })
    .await
}

/// 连接与模型探测。`probe_id` 由界面提供，否则取消命令来不及命中。
#[tauri::command]
pub async fn probes_start(
    window: WebviewWindow,
    state: Desktop<'_>,
    probe_id: Option<String>,
    provider_id: String,
    credential_id: String,
    model_id: Option<String>,
    include_generate: bool,
) -> Result<ProbeReport, CoreError> {
    run(window, state, move |desktop| {
        let repository = desktop.workspace.repository.clone();
        let provider = repository
            .get_provider(&ProviderId::new(&provider_id))?
            .ok_or_else(|| CoreError::not_found("供应商"))?;
        let model = match model_id.as_deref() {
            Some(id) => Some(
                repository
                    .get_model(&ModelId::new(id))?
                    .ok_or_else(|| CoreError::not_found("模型"))?,
            ),
            None => None,
        };
        let protocol = model
            .as_ref()
            .and_then(|model| model.protocol_override)
            .unwrap_or(provider.protocol);
        let target = ProbeTarget {
            provider_id: provider_id.clone(),
            credential_id: credential_id.clone(),
            model_id: model_id.clone(),
            upstream_id: model.as_ref().map(|model| model.upstream_id.clone()),
            label: model
                .as_ref()
                .map(|model| model.display_name.clone())
                .unwrap_or_else(|| provider.name.clone()),
        };
        let plan = ProbePlan {
            protocol,
            include_generate,
        };
        let probe_id = probe_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(uuid_like);
        let secret = desktop.workspace.resolve_secret(&credential_id)?;

        let report = desktop.probes().run(
            &probe_id,
            &provider.endpoint,
            &target,
            &plan,
            secret.expose(),
        );

        let level = if report.passed() {
            LogLevel::Info
        } else {
            LogLevel::Warning
        };
        desktop.diagnostics().record(
            DiagnosticEvent::new(
                switch_core::diagnostics::now_rfc3339(),
                level,
                "probe",
                report.target_label.clone(),
                "result.probeFinished",
            )
            .with_metadata("provider_id", provider_id)
            .with_metadata("model_id", model_id.unwrap_or_default())
            .with_metadata("byte_size", report.stages.len().to_string()),
        );
        Ok(report)
    })
    .await
}

/// 取消探测。已经进入的阻塞请求不会被中断，但后续阶段不再执行。
#[tauri::command]
pub async fn probes_cancel(
    window: WebviewWindow,
    state: Desktop<'_>,
    probe_id: String,
) -> Result<bool, CoreError> {
    run(window, state, move |desktop| {
        Ok(desktop.probes().cancel(&probe_id))
    })
    .await
}

/// 不引入额外依赖的随机标识，供未指定 probeId 的调用方使用。
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("probe_{nanos:x}")
}

fn status_of(desktop: &DesktopState, operation_id: &str) -> Result<ApplyStatus, CoreError> {
    let state = desktop.apply.status(operation_id)?;
    Ok(ApplyStatus {
        operation_id: state.operation.id.as_str().to_owned(),
        open: !state.finished(),
        events: state.operation.events.clone(),
    })
}

/* ------------------------------------------------------------------ 工具管理 */

/// 当前界面语言，用于取工具展示名。前端传的是完整 locale（`zh-Hans` / `en`）。
fn locale_of(value: Option<String>) -> String {
    match value.as_deref() {
        Some(locale) if locale.starts_with("zh") => "zh-Hans".to_owned(),
        _ => "en".to_owned(),
    }
}

fn now_seconds() -> i64 {
    switch_core::time_now()
}

/// 工具清单与状态。`refresh = false` 时复用未过期的探测结论。
#[tauri::command]
pub async fn tools_state(
    window: WebviewWindow,
    state: Desktop<'_>,
    locale: Option<String>,
    refresh: bool,
) -> Result<Vec<switch_core::toolhub::ToolState>, CoreError> {
    run(window, state, move |desktop| {
        let locale = locale_of(locale);
        desktop.tools()?.list(&locale, refresh, now_seconds())
    })
    .await
}

/// 强制重探一个工具。
#[tauri::command]
pub async fn tools_probe(
    window: WebviewWindow,
    state: Desktop<'_>,
    tool_id: String,
    locale: Option<String>,
) -> Result<switch_core::toolhub::ToolState, CoreError> {
    run(window, state, move |desktop| {
        let locale = locale_of(locale);
        desktop.tools()?.probe_one(&tool_id, &locale, now_seconds())
    })
    .await
}

/// 支持安装技能的目标工具及其技能根目录。
#[tauri::command]
pub async fn tools_skill_targets(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Vec<switch_core::toolhub::SkillTarget>, CoreError> {
    run(window, state, |desktop| {
        Ok(desktop.tools()?.skill_targets())
    })
    .await
}

/* ------------------------------------------------------------------ 插件中心 */

#[tauri::command]
pub async fn plugins_sources(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Vec<switch_core::plugins::SourceSummary>, CoreError> {
    run(window, state, |desktop| desktop.plugins().sources()).await
}

#[tauri::command]
pub async fn plugins_add_source(
    window: WebviewWindow,
    state: Desktop<'_>,
    repo: String,
) -> Result<Vec<switch_core::plugins::SourceSummary>, CoreError> {
    run(window, state, move |desktop| {
        desktop.plugins().add_source(&repo)
    })
    .await
}

#[tauri::command]
pub async fn plugins_remove_source(
    window: WebviewWindow,
    state: Desktop<'_>,
    repo: String,
) -> Result<Vec<switch_core::plugins::SourceSummary>, CoreError> {
    run(window, state, move |desktop| {
        desktop.plugins().remove_source(&repo)
    })
    .await
}

/// 浏览一个来源的技能目录。这一步会联网，失败原因原样返回给界面。
#[tauri::command]
pub async fn plugins_browse(
    window: WebviewWindow,
    state: Desktop<'_>,
    repo: String,
) -> Result<switch_core::plugins::RepoCatalog, CoreError> {
    run(window, state, move |desktop| {
        desktop.plugins().browse(&repo, now_seconds())
    })
    .await
}

/// 生成安装计划。不写文件——界面据此展示将写入什么、哪里会冲突。
#[tauri::command]
pub async fn plugins_preview(
    window: WebviewWindow,
    state: Desktop<'_>,
    request: switch_core::plugins::InstallRequest,
) -> Result<switch_core::plugins::InstallPreview, CoreError> {
    run(window, state, move |desktop| {
        desktop.plugins().preview(&request)
    })
    .await
}

#[tauri::command]
pub async fn plugins_install(
    window: WebviewWindow,
    state: Desktop<'_>,
    request: switch_core::plugins::InstallRequest,
) -> Result<switch_core::plugins::InstallReport, CoreError> {
    run(window, state, move |desktop| {
        desktop.plugins().install(&request, now_seconds())
    })
    .await
}

#[tauri::command]
pub async fn plugins_installed(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Vec<switch_core::plugins::SkillRecord>, CoreError> {
    run(window, state, |desktop| desktop.plugins().installed()).await
}

#[tauri::command]
pub async fn plugins_check_updates(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Vec<switch_core::plugins::UpdateInfo>, CoreError> {
    run(window, state, |desktop| desktop.plugins().check_updates()).await
}

/// 启用 / 禁用某个技能。实现是给技能目录改名，界面必须说明这一点。
#[tauri::command]
pub async fn plugins_set_enabled(
    window: WebviewWindow,
    state: Desktop<'_>,
    skill_id: String,
    target_tool: String,
    enabled: bool,
) -> Result<switch_core::plugins::SkillRecord, CoreError> {
    run(window, state, move |desktop| {
        desktop
            .plugins()
            .set_enabled(&skill_id, &target_tool, enabled)
    })
    .await
}

#[tauri::command]
pub async fn plugins_uninstall(
    window: WebviewWindow,
    state: Desktop<'_>,
    skill_id: String,
    targets: Vec<String>,
) -> Result<Vec<switch_core::plugins::UninstallOutcome>, CoreError> {
    run(window, state, move |desktop| {
        desktop.plugins().uninstall(&skill_id, &targets)
    })
    .await
}

/* ------------------------------------------------------------------ 内容中心 */

#[tauri::command]
pub async fn content_sources(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<Vec<switch_core::content::FeedSource>, CoreError> {
    run(window, state, |desktop| desktop.content().sources()).await
}

#[tauri::command]
pub async fn content_save_source(
    window: WebviewWindow,
    state: Desktop<'_>,
    draft: switch_core::content::FeedSourceDraft,
) -> Result<switch_core::content::FeedSource, CoreError> {
    run(window, state, move |desktop| {
        desktop.content().save_source(draft, now_seconds())
    })
    .await
}

#[tauri::command]
pub async fn content_delete_source(
    window: WebviewWindow,
    state: Desktop<'_>,
    source_id: String,
) -> Result<(), CoreError> {
    run(window, state, move |desktop| {
        desktop.content().delete_source(&source_id)
    })
    .await
}

#[tauri::command]
pub async fn content_items(
    window: WebviewWindow,
    state: Desktop<'_>,
    source_id: Option<String>,
    lang: Option<String>,
    limit: usize,
    offset: usize,
) -> Result<Vec<switch_core::content::FeedItem>, CoreError> {
    run(window, state, move |desktop| {
        desktop
            .content()
            .items(source_id.as_deref(), lang.as_deref(), limit, offset)
    })
    .await
}

/// 抓取。`sourceId` 指定单个源（手动刷新那一个）；`force` 忽略到期时间。
#[tauri::command]
pub async fn content_refresh(
    window: WebviewWindow,
    state: Desktop<'_>,
    source_id: Option<String>,
    force: bool,
) -> Result<switch_core::content::RefreshReport, CoreError> {
    run(window, state, move |desktop| {
        desktop
            .content()
            .refresh(source_id.as_deref(), force, now_seconds())
    })
    .await
}

#[tauri::command]
pub async fn content_status(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<switch_core::content::ContentStatus, CoreError> {
    run(window, state, |desktop| {
        desktop.content().status(now_seconds())
    })
    .await
}

/// GitHub 令牌是否已配置。**不返回令牌本身**。
#[tauri::command]
pub async fn content_github_token_status(
    window: WebviewWindow,
    state: Desktop<'_>,
) -> Result<bool, CoreError> {
    run(window, state, |desktop| {
        Ok(desktop.github_token().is_some())
    })
    .await
}

/// 写入或清除 GitHub 令牌。令牌只进系统凭据库。
#[tauri::command]
pub async fn content_set_github_token(
    window: WebviewWindow,
    state: Desktop<'_>,
    token: Option<String>,
) -> Result<bool, CoreError> {
    run(window, state, move |desktop| {
        match token.filter(|value| !value.trim().is_empty()) {
            Some(token) => {
                desktop
                    .vault()
                    .store(switch_core::content::GITHUB_TOKEN_REF, token.trim())?;
                Ok(true)
            }
            None => {
                desktop
                    .vault()
                    .delete(switch_core::content::GITHUB_TOKEN_REF)?;
                Ok(false)
            }
        }
    })
    .await
}
