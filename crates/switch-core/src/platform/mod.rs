//! 平台适配：OS 路径、文件权限与窗口外观策略。
//!
//! 规则来自 [安全与跨平台](../../../../docs/architecture/05-security-and-platforms.md)
//! 与 [页面与流程](../../../../docs/design/04-pages-and-flows.md)：平台差异集中在这里，
//! 上层只问“本平台是什么策略”，不自己写 `cfg!`。本模块不依赖任何窗口框架。

use std::path::{Path, PathBuf};

/// 支持的目标平台。首发是 macOS 与 Windows，Linux 只保证编译与逻辑正确。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

impl Platform {
    /// 编译期决定的当前平台。
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    /// 与前端 `data-platform` 属性一致的标识。
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Macos => "macos",
            Platform::Windows => "windows",
            Platform::Linux => "linux",
        }
    }
}

/// 窗口外观策略。数值与设计令牌一致，改这里就要同步改 `tokens.css`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowChrome {
    /// 自绘标题栏高度。
    pub titlebar_height: u32,
    /// 标题栏左侧需要让出的宽度：macOS 是交通灯，其余平台为 0。
    pub leading_reserve: u32,
    /// 是否使用系统标题栏。
    pub system_decorations: bool,
}

/// 设计令牌里的初始值。
pub const TITLEBAR_HEIGHT: u32 = 44;
pub const MACOS_TRAFFIC_LIGHT_RESERVE: u32 = 84;

/// 一条要执行的命令。程序与参数分开存，便于断言，也避免拼字符串时被引号咬到。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

/// 重启宿主要执行的命令：先退出，再打开。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPlan {
    /// 退出是「请求退出」：平台不支持时为空。
    pub quit: Option<CommandSpec>,
    pub launch: CommandSpec,
}

/// 生成重启宿主的命令。
///
/// 为什么需要它：Codex 只在**启动时**读 `config.toml`，写完配置不重启，模型就不会出现在
/// 它的模型菜单里。这里只构造命令，执行由外壳负责——退出是异步的，因此调用方**不得**
/// 据此声称宿主已经加载了新配置。
pub fn restart_plan(platform: Platform, app_path: &str) -> RestartPlan {
    match platform {
        // `quit app` 接受 .app 的 POSIX 路径；`open -a` 走 Launch Services。
        Platform::Macos => RestartPlan {
            quit: Some(CommandSpec {
                program: "osascript".to_owned(),
                args: vec!["-e".to_owned(), format!("quit app \"{app_path}\"")],
            }),
            launch: CommandSpec {
                program: "open".to_owned(),
                args: vec!["-a".to_owned(), app_path.to_owned()],
            },
        },
        // taskkill 按映像名结束；启动直接执行那个可执行文件，不经 `cmd /C start`——
        // 后者会把路径交给 cmd 再解析一遍，路径里带 & 或引号时就成了注入点。
        Platform::Windows => RestartPlan {
            quit: Some(CommandSpec {
                program: "taskkill".to_owned(),
                args: vec!["/IM".to_owned(), exe_name(app_path), "/F".to_owned()],
            }),
            launch: CommandSpec {
                program: app_path.to_owned(),
                args: Vec::new(),
            },
        },
        // 本仓库不发布 Linux 包，只保证编译与逻辑正确。
        Platform::Linux => RestartPlan {
            quit: None,
            launch: CommandSpec {
                program: "xdg-open".to_owned(),
                args: vec![app_path.to_owned()],
            },
        },
    }
}

/// 从路径里取出可执行文件名。两种分隔符都认：测试在 macOS 上跑，但值要在 Windows 上用。
fn exe_name(app_path: &str) -> String {
    app_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("ChatGPT.exe")
        .to_owned()
}

/// 各平台的窗口策略。
///
/// macOS 用自绘标题栏并为交通灯预留位置；Windows 交给系统标题栏，
/// 不去复刻 Fluent 控件——复刻出来的控件在缩放与高对比度下更难对齐。
pub fn window_chrome(platform: Platform) -> WindowChrome {
    match platform {
        Platform::Macos => WindowChrome {
            titlebar_height: TITLEBAR_HEIGHT,
            leading_reserve: MACOS_TRAFFIC_LIGHT_RESERVE,
            system_decorations: false,
        },
        Platform::Windows | Platform::Linux => WindowChrome {
            titlebar_height: 0,
            leading_reserve: 0,
            system_decorations: true,
        },
    }
}

