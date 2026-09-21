//! 共存模式（Desktop Bridge）：官方订阅模型与我们代理的模型出现在**同一份菜单**里。
//!
//! 协议部分在 `crates/bridge`（一个独立的可执行文件，宿主通过 `CODEX_CLI_PATH` 调用它，
//! 它再起两根 codex）。本模块只管**装配**，四件事：
//!
//! 1. bridge 装在哪、从哪来；
//! 2. 托管那一根 codex 用哪个 `CODEX_HOME`、它对应哪个 `CodexInstance`（这样应用管线
//!    可以原样复用：它的产物只写进我们自己的目录，用户真实的 `~/.codex` 一个字节都不动）；
//! 3. 重启宿主时要带哪些环境变量；
//! 4. 「宿主现在是不是真的在跑 bridge」怎么判断——只看日志与进程启动时间这两个可观察事实，
//!    不拿「我们上次设过开关」当结论。

use std::path::{Path, PathBuf};

use crate::codex::detect::CodexInstance;
use crate::domain::error::{CoreError, ErrorCode};
use crate::domain::ids::InstanceId;

/// 托管那根在实例列表里的 id。与真实实例分开，目录版本也就会分成两族。
pub const MANAGED_INSTANCE_ID: &str = "managed-main";
/// 托管 CODEX_HOME 的目录名（位于应用数据目录下）。
pub const MANAGED_HOME_DIR: &str = "codex-home";
/// bridge 可执行文件名。宿主拿到的 `CODEX_CLI_PATH` 必须指向一个真实文件。
pub const BRIDGE_FILE_NAME: &str = "gptswitch-bridge";
/// bridge 日志里「我起来了」的事件名。
///
/// 与 `crates/bridge` 写日志时用的事件名必须一致；`crates/bridge/tests/multiplex.rs`
/// 会断言这个名字，所以改名不会只改一边。
pub const BRIDGE_STARTED_EVENT: &str = "bridge-started";

/// 设置键：共存模式开关（`"1"` / `"0"`）。存的是**意图**，不是事实。
pub const SETTING_ENABLED: &str = "coexist.enabled";

/// 托管 CODEX_HOME。它在应用数据目录里：我们的产物只落在自己的地方。
pub fn managed_home(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(MANAGED_HOME_DIR)
}

/// 托管那根的 config.toml。
pub fn managed_config_file(app_data_dir: &Path) -> PathBuf {
    managed_home(app_data_dir).join("config.toml")
}

/// bridge 可执行文件的位置。与 `platform::helper_file_name` 同一个 `bin/` 目录：
/// 两者都是「应用数据目录里的可执行件」，没有理由分成两处。
pub fn bridge_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("bin").join(bridge_file_name())
}

/// bridge 日志。诊断「宿主到底有没有跑 bridge」靠它。
pub fn bridge_log(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("bridge.log")
}

/// 文件名按平台取；Windows 上是 `.exe`，但装配层会在那儿直接拒绝（见 [`install_bridge`]）。
fn bridge_file_name() -> &'static str {
    crate::platform::bridge_file_name(crate::platform::Platform::current())
}

/// 由检测到的真实实例派生出「托管那一根」。
///
/// 复制的是**环境**（CLI 路径、桌面端版本、兼容性判断），换掉的是**配置根**：
/// 托管那根读我们自己的目录，因此可以随便写、随时改，用户真实的 `~/.codex` 不受影响。
pub fn managed_instance(base: &CodexInstance, app_data_dir: &Path) -> CodexInstance {
    let config_file = managed_config_file(app_data_dir);
    CodexInstance {
        id: InstanceId::new(MANAGED_INSTANCE_ID),
        app_path: base.app_path.clone(),
        cli_path: base.cli_path.clone(),
        desktop_version: base.desktop_version.clone(),
        cli_version: base.cli_version.clone(),
        config_root: managed_home(app_data_dir).display().to_string(),
        config_file: config_file.display().to_string(),
        config_exists: config_file.exists(),
        startup_mode: base.startup_mode,
        compatibility: base.compatibility,
        fingerprint: base.fingerprint.clone(),
        // 托管目录是我们自己的，别的工具不会往里写标记；有冲突的是原生那份。
        conflicting_managers: Vec::new(),
        blocked_reason_key: base.blocked_reason_key.clone(),
    }
}

