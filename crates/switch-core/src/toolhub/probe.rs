//! 子进程探针：本板块唯一会执行外部程序的地方。
//!
//! 三条硬规则，实现里不能松：
//! 1. **程序与参数分开传**，永远不拼 shell 字符串；
//! 2. **必须有超时**，超时就杀进程并如实记为超时——一个卡住的探针会让整页没有结论，
//!    比返回一个「未知」更糟；
//! 3. **超时要连子进程拉起的进程一起杀**（见 [`kill_tree`]）。工具常常是包装脚本，
//!    只杀外壳等于没杀：真正干活的那个还占着我们的管道，读线程一直等 EOF。

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// 探针输出的保留上限。探针只用来看版本号和登录状态，不需要完整输出；
/// 截断是为了不让某个话多的程序把内存和界面一起拖住。
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

/// 一次探针的结果。**没有 `success` 这种字段**：成功与否由调用方按自己的
/// 退出码约定判断，这里只报告观察到的事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    /// 进程退出码。被信号杀死或超时时为 `None`。
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl ProbeOutcome {
    /// 合并输出，用于文本特征匹配。
    pub fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    /// 输出的最后若干行，供界面「探针原文」区域展示。
    pub fn tail(&self, lines: usize) -> String {
        let combined = self.combined();
        let all: Vec<&str> = combined
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .collect();
        let start = all.len().saturating_sub(lines);
        all[start..].join("\n")
    }

    pub fn matches_exit(&self, expected: i32) -> bool {
        self.exit_code == Some(expected)
    }
}

/// 在 PATH 里按名字找可执行文件。
pub fn which(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Windows 上可执行文件带后缀，PATH 查找要跟着试一遍。
        #[cfg(windows)]
        {
            for ext in ["exe", "cmd", "bat"] {
                let with_ext = dir.join(format!("{command}.{ext}"));
                if with_ext.is_file() {
                    return Some(with_ext);
                }
            }
        }
    }
    None
}

/// 执行一次探针。`program` 必须是已解析出的真实路径，不做二次 PATH 查找。
pub fn run(program: &Path, args: &[String], timeout: Duration) -> ProbeOutcome {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // 让子进程进自己的进程组：超时时要按组杀，见 kill_tree。
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return ProbeOutcome {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
            }
        }
    };

    // 读取放到两个线程里：管道写满时子进程会阻塞，先 wait 再读会双双卡死。
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let read_all = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe
                    .by_ref()
                    .take(MAX_OUTPUT_BYTES as u64)
                    .read_to_end(&mut buffer);
            }
            String::from_utf8_lossy(&buffer).into_owned()
        })
    };
    let stdout_handle = read_all(stdout.map(|pipe| Box::new(pipe) as Box<dyn Read + Send>));
    let stderr_handle = read_all(stderr.map(|pipe| Box::new(pipe) as Box<dyn Read + Send>));

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_tree(&mut child);
                    timed_out = true;
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break None,
        }
    };

    ProbeOutcome {
        exit_code,
        stdout: stdout_handle.join().unwrap_or_default(),
        stderr: stderr_handle.join().unwrap_or_default(),
        timed_out,
    }
}

/// 杀掉子进程**及其派生的进程**。
///
/// 只杀直接子进程是不够的：`sh -c "…"` 里 shell 常常留在原地、真正干活的是它的子进程，
/// 而那个子进程继承着我们的 stdout/stderr 管道——它会一直占着管道不关，读线程就永远等
/// 不到 EOF。CI 上实测：超时设 300ms，函数却等了 29.5 秒（整个 `sleep 30`），
/// 「必须有超时」那条规则等于没生效。
///
/// 所以子进程在 spawn 时就进自己的进程组，这里按组杀：`kill` 的目标写成负的 pid
/// 表示整个进程组。组可能已经空了（子进程先退出），失败按正常情况处理。
#[cfg(unix)]
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    // SAFETY: 只发一个信号，参数是有效的 pid 与信号编号，没有内存访问。
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = child.wait();
}

#[cfg(not(unix))]
fn kill_tree(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// 从探针输出里取第一行有内容的文本作为版本号。
///
/// 不解析语义化版本：各工具的 `--version` 格式差异太大，硬解析只会把
/// `goose 1.0.0-beta.3` 这类真实版本变成「未知」。原样截断展示更诚实。
pub fn first_line(outcome: &ProbeOutcome) -> Option<String> {
    outcome
        .combined()
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_a_program_that_must_exist() {
        // sh 在所有支持平台上都在 PATH 里；这里只验证查找逻辑本身。
        #[cfg(unix)]
        assert!(which("sh").is_some(), "PATH 查找失效");
    }

    #[test]
    fn which_returns_none_for_nonsense() {
        assert!(which("definitely-not-a-real-program-9f3a").is_none());
    }

    #[test]
    fn probe_reports_exit_code_and_output() {
        #[cfg(unix)]
        {
            let outcome = run(
                Path::new("/bin/sh"),
                &["-c".to_owned(), "echo hello; exit 3".to_owned()],
                Duration::from_secs(5),
            );
            assert_eq!(outcome.exit_code, Some(3));
            assert!(outcome.stdout.contains("hello"));
            assert!(!outcome.timed_out);
            assert_eq!(first_line(&outcome).as_deref(), Some("hello"));
        }
    }

    #[test]
    fn probe_times_out_and_kills_the_child() {
        #[cfg(unix)]
        {
            let started = Instant::now();
            // 子命令写成 `sleep 30; true` 而不是 `sleep 30`：后者在 bash（macOS 的 /bin/sh）
            // 下会被 exec 成自己，只剩一个进程；而在 dash（Ubuntu 的 /bin/sh）下 shell 会
            // 留在原地、sleep 是它的子进程。带一个后续命令可以让两种 shell 都**不 exec**，
            // 于是「子进程还活着并占着管道」这个条件在哪个平台都成立。
            //
            // 这不是为了刁难：真实的工具常常是包装脚本，超时只杀到外壳、真正干活的那个还在
            // 往我们的管道里写，读线程就一直等下去——CI 上实测等了 29.5 秒（整个
            // `sleep 30`），而这里量的是「必须立刻返回」。
            let outcome = run(
                Path::new("/bin/sh"),
                &["-c".to_owned(), "sleep 30; true".to_owned()],
                Duration::from_millis(300),
            );
            assert!(outcome.timed_out, "超时必须被如实报告");
            assert_eq!(outcome.exit_code, None);
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "超时后必须立刻返回，不能等子进程自己结束"
            );
        }
    }

    #[test]
    fn missing_program_is_not_a_panic() {
        let outcome = run(
            Path::new("/definitely/not/here"),
            &[],
            Duration::from_millis(100),
        );
        assert_eq!(outcome.exit_code, None);
        assert!(first_line(&outcome).is_none());
    }

    #[test]
    fn tail_keeps_the_last_lines_only() {
        let outcome = ProbeOutcome {
            exit_code: Some(0),
            stdout: "a\nb\nc\nd\n".to_owned(),
            stderr: String::new(),
            timed_out: false,
        };
        assert_eq!(outcome.tail(2), "c\nd");
    }
}