/// 平台候选配置根。只生成候选，不做存在性判断——探测由 `codex/detect.rs`
/// 的 `PathProbe` 负责，这样测试可以注入确定性的文件系统。
pub fn config_root_candidates(home: &Path, platform: Platform) -> Vec<PathBuf> {
    match platform {
        // Windows 上 `~/.codex` 仍是首选：Codex 自己以用户目录为准，
        // 另外两处是旧版与商店版可能留下的位置，仅作为候选。
        Platform::Windows => vec![
            home.join(".codex"),
            home.join("AppData").join("Roaming").join("Codex"),
            home.join("AppData").join("Local").join("Codex"),
        ],
        Platform::Macos | Platform::Linux => vec![home.join(".codex")],
    }
}

/// 应用数据目录。与窗口框架的 `app_data_dir()` 必须一致，
/// 否则诊断包、helper 与元数据会落在两个地方。
pub fn app_data_dir(home: &Path, platform: Platform, identifier: &str) -> PathBuf {
    match platform {
        Platform::Macos => home
            .join("Library")
            .join("Application Support")
            .join(identifier),
        Platform::Windows => home.join("AppData").join("Roaming").join(identifier),
        Platform::Linux => home.join(".local").join("share").join(identifier),
    }
}

/// 凭据 helper 的文件名。宿主只接受一个可执行文件路径，扩展名必须正确。
pub fn helper_file_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "gptswitch-auth-helper.cmd",
        Platform::Macos | Platform::Linux => "gptswitch-auth-helper",
    }
}

/// 私密文件应有的权限位。Windows 依赖用户目录 ACL，没有等价的 mode。
pub fn private_file_mode(platform: Platform) -> Option<u32> {
    match platform {
        Platform::Windows => None,
        Platform::Macos | Platform::Linux => Some(0o600),
    }
}

/// 私有目录应有的权限位。
pub fn private_dir_mode(platform: Platform) -> Option<u32> {
    match platform {
        Platform::Windows => None,
        Platform::Macos | Platform::Linux => Some(0o700),
    }
}