/// 重启宿主时要带的环境变量。
///
/// `CODEX_CLI_PATH` 是接入点（桌面端用它顶替内置 codex），其余三项把 bridge 需要的
/// 事实一次交代清楚——bridge 自己不去猜用户的主目录，也不去猜 CLI 在哪。
pub fn launch_env(
    app_data_dir: &Path,
    base: &CodexInstance,
) -> Result<Vec<(String, String)>, CoreError> {
    let cli = base.cli_path.clone().ok_or_else(|| {
        CoreError::new(ErrorCode::CapabilityUnsupported, "error.coexistNeedsCli").with_detail(
            "这个实例没有可用的 codex CLI 路径，共存模式无法接管。先让 Switchelp 认出桌面端。"
                .to_owned(),
        )
    })?;
    let pair = |key: &str, value: String| (key.to_owned(), value);
    Ok(vec![
        pair(
            "CODEX_CLI_PATH",
            bridge_path(app_data_dir).display().to_string(),
        ),
        pair("GPTSWITCH_BRIDGE_CODEX", cli),
        pair(
            "GPTSWITCH_BRIDGE_MANAGED_HOME",
            managed_home(app_data_dir).display().to_string(),
        ),
        // 原生那根显式写出用户真实的配置根：宿主自己的 CODEX_HOME 未必是它。
        pair("GPTSWITCH_BRIDGE_NATIVE_HOME", base.config_root.clone()),
        pair(
            "GPTSWITCH_BRIDGE_LOG",
            bridge_log(app_data_dir).display().to_string(),
        ),
    ])
}

/// 把随包分发的 bridge 装进应用数据目录，返回落点。
///
/// 为什么要拷贝而不是直接用包内的路径：宿主会长期持有这个路径（它自己 spawn 这个进程），
/// 而应用升级会替换整个 bundle——路径变得不再指向同一份文件。装一份到应用数据目录，
/// 路径就固定了。
pub fn install_bridge(app_data_dir: &Path, source: &Path) -> Result<PathBuf, CoreError> {
    // Windows 上没有随包的 bridge，也没有实现注入路径。**必须在这里失败**，不能"装上"：
    // 装上了应用会照常写配置、照常显示已应用，而宿主根本没被顶替——菜单里既没有原生
    // 模型也没有我们的模型，用户只能看到「什么都没变」。
    if crate::platform::Platform::current() == crate::platform::Platform::Windows {
        return Err(CoreError::new(
            ErrorCode::CapabilityUnsupported,
            "error.coexistUnsupportedPlatform",
        )
        .with_detail("共存模式目前只有 macOS 装配完整：Windows 上还没有注入路径。".to_owned()));
    }
    if !source.exists() {
        return Err(
            CoreError::new(ErrorCode::Internal, "error.bridgeMissing").with_detail(format!(
                "随包分发的 bridge 不在 {}，无法安装共存模式",
                source.display()
            )),
        );
    }
    // 构建期占位件是一个 shell 脚本（见 `src-tauri/build.rs`）。它必须在这里被挡下：
    // 交给宿主之后，Codex 会拿它当 CLI 启动，然后每一次会话都失败——而菜单里看起来一切正常。
    if looks_like_placeholder(source)? {
        return Err(CoreError::new(ErrorCode::Internal, "error.bridgePlaceholder").with_detail(
            "随包分发的 bridge 是构建期的占位件，不是真正的可执行文件；请用 `pnpm desktop:build` 打包。"
                .to_owned(),
        ));
    }
    let target = bridge_path(app_data_dir);
    let parent = target
        .parent()
        .ok_or_else(|| CoreError::internal("bridge 落点没有父目录"))?;
    std::fs::create_dir_all(parent).map_err(|_| CoreError::internal("无法创建 bin 目录"))?;
    crate::platform::restrict(
        parent,
        crate::platform::private_dir_mode(crate::platform::Platform::current()),
    )
    .map_err(|_| CoreError::internal("无法设置 bin 目录权限"))?;
    std::fs::copy(source, &target).map_err(|_| CoreError::internal("无法写入 bridge"))?;
    crate::platform::restrict(
        &target,
        crate::platform::private_file_mode(crate::platform::Platform::current()),
    )
    .map_err(|_| CoreError::internal("无法设置 bridge 权限"))?;
    // 可执行位：私有文件权限（0600）会把宿主挡在门外，这里必须显式放开执行。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| CoreError::internal("无法设置 bridge 执行权限"))?;
    }
    Ok(target)
}

