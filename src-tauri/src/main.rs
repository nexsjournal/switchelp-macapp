#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;

use std::sync::Arc;
use switch_core::{
    application::{ApplyService, GatewayLayout, SystemClock, WorkspaceService},
    codex::{backup::BackupStore, config::AUTH_HELPER_INSTANCE},
    credentials::{SecretVault, SystemVault},
    diagnostics::DiagnosticLog,
    domain::ids::InstanceId,
    gateway::{
        self, helper_path, install_auth_helper, Gateway, GatewayConfig, GatewayRouter, GatewayToken,
    },
    storage::{OperationStore, Repository, SqliteOperationStore, SqliteRepository},
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager,
};

use state::DesktopState;

/// 本机网关的凭据 helper 路径。
///
/// Codex 以 `<command> --instance <固定值>` 调用它并读取输出作为本机令牌，
/// 上游 Key 因此永不写入 `config.toml`。路径必须与写进配置里的完全一致，
/// 否则宿主调用一个不存在的可执行文件。
fn auth_helper(directory: &std::path::Path) -> String {
    helper_path(directory).display().to_string()
}

/// 组装本机网关。绑定失败不致命：状态里保留原因，界面据此显示“网关未启动”。
///
/// 每次启动都重新生成令牌并覆盖 helper，上一次运行的令牌随之失效。
fn start_gateway(
    directory: &std::path::Path,
    repository: Arc<dyn Repository>,
    vault: Arc<dyn SecretVault>,
    router: Arc<GatewayRouter>,
    diagnostics: Arc<DiagnosticLog>,
) -> Result<Arc<Gateway>, switch_core::CoreError> {
    let token = GatewayToken::generate();
    let gateway = Arc::new(Gateway::new(
        repository,
        vault,
        router,
        GatewayConfig {
            instance_id: InstanceId::new(AUTH_HELPER_INSTANCE),
            token: token.clone(),
            port: gateway::DEFAULT_PORT,
            timeouts: Default::default(),
            diagnostics,
        },
    ));
    gateway.bind()?;
    // 只有取得端口后才能替换 helper 令牌；第二个启动失败的进程不能使已有网关失联。
    install_auth_helper(directory, AUTH_HELPER_INSTANCE, &token)?;
    gateway.spawn()?;
    Ok(gateway)
}

