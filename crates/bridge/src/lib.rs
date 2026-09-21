//! Switchelp Bridge：让官方订阅模型与我们代理的模型出现在**同一份菜单**里。
//!
//! 为什么需要两根子进程：`model_provider` 是**进程级**配置——一个 codex 进程只有一套上游
//! 设置。所以「选官方模型走账号登录、选我们的模型走供应商 Key」不可能在同一个进程里做到。
//! 这个 bridge 顶替宿主眼里的 codex CLI（`CODEX_CLI_PATH`），起**两根**真正的 codex：
//!
//! - 原生那根用用户真实的 `~/.codex`：登录态、官方 provider、历史会话原样不动；
//! - 托管那根用 Switchelp 的 CODEX_HOME：网关 provider + 我们发布的模型目录。
//!
//! 它做三件事：合并 `model/list`（原生在前、我们的在后）、合并 `thread/list`、按线程把每个
//! 会话钉在对应那一根上。第三件是关键——后续调用往往只带 threadId 不带模型，不钉住就会
//! 在两根之间跳，表现为「聊到一半上下文丢了」。
//!
//! 两条硬约束（与 [探针](../../../scripts/g0/codex-probe.mjs) 相同）：
//! **不记内容**（日志只记方法名、id、参数键名、路由结论，不记 payload——握手与后续消息里
//! 有账号凭据，落盘就是泄漏）、**不假装成功**（托管那根没就绪就如实报错，绝不返回一份
//! 「能选、一发就失败」的菜单）。

#[cfg(debug_assertions)]
mod mock;

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 握手超时：与宿主自己的容忍度无关，只用于「子进程根本没起来」这种情况。
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);
/// 列表类调用：菜单要等它。
const LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// 会话类调用：起线程、续接可能包含项目扫描。
const THREAD_TIMEOUT: Duration = Duration::from_secs(120);
/// 冷路径探测：只想问「这条线程是不是你的」，不该把宿主拖住。
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// 日志上限：超了就轮转一次，长跑的宿主不会把磁盘写满。
const LOG_LIMIT_BYTES: u64 = 2 * 1024 * 1024;

/// 喂给子进程的哨兵：它已经在 bridge 里了。
///
/// 用来挡住一种会让机器炸掉的配置错误——`GPTSWITCH_BRIDGE_CODEX` 指向 bridge 自己
/// （例如把 CODEX_CLI_PATH 又填了一遍）。没有这道闸，bridge 会一层层自我复制，
/// 直到把用户的内存吃光。
pub const INSIDE: &str = "GPTSWITCH_BRIDGE_INSIDE";

/// 托管那根没就绪时的错误码。JSON-RPC 的 server error 区间，宿主会如实显示。
pub const NO_MANAGED_HOME: i64 = -32603;

// ---------------------------------------------------------------- 配置

/// 运行配置。全部可注入，便于在真机上做对照实验。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeConfig {
    /// 真实的 codex CLI（由 Switchelp 从实例检测里填好）。
    pub codex: PathBuf,
    /// 原生那根的 CODEX_HOME，默认用户真实的 `~/.codex`。
    pub native_home: PathBuf,
    /// 托管那根的 CODEX_HOME。为空表示没配置——那就退化成纯透传。
    pub managed_home: Option<PathBuf>,
    /// 日志落点；为空则不打日志。
    pub log_path: Option<PathBuf>,
    /// 宿主传给 codex 的原始 argv。
    pub argv: Vec<String>,
    /// 测试用：子进程命令由 mock 顶替。
    pub mock: bool,
}

