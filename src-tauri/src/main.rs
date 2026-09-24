#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;

use std::sync::Arc;
use switch_core::{
    application::{ApplyService, GatewayLayout, SystemClock, WorkspaceService},
    codex::{backup::BackupStore, config::AUTH_HELPER_INSTANCE},
    content::ContentService,
    credentials::{SecretVault, SystemVault},
    diagnostics::DiagnosticLog,
    domain::ids::InstanceId,
    gateway::{
        self, helper_path, install_auth_helper, Gateway, GatewayConfig, GatewayRouter, GatewayToken,
    },
    plugins::{GithubFetcher, PluginService},
    storage::{
        HubStore, OperationStore, Repository, SqliteHubStore, SqliteOperationStore,
        SqliteRepository,
    },
    toolhub::{catalog::ToolCatalog, ToolHubService},
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager,
};

use state::{DesktopState, StartupParts};

/// 本机网关的凭据 helper 路径。
///
/// Codex 以 `<command> --instance <固定值>` 调用它并读取输出作为本机令牌，
/// 上游 Key 因此永不写入 `config.toml`。路径必须与写进配置里的完全一致，
/// 否则宿主调用一个不存在的可执行文件。
fn auth_helper(directory: &std::path::Path) -> String {
    helper_path(directory).display().to_string()
}

/// 把随包分发的 bridge 装进应用数据目录。
///
/// 随包的那份就在主可执行文件旁边：打包后是 `Switchelp.app/Contents/MacOS/`，开发时是
/// `target/<profile>/`——cargo 把同一个工作区的 bin 放在一起，所以一条规则覆盖两种情形。
/// 装一份到应用数据目录的理由是**路径要稳定**：宿主会长期持有这个路径，而应用升级会
/// 替换整个 bundle。
fn install_bridge(
    directory: &std::path::Path,
) -> Result<std::path::PathBuf, switch_core::CoreError> {
    let executable = std::env::current_exe()
        .map_err(|_| switch_core::CoreError::internal("取不到自身可执行文件路径"))?;
    let beside = executable
        .parent()
        .ok_or_else(|| switch_core::CoreError::internal("自身可执行文件没有父目录"))?;
    let platform = switch_core::platform::Platform::current();
    // 打包后 sidecar 叫 `<名字>-app`（与开发时的 bin 区分开，见 bridge_bundle_name）；
    // 开发时若开发者自己把产物放到旁边，也认那个不带后缀的名字。
    let candidates = [
        beside.join(switch_core::platform::bridge_bundle_name(platform)),
        beside.join(switch_core::platform::bridge_file_name(platform)),
    ];
    let source = candidates
        .iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            switch_core::CoreError::new(switch_core::ErrorCode::Internal, "error.bridgeMissing")
                .with_detail(format!(
                    "随包分发的共存组件不在 {}，无法安装共存模式",
                    candidates[0].display()
                ))
        })?;
    switch_core::codex::coexist::install_bridge(directory, source)
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
    // 托盘图标：macOS 用**专用**的模板图，其余平台继续用应用图标。
    //
    // 应用图标是「黑底 + 白 S」的方块，放进菜单栏就是一块死黑的色块，和旁边那些系统
    // 图标的取向完全不同（用户点名要求改）。模板图的做法是：源图只要「透明底 + 单色字形」，
    // macOS 按 alpha 决定着色、颜色交给系统按菜单栏明暗给——浅色菜单栏画黑、深色画白，
    // 于是它自动和邻居一致，不需要我们判断当前是什么主题。
    //
    // 只在 macOS 换：模板图是 macOS 的概念，Windows 的通知区域不会替我们上色，
    // 一张纯黑字形在深色任务栏上等于看不见——那边的图标不该跟着改。
    #[cfg(target_os = "macos")]
    {
        match tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png")) {
            Ok(icon) => builder = builder.icon(icon).icon_as_template(true),
            // 读不到就退回应用图标：托盘没图标 = 用户找不到出口，比图标不好看严重得多。
            Err(_) => {
                if let Some(icon) = app.default_window_icon().cloned() {
                    builder = builder.icon(icon);
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
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
        // 应用内更新：检查、下载、验签、替换应用包。更新源与公钥在 tauri.conf.json 的
        // `plugins.updater` 里，机制与发布步骤见 docs/architecture/06-updates.md。
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            let gateway = start_gateway(
                &directory,
                repository.clone(),
                vault.clone(),
                router,
                diagnostics.clone(),
            );
            if let Err(error) = &gateway {
                // 端口被占用时不能假装在跑：状态里保留原因，界面显示“网关未启动”。
                eprintln!(
                    "Switchelp 网关未启动：{} {:?}",
                    error.message_key, error.safe_details
                );
            }
            // bridge 装不上不该拦住应用启动：共存模式用不了，其余功能照常；
            // 界面会读到原因并如实显示（见 commands::coexist_status）。
            let bridge = install_bridge(&directory);
            if let Err(error) = &bridge {
                eprintln!(
                    "Switchelp bridge 未安装：{} {:?}",
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

            // 扩展板块：同一个元数据库文件，独立连接（与操作记录同样的理由）。
            let hub: Arc<dyn HubStore> = Arc::new(SqliteHubStore::open(&db_path)?);
            // 清单加载失败不阻止启动，但工具管理板块会如实显示不可用。
            let catalog = ToolCatalog::embedded().map(|catalog| {
                Arc::new(ToolHubService::new(
                    catalog,
                    switch_core::platform::Platform::current(),
                    home.clone(),
                    hub.clone(),
                ))
            });
            if let Err(error) = &catalog {
                eprintln!(
                    "Switchelp 工具清单不可用：{} {:?}",
                    error.message_key, error.safe_details
                );
            }
            // 插件目录来自公开仓库：只读 GET，令牌可缺省。
            let fetcher = Arc::new(GithubFetcher::new(
                // 令牌在状态里按需读取；这里先给一份初始化用的。
                None,
                switch_core::content::DEFAULT_USER_AGENT.to_owned(),
            ));
            let plugins = Arc::new(PluginService::new(
                hub.clone(),
                repository.clone(),
                catalog.clone().unwrap_or_else(|_| {
                    // 清单不可用时用一个空清单，插件板块会在解析目标时报「不支持」。
                    Arc::new(ToolHubService::new(
                        ToolCatalog::parse(r#"{"schemaVersion":1,"tools":[{"id":"placeholder","names":{},"category":"utility","description":"清单不可用","command":"none","readiness":{"versionArgs":["--version"],"okExit":0,"cacheSeconds":3600},"paths":{"macos":["/nonexistent"]}}]}"#).expect("占位清单固定合法"),
                        switch_core::platform::Platform::current(),
                        home.clone(),
                        hub.clone(),
                    ))
                }),
                fetcher,
            ));

            // 首次运行时写入预置订阅源（已有源不动）。
            let content = ContentService::new(
                hub.clone(),
                Arc::new(switch_core::content::HttpFeedFetcher::new()),
                None,
            );
            if let Err(error) = content.ensure_defaults(switch_core::time_now()) {
                eprintln!("Switchelp 订阅源初始化失败：{error}");
            }

            app.manage(Arc::new(DesktopState::new(
                workspace,
                apply,
                home,
                directory.clone(),
                StartupParts {
                    gateway,
                    bridge,
                    catalog,
                },
                diagnostics,
                backups,
                hub.clone(),
                plugins,
                vault,
            )));

            // 定时刷新资讯。只在应用运行时发生，界面在订阅源页直说这一点。
            // 每 5 分钟醒一次，是否真的抓由每个源的到期时间决定；抓取本身放到
            // blocking 线程，ureq 是阻塞客户端，不能在 async 上下文里直接跑。
            let refresher = hub.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    // 每 5 分钟醒一次；是否真的抓由每个源的到期时间决定（本地 06:00 / 18:00）。
                    // 醒得比计划时刻密，是为了让「应用刚打开时已经过点了」这种情况立即补上。
                    tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                    let store = refresher.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        let service = ContentService::new(
                            store,
                            Arc::new(switch_core::content::HttpFeedFetcher::new()),
                            None,
                        );
                        let _ = service.refresh(None, false, switch_core::time_now());
                    })
                    .await;
                }
            });
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
            commands::coexist_status,
            commands::coexist_set,
            commands::coexist_resync,
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
            commands::update_install,
            commands::update_take_result,
            commands::update_open_release_page,
            commands::open_external_url,
            commands::backups_list,
            commands::backups_create,
            commands::backups_preview,
            commands::backups_restore,
            commands::probes_start,
            commands::probes_cancel,
            commands::tools_state,
            commands::tools_probe,
            commands::tools_skill_targets,
            commands::plugins_sources,
            commands::plugins_add_source,
            commands::plugins_remove_source,
            commands::plugins_browse,
            commands::plugins_preview,
            commands::plugins_install,
            commands::plugins_installed,
            commands::plugins_check_updates,
            commands::plugins_set_enabled,
            commands::plugins_uninstall,
            commands::content_sources,
            commands::content_save_source,
            commands::content_delete_source,
            commands::content_items,
            commands::content_refresh,
            commands::content_status,
            commands::content_github_token_status,
            commands::content_set_github_token,
        ])
        .run(tauri::generate_context!())
        .expect("Switchelp 无法启动");
}

