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
    dir: tempdir::Dir,
}

/// 极简临时目录（不引第三方依赖）：测试结束即删。
///
/// `shared` 为真时**不删**：几个实例共用同一个应用数据目录（宿主对每根 app-server
/// 各起一个 bridge），谁先析构都不能把别人的目录端掉。
mod tempdir {
    use std::path::{Path, PathBuf};

    pub struct Dir(pub PathBuf, pub bool);

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
            Self(path, false)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            if !self.1 {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}

impl Bridge {
    fn start(tag: &str, managed: bool) -> Self {
        Self::launch(tempdir::Dir::new(tag), tag, managed)
    }

    /// 在**已有的**应用数据目录里起一个实例。
    ///
    /// 宿主对每根 app-server 各起一个 bridge，它们共享同一个应用数据目录，
    /// 因此也共享 `bridge-model-state`。这个入口就是用来验证跨实例的选择记忆的。
    ///
    /// 拿到的是一份**不删**的句柄：目录的收尾交给建它的那个 `start`（它才是所有者）。
    fn start_in(dir: &tempdir::Dir, tag: &str, managed: bool) -> Self {
        Self::launch(tempdir::Dir(dir.path().to_path_buf(), true), tag, managed)
    }

    /// 目录**按值**传进来、最后由 `Bridge` 持有。
    ///
    /// 不能在前面那些入口里建一个局部 `Dir` 再借出去：那个局部变量在函数返回时就析构，
    /// 于是一个还活着的 bridge 子进程会顺手把它正在写的日志 `create` 回来，
    /// `remove_dir_all` 随即以 ENOTEMPTY 失败——本机因此攒过 142 个残留目录；
    /// 而在删得掉的机器上（CI）日志真的没了，`events()` 读到空文件，测试就红。
    /// 所有者必须活到 `Bridge::drop` 把孩子杀掉之后（字段在其后析构）。
    fn launch(dir: tempdir::Dir, tag: &str, managed: bool) -> Self {
        let native_home = dir.path().join("native-home");
        let managed_home = dir.path().join("codex-home");
        std::fs::create_dir_all(&native_home).unwrap();
        std::fs::create_dir_all(&managed_home).unwrap();
        let log = dir.path().join(format!("bridge-{tag}.log"));

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
            dir,
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
    /// 顺序要紧：先让孩子退出，`dir` 才轮到析构（字段在 `drop` 之后按声明顺序析构）。
    /// 孩子还活着就删目录的话，它会顺手把日志 `create` 回来，`remove_dir_all` 以
    /// ENOTEMPTY 失败，临时目录就留在盘上了（本机实测攒过 142 个）。
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

/// 关键回归：**选了网关模型之后，读回来的必须还是它**，不能被切回原生模型。
///
/// 这是用户报的那个症状本身。真机日志里它是这样发生的：宿主写 `model=gs/…` →
/// 被摘掉 → 宿主立刻 `config/read` → 读到原生模型 `gpt-6-luna` → 选择器自己勾回去。
/// 5 次点击，5 次回滚（10-17 12:32:40 / :45、12:33:06 / :39 / :43）。
///
/// 修复的判据就在这一条：写入还是被摘（原生配置不许被污染），但读回来的选择是用户的。
#[test]
fn a_gateway_selection_does_not_get_reverted_by_config_read() {
    let mut bridge = Bridge::start("selection-sticks", true);
    bridge.result("1", "initialize", json!({}));
    bridge.result("2", "model/list", json!({"limit": 100}));

    // 选之前：读到的就是原生模型。
    let before = bridge.result("3", "config/read", json!({"includeLayers": false}));
    assert_eq!(before["config"]["model"], json!("gpt-5.6-sol"));

    // 用户点了网关模型：这一笔照旧被摘掉，不许进原生配置。
    let written = bridge.result(
        "4",
        "config/batchWrite",
        json!({
            "edits": [{"keyPath": "model", "value": "gs/mock-managed-1", "mergeStrategy": "replace"}],
            "expectedVersion": Value::Null,
            "filePath": "/Users/someone/.codex/config.toml",
            "reloadUserConfig": true
        }),
    );
    assert_eq!(
        written["mockSide"], "native",
        "写入仍然落在原生那根：{written}"
    );
    assert_eq!(
        written["editsReceived"], 0,
        "网关模型那一笔必须被摘掉：{written}"
    );

    // 宿主写完立刻重读（真机上每次都紧跟一次）——**读到的必须是刚选的那个**。
    let after = bridge.result("5", "config/read", json!({"includeLayers": false}));
    assert_eq!(
        after["config"]["model"],
        json!("gs/mock-managed-1"),
        "读回来还得是用户选的那个网关模型，否则界面会自己切回原生：{after}"
    );
    // provider 必须跟着换：宿主开线程时把 `model` 与 `modelProvider` 一起发给子进程，
    // 留着原生那根的 provider 就是「选是选上了、一发就报 provider not found」。
    assert_eq!(
        after["config"]["model_provider"],
        json!("gptswitch"),
        "provider 不跟着盖，托管那根会拒收：{after}"
    );
    // 只改路由字段，别的字段与 origins 结构都得留着。
    assert_eq!(after["config"]["model_reasoning_effort"], json!("high"));
    assert_eq!(after["origins"]["model"]["name"]["type"], json!("user"));
    assert_eq!(
        after["origins"]["model_provider"]["version"],
        json!("bridge/selection")
    );

    // 用这个选择开对话：必须路由进托管那根，而不是又走原生账号额度。
    // `modelProvider` 按宿主的行为带上——值就是刚从 `config/read` 读到的那个。
    let thread = bridge.result(
        "6",
        "thread/start",
        json!({
            "model": "gs/mock-managed-1",
            "modelProvider": after["config"]["model_provider"],
        }),
    );
    assert_eq!(
        thread["thread"]["mockSide"], "managed",
        "开出来的线程要在托管那根：{thread}"
    );

    // 选回原生模型：选择要被清掉，此后读到的就是原生模型，不能一直被盖回去。
    bridge.result(
        "7",
        "config/batchWrite",
        json!({
            "edits": [{"keyPath": "model", "value": "gpt-5.6-sol", "mergeStrategy": "replace"}],
            "expectedVersion": Value::Null,
            "filePath": "/Users/someone/.codex/config.toml",
            "reloadUserConfig": true
        }),
    );
    let back = bridge.result("8", "config/read", json!({"includeLayers": false}));
    assert_eq!(
        back["config"]["model"],
        json!("gpt-5.6-sol"),
        "要能回到原生模型：{back}"
    );
}

/// 选择要跨 bridge 实例活着：宿主对每根 app-server 各起一个 bridge，
/// **写模型的那根与读 config 的那根未必是同一个进程**。只放内存会「写了不算数」。
#[test]
fn the_selection_survives_a_bridge_restart() {
    let mut first = Bridge::start("selection-restart", true);
    first.result("1", "initialize", json!({}));
    first.result("2", "model/list", json!({"limit": 100}));
    first.result(
        "3",
        "config/batchWrite",
        json!({
            "edits": [{"keyPath": "model", "value": "gs/mock-managed-2", "mergeStrategy": "replace"}],
            "expectedVersion": Value::Null,
            "filePath": "/Users/someone/.codex/config.toml",
            "reloadUserConfig": true
        }),
    );
    let state = first.dir.path().join("bridge-model-state");
    assert!(state.exists(), "选择要落在 bridge.log 旁边的状态文件里");
    first.close_stdin();
    first.wait_exit(Duration::from_secs(10));

    // 同一个应用数据目录（同一个状态文件）里新起一个实例，扮演「读 config 的那根」。
    let mut second = Bridge::start_in(&first.dir, "selection-restart-2", true);
    second.result("1", "initialize", json!({}));
    let read = second.result("2", "config/read", json!({"includeLayers": false}));
    assert_eq!(
        read["config"]["model"],
        json!("gs/mock-managed-2"),
        "新实例没读回选择，宿主就会把界面切回原生：{read}"
    );
    assert_eq!(
        read["config"]["model_provider"],
        json!("gptswitch"),
        "provider 也得从状态文件那一侧对齐，否则新实例开线程会被托管那根拒收：{read}"
    );
}

/// 网关模型落进原生配置的**冷启动口子**：菜单还没到，判定不能是瞎的。
///
/// 真机 10-13 20:07:55 就这样漏过一次：`model/list` 还没被调用，那份写入被原样放行，
/// 网关模型进了 `~/.codex/config.toml`。`gs/` 前缀本身就该被认出来。
#[test]
fn a_gateway_write_is_caught_before_the_menu_arrives() {
    let mut bridge = Bridge::start("cold-start", true);
    bridge.result("1", "initialize", json!({}));
    // 故意不调 model/list：这时名单还是空的。
    let written = bridge.result(
        "2",
        "config/batchWrite",
        json!({
            "edits": [{"keyPath": "model", "value": "gs/mock-managed-1", "mergeStrategy": "replace"}],
            "expectedVersion": Value::Null,
            "filePath": "/Users/someone/.codex/config.toml",
            "reloadUserConfig": true
        }),
    );
    assert_eq!(
        written["editsReceived"], 0,
        "没有菜单时也必须认出网关模型，否则它直接落进原生配置：{written}"
    );
}

/// 平铺写入（`config/value/write`）是同一个病的另一个口子。
#[test]
fn flat_config_writes_cannot_smuggle_our_provider_in() {
    let mut bridge = Bridge::start("flat-write", true);
    bridge.result("1", "initialize", json!({}));
    bridge.result("2", "model/list", json!({"limit": 100}));
    // 平铺写 gateway provider 名：这是我们托管那根的 provider，不属于原生配置。
    bridge.result(
        "3",
        "config/value/write",
        json!({
            "keyPath": "model_provider",
            "value": "gptswitch",
            "mergeStrategy": "replace",
            "filePath": "/Users/someone/.codex/config.toml",
        }),
    );
    let sanitized = bridge
        .events()
        .into_iter()
        .filter(|event| event["event"] == "config-write-sanitized")
        .collect::<Vec<_>>();
    assert_eq!(
        sanitized.len(),
        1,
        "平铺写入也要留下拦下的证据：{sanitized:?}"
    );
    assert_eq!(sanitized[0]["method"], json!("config/value/write"));
    assert_eq!(sanitized[0]["dropped"], json!(1));
}

/// 桌面端会把模型选择写进它看到的 `config.toml`；桥接里那份是**原生那根**的。
///
/// 网关模型不能落进原生配置：写进去就成了「账号 provider + 网关模型」，之后按老路由
/// 发送必然撞上游 400（`The '<model>' model is not supported when using Codex with a
/// ChatGPT account.`）。这一笔必须被摘掉，其余键照常落盘。
#[test]
fn config_writes_never_carry_our_models_into_the_native_config() {
    let mut bridge = Bridge::start("config-write", true);
    bridge.result("1", "initialize", json!({}));
    // 名单来自菜单合并：先取一次菜单，bridge 才知道哪些模型是我们的。
    let models = bridge.result("2", "model/list", json!({"limit": 100}));
    assert!(model_ids(&models).contains(&"gs/mock-managed-1".to_owned()));

    let written = bridge.result(
        "3",
        "config/batchWrite",
        json!({
            "edits": [
                {"keyPath": "model", "value": "gs/mock-managed-1", "mergeStrategy": "replace"},
                {"keyPath": "model_reasoning_effort", "value": "high", "mergeStrategy": "replace"}
            ],
            "expectedVersion": Value::Null,
            "filePath": "/Users/someone/.codex/config.toml",
            "reloadUserConfig": true
        }),
    );
    assert_eq!(
        written["mockSide"], "native",
        "配置写入仍然走原生那根：{written}"
    );
    assert_eq!(
        written["editsReceived"], 1,
        "网关模型那一笔必须被摘掉：{written}"
    );
    assert_eq!(written["modelSeen"], Value::Null);

    // 原生模型的写入照常落盘，一个字节都不动。
    let native = bridge.result(
        "4",
        "config/batchWrite",
        json!({
            "edits": [{"keyPath": "model", "value": "gpt-5.6-sol", "mergeStrategy": "replace"}],
            "expectedVersion": Value::Null,
            "filePath": "/Users/someone/.codex/config.toml",
            "reloadUserConfig": true
        }),
    );
    assert_eq!(native["modelSeen"], "gpt-5.6-sol");

    let sanitized = bridge
        .events()
        .into_iter()
        .find(|event| event["event"] == "config-write-sanitized")
        .expect("摘掉过一笔就必须留下证据");
    assert_eq!(sanitized["dropped"], 1);
    assert_eq!(sanitized["method"], "config/batchWrite");
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