/// 源文件是不是构建期的占位件。
///
/// 真的 bridge 是一个编译出来的可执行文件，不会以 `#!` 开头。用这个判据而不是比对内容：
/// 占位件的文案会改，而「它是个脚本」这一点不会。
fn looks_like_placeholder(source: &Path) -> Result<bool, CoreError> {
    use std::io::Read;
    let mut file = std::fs::File::open(source)
        .map_err(|_| CoreError::internal("无法读取随包分发的 bridge"))?;
    let mut head = [0u8; 2];
    let read = file
        .read(&mut head)
        .map_err(|_| CoreError::internal("无法读取随包分发的 bridge"))?;
    Ok(read == 2 && &head == b"#!")
}

/// 丢掉托管 profile 的配置文件，让它下次生成计划时从当前的原生配置重新复制一份。
///
/// 托管 profile 的底子（除路由外的设置：插件、市场、项目信任）是开启共存时复制的那一份
/// 快照。用户在原生配置里加了东西之后，想让那边也带上，就得重新复制一次——
/// 不给他这个入口，那句「不会自动同步」就变成了一句无解的说明。
pub fn clear_managed_config(app_data_dir: &Path) -> Result<(), CoreError> {
    let target = managed_config_file(app_data_dir);
    match std::fs::remove_file(&target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(CoreError::internal("无法重建托管 profile")),
    }
}

/// bridge 日志里最近一次「我起来了」的 Unix 秒。
///
/// 读不到、或从来没起来过，都返回 `None`——「没证据」不能当成「没有」，调用方据此
/// 停在「无法确认」，而不是断言用户此刻不在共存模式。
pub fn last_start_unix(log: &Path) -> Option<i64> {
    let bytes = std::fs::read(log).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .filter(|entry| {
            entry.get("event").and_then(|value| value.as_str()) == Some(BRIDGE_STARTED_EVENT)
        })
        .filter_map(|entry| entry.get("ts").and_then(|value| value.as_i64()))
        .max()
}