impl BridgeConfig {
    /// 从环境与 argv 组装。缺失关键项时返回可读原因，由调用方决定怎么失败。
    pub fn resolve(argv: Vec<String>, env: &HashMap<String, String>) -> Result<Self, String> {
        let codex = env
            .get("GPTSWITCH_BRIDGE_CODEX")
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| "缺少 GPTSWITCH_BRIDGE_CODEX（真实 codex CLI 的路径）".to_owned())?;
        let native_home = env
            .get("GPTSWITCH_BRIDGE_NATIVE_HOME")
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| env.get("CODEX_HOME").map(PathBuf::from))
            .or_else(|| home_dir().map(|home| home.join(".codex")))
            .ok_or_else(|| "无法确定原生 CODEX_HOME".to_owned())?;
        let managed_home = env
            .get("GPTSWITCH_BRIDGE_MANAGED_HOME")
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
        let log_path = env
            .get("GPTSWITCH_BRIDGE_LOG")
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
        let mock = env
            .get("GPTSWITCH_BRIDGE_MOCK")
            .map(|value| value == "1")
            .unwrap_or(false);
        Ok(Self {
            codex,
            native_home,
            managed_home,
            log_path,
            argv,
            mock,
        })
    }

    /// 这次调用是不是 app-server 会话。别的子命令（`--version` 之类）一律透明转发：
    /// 宿主验证 CLI 用的就是这类调用，顶替它却答不上版本，等于把宿主自己的检查推翻。
    pub fn is_app_server(&self) -> bool {
        self.argv.iter().any(|arg| arg == "app-server")
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 日志

/// JSON 行日志。**只记形状不记内容**。
pub struct Logger {
    path: Option<PathBuf>,
    written: AtomicU64,
    lock: Mutex<()>,
}

impl Logger {
    pub fn open(path: Option<&Path>) -> Self {
        if let Some(path) = path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(metadata) = std::fs::metadata(path) {
                if metadata.len() > LOG_LIMIT_BYTES {
                    let _ = std::fs::rename(path, path.with_extension("log.1"));
                }
            }
        }
        Self {
            path: path.map(Path::to_path_buf),
            written: AtomicU64::new(0),
            lock: Mutex::new(()),
        }
    }

    pub fn event(&self, entry: Value) {
        let Some(path) = &self.path else { return };
        let _guard = self.lock.lock();
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        else {
            return;
        };
        // 每条都带时间戳：应用侧靠它比较「bridge 是什么时候起来的」与「当前宿主进程
        // 是什么时候起来的」，从而判断宿主这次到底有没有走 bridge。
        let mut entry = entry;
        if let Some(object) = entry.as_object_mut() {
            object.insert("ts".to_owned(), json!(now_unix()));
        }
        let line = format!("{entry}\n");
        if file.write_all(line.as_bytes()).is_ok() {
            let total = self.written.load(Ordering::Relaxed) + line.len() as u64;
            self.written.store(total, Ordering::Relaxed);
            if total > LOG_LIMIT_BYTES {
                drop(file);
                let _ = std::fs::rename(path, path.with_extension("log.1"));
                self.written.store(0, Ordering::Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------- 消息形状

/// 从列表类结果里取数组：不同方法的字段名不同，都认。
pub fn list_of(result: &Value) -> Vec<Value> {
    for field in ["data", "models", "threads"] {
        if let Some(items) = result.get(field).and_then(Value::as_array) {
            return items.clone();
        }
    }
    Vec::new()
}

fn list_field(result: &Value) -> &'static str {
    for field in ["data", "models", "threads"] {
        if result.get(field).map(Value::is_array).unwrap_or(false) {
            return field;
        }
    }
    "data"
}

/// 合并两份列表：主那根在前、从那根在后，同一 id 只留一份。
///
/// 顺序是有意的：原生模型排在前面（用户平时用的就是它们），我们发布的那几条跟在后面。
/// 分页字段一律清空——合并后的列表是完整的一份，留着游标会让宿主以为还有下一页。
pub fn merge_lists(primary: &Value, secondary: &Value) -> Value {
    let mut merged: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for item in list_of(primary).into_iter().chain(list_of(secondary)) {
        let Some(key) = item_key(&item) else { continue };
        if !seen.insert(key) {
            continue;
        }
        merged.push(item);
    }
    let mut out = primary.clone();
    let field = list_field(primary);
    if let Some(object) = out.as_object_mut() {
        object.insert(field.to_owned(), Value::Array(merged));
        object.insert("nextCursor".to_owned(), Value::Null);
        object.insert("cursor".to_owned(), Value::Null);
    }
    out
}

/// 列表项的身份：模型用 id/model/slug，线程用 id。
pub fn item_key(item: &Value) -> Option<String> {
    for field in ["id", "model", "slug"] {
        if let Some(value) = item.get(field) {
            match value {
                Value::String(text) if !text.is_empty() => return Some(text.clone()),
                Value::Number(number) => return Some(number.to_string()),
                _ => {}
            }
        }
    }
    None
}

/// 线程 id 在几个不同位置出现过；bridge 归一后记住。
pub fn thread_id_of(result: &Value) -> Option<String> {
    if let Some(id) = result
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
    {
        return Some(id.to_owned());
    }
    for field in ["threadId", "id"] {
        if let Some(id) = result.get(field).and_then(Value::as_str) {
            return Some(id.to_owned());
        }
    }
    None
}

/// 请求里选的模型：字段名按版本不同，都认。
pub fn model_of(message: &Value) -> Option<String> {
    let params = message.get("params")?;
    for field in ["model", "modelId", "modelSlug"] {
        if let Some(value) = params.get(field).and_then(Value::as_str) {
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn thread_of(message: &Value) -> Option<String> {
    message
        .get("params")?
        .get("threadId")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn method_of(message: &Value) -> String {
    message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// 参数键名，用于诊断。**不记值。**
fn param_keys(message: &Value) -> Vec<String> {
    let mut keys: Vec<String> = message
        .get("params")
        .and_then(Value::as_object)
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

// ---------------------------------------------------------------- 路由

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Native,
    Managed,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Native => "native",
            Side::Managed => "managed",
        }
    }
}

/// 路由理由：诊断里必须能看出「为什么走了这根」，否则出错时无从下手。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    ThreadPinned,
    ThreadProbed,
    ModelIsOurs,
    ModelIsNative,
    Default,
}

impl Why {
    pub fn name(self) -> &'static str {
        match self {
            Why::ThreadPinned => "thread-pinned",
            Why::ThreadProbed => "thread-probed",
            Why::ModelIsOurs => "model-is-ours",
            Why::ModelIsNative => "model-is-native",
            Why::Default => "default",
        }
    }
}

/// 路由状态：线程归属 + 我们目录里的 slug。两根子进程各有一份自己的历史与菜单，
/// 只有这张表能把它们缝成「一个宿主眼里的一份状态」。
#[derive(Default)]
pub struct Routing {
    pins: HashMap<String, Side>,
    managed_keys: HashSet<String>,
}

impl Routing {
    pub fn pin(&mut self, thread_id: &str, side: Side) {
        self.pins.insert(thread_id.to_owned(), side);
    }

    pub fn pinned(&self, thread_id: &str) -> Option<Side> {
        self.pins.get(thread_id).copied()
    }

    /// 记住我们目录里的模型标识（slug/id）。菜单合并时顺手记下来。
    pub fn remember_managed(&mut self, items: &[Value]) {
        for item in items {
            if let Some(key) = item_key(item) {
                self.managed_keys.insert(key);
            }
        }
    }

    /// 只按线程与模型判断，不做探测。探测在 `resolve_side` 里单独走。
    pub fn direct(&self, message: &Value) -> (Side, Why) {
        if let Some(thread_id) = thread_of(message) {
            if let Some(side) = self.pinned(&thread_id) {
                return (side, Why::ThreadPinned);
            }
        }
        if let Some(model) = model_of(message) {
            return if self.managed_keys.contains(&model) {
                (Side::Managed, Why::ModelIsOurs)
            } else {
                (Side::Native, Why::ModelIsNative)
            };
        }
        (Side::Native, Why::Default)
    }

    /// 需要探测的冷路径：带 threadId 但我们没见过这条线程（例如 bridge 重启过）。
    pub fn needs_probe(&self, message: &Value) -> Option<String> {
        let thread_id = thread_of(message)?;
        if self.pins.contains_key(&thread_id) {
            return None;
        }
        Some(thread_id)
    }
}

// ---------------------------------------------------------------- 子进程

/// 子进程里冒出来的一行。
pub struct ChildLine {
    pub side: Side,
    pub raw: String,
    pub parsed: Option<Value>,
}

struct ChildHandle {
    side: Side,
    home: PathBuf,
    process: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>,
    seq: AtomicU64,
}

impl fmt::Debug for ChildHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChildHandle")
            .field("side", &self.side)
            .field("home", &self.home)
            .finish()
    }
}

impl ChildHandle {
    fn spawn(
        side: Side,
        codex: &Path,
        argv: &[String],
        home: &Path,
        mock: bool,
        log: Arc<Logger>,
    ) -> Result<(Arc<Self>, mpsc::Receiver<ChildLine>), String> {
        let mut command = Command::new(codex);
        command.args(argv);
        if mock {
            // 测试用：同一个二进制顶替 app-server，按 CODEX_HOME 扮演某一根。
            command.arg("--mock-app-server");
        }
        let mut child = command
            .current_dir("/")
            .env("CODEX_HOME", home)
            .env(INSIDE, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("启动 {} 那根失败：{error}", side.name()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("{} 那根拿不到 stdin", side.name()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("{} 那根拿不到 stdout", side.name()))?;
        if let Some(stderr) = child.stderr.take() {
            let log = log.clone();
            let name = side.name();
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    log.event(
                        json!({"event": "child-stderr", "child": name, "length": line.len()}),
                    );
                }
            });
        }

        let (sender, receiver) = mpsc::channel::<ChildLine>();
        let pending: Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        {
            let pending = pending.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let parsed = serde_json::from_str::<Value>(trimmed).ok();
                    if let Some(message) = &parsed {
                        if let Some(id) = message.get("id") {
                            let key = id_key(id);
                            let waiter = pending.lock().ok().and_then(|mut map| map.remove(&key));
                            if let Some(waiter) = waiter {
                                let _ = waiter.send(message.clone());
                                continue;
                            }
                        }
                    }
                    // 通知，或者我们没见过的 id：一律转给宿主，和「没有 bridge」时看到的一样。
                    if sender
                        .send(ChildLine {
                            side,
                            raw: trimmed.to_owned(),
                            parsed,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }

        Ok((
            Arc::new(Self {
                side,
                home: home.to_path_buf(),
                process: Mutex::new(child),
                stdin: Mutex::new(stdin),
                pending,
                seq: AtomicU64::new(0),
            }),
            receiver,
        ))
    }

    /// 发一条请求并等它自己那条响应。子进程侧的 id 由 bridge 合成：宿主给的 id 只属于
    /// 宿主与 bridge 之间，重用它去问两根子进程会让响应认错门。
    fn request(&self, message: &Value, timeout: Duration, log: &Logger) -> Result<Value, String> {
        let method = method_of(message);
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let child_id = format!("b-{}-{seq}", self.side.name());
        let key = id_key(&Value::String(child_id.clone()));
        let mut outbound = message.clone();
        if let Some(object) = outbound.as_object_mut() {
            object.insert("id".to_owned(), Value::String(child_id));
        }
        let (sender, receiver) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "pending 表不可用".to_owned())?
            .insert(key.clone(), sender);
        self.write(&outbound)?;

        match receiver.recv_timeout(timeout) {
            Ok(mut reply) => {
                // 回给宿主的是宿主自己那个 id；子进程的 id 不许漏出去。
                if let (Some(host_id), Some(object)) = (message.get("id"), reply.as_object_mut()) {
                    object.insert("id".to_owned(), host_id.clone());
                }
                if let Some(error) = reply.get("error") {
                    log.event(json!({
                        "event": "child-error",
                        "child": self.side.name(),
                        "method": method,
                        "code": error.get("code"),
                    }));
                    return Err(error_message(error));
                }
                Ok(reply.get("result").cloned().unwrap_or(Value::Null))
            }
            Err(_) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&key);
                }
                log.event(json!({
                    "event": "child-timeout",
                    "child": self.side.name(),
                    "method": method,
                }));
                Err(format!("{} 超时：{method}", self.side.name()))
            }
        }
    }

    fn write(&self, message: &Value) -> Result<(), String> {
        let line = format!("{message}\n");
        let mut stdin = self.stdin.lock().map_err(|_| "stdin 不可用".to_owned())?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("写入 {} 那根失败：{error}", self.side.name()))
    }

    fn kill(&self) {
        if let Ok(mut child) = self.process.lock() {
            let _ = child.kill();
        }
    }
}

