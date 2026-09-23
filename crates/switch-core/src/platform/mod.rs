//! 平台适配：OS 路径、文件权限与窗口外观策略。
//!
//! 规则来自 [安全与跨平台](../../../../docs/architecture/05-security-and-platforms.md)
//! 与 [页面与流程](../../../../docs/design/04-pages-and-flows.md)：平台差异集中在这里，
//! 上层只问“本平台是什么策略”，不自己写 `cfg!`。本模块不依赖任何窗口框架。

use std::path::{Path, PathBuf};

/// 系统代理与回环地址的冲突（宿主连不上本机网关的那一类故障）。
pub mod proxy;

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
    /// 需要传给**被启动进程**的环境变量。为空表示不额外设置。
    ///
    /// macOS 上 `open` 不会继承调用方的环境，所以那里的值还要同时出现在
    /// `--env KEY=VALUE` 参数里（见 `restart_plan_with_env`）。
    pub env: Vec<(String, String)>,
}

/// 重启宿主要执行的命令：先退出，再打开。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPlan {
    /// 优雅退出：请求应用自己退出。平台不支持时为空。
    ///
    /// **这条路可能被系统权限拦下**：macOS 下 `osascript` 向另一个应用发事件需要
    /// 「自动化」授权，而授权是按（发起方, 目标）成对授予的，一个刚装上的应用默认没有。
    /// 被拒时 `osascript` 不会失败得很显眼，应用照样开着——所以必须有退路。
    pub quit: Option<CommandSpec>,
    /// 兜底退出：直接给进程发信号。不需要任何系统授权，因为信号只作用于自己名下的进程。
    ///
    /// 代价是应用来不及保存界面状态（未保存的对话可能丢失），所以只有在
    /// 优雅退出超时之后才用它，并且要把「用了兜底」报告给用户。
    pub quit_force: Option<CommandSpec>,
    pub launch: CommandSpec,
}

/// 生成重启宿主的命令。
///
/// 为什么需要它：Codex 只在**启动时**读 `config.toml`，写完配置不重启，模型就不会出现在
/// 它的模型菜单里。这里只构造命令，执行由 `restart_host` 负责——退出是异步的，
/// 因此调用方**不得**据此声称宿主已经加载了新配置。
pub fn restart_plan(platform: Platform, app_path: &str) -> RestartPlan {
    restart_plan_with_env(platform, app_path, &[])
}

/// 同 [`restart_plan`]，但给被启动的宿主带上环境变量。
///
/// 共存模式靠它注入 `CODEX_CLI_PATH`（指向 bridge）。macOS 的 `open` **不继承**调用方的
/// 环境，只能靠 `--env` 参数把变量交过去；其余平台直接在被启动进程的环境上设置。
/// 两条路都要留：它们的生效机制不同，只做一半就会出现「参数写了、宿主没收到」。
pub fn restart_plan_with_env(
    platform: Platform,
    app_path: &str,
    env: &[(String, String)],
) -> RestartPlan {
    let mut plan = restart_plan_bare(platform, app_path);
    if env.is_empty() {
        return plan;
    }
    if platform == Platform::Macos {
        for (key, value) in env {
            plan.launch.args.push("--env".to_owned());
            plan.launch.args.push(format!("{key}={value}"));
        }
    }
    plan.launch.env = env.to_vec();
    plan
}