/// 宿主是否真的在跑 bridge：只看两个可观察事实——bridge 什么时候起来的、
/// 当前宿主进程什么时候起来的。
///
/// 为什么不用我们自己存的开关：开关只说明「我们点过按钮」。宿主被用户从 Dock 重新打开时
/// 不带我们注入的环境，那时开关还是 1，而共存其实没生效——界面照开关显示就是在骗人。
pub fn running_under_bridge(
    last_start_unix: Option<i64>,
    host_started_at_unix: Option<i64>,
) -> Option<bool> {
    let last_start = last_start_unix?;
    let host_start = host_started_at_unix?;
    Some(last_start >= host_start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::detect::StartupMode;
    use crate::domain::version::{CompatibilityStatus, VersionFingerprint};

    fn instance() -> CodexInstance {
        CodexInstance {
            id: InstanceId::new("local-main"),
            app_path: Some("/Applications/ChatGPT.app".to_owned()),
            cli_path: Some("/Applications/ChatGPT.app/Contents/Resources/codex".to_owned()),
            desktop_version: Some("1.0".to_owned()),
            cli_version: Some("0.155.0".to_owned()),
            config_root: "/Users/someone/.codex".to_owned(),
            config_file: "/Users/someone/.codex/config.toml".to_owned(),
            config_exists: true,
            startup_mode: StartupMode::Managed,
            compatibility: CompatibilityStatus::Stable,
            fingerprint: VersionFingerprint::unknown(),
            conflicting_managers: Vec::new(),
            blocked_reason_key: None,
        }
    }

    #[test]
    fn managed_instance_only_swaps_the_config_root() {
        let dir = tempfile::tempdir().unwrap();
        let managed = managed_instance(&instance(), dir.path());
        assert_eq!(managed.id.as_str(), MANAGED_INSTANCE_ID);
        assert_eq!(
            managed.config_file,
            managed_config_file(dir.path()).display().to_string()
        );
        assert!(managed
            .config_file
            .starts_with(&dir.path().display().to_string()));
        assert_eq!(
            managed.cli_path,
            instance().cli_path,
            "CLI 与桌面端还是同一个，只有配置根换了"
        );
        assert_eq!(
            managed.config_root,
            managed_home(dir.path()).display().to_string()
        );
        assert_eq!(
            instance().config_root,
            "/Users/someone/.codex",
            "派生托管实例不能反过来改动真实实例"
        );
    }

    #[test]
    fn launch_env_points_at_the_bridge_and_both_homes() {
        let dir = tempfile::tempdir().unwrap();
        let env: std::collections::HashMap<String, String> = launch_env(dir.path(), &instance())
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(
            env["CODEX_CLI_PATH"],
            bridge_path(dir.path()).display().to_string()
        );
        assert_eq!(
            env["GPTSWITCH_BRIDGE_MANAGED_HOME"],
            managed_home(dir.path()).display().to_string()
        );
        assert_eq!(env["GPTSWITCH_BRIDGE_NATIVE_HOME"], "/Users/someone/.codex");
        assert!(env["GPTSWITCH_BRIDGE_CODEX"].ends_with("/codex"));
    }

    #[test]
    fn launch_env_refuses_an_instance_without_a_cli() {
        let dir = tempfile::tempdir().unwrap();
        let mut base = instance();
        base.cli_path = None;
        assert!(launch_env(dir.path(), &base).is_err());
    }

    #[test]
    fn install_bridge_copies_the_binary_and_keeps_it_executable() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("from-bundle");
        // 头两字节按 Mach-O 写：占位件是脚本，不能被当成真 bridge（见下一条测试）。
        std::fs::write(&source, b"\xcf\xfa\xed\xfe fake executable").unwrap();
        let installed = install_bridge(dir.path(), &source).unwrap();
        assert_eq!(installed, bridge_path(dir.path()));
        assert!(installed.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&installed).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "bridge 要能被执行，但不能给别的用户看");
        }
    }

    #[test]
    fn install_bridge_fails_loudly_when_the_bundle_has_no_bridge() {
        let dir = tempfile::tempdir().unwrap();
        let error = install_bridge(dir.path(), &dir.path().join("不存在")).unwrap_err();
        assert!(error.safe_details[0].contains("无法安装共存模式"));
    }

    #[test]
    fn install_bridge_refuses_the_build_time_placeholder() {
        // 构建期占位件是个 shell 脚本。把它装上去等于给宿主一个「跑不起来但看起来正常」的 CLI。
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("placeholder");
        std::fs::write(&source, b"#!/bin/sh\nexit 2\n").unwrap();
        let error = install_bridge(dir.path(), &source).unwrap_err();
        assert!(
            error.safe_details[0].contains("占位件"),
            "{:?}",
            error.safe_details
        );
        assert!(!bridge_path(dir.path()).exists(), "不能把占位件拷进去");
    }

    #[test]
    fn clearing_the_managed_config_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let config = managed_config_file(dir.path());
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "model = \"x\"\n").unwrap();
        clear_managed_config(dir.path()).unwrap();
        assert!(!config.exists());
        // 已经没有了再调一次也不该报错：用户可能连点两次。
        clear_managed_config(dir.path()).unwrap();
    }

    #[test]
    fn last_start_reads_only_our_start_events() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("bridge.log");
        std::fs::write(
            &log,
            "{\"event\":\"routing\",\"ts\":999}\n{\"event\":\"bridge-started\",\"ts\":100}\nnot json\n{\"event\":\"bridge-started\",\"ts\":4200}\n",
        )
        .unwrap();
        assert_eq!(last_start_unix(&log), Some(4200));
        assert_eq!(last_start_unix(&dir.path().join("没有这个文件")), None);
    }

    #[test]
    fn running_under_bridge_needs_both_facts() {
        // bridge 比宿主后起来：宿主这次确实是带着 bridge 起来的。
        assert_eq!(running_under_bridge(Some(120), Some(100)), Some(true));
        // bridge 比宿主早：当前这个宿主不是我们起的（用户自己从 Dock 打开了它）。
        assert_eq!(running_under_bridge(Some(100), Some(120)), Some(false));
        // 缺任一条证据都不下结论——宁可显示「无法确认」。
        assert_eq!(running_under_bridge(None, Some(100)), None);
        assert_eq!(running_under_bridge(Some(100), None), None);
    }
}