fn id_key(id: &Value) -> String {
    match id {
        Value::String(text) => format!("s:{text}"),
        other => format!("v:{other}"),
    }
}

fn error_message(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("子进程报错")
        .chars()
        .take(200)
        .collect()
}

// ---------------------------------------------------------------- 主循环

/// multiplex 模式的入口。返回进程退出码。
pub fn run_multiplex(config: &BridgeConfig) -> i32 {
    let log = Arc::new(Logger::open(config.log_path.as_deref()));
    log.event(json!({
        "event": "bridge-started",
        "argv": config.argv,
        "nativeHome": config.native_home.display().to_string(),
        "managedHome": config.managed_home.as_ref().map(|path| path.display().to_string()),
    }));

    let Some(managed_home) = config.managed_home.clone() else {
        // 没有托管 home 就退化成纯透传：宁可什么都不做，也不要做出一个
        // 「菜单里能选、一发请求就失败」的半成品。
        log.event(json!({"event": "no-managed-home-passthrough"}));
        return run_passthrough(config);
    };

    let (native, native_rx) = match ChildHandle::spawn(
        Side::Native,
        &config.codex,
        &config.argv,
        &config.native_home,
        config.mock,
        log.clone(),
    ) {
        Ok(pair) => pair,
        Err(error) => {
            log.event(json!({"event": "spawn-failed", "child": "native", "message": error}));
            return 127;
        }
    };
    let (managed, managed_rx) = match ChildHandle::spawn(
        Side::Managed,
        &config.codex,
        &config.argv,
        &managed_home,
        config.mock,
        log.clone(),
    ) {
        Ok(pair) => pair,
        Err(error) => {
            log.event(json!({"event": "spawn-failed", "child": "managed", "message": error}));
            native.kill();
            return 127;
        }
    };
    log.event(json!({"event": "children-started"}));

    // 两根子进程的通知走同一条管道出去：宿主按 threadId 过滤自己关心的事件。
    let (forward_tx, forward_rx) = mpsc::channel::<ChildLine>();
    for receiver in [native_rx, managed_rx] {
        let sender = forward_tx.clone();
        thread::spawn(move || {
            while let Ok(line) = receiver.recv() {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
    }
    drop(forward_tx);

    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    {
        let stdout = stdout.clone();
        let log = log.clone();
        thread::spawn(move || {
            while let Ok(line) = forward_rx.recv() {
                // 不是 JSON 的字节也照原样转过去：那本来就会出现在宿主的 stdout 上，
                // 悄悄吞掉等于改变了「直接跑 codex」的行为。
                if write_line(&stdout, &line.raw).is_err() {
                    return;
                }
                if line.parsed.is_none() {
                    log.event(json!({
                        "event": "child-non-json",
                        "child": line.side.name(),
                        "length": line.raw.len(),
                    }));
                }
            }
        });
    }

    let routing = Arc::new(Mutex::new(Routing::default()));
    let (host_tx, host_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if host_tx.send(line).is_err() {
                return;
            }
        }
        // stdin 关闭 = 宿主退出。孩子必须跟着走，否则每重启一次宿主就多两根孤儿进程。
        drop(host_tx);
    });

    let mut workers: Vec<thread::JoinHandle<()>> = Vec::new();
    for line in host_rx {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
            log.event(json!({"event": "host-non-json", "length": trimmed.len()}));
            continue;
        };
        // 每条宿主请求各起一个线程：宿主会并发发问（菜单、历史、探活），
        // 串行处理会让一个慢调用把整个界面拖住。
        let native = native.clone();
        let managed = managed.clone();
        let routing = routing.clone();
        let stdout = stdout.clone();
        let log = log.clone();
        workers.push(thread::spawn(move || {
            handle_host_message(message, &native, &managed, &routing, &stdout, &log);
        }));
    }

    log.event(json!({"event": "host-closed", "workers": workers.len()}));
    native.kill();
    managed.kill();
    0
}

