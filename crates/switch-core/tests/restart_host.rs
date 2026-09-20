//! 真的去开一次宿主的测试。
//!
//! 默认 `#[ignore]`：它会退出并重新打开用户正在用的 Codex，属于**有副作用**的操作，
//! 不能在 `cargo test` 里顺带跑。要在目标机器上显式验收时执行：
//!
//! ```sh
//! cargo test -p switch-core --test restart_host -- --ignored --nocapture
//! ```
//!
//! 验收的是这件事：`restart_host` 报的 `quit_confirmed` / `launched_confirmed`
//! 必须与实际进程状态一致。早先的实现只要命令发出去就报成功，而 `open` 对已经在运行的
//! 应用只是把它拉到前台——配置没被重读，界面却说重启好了。

use switch_core::platform::{
    host_process_name, restart_host, restart_plan, Platform, ProcessProbe, RestartTiming,
    SystemProcessProbe,
};

/// 被测的宿主。只在这台机器上存在时才有意义。
const APP_PATH: &str = "/Applications/ChatGPT.app";

fn app_present() -> bool {
    std::path::Path::new(APP_PATH).is_dir()
}

#[test]
#[ignore = "会真的退出并重开用户正在用的 Codex，需在目标机器上显式运行"]
fn restarting_the_real_host_reports_the_real_outcome() {
    if !app_present() {
        eprintln!("跳过：{APP_PATH} 不存在，这不是一台装了 Codex 的机器");
        return;
    }
    let platform = Platform::current();
    assert_eq!(platform, Platform::Macos, "这条验收只在 macOS 上有意义");

    let probe = SystemProcessProbe;
    let name = host_process_name(platform, APP_PATH);
    assert_eq!(name, "ChatGPT", "从 .app 路径推出的进程名");

    println!("重启前：{name} 在运行吗 = {}", probe.is_running(&name));

    let plan = restart_plan(platform, APP_PATH);
    let outcome = restart_host(
        &probe,
        &plan,
        &name,
        RestartTiming {
            graceful_quit_timeout_ms: 20_000,
            force_quit_timeout_ms: 20_000,
            launch_timeout_ms: 45_000,
            poll_interval_ms: 250,
        },
    );
    println!("结果：{outcome:?}");
    println!("重启后：{name} 在运行吗 = {}", probe.is_running(&name));

    // 报出来的结论必须与真实状态一致——这正是这次修复的核心。
    assert!(outcome.quit_confirmed, "请求退出后应确认到进程已消失");
    assert_eq!(
        probe.is_running(&name),
        outcome.launched_confirmed,
        "launched_confirmed 必须等于真实的进程状态，不能只是「命令发出去了」"
    );
    assert!(outcome.launched_confirmed, "启动后应确认到进程回来了");
}

/// 优雅退出这条路被系统权限拦下时，兜底信号必须真的能把宿主关掉。
///
/// 模拟办法：把优雅退出的等待预算压到 0，于是它**立刻**升级到发信号——这正是
/// macOS 上「没有自动化授权」时的实际情形。跑完必须确认宿主回来了。
#[test]
#[ignore = "会真的退出并重开用户正在用的 Codex，需在目标机器上显式运行"]
fn the_signal_fallback_restarts_the_real_host_when_graceful_quit_is_denied() {
    if !app_present() {
        eprintln!("跳过：{APP_PATH} 不存在");
        return;
    }
    let probe = SystemProcessProbe;
    let name = host_process_name(Platform::current(), APP_PATH);
    let plan = restart_plan(Platform::current(), APP_PATH);

    // 先确保它在运行，否则测不到退出这一段。
    if !probe.is_running(&name) {
        assert!(probe.spawn_detached(&plan.launch), "先把它打开");
        assert!(
            probe.is_running(&name),
            "等它起来才能测退出——请重跑一次这条用例"
        );
    }

    let outcome = restart_host(
        &probe,
        &plan,
        &name,
        RestartTiming {
            // 0 表示不等优雅退出，直接升级——等价于「osascript 被拒」。
            graceful_quit_timeout_ms: 0,
            force_quit_timeout_ms: 20_000,
            launch_timeout_ms: 45_000,
            poll_interval_ms: 250,
        },
    );
    println!("结果：{outcome:?}");

    assert!(outcome.quit_forced, "必须走了兜底信号");
    assert!(outcome.quit_confirmed, "兜底之后进程确实消失了");
    assert!(outcome.launched_confirmed, "并且重新起来了");
    assert!(probe.is_running(&name), "最终状态与报告一致");
}