/// 按平台策略收紧权限。`None` 表示本平台不做处理（Windows），且不算失败。
pub fn restrict(path: &Path, mode: Option<u32>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(mode) = mode {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// 宿主应用在本平台的可执行名，供“需要重载”的提示使用。
pub fn host_executable_names(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Macos => &["ChatGPT", "codex"],
        Platform::Windows => &["ChatGPT.exe", "codex.exe"],
        Platform::Linux => &["chatgpt", "codex"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_is_one_of_the_supported_ones() {
        let platform = Platform::current();
        assert!(matches!(
            platform,
            Platform::Macos | Platform::Windows | Platform::Linux
        ));
        assert!(!platform.as_str().is_empty());
    }

    #[test]
    fn only_macos_reserves_space_for_traffic_lights() {
        let macos = window_chrome(Platform::Macos);
        assert_eq!(macos.leading_reserve, MACOS_TRAFFIC_LIGHT_RESERVE);
        assert_eq!(macos.titlebar_height, TITLEBAR_HEIGHT);
        assert!(!macos.system_decorations);

        for platform in [Platform::Windows, Platform::Linux] {
            let chrome = window_chrome(platform);
            assert_eq!(chrome.leading_reserve, 0, "非 macOS 不该为交通灯留白");
            assert_eq!(chrome.titlebar_height, 0);
            assert!(chrome.system_decorations, "交给系统标题栏而不是自绘复刻");
        }
    }

    #[test]
    fn config_root_prefers_the_user_directory_on_every_platform() {
        let home = Path::new("/home/user");
        for platform in [Platform::Macos, Platform::Windows, Platform::Linux] {
            assert!(
                !config_root_candidates(home, platform).is_empty(),
                "{platform:?} 至少要有一个候选配置根"
            );
        }
        assert_eq!(
            config_root_candidates(home, Platform::Macos),
            vec![home.join(".codex")]
        );
        assert_eq!(
            config_root_candidates(home, Platform::Windows)[0],
            home.join(".codex"),
            "Windows 的首选仍是 Codex 自己用的用户目录"
        );
    }

    #[test]
    fn app_data_dir_follows_each_platform_convention() {
        let home = Path::new("/home/user");
        assert_eq!(
            app_data_dir(home, Platform::Macos, "app.gptswitch.desktop"),
            home.join("Library/Application Support/app.gptswitch.desktop")
        );
        assert_eq!(
            app_data_dir(home, Platform::Windows, "app.gptswitch.desktop"),
            home.join("AppData/Roaming/app.gptswitch.desktop")
        );
        assert!(app_data_dir(home, Platform::Linux, "app.gptswitch.desktop")
            .ends_with(".local/share/app.gptswitch.desktop"));
    }

    #[test]
    fn helper_names_carry_the_right_extension_per_platform() {
        assert_eq!(helper_file_name(Platform::Macos), "gptswitch-auth-helper");
        assert_eq!(helper_file_name(Platform::Linux), "gptswitch-auth-helper");
        assert_eq!(
            helper_file_name(Platform::Windows),
            "gptswitch-auth-helper.cmd"
        );
    }

    #[test]
    fn private_modes_are_unix_only() {
        assert_eq!(private_file_mode(Platform::Macos), Some(0o600));
        assert_eq!(private_dir_mode(Platform::Macos), Some(0o700));
        assert_eq!(private_file_mode(Platform::Windows), None);
        assert_eq!(private_dir_mode(Platform::Windows), None);
    }

    #[cfg(unix)]
    #[test]
    fn restrict_applies_the_mode_and_ignores_none() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("secret");
        std::fs::write(&file, "x").unwrap();

        restrict(&file, Some(0o600)).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );

        // Windows 语义：None 表示不做处理，且不应报错，也不得改动现有权限。
        restrict(&file, None).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn host_executable_names_are_platform_appropriate() {
        for platform in [Platform::Macos, Platform::Windows, Platform::Linux] {
            assert!(!host_executable_names(platform).is_empty());
        }
        assert!(host_executable_names(Platform::Windows)
            .iter()
            .all(|name| name.ends_with(".exe")));
        assert!(host_executable_names(Platform::Linux)
            .iter()
            .all(|name| !name.contains('.')));
    }

    #[test]
    fn restart_plan_quits_then_relaunches_the_same_bundle() {
        let plan = restart_plan(Platform::Macos, "/Applications/ChatGPT.app");
        let quit = plan.quit.expect("macOS 应能请求退出");
        assert_eq!(quit.program, "osascript");
        assert_eq!(quit.args[1], "quit app \"/Applications/ChatGPT.app\"");
        assert_eq!(plan.launch.program, "open");
        assert_eq!(plan.launch.args, vec!["-a", "/Applications/ChatGPT.app"]);
    }

    #[test]
    fn restart_plan_ends_the_process_by_image_name_on_windows() {
        let exe = "C:\\Program Files\\ChatGPT\\ChatGPT.exe";
        let plan = restart_plan(Platform::Windows, exe);
        let quit = plan.quit.expect("Windows 应能请求退出");
        assert_eq!(quit.program, "taskkill");
        assert_eq!(quit.args, vec!["/IM", "ChatGPT.exe", "/F"]);
        // 启动直接执行该文件：参数为空，不会经过 cmd 再解析一次。
        assert_eq!(plan.launch.program, exe);
        assert!(plan.launch.args.is_empty());
    }

    #[test]
    fn restart_plan_passes_the_app_path_verbatim_without_a_shell() {
        // 路径含空格与 shell 元字符时，必须原样作为**一个**参数传递；任何环节都不许出现
        // 解释器（sh / cmd / osascript 的字符串拼接）。
        let odd = "/tmp/weird name; touch /tmp/pwned.app";
        for platform in [Platform::Macos, Platform::Windows, Platform::Linux] {
            let plan = restart_plan(platform, odd);
            let argv: Vec<String> = std::iter::once(plan.launch.program.clone())
                .chain(plan.launch.args.iter().cloned())
                .collect();
            assert_eq!(
                argv.iter().filter(|part| *part == odd).count(),
                1,
                "{platform:?} 应把整条路径原样作为单个参数：{argv:?}"
            );
            if platform == Platform::Windows {
                // Windows 直接执行该文件，不经 cmd。
                assert_eq!(plan.launch.program, odd);
                assert!(plan.launch.args.is_empty());
            }
        }
    }
}