fn write_line(stdout: &Arc<Mutex<std::io::Stdout>>, line: &str) -> std::io::Result<()> {
    let mut out = stdout
        .lock()
        .map_err(|_| std::io::Error::other("stdout 不可用"))?;
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

fn send_result(stdout: &Arc<Mutex<std::io::Stdout>>, id: &Value, result: Value) {
    let message = json!({"jsonrpc": "2.0", "id": id, "result": result});
    let _ = write_line(stdout, &message.to_string());
}

fn send_error(stdout: &Arc<Mutex<std::io::Stdout>>, id: &Value, code: i64, message: &str) {
    let payload = json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}});
    let _ = write_line(stdout, &payload.to_string());
}

fn handle_host_message(
    message: Value,
    native: &Arc<ChildHandle>,
    managed: &Arc<ChildHandle>,
    routing: &Arc<Mutex<Routing>>,
    stdout: &Arc<Mutex<std::io::Stdout>>,
    log: &Logger,
) {
    let method = method_of(&message);
    let Some(id) = message.get("id").cloned() else {
        // 宿主发来的通知（如 initialized）：两根都要知道，否则先收到的那根会一直等。
        let mut outbound = message.clone();
        if let Some(object) = outbound.as_object_mut() {
            object.remove("id");
        }
        for child in [native, managed] {
            let _ = child.write(&outbound);
        }
        return;
    };

    match method.as_str() {
        "initialize" => {
            // 两根都要先握手：app-server 在 initialize 之前会拒绝一切调用。
            // 回给宿主的是**原生那根**的结果——它代表用户真实的 home 与登录环境。
            let native_result = match native.request(&message, INITIALIZE_TIMEOUT, log) {
                Ok(result) => result,
                Err(error) => {
                    log.event(
                        json!({"event": "initialize-failed", "child": "native", "message": error}),
                    );
                    return send_error(stdout, &id, NO_MANAGED_HOME, &format!("bridge: {error}"));
                }
            };
            let mut managed_request = message.clone();
            if let Some(object) = managed_request.as_object_mut() {
                object.insert("id".to_owned(), Value::String("bridge-init".to_owned()));
            }
            match managed.request(&managed_request, INITIALIZE_TIMEOUT, log) {
                Ok(_) => log.event(json!({"event": "initialized", "children": 2})),
                Err(error) => log.event(json!({"event": "managed-init-failed", "message": error})),
            }
            send_result(stdout, &id, native_result);
        }
        "model/list" => {
            let native_result = match native.request(&message, LIST_TIMEOUT, log) {
                Ok(result) => result,
                Err(error) => {
                    return send_error(stdout, &id, NO_MANAGED_HOME, &format!("bridge: {error}"))
                }
            };
            // 托管那根出错不该拖垮菜单：如实记下来，只给原生那些，并留下证据。
            let managed_result =
                managed
                    .request(&message, LIST_TIMEOUT, log)
                    .unwrap_or_else(|error| {
                        log.event(json!({"event": "managed-list-failed", "message": error}));
                        json!({"data": []})
                    });
            let ours = list_of(&managed_result);
            if let Ok(mut state) = routing.lock() {
                state.remember_managed(&ours);
            }
            let merged = merge_lists(&native_result, &managed_result);
            log.event(json!({
                "event": "model-list-merged",
                "native": list_of(&native_result).len(),
                "managed": ours.len(),
                "merged": list_of(&merged).len(),
            }));
            send_result(stdout, &id, merged);
        }
        "thread/list" => {
            // 侧栏必须同时看得到两边的会话。只顾菜单不顾列表，会让用我们模型开的对话
            // 从侧栏消失——那比不能选更难解释。
            let native_result = match native.request(&message, LIST_TIMEOUT, log) {
                Ok(result) => result,
                Err(error) => {
                    return send_error(stdout, &id, NO_MANAGED_HOME, &format!("bridge: {error}"))
                }
            };
            let managed_result =
                managed
                    .request(&message, LIST_TIMEOUT, log)
                    .unwrap_or_else(|error| {
                        log.event(json!({"event": "managed-thread-list-failed", "message": error}));
                        json!({"data": []})
                    });
            if let Ok(mut state) = routing.lock() {
                for item in list_of(&native_result) {
                    if let Some(key) = item_key(&item) {
                        state.pin(&key, Side::Native);
                    }
                }
                for item in list_of(&managed_result) {
                    if let Some(key) = item_key(&item) {
                        state.pin(&key, Side::Managed);
                    }
                }
            }
            let merged = merge_lists(&native_result, &managed_result);
            log.event(json!({
                "event": "thread-list-merged",
                "native": list_of(&native_result).len(),
                "managed": list_of(&managed_result).len(),
                "merged": list_of(&merged).len(),
            }));
            send_result(stdout, &id, merged);
        }
        _ => route_request(message, id, native, managed, routing, stdout, log),
    }
}