/// 托盘菜单。
///
/// 关闭窗口按设计**隐藏到托盘**而不是退出（见 `on_window_event`）：托盘常驻，
/// 退出只从托盘菜单走。托盘本身的价值是状态可见 + 快速回到窗口，以及一个明确的
/// 退出出口——正在使用本机网关的第三方模型会随进程退出而断开。
fn install_tray(
    app: &tauri::AppHandle,
    gateway_running: bool,
    port: Option<u16>,
) -> tauri::Result<()> {
    let status = match (gateway_running, port) {
        (true, Some(port)) => format!("网关运行中 · 127.0.0.1:{port}"),
        _ => "网关未启动".to_owned(),
    };
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    // 状态项只读：它是信息，不是命令。
    let status_item = MenuItem::with_id(app, "status", &status, false, None::<&str>)?;
    // 暂停只拦新请求：在途请求继续跑完，符合“不默认中断正在生成的任务”。
    let pause = MenuItem::with_id(app, "pause", "暂停新请求", true, None::<&str>)?;
    let open_codex = MenuItem::with_id(app, "open_codex", "打开 Codex", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Switchelp", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &status_item,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &open_codex,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("gptswitch")
        .menu(&menu)
        .tooltip(status.as_str())
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "pause" => {
                if let Some(state) = app.try_state::<Arc<DesktopState>>() {
                    if let Some(gateway) = state.gateway() {
                        let next = !gateway.is_paused();
                        gateway.set_paused(next);
                        // 菜单文字必须跟着状态走，否则用户会以为点了没反应。
                        if let Some(item) = app.menu().and_then(|menu| menu.get("pause")) {
                            if let Some(item) = item.as_menuitem() {
                                let _ = item.set_text(if next {
                                    "继续接受新请求"
                                } else {
                                    "暂停新请求"
                                });
                            }
                        }
                    }
                }
            }
            "open_codex" => {
                // 只在本机打开宿主应用；不安装、不修改它的配置。
                open_host_app();
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// 打开宿主应用。平台差异集中在这里，不在事件回调里写 `cfg`。
fn open_host_app() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .args(["-a", "ChatGPT"])
        .spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", "ChatGPT"])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = std::process::Command::new("chatgpt").spawn();
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let mut directory = app.path().app_data_dir()?;
            // 开发验证使用隔离目录，发行包不读取此覆盖变量。
            #[cfg(debug_assertions)]
            if let Some(value) = std::env::var_os("GPTSWITCH_TEST_DATA_DIR") {
                directory = std::path::PathBuf::from(value);
            }
            std::fs::create_dir_all(&directory)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
            }
            let db_path = directory.join("metadata.sqlite");
            // 仓库与事务记录共用同一个库文件；两个连接各自持有自己的迁移入口。
            let repository: Arc<dyn Repository> = Arc::new(SqliteRepository::open(&db_path)?);
            let operations: Arc<dyn OperationStore> =
                Arc::new(SqliteOperationStore::open(&db_path)?);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600))?;
            }
            let vault: Arc<dyn SecretVault> = Arc::new(SystemVault::new("app.gptswitch.desktop")?);
            let workspace = Arc::new(WorkspaceService::new(repository.clone(), vault.clone()));
            // 路由注册表必须由配置事务与网关共享：应用成功即发布，
            // 网关立刻按新目录版本服务；两个实例各建一个会让网关永远看不到路由。
            let router = Arc::new(GatewayRouter::new());
            let backups = Arc::new(BackupStore::new(&directory));
            let apply = Arc::new(
                ApplyService::new(
                    repository.clone(),
                    operations,
                    router.clone(),
                    GatewayLayout {
                        app_data_dir: directory.clone(),
                        port: gateway::DEFAULT_PORT,
                        auth_helper: auth_helper(&directory),
                        base_instructions: "通过 Switchelp 本机网关访问第三方模型。".to_owned(),
                    },
                    Arc::new(SystemClock),
                )
                // 提交前自动备份：写用户配置之前先留原样副本。
                .with_backups(backups.clone()),
            );
            // 未完成事务在下一次启动时按记录判定恢复；窗口重建不新建事务。
            for report in apply.startup_recovery()? {
                eprintln!(
                    "Switchelp 启动恢复：operation={} instance={} applied={}",
                    report.operation_id, report.instance_id, report.applied
                );
            }
            // 网关与界面共用一份诊断日志：网关写，界面读。
            let diagnostics = Arc::new(DiagnosticLog::default());
            let gateway = start_gateway(&directory, repository, vault, router, diagnostics.clone());
            if let Err(error) = &gateway {
                // 端口被占用时不能假装在跑：状态里保留原因，界面显示“网关未启动”。
                eprintln!(
                    "Switchelp 网关未启动：{} {:?}",
                    error.message_key, error.safe_details
                );
            }
            let (tray_running, tray_port) = match &gateway {
                Ok(gateway) => (true, gateway.local_addr().map(|address| address.port())),
                Err(_) => (false, None),
            };
            // 托盘失败不应阻止应用启动：它只是入口，不是功能本体。
            if let Err(error) = install_tray(app.handle(), tray_running, tray_port) {
                eprintln!("Switchelp 托盘未创建：{error}");
            }
            let home = app.path().home_dir()?;
            app.manage(Arc::new(DesktopState::new(
                workspace,
                apply,
                home,
                directory.clone(),
                gateway,
                diagnostics,
                backups,
            )));
            Ok(())
        })
        // 关闭窗口隐藏到托盘而不是退出：托盘菜单里的「退出 Switchelp」才是出口。
        // 这是托盘应用的常规预期，但必须让用户找得到退出口，所以托盘里保留独立项。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::providers_list,
            commands::providers_save,
            commands::credentials_list,
            commands::credentials_add,
            commands::credentials_replace,
            commands::credentials_select,
            commands::models_list,
            commands::models_save,
            commands::providers_delete,
            commands::credentials_rename,
            commands::credentials_set_disabled,
            commands::credentials_delete,
            commands::models_delete,
            commands::instances_detect,
            commands::config_inspect,
            commands::apply_plan,
            commands::apply_execute,
            commands::apply_status,
            commands::apply_confirm_reload,
            commands::apply_reconcile_reload,
            commands::host_restart,
            commands::apply_summary,
            commands::restore_plan,
            commands::restore_execute,
            commands::gateway_status,
            commands::diagnostics_list,
            commands::diagnostics_preview,
            commands::diagnostics_export,
            commands::diagnostics_clear,
            commands::gateway_set_paused,
            commands::models_discover,
            commands::platform_info,
            commands::update_check,
            commands::backups_list,
            commands::backups_create,
            commands::backups_preview,
            commands::backups_restore,
            commands::probes_start,
            commands::probes_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("Switchelp 无法启动");
}