fn restart_plan_bare(platform: Platform, app_path: &str) -> RestartPlan {
    let process = host_process_name(platform, app_path);
    match platform {
        // 先请应用自己退出（走 Launch Services 的路径解析），超时再发信号。
        Platform::Macos => RestartPlan {
            quit: Some(CommandSpec {
                program: "osascript".to_owned(),
                args: vec!["-e".to_owned(), format!("quit app \"{app_path}\"")],
                env: Vec::new(),
            }),
            quit_force: Some(CommandSpec {
                program: "killall".to_owned(),
                args: vec!["-TERM".to_owned(), process],
                env: Vec::new(),
            }),
            launch: CommandSpec {
                program: "open".to_owned(),
                args: vec!["-a".to_owned(), app_path.to_owned()],
                env: Vec::new(),
            },
        },
        // taskkill 本身就是强制的，没有「优雅」这一档；启动直接执行那个可执行文件，
        // 不经 `cmd /C start`——后者会把路径交给 cmd 再解析一遍，路径里带 & 或引号时
        // 就成了注入点。
        Platform::Windows => RestartPlan {
            quit: Some(CommandSpec {
                program: "taskkill".to_owned(),
                args: vec!["/IM".to_owned(), exe_name(app_path), "/F".to_owned()],
                env: Vec::new(),
            }),
            quit_force: None,
            launch: CommandSpec {
                program: app_path.to_owned(),
                args: Vec::new(),
                env: Vec::new(),
            },
        },
        // 本仓库不发布 Linux 包，只保证编译与逻辑正确。
        Platform::Linux => RestartPlan {
            quit: Some(CommandSpec {
                program: "pkill".to_owned(),
                args: vec!["-TERM".to_owned(), "-x".to_owned(), process],
                env: Vec::new(),
            }),
            quit_force: None,
            launch: CommandSpec {
                program: "xdg-open".to_owned(),
                args: vec![app_path.to_owned()],
                env: Vec::new(),
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

/// 用于进程探测（`pgrep -x`）的宿主进程名。
///
/// 为什么不看命令的退出码：实测 `osascript -e 'quit app "<不存在的路径>"'` 和
/// `open -a "<不存在的路径>"` **都返回 0**。退出码不携带任何信息，唯一可靠的信号是
/// 进程在不在。
pub fn host_process_name(platform: Platform, app_path: &str) -> String {
    let name = exe_name(app_path);
    match platform {
        // `ChatGPT.app` → `ChatGPT`：`pgrep -x` 匹配的是可执行名，不带扩展名。
        Platform::Macos => name.strip_suffix(".app").unwrap_or(&name).to_owned(),
        // `ChatGPT.exe` → `ChatGPT`。
        Platform::Windows => name
            .strip_suffix(".exe")
            .or_else(|| name.strip_suffix(".EXE"))
            .unwrap_or(&name)
            .to_owned(),
        Platform::Linux => name,
    }
}

/// 进程探测与进程启动。外壳实现，核心只做判断——这样重试、等待与超时逻辑可以
/// 在测试里用假实现跑完，不必真的去开关用户的 Codex。
pub trait ProcessProbe {
    /// 这个进程名当前是否有实例在运行。
    fn is_running(&self, name: &str) -> bool;
    /// 起一个进程就走，不等它结束。返回是否成功启动。
    fn spawn_detached(&self, spec: &CommandSpec) -> bool;
    /// 等待若干毫秒。测试里立刻返回，不真的睡。
    fn sleep_ms(&self, ms: u64);
    /// 这个进程名**最早**那个实例的启动时间（Unix 秒）；没有实例或查不到时 `None`。
    ///
    /// 取最早的那个是有意的：调用方拿它判断「宿主是否在这次发布之后重新启动过」，
    /// 而只要还有一个更早的实例在跑，就不能断言宿主读的是新配置。宁可不确认，也不能错认。
    fn started_at_unix(&self, name: &str) -> Option<i64>;
}

/// 重启各阶段的等待预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartTiming {
    /// 等优雅退出。正常情况下一秒内就退了；给足时间是因为退出太慢只是慢，不是错。
    pub graceful_quit_timeout_ms: u64,
    /// 优雅退出超时后，发信号再等这么久。
    pub force_quit_timeout_ms: u64,
    /// 等新进程起来。冷启动一个桌面应用比退出慢。
    pub launch_timeout_ms: u64,
    pub poll_interval_ms: u64,
}

impl Default for RestartTiming {
    fn default() -> Self {
        Self {
            graceful_quit_timeout_ms: 6_000,
            force_quit_timeout_ms: 5_000,
            launch_timeout_ms: 25_000,
            poll_interval_ms: 250,
        }
    }
}

/// 重启的实际结果。字段名以「确认」为准：只有观察到进程状态才置为 true。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartOutcome {
    /// 确认旧进程已经退出（或本来就没在运行）。
    pub quit_confirmed: bool,
    /// 优雅退出没成，最后是发信号结束的。界面要据此提醒未保存内容可能丢失。
    pub quit_forced: bool,
    /// 确认新进程已经起来。
    pub launched_confirmed: bool,
}

/// 让宿主退出再起来，并且**确认**每一步真的发生了。
///
/// 为什么必须确认：写配置不重启，Codex 只在启动时读 `config.toml`，模型就不会出现在
/// 它的菜单里。而"重启"最容易出的错是——旧进程还没退干净就执行启动命令，于是启动命令
/// 只是把旧进程拉到前台：配置没重读，但一切看起来都成功了。固定 sleep 挡不住这件事，
/// 因为退出耗时不是常数。
///
/// 为什么先优雅后强杀，而不是像参考项目那样直接 `killall -9`：本工具的设计原则是
/// **不默认中断正在生成的任务**（见 `docs/design/05-patterns-and-accessibility.md`）。
/// 先请应用自己退出，正常情况无损；只有它不退（或系统权限把请求拦下了）才升级到发信号，
/// 并把「用了兜底」如实报告给用户。
pub fn restart_host(
    probe: &dyn ProcessProbe,
    plan: &RestartPlan,
    process_name: &str,
    timing: RestartTiming,
) -> RestartOutcome {
    let mut quit_confirmed = !probe.is_running(process_name);
    let mut quit_forced = false;

    // 1. 优雅退出，并等它真的退出。轮询的是进程，不是命令的退出码。
    if !quit_confirmed {
        if let Some(spec) = &plan.quit {
            probe.spawn_detached(spec);
            quit_confirmed = wait_until(
                probe,
                timing.graceful_quit_timeout_ms,
                timing.poll_interval_ms,
                || !probe.is_running(process_name),
            );
        }
    }

    // 2. 还没退就发信号。macOS 上「优雅退出没成」最常见的成因不是应用不肯退，
    //    而是系统没有授予我们向它发送 Apple 事件的权限——授权缺失是环境问题，
    //    不该让「重启」这个功能整体失效。
    if !quit_confirmed {
        if let Some(spec) = &plan.quit_force {
            probe.spawn_detached(spec);
            let gone = wait_until(
                probe,
                timing.force_quit_timeout_ms,
                timing.poll_interval_ms,
                || !probe.is_running(process_name),
            );
            if gone {
                quit_confirmed = true;
                quit_forced = true;
            }
        }
    }

    // 3. 旧进程还在就不要再启动：此时启动命令只会激活旧进程，而配置并没有被重读。
    //    如实返回，让界面说「Codex 仍在运行，未能重启」。
    if !quit_confirmed {
        return RestartOutcome {
            quit_confirmed: false,
            quit_forced: false,
            launched_confirmed: false,
        };
    }

    // 4. 启动，并等它真的起来。
    let launched = probe.spawn_detached(&plan.launch);
    let launched_confirmed = launched
        && wait_until(
            probe,
            timing.launch_timeout_ms,
            timing.poll_interval_ms,
            || probe.is_running(process_name),
        );

    RestartOutcome {
        quit_confirmed,
        quit_forced,
        launched_confirmed,
    }
}

/// 轮询到条件成立或超时。返回条件是否成立。
fn wait_until(
    probe: &dyn ProcessProbe,
    timeout_ms: u64,
    interval_ms: u64,
    mut condition: impl FnMut() -> bool,
) -> bool {
    if condition() {
        return true;
    }
    let interval = interval_ms.max(1);
    let mut waited = 0;
    while waited < timeout_ms {
        probe.sleep_ms(interval);
        waited += interval;
        if condition() {
            return true;
        }
    }
    false
}

/// 真的去看进程、真的起进程。与 `codex/detect.rs` 的 `RealFs` 同一套路数：
/// 接口与实现都在核心，外壳只负责挑一个实现传进来。
///
/// 用 `pgrep -x` 而不是系统 API：它判定的正是我们关心的那件事——这个可执行名
/// 有没有活着的实例，而且 macOS 与 Linux 行为一致。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemProcessProbe;

impl ProcessProbe for SystemProcessProbe {
    fn is_running(&self, name: &str) -> bool {
        if name.trim().is_empty() {
            return false;
        }
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = std::process::Command::new("tasklist");
            command.args(["/FI", &format!("IMAGENAME eq {name}.exe"), "/NH"]);
            command
        };
        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut command = std::process::Command::new("pgrep");
            command.arg("-x").arg(name);
            command
        };
        // 只看退出码，不解析输出：有匹配就有进程。
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn spawn_detached(&self, spec: &CommandSpec) -> bool {
        // 起一个进程就走：不等它结束（`open` 会立刻返回），也不接管道——
        // 继承的管道会让子进程随我们的生命周期被收割。
        let mut command = std::process::Command::new(&spec.program);
        command.args(&spec.args);
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn started_at_unix(&self, name: &str) -> Option<i64> {
        if name.trim().is_empty() {
            return None;
        }
        // Windows 上还没有对应实现，返回「查不到」而不是猜一个时间——调用方据此
        // 停在等待人工确认上。目前也走不到这里：Windows 的凭据 helper 还是桩，
        // 网关起不来，apply 在 commands.rs 的前置检查里就被拦住了。
        #[cfg(target_os = "windows")]
        {
            let _ = name;
            return None;
        }
        #[cfg(not(target_os = "windows"))]
        {
            let pids = std::process::Command::new("pgrep")
                .arg("-x")
                .arg(name)
                .output()
                .ok()?;
            if !pids.status.success() {
                return None;
            }
            let list: Vec<String> = String::from_utf8_lossy(&pids.stdout)
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            if list.is_empty() {
                return None;
            }
            // macOS 的 ps **没有** `etimes`（只有格式化的 `etime`），所以这里解析
            // `[[dd-]hh:]mm:ss` 而不是拿现成的秒数。Linux 上两者都有，用同一个更省事。
            let output = std::process::Command::new("ps")
                .arg("-o")
                .arg("etime=")
                .arg("-p")
                .args(&list)
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let oldest_elapsed = String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .filter_map(parse_elapsed_seconds)
                .max()?;
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            Some(now - oldest_elapsed)
        }
    }
}

/// 解析 `ps -o etime=` 的输出：`MM:SS`、`HH:MM:SS`、`DD-HH:MM:SS`。
///
/// 不依赖 `lstart`：那是本地化格式（星期缩写随语言变），而这个只有数字和冒号。
/// 字段数不在预期内的输入一律返回 `None`——把「看不懂」当成 0 秒，等于把「查不到」
/// 变成「刚刚启动」，那会凭空确认一次宿主回执。
fn parse_elapsed_seconds(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (days, clock) = match text.split_once('-') {
        Some((days, rest)) => (days.trim().parse::<i64>().ok()?, rest),
        None => (0, text),
    };
    let fields = clock
        .split(':')
        .map(|field| field.trim().parse::<i64>().ok())
        .collect::<Option<Vec<i64>>>()?;
    // 最短形态是 `MM:SS`：ps 从不只打印一个字段。
    let (seconds, minutes, hours) = match fields.as_slice() {
        [minutes, seconds] => (*seconds, *minutes, 0),
        [hours, minutes, seconds] => (*seconds, *minutes, *hours),
        _ => return None,
    };
    Some(((days * 24 + hours) * 60 + minutes) * 60 + seconds)
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

/// 共存模式 bridge 装到应用数据目录后的文件名。宿主把它当 codex CLI 调用。
pub fn bridge_file_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "gptswitch-bridge.exe",
        Platform::Macos | Platform::Linux => "gptswitch-bridge",
    }
}

/// 随包分发的 bridge 在包里的文件名。
///
/// 故意与开发时那个 bin（`gptswitch-bridge`）**不同名**：Tauri 会把 sidecar 按这个名字
/// 拷到主可执行文件旁边，而开发时 `cargo build -p gptswitch-bridge` 的产物就在同一个
/// 目录里。同名意味着 sidecar 的拷贝会**覆盖**开发产物——曾经真的发生过：一次
/// `cargo build -p gptswitch` 之后，`target/debug/gptswitch-bridge` 变成了构建期的
/// 占位件，而测试与真机探针都拿着它去跑。
pub fn bridge_bundle_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "gptswitch-bridge-app.exe",
        Platform::Macos | Platform::Linux => "gptswitch-bridge-app",
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
        // 兜底命令不需要系统授权（信号只作用于自己名下的进程），进程名从 .app 推出。
        let force = plan.quit_force.expect("macOS 应有兜底退出");
        assert_eq!(force.program, "killall");
        assert_eq!(force.args, vec!["-TERM", "ChatGPT"]);
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

    #[test]
    fn process_name_is_the_executable_name_not_the_bundle() {
        // `pgrep -x` 匹配可执行名：`.app` 与 `.exe` 都不是它的一部分。
        assert_eq!(
            host_process_name(Platform::Macos, "/Applications/ChatGPT.app"),
            "ChatGPT"
        );
        assert_eq!(
            host_process_name(Platform::Windows, "C:\\Program Files\\ChatGPT\\ChatGPT.exe"),
            "ChatGPT"
        );
        assert_eq!(
            host_process_name(Platform::Linux, "/usr/bin/chatgpt"),
            "chatgpt"
        );
    }

    /// 每次条件检查后触发，用来在「第 N 次轮询」时改变进程状态。
    type PollHook = Box<dyn FnMut(&mut bool)>;

    /// 假探测：进程状态由测试脚本决定，不碰真实进程，也不真的睡。
    struct FakeProbe {
        running: std::cell::RefCell<bool>,
        on_poll: std::cell::RefCell<PollHook>,
        spawned: std::cell::RefCell<Vec<String>>,
        spawn_result: bool,
        sleeps: std::cell::Cell<u32>,
    }

    impl FakeProbe {
        fn new(running: bool, on_poll: impl FnMut(&mut bool) + 'static) -> Self {
            Self {
                running: std::cell::RefCell::new(running),
                on_poll: std::cell::RefCell::new(Box::new(on_poll)),
                spawned: std::cell::RefCell::new(Vec::new()),
                spawn_result: true,
                sleeps: std::cell::Cell::new(0),
            }
        }
        fn spawned(&self) -> Vec<String> {
            self.spawned.borrow().clone()
        }
    }

    impl ProcessProbe for FakeProbe {
        fn is_running(&self, _name: &str) -> bool {
            let mut running = self.running.borrow_mut();
            (self.on_poll.borrow_mut())(&mut running);
            *running
        }
        fn spawn_detached(&self, spec: &CommandSpec) -> bool {
            self.spawned.borrow_mut().push(spec.program.clone());
            self.spawn_result
        }
        fn sleep_ms(&self, _ms: u64) {
            self.sleeps.set(self.sleeps.get() + 1);
        }
        fn started_at_unix(&self, _name: &str) -> Option<i64> {
            // 重启流程不用启动时间；回执判定在 apply 层，时间由装配层探测后传进去。
            None
        }
    }

    #[test]
    fn elapsed_seconds_parses_every_shape_ps_etime_prints() {
        // ps 实际会打印的三种形态：刚起来、跑了几小时、跨了天。
        assert_eq!(parse_elapsed_seconds("00:00"), Some(0));
        assert_eq!(parse_elapsed_seconds("00:42"), Some(42));
        assert_eq!(parse_elapsed_seconds("05:23"), Some(5 * 60 + 23));
        assert_eq!(
            parse_elapsed_seconds("02:05:23"),
            Some(2 * 3600 + 5 * 60 + 23)
        );
        assert_eq!(
            parse_elapsed_seconds("3-02:05:23"),
            Some((3 * 24 + 2) * 3600 + 5 * 60 + 23)
        );
        // 空行与畸形输入不能被当成 0 秒：那等于把「查不到」变成「刚刚启动」，
        // 会凭空确认一次宿主回执。
        assert_eq!(parse_elapsed_seconds(""), None);
        assert_eq!(parse_elapsed_seconds("   "), None);
        assert_eq!(parse_elapsed_seconds("42"), None);
        assert_eq!(parse_elapsed_seconds("1:2:3:4"), None);
        assert_eq!(parse_elapsed_seconds("abc"), None);
    }

    fn timing() -> RestartTiming {
        RestartTiming {
            graceful_quit_timeout_ms: 1_000,
            force_quit_timeout_ms: 1_000,
            launch_timeout_ms: 1_000,
            poll_interval_ms: 250,
        }
    }

    #[test]
    fn restart_confirms_both_the_exit_and_the_relaunch() {
        // 先在第 2 次轮询时退出，再在第 4 次轮询时起来。
        let probe = FakeProbe::new(true, {
            let mut polls = 0;
            move |running| {
                polls += 1;
                *running = match polls {
                    1 => true,      // 开始时还在
                    2 | 3 => false, // 请求退出后逐渐退出
                    _ => true,      // 启动后回来了
                };
            }
        });
        let plan = restart_plan(Platform::Macos, "/Applications/ChatGPT.app");
        let outcome = restart_host(&probe, &plan, "ChatGPT", timing());

        assert!(outcome.quit_confirmed, "确认了退出");
        assert!(!outcome.quit_forced, "优雅退出就够了，不该动兜底");
        assert!(outcome.launched_confirmed, "确认了重新起来");
        assert_eq!(probe.spawned(), vec!["osascript", "open"], "先退出再启动");
    }

    #[test]
    fn restart_does_not_launch_while_the_old_process_is_still_running() {
        // 旧进程一直没退出。此时启动命令只会把旧进程拉到前台，配置并不会被重读，
        // 所以**不该**执行它——这正是「点了重启没反应但界面说成功」的成因。
        let probe = FakeProbe::new(true, |_running| {});
        let plan = restart_plan(Platform::Macos, "/Applications/ChatGPT.app");
        let outcome = restart_host(&probe, &plan, "ChatGPT", timing());

        assert!(!outcome.quit_confirmed);
        assert!(!outcome.launched_confirmed);
        assert_eq!(
            probe.spawned(),
            vec!["osascript", "killall"],
            "优雅退出和兜底都试过，但都没有执行启动命令"
        );
    }

    #[test]
    fn restart_reports_failure_when_the_relaunch_never_appears() {
        // 退出了，但启动后进程一直没回来。
        let probe = FakeProbe::new(true, {
            let mut polls = 0;
            move |running| {
                polls += 1;
                *running = polls == 1;
            }
        });
        let plan = restart_plan(Platform::Macos, "/Applications/ChatGPT.app");
        let outcome = restart_host(&probe, &plan, "ChatGPT", timing());

        assert!(outcome.quit_confirmed, "退出是确认到的");
        assert!(!outcome.launched_confirmed, "起没起来要如实说没起来");
        assert_eq!(probe.spawned(), vec!["osascript", "open"]);
    }

    #[test]
    fn restart_skips_the_quit_when_the_host_is_not_running() {
        let probe = FakeProbe::new(false, |_running| {});
        let plan = restart_plan(Platform::Macos, "/Applications/ChatGPT.app");
        let outcome = restart_host(&probe, &plan, "ChatGPT", timing());

        assert!(outcome.quit_confirmed, "本来就没运行，等于已经退出");
        assert!(!outcome.quit_forced);
        assert_eq!(probe.spawned(), vec!["open"], "只启动，不请求退出");
    }

    #[test]
    fn restart_escalates_to_a_signal_when_graceful_quit_is_ignored() {
        // macOS 上「优雅退出没成」多半不是应用不肯退，而是系统没给我们发事件的权利。
        // 授权缺失是环境问题，不该让重启整体失效——所以超时要升级到发信号。
        let probe = FakeProbe::new(true, {
            let mut polls = 0;
            move |running| {
                polls += 1;
                // 只在前 8 次轮询里活着：刚好撑过优雅退出的 4 次，被信号结束。
                *running = polls <= 8;
            }
        });
        let plan = restart_plan(Platform::Macos, "/Applications/ChatGPT.app");
        let outcome = restart_host(&probe, &plan, "ChatGPT", timing());

        assert!(outcome.quit_confirmed, "兜底之后终于退出了");
        assert!(outcome.quit_forced, "必须如实报告用了兜底");
        assert_eq!(probe.spawned(), vec!["osascript", "killall", "open"]);
    }
}