fn route_request(
    message: Value,
    id: Value,
    native: &Arc<ChildHandle>,
    managed: &Arc<ChildHandle>,
    routing: &Arc<Mutex<Routing>>,
    stdout: &Arc<Mutex<std::io::Stdout>>,
    log: &Logger,
) {
    let method = method_of(&message);
    let (mut side, mut why) = routing
        .lock()
        .map(|state| state.direct(&message))
        .unwrap_or((Side::Native, Why::Default));

    // 冷路径：bridge 重启过，或者宿主直接按 id 打开一条我们没见过的线程。
    // 问一句托管那根「这条线程是不是你的」，问不到就交给原生——探测失败不改路由。
    let probe = routing
        .lock()
        .ok()
        .and_then(|state| state.needs_probe(&message));
    if let Some(thread_id) = probe {
        let probe_message = json!({
            "jsonrpc": "2.0",
            "id": "bridge-probe",
            "method": "thread/read",
            "params": {"threadId": thread_id},
        });
        if managed.request(&probe_message, PROBE_TIMEOUT, log).is_ok() {
            side = Side::Managed;
            why = Why::ThreadProbed;
            if let Ok(mut state) = routing.lock() {
                state.pin(&thread_id, Side::Managed);
            }
        }
    }

    let target = match side {
        Side::Native => native,
        Side::Managed => managed,
    };
    log.event(json!({
        "event": "routing",
        "method": method,
        "child": target.side.name(),
        "why": why.name(),
        "model": model_of(&message),
        // 只记键名：这些消息里有账号凭据，值落盘就是泄漏。
        "paramKeys": param_keys(&message),
    }));

    let timeout = if method.starts_with("thread/") || method.starts_with("turn/") {
        THREAD_TIMEOUT
    } else {
        LIST_TIMEOUT
    };
    match target.request(&message, timeout, log) {
        Ok(result) => {
            if matches!(
                method.as_str(),
                "thread/start" | "thread/resume" | "thread/fork"
            ) {
                if let Some(thread_id) = thread_id_of(&result) {
                    if let Ok(mut state) = routing.lock() {
                        state.pin(&thread_id, target.side);
                    }
                    log.event(json!({
                        "event": "routed",
                        "method": method,
                        "child": target.side.name(),
                        "threadId": thread_id,
                    }));
                }
            }
            send_result(stdout, &id, result);
        }
        Err(error) => {
            log.event(json!({"event": "route-failed", "method": method, "message": error}));
            send_error(stdout, &id, NO_MANAGED_HOME, &format!("bridge: {error}"));
        }
    }
}