#[cfg(test)]
mod tray_icon_tests {
    /// 托盘图标必须是**能解码的**那一个。
    ///
    /// 失败面很窄但很疼：`Image::from_bytes` 返回 `Err` 时运行时会静默退回应用图标
    /// （见 `install_tray` 的兜底），于是「图标改好了」这句话就成了假的，而菜单栏要人手去看
    /// 才发现。这里把「PNG 能被解码、尺寸是方的、颜色是单色且带透明」钉住。
    #[test]
    fn the_tray_template_decodes_as_a_square_monochrome_image() {
        let image = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))
            .expect("托盘模板图必须能被解码");
        assert_eq!(image.width(), image.height(), "模板图必须是方的");
        assert!(image.width() >= 32, "菜单栏是 2x 屏：源图太小会发虚");

        let rgba = image.rgba();
        let opaque: Vec<&[u8]> = rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 200)
            .collect();
        assert!(!opaque.is_empty(), "整张图都是透明的等于没有图标");
        assert!(
            opaque
                .iter()
                .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2]),
            "模板图必须是单色：有颜色说明渲染时把品牌色带进来了"
        );
        // 有透明区才叫模板图——底色不透明的方块就是用户抱怨的那块「黑方块」。
        let transparent = rgba.chunks_exact(4).filter(|pixel| pixel[3] < 8).count();
        assert!(transparent > 0, "模板图必须有透明底");
    }
}
