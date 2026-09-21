//! bridge 的协议验收：像宿主那样对它说话，核对合并与路由。
//!
//! 跑的是**真的 bridge 进程**与**真的两根子进程**——只不过那两根是同一个二进制里的
//! mock app-server（`--mock-app-server`，只在 debug 构建里存在）。这样 CI 里不需要
//! codex、也不需要 ChatGPT 桌面端，而 bridge 的多进程行为仍然被测到。
//!
//! 真机上的端到端（真实 codex + 真宿主）见 `docs/development/02-testing-and-release.md`。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BRIDGE: &str = env!("CARGO_BIN_EXE_gptswitch-bridge");
/// 宿主真实传给 codex 的 argv 形状（见 docs/01-product-requirements.md 的实测记录）。
const HOST_ARGV: [&str; 4] = ["-c", "features.code_mode_host=true", "app-server", "-c"];

struct Bridge {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    log: PathBuf,
    _dir: tempdir::Dir,
}

/// 极简临时目录（不引第三方依赖）：测试结束即删。
mod tempdir {
    use std::path::{Path, PathBuf};

    pub struct Dir(pub PathBuf);

    impl Dir {
        pub fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "gptswitch-bridge-test-{tag}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

impl Bridge {
    fn start(tag: &str, managed: bool) -> Self {
        let dir = tempdir::Dir::new(tag);
        let native_home = dir.path().join("native-home");
        let managed_home = dir.path().join("codex-home");
        std::fs::create_dir_all(&native_home).unwrap();
        std::fs::create_dir_all(&managed_home).unwrap();
        let log = dir.path().join("bridge.log");

        let mut env: HashMap<String, String> = std::env::vars().collect();
        env.insert("GPTSWITCH_BRIDGE_CODEX".into(), BRIDGE.to_owned());
        env.insert("GPTSWITCH_BRIDGE_MOCK".into(), "1".into());
        env.insert(
            "GPTSWITCH_BRIDGE_MOCK_MANAGED".into(),
            managed_home.display().to_string(),
        );
        env.insert(
            "GPTSWITCH_BRIDGE_NATIVE_HOME".into(),
            native_home.display().to_string(),
        );
        env.insert("GPTSWITCH_BRIDGE_LOG".into(), log.display().to_string());
        if managed {
            env.insert(
                "GPTSWITCH_BRIDGE_MANAGED_HOME".into(),
                managed_home.display().to_string(),
            );
        } else {
            env.remove("GPTSWITCH_BRIDGE_MANAGED_HOME");
        }

        let mut child = Command::new(BRIDGE)
            .args(HOST_ARGV)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("bridge 起不来");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            lines: receiver,
            log,
            _dir: dir,
        }
    }

    fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().expect("stdin 已关闭");
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    /// 等一条 id 匹配的响应。别的行（通知）跳过——宿主也是这么做的。
    fn call(&mut self, id: &str, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "{method} 没有响应");
            let Ok(line) = self.lines.recv_timeout(remaining) else {
                panic!("{method} 没有响应");
            };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("id") == Some(&Value::String(id.to_owned())) {
                return message;
            }
        }
    }

    fn result(&mut self, id: &str, method: &str, params: Value) -> Value {
        let message = self.call(id, method, params);
        assert!(
            message.get("error").is_none(),
            "{method} 报错了：{:?}",
            message.get("error")
        );
        message.get("result").cloned().unwrap_or(Value::Null)
    }

    fn events(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect()
    }

    /// 关掉 stdin：宿主退出就是这个动作，孩子必须跟着走。
    fn close_stdin(&mut self) {
        self.stdin = None;
    }

    fn wait_exit(&mut self, timeout: Duration) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Some(status.code().unwrap_or(-1));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn model_ids(result: &Value) -> Vec<String> {
    result["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| {
            item.get("id")
                .or_else(|| item.get("model"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn thread_id(result: &Value) -> String {
    result["thread"]["id"].as_str().unwrap().to_owned()
}

/// 日志的形状是应用侧判断「宿主有没有走 bridge」的唯一依据，不能悄悄改。
///
/// 应用侧读的是「事件名 `bridge-started` + 时间戳」这两项（见
/// `switch-core` 的 `codex::coexist::last_start_unix`）。改名或去掉时间戳，
/// 会让那个判断永远返回「无法确认」——所以这里把它钉住。
#[test]
fn log_carries_the_start_event_with_a_timestamp() {
    let mut bridge = Bridge::start("log", true);
    bridge.result("1", "initialize", json!({}));
    let started = bridge
        .events()
        .into_iter()
        .find(|event| event["event"] == "bridge-started")
        .expect("日志里必须有 bridge-started");
    assert!(
        started["ts"].as_i64().unwrap_or(0) > 0,
        "bridge-started 必须带时间戳：{started}"
    );
    assert!(
        started["managedHome"].as_str().is_some(),
        "日志里要能看出托管 home 是哪一个"
    );
}

/// 同一个菜单：原生模型在前、我们目录的模型在后，且都能选。
#[test]
fn merges_both_catalogs_into_one_menu() {
    let mut bridge = Bridge::start("menu", true);
    let info = bridge.result("1", "initialize", json!({"clientInfo": {"name": "host"}}));
    assert!(
        info["codexHome"].as_str().unwrap().ends_with("native-home"),
        "initialize 要回原生那根的结果（用户的真实环境）：{info}"
    );

    let list = bridge.result(
        "2",
        "model/list",
        json!({"includeHidden": true, "limit": 100}),
    );
    let ids = model_ids(&list);
    assert_eq!(
        ids,
        vec![
            "gpt-5.6-sol",
            "gpt-5.5",
            "gs/mock-managed-1",
            "gs/mock-managed-2"
        ],
        "原生在前、我们的在后，且不重复：{ids:?}"
    );
    assert_eq!(list["nextCursor"], Value::Null, "合并后的列表是完整的一份");
}

/// 选官方模型 → 落到原生那根；选我们的 slug → 落到托管那根。
#[test]
fn routes_by_model_choice() {
    let mut bridge = Bridge::start("route", true);
    bridge.result("1", "initialize", json!({}));
    bridge.result("2", "model/list", json!({"limit": 100}));

    let native_thread = thread_id(&bridge.result(
        "3",
        "thread/start",
        json!({"model": "gpt-5.6-sol", "cwd": "/tmp"}),
    ));
    assert!(
        native_thread.starts_with("native-"),
        "官方模型必须落在原生那根：{native_thread}"
    );

    let managed_thread = thread_id(&bridge.result(
        "4",
        "thread/start",
        json!({"model": "gs/mock-managed-1", "cwd": "/tmp"}),
    ));
    assert!(
        managed_thread.starts_with("managed-"),
        "我们的 slug 必须落在托管那根：{managed_thread}"
    );

    let routed: Vec<String> = bridge
        .events()
        .iter()
        .filter(|event| event["event"] == "routing" && event["method"] == "thread/start")
        .map(|event| {
            format!(
                "{}:{}",
                event["child"].as_str().unwrap(),
                event["why"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        routed,
        vec!["native:model-is-native", "managed:model-is-ours"],
        "路由理由要留在日志里，出错时才有得查"
    );
}

/// 关键回归：只带 threadId 的续接必须跟着线程走，不看模型。
///
/// 不钉住的话，一个已开的会话会在两根之间跳，表现是「聊到一半上下文丢了」。
#[test]
fn follows_the_thread_when_no_model_is_given() {
    let mut bridge = Bridge::start("pinned", true);
    bridge.result("1", "initialize", json!({}));
    bridge.result("2", "model/list", json!({"limit": 100}));
    let managed_thread =
        thread_id(&bridge.result("3", "thread/start", json!({"model": "gs/mock-managed-1"})));

    let resumed = bridge.result("4", "thread/resume", json!({"threadId": managed_thread}));
    assert_eq!(
        resumed["thread"]["mockSide"], "managed",
        "续接必须回到托管那根：{resumed}"
    );
}

/// 侧栏也要两边都看得到：只顾菜单不顾列表，会让用我们模型开的对话从侧栏消失。
#[test]
fn merges_thread_lists_and_remembers_ownership() {
    let mut bridge = Bridge::start("threads", true);
    bridge.result("1", "initialize", json!({}));
    let list = bridge.result("2", "thread/list", json!({"limit": 100}));
    let ids: Vec<String> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(ids, vec!["native-thread-1", "managed-thread-1"], "{list}");

    // 列表已经告诉过我们归属：此后不带模型的调用不需要探测。
    let resumed = bridge.result(
        "3",
        "thread/resume",
        json!({"threadId": "managed-thread-1"}),
    );
    assert_eq!(resumed["thread"]["mockSide"], "managed");
    let last = bridge
        .events()
        .iter()
        .rev()
        .find(|event| event["event"] == "routing" && event["method"] == "thread/resume")
        .cloned()
        .unwrap();
    assert_eq!(last["why"], "thread-pinned", "列表已经给过答案，不该再探测");
}

/// 冷路径：bridge 重启过，宿主直接按 id 打开一条我们没见过的线程。
#[test]
fn probes_the_managed_side_for_an_unknown_thread() {
    let mut bridge = Bridge::start("cold", true);
    bridge.result("1", "initialize", json!({}));
    // 没有 thread/list、也没有 thread/start：这条线程只能是问出来的。
    let resumed = bridge.result(
        "2",
        "thread/resume",
        json!({"threadId": "managed-thread-9"}),
    );
    assert_eq!(resumed["thread"]["mockSide"], "managed");
    let last = bridge
        .events()
        .iter()
        .rev()
        .find(|event| event["event"] == "routing" && event["method"] == "thread/resume")
        .cloned()
        .unwrap();
    assert_eq!(last["why"], "thread-probed");
    assert_eq!(last["child"], "managed");
}

/// 子进程如实报错时，bridge 不能把它变成「看起来成功」。
#[test]
fn surfaces_child_errors_instead_of_faking_success() {
    let mut bridge = Bridge::start("errors", true);
    bridge.result("1", "initialize", json!({}));
    bridge.result("2", "model/list", json!({"limit": 100}));
    let managed_thread =
        thread_id(&bridge.result("3", "thread/start", json!({"model": "gs/mock-managed-1"})));
    // mock 不认识的调用 → 子进程回错误 → bridge 必须原样告诉宿主。
    let message = bridge.call(
        "4",
        "thread/compact/start",
        json!({"threadId": managed_thread}),
    );
    assert!(
        message.get("error").is_some(),
        "子进程的错误不能被吞掉：{message}"
    );
}

/// 托管那根没有配置时退化成纯透传：宁可什么都不做，也不给一份「能选、一发就失败」的菜单。
#[test]
fn degrades_to_passthrough_without_a_managed_home() {
    let mut bridge = Bridge::start("passthrough", false);
    let list = bridge.result("1", "model/list", json!({"limit": 100}));
    let ids = model_ids(&list);
    assert_eq!(
        ids,
        vec!["gpt-5.6-sol", "gpt-5.5"],
        "没有托管 home 时只该看到原生那根：{ids:?}"
    );
    assert!(bridge
        .events()
        .iter()
        .any(|event| event["event"] == "no-managed-home-passthrough"));
}

/// 宿主退出（stdin 关闭）时 bridge 必须跟着退出，否则每重启一次宿主就多两根孤儿进程。
#[test]
fn exits_when_the_host_closes_stdin() {
    let mut bridge = Bridge::start("lifetime", true);
    bridge.result("1", "initialize", json!({}));
    bridge.close_stdin();
    assert_eq!(
        bridge.wait_exit(Duration::from_secs(10)),
        Some(0),
        "stdin 关闭后 bridge 必须自己退出"
    );
}

/// 防自我复制：`GPTSWITCH_BRIDGE_CODEX` 指回 bridge 自己时必须停下并说清原因。
///
/// 没有这道闸，bridge 会一层层自我复制，直到把机器吃光。
#[test]
fn refuses_to_nest() {
    let output = Command::new(BRIDGE)
        .args(HOST_ARGV)
        .env("GPTSWITCH_BRIDGE_CODEX", BRIDGE)
        .env("GPTSWITCH_BRIDGE_INSIDE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("嵌套"), "{stderr}");
}