// ---------------------------------------------------------------- 透传

/// 非 app-server 调用（`--version`、`--help`、将来别的子命令）：原样转发，不解析。
///
/// 宿主用这类调用验证「这个 CLI 行不行」。顶替了它却答不上版本，等于把宿主自己的
/// 检查推翻——那比不顶替更糟。
pub fn run_passthrough(config: &BridgeConfig) -> i32 {
    let mut command = Command::new(&config.codex);
    command.args(&config.argv);
    if config.mock {
        // 与 multiplex 同一处测试接缝：同一个二进制顶替 app-server。
        command.arg("--mock-app-server");
    }
    let mut child = match command
        .env(INSIDE, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return 127,
    };

    let mut handles = Vec::new();
    if let Some(mut target) = child.stdin.take() {
        handles.push(thread::spawn(move || {
            let mut source = std::io::stdin().lock();
            let _ = std::io::copy(&mut source, &mut target);
        }));
    }
    if let Some(mut source) = child.stdout.take() {
        handles.push(thread::spawn(move || {
            let mut target = std::io::stdout();
            let _ = std::io::copy(&mut source, &mut target);
            let _ = target.flush();
        }));
    }
    if let Some(mut source) = child.stderr.take() {
        handles.push(thread::spawn(move || {
            let mut target = std::io::stderr();
            let _ = std::io::copy(&mut source, &mut target);
            let _ = target.flush();
        }));
    }
    let status = child.wait();
    for handle in handles {
        let _ = handle.join();
    }
    status.ok().and_then(|status| status.code()).unwrap_or(1)
}

// ---------------------------------------------------------------- 入口

/// 进程入口。返回退出码。
pub fn run() -> i32 {
    let argv: Vec<String> = env::args().skip(1).collect();
    // 测试用：同一个二进制顶替 app-server。release 构建里没有这段。
    #[cfg(debug_assertions)]
    {
        if argv.iter().any(|arg| arg == "--mock-app-server") {
            return mock::run(&argv);
        }
    }
    let env_map: HashMap<String, String> = env::vars().collect();
    // 已经在 bridge 里了：`GPTSWITCH_BRIDGE_CODEX` 指回了自己。
    // 继续跑下去是无限自我复制，停下来并说清原因。
    if env_map.contains_key(INSIDE) {
        eprintln!(
            "gptswitch-bridge: 检测到嵌套调用（{INSIDE} 已存在）。GPTSWITCH_BRIDGE_CODEX 必须指向真正的 codex，而不是 bridge 自己。"
        );
        return 2;
    }
    let config = match BridgeConfig::resolve(argv, &env_map) {
        Ok(config) => config,
        Err(reason) => {
            eprintln!("gptswitch-bridge: {reason}");
            return 2;
        }
    };
    if config.is_app_server() {
        run_multiplex(&config)
    } else {
        run_passthrough(&config)
    }
}
