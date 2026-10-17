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

/// 托管那根的 provider 名。必须与 `switch-core` 的 `codex::config::PROVIDER_ID`
/// 一致——它是我们写进托管 profile 的 `[model_providers.<这个名字>]`，
/// 而宿主开线程时会把我们从 `config/read` 还回去的这个名字原样带给子进程。
pub const PROVIDER_ID: &str = "gptswitch";

/// 路由字段：这几个键的**归属权在我们**，原生那份配置里不该由宿主落我们的值。
///
/// 必须与 `switch-core` 的 `codex::config::MANAGED_KEYS` 保持一致——那份决定了
/// 「复制用户配置当托管 profile 时去掉哪些键」，两份名单不一致会让同一份配置在两个
/// 方向上都被改写。`crates/bridge/tests/multiplex.rs` 里有一条测试盯着这个对齐。
pub const MANAGED_KEYS: [&str; 5] = [
    "model",
    "model_provider",
    "model_catalog_json",
    "model_context_window",
    "model_reasoning_effort",
];

/// 这个 keyPath 的顶层键是不是路由字段。`model_providers.custom.name` 这类子路径
/// 不属于路由字段：它改的是供应商表本身，宿主有权落盘。
fn is_managed_key_path(key_path: &str) -> bool {
    MANAGED_KEYS.contains(&key_path)
}

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

/// 路由状态：线程归属 + 我们目录里的 slug + 宿主当前的模型选择。两根子进程各有一份
/// 自己的历史与菜单，只有这张表能把它们缝成「一个宿主眼里的一份状态」。
#[derive(Default)]
pub struct Routing {
    pins: HashMap<String, Side>,
    managed_keys: HashSet<String>,
    /// 宿主模型选择器当前选中的模型；只记我们目录里的，选原生模型时是 None。
    ///
    /// 为什么还要落一份状态文件：宿主对每根 app-server 各起一个 bridge 实例，
    /// **写模型的那根与读 config 的那根未必同一个**——只放内存会「写了不算数」。
    selection: Option<String>,
    state_path: Option<PathBuf>,
}

impl Routing {
    /// 跨进程的选择记忆落点：`<bridge.log 同目录>/bridge-model-state`。
    /// bridge.log 在应用数据目录里（`coexist::bridge_log`），与托管 home 同一屋檐。
    pub fn state_path_for(log_path: Option<&Path>) -> Option<PathBuf> {
        log_path.map(|path| path.with_file_name("bridge-model-state"))
    }

    /// 启动时把上次的选择读回来。状态文件坏了一律当「没选过」，不猜。
    pub fn load_state(&mut self, state_path: Option<PathBuf>) {
        self.state_path = state_path;
        self.selection = self
            .state_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|raw| {
                let value = serde_json::from_str::<Value>(&raw).ok()?;
                value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
    }

    /// 记住宿主的模型选择（None = 选回了原生模型）。
    /// 写穿到状态文件：别的 bridge 实例也在服务同一个宿主，它们得看得见。
    pub fn set_selection(&mut self, model: Option<String>) {
        self.selection = model;
        if let Some(path) = &self.state_path {
            let payload = json!({"model": self.selection});
            let _ = std::fs::write(path, payload.to_string());
        }
    }

    /// 宿主当前选中的网关模型（没有就是 None）。
    pub fn selection(&self) -> Option<&str> {
        self.selection.as_deref()
    }

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

    /// 这个模型是不是我们目录里的（名单来自 `model/list` 的合并）。
    pub fn is_managed(&self, model: &str) -> bool {
        self.owns_model(model)
    }

    /// 只按线程与模型判断，不做探测。探测在 `resolve_side` 里单独走。
    ///
    /// **显式模型优先于线程归属**。这条顺序是被真机日志逼出来的：`thread/settings/update`
    /// 带着 `gs/…`（用户在这个对话里换成了网关模型）却因为线程早先被钉在原生那根，
    /// 被整条路由进原生子进程——那根根本没有这个模型，网关模型等于白选。
    /// 用户在这次请求里明确点名了一个模型，就该按模型走；只有当请求没带模型时，
    /// 线程归属才是唯一线索。
    pub fn direct(&self, message: &Value) -> (Side, Why) {
        if let Some(model) = model_of(message) {
            if self.owns_model(&model) {
                return (Side::Managed, Why::ModelIsOurs);
            }
            // 带的不是我们的模型：这是「在这个对话里换回原生模型」，宁可重钉也不要
            // 顺着旧钉子走到托管那根去。
            return (Side::Native, Why::ModelIsNative);
        }
        if let Some(thread_id) = thread_of(message) {
            if let Some(side) = self.pinned(&thread_id) {
                return (side, Why::ThreadPinned);
            }
        }
        (Side::Native, Why::Default)
    }

    /// 这个模型是不是我们的。判据**不能只看菜单名单**：菜单要等宿主调一次
    /// `model/list` 才填充，而配置写入可能早于它到达（真机 10-13 20:07:55 就发生过：
    /// 菜单还没来，网关模型那笔写入原样落进了原生配置）。`gs/` 前缀是发布布局的约定
    /// （与 `switch-core` 的 `MANAGED_KEYS` 判定同源），名单只是补充。
    pub fn owns_model(&self, model: &str) -> bool {
        model.starts_with("gs/") || self.managed_keys.contains(model)
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

    // 选择记忆要跨实例：宿主对每根 app-server 各起一个 bridge，写模型的那根与读 config
    // 的那根未必是同一个进程。只放内存会「写了不算数」，所以启动时从状态文件读回来。
    let mut routing = Routing::default();
    routing.load_state(Routing::state_path_for(config.log_path.as_deref()));
    let routing = Arc::new(Mutex::new(routing));
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
        "config/batchWrite" => {
            // 减掉网关模型那笔写入之后再走普通路由（见 sanitize_config_write 的说明）。
            let (sanitized, dropped, chosen) = routing
                .lock()
                .map(|state| sanitize_config_write(&message, &state))
                .unwrap_or_else(|_| (message.clone(), 0, None));
            // 宿主明确写了一个**原生模型**：那就是「选回原生」，记得的选择必须清掉，
            // 否则 config/read 会一直把它盖回去，用户根本回不到原生模型。
            let wrote_native_model = dropped == 0
                && batch_write_model(&message)
                    .map(|model| !model.starts_with("gs/"))
                    .unwrap_or(false);
            if dropped > 0 {
                // 选择被摘下了写入，但**不能就此丢掉**：记进路由状态，再由 config/read
                // 的改写还给宿主。摘写 + 读回改写，合起来才是一次「成功但落在另一根」的写入。
                if let (Ok(mut state), Some(model)) = (routing.lock(), chosen.clone()) {
                    state.set_selection(Some(model));
                }
            } else if wrote_native_model {
                if let Ok(mut state) = routing.lock() {
                    state.set_selection(None);
                }
            }
            if dropped > 0 || wrote_native_model {
                log.event(json!({
                    "event": "config-write-sanitized",
                    "method": method,
                    "dropped": dropped,
                    "remembered": chosen.is_some(),
                }));
            }
            route_request(sanitized, id, native, managed, routing, stdout, log);
        }
        "config/value/write" => {
            // 平铺写入是同一个病的另一个口子：`model_provider = "gptswitch"` 落在原生
            // 配置里照样把那一根写成自相矛盾状态。这里拦下值，理由同 batchWrite。
            let (sanitized, dropped, chosen) = routing
                .lock()
                .map(|state| sanitize_config_value_write(&message, &state))
                .unwrap_or_else(|_| (message.clone(), 0, None));
            if dropped > 0 {
                let remembered = chosen.is_some();
                if let (Ok(mut state), Some(model)) = (routing.lock(), chosen.clone()) {
                    state.set_selection(Some(model));
                }
                log.event(json!({
                    "event": "config-write-sanitized",
                    "method": method,
                    "dropped": dropped,
                    "remembered": remembered,
                }));
            }
            route_request(sanitized, id, native, managed, routing, stdout, log);
        }
        "config/read" => {
            // 宿主读配置：路由字段要盖回它自己选的那个网关模型。
            // 不这么做，界面在写完的选择被摘掉之后会把选择器勾回原生模型——
            // 那正是「选了第三方模型，又被切回原生」的直接原因。
            let chosen = routing
                .lock()
                .ok()
                .and_then(|state| state.selection().map(str::to_owned));
            let mut result = match native.request(&message, LIST_TIMEOUT, log) {
                Ok(result) => result,
                Err(error) => {
                    return send_error(stdout, &id, NO_MANAGED_HOME, &format!("bridge: {error}"))
                }
            };
            let overlaid = routing
                .lock()
                .map(|state| virtualize_config_read(&mut result, &state))
                .unwrap_or(false);
            if overlaid {
                log.event(json!({
                    "event": "config-read-virtualized",
                    "model": chosen,
                }));
            }
            send_result(stdout, &id, result);
        }
        _ => route_request(message, id, native, managed, routing, stdout, log),
    }
}

/// 宿主往配置里落盘的「模型选择」都从这里过：网关模型那笔写入要摘掉。
///
/// 桌面端把模型选择器里的选择写进它看到的 `config.toml`——桥接里那份是**原生那根**的。
/// 选的是我们目录里的模型（`gs/…`）时，这条写入会把原生配置写成一个自相矛盾的状态：
/// 账号 provider + 网关模型；之后任何一次按老路由发送都会撞上 ChatGPT 后端的 400
/// （The '<model>' model is not supported when using Codex with a ChatGPT account）。
/// 我们模型的那份选择由托管那根的配置记住，所以这里只减去这一笔，其余键照常落盘；
/// 全被减光时照样发一次空写入——空 `edits` 是合法请求，宿主拿到的是子进程的真实回执
/// （`{status:"ok", version, filePath, overriddenMetadata}`，已按报文实测）。
///
/// 桌面端落盘模型选择用的是 `config/batchWrite`（真机日志里只有这一种）；
/// `config/value/write` 是平铺的单条写入，形状里没有可减的余地，出现时原样转发。
///
/// 返回：改写后的报文、被摘掉的笔数、以及**被摘掉的那次模型选择**（如果有）。
/// 第三项是这次修复的关键——摘掉写入不能等于把这选择丢了，它得被记进 [`Routing`]，
/// 否则宿主读回旧值就会把界面切回去（见 `virtualize_config_read`）。
fn sanitize_config_write(message: &Value, routing: &Routing) -> (Value, usize, Option<String>) {
    let mut out = message.clone();
    let mut chosen: Option<String> = None;
    let Some(edits) = out
        .get_mut("params")
        .and_then(|params| params.get_mut("edits"))
        .and_then(Value::as_array_mut)
    else {
        return (out, 0, None);
    };
    let before = edits.len();
    edits.retain(|edit| {
        let Some(key) = edit.get("keyPath").and_then(Value::as_str) else {
            return true;
        };
        if !is_managed_key_path(key) {
            return true;
        }
        let value = edit.get("value").and_then(Value::as_str);
        // 值缺席（不是字符串）时不动它：我们只拦「明确写了我们的值」这一种。
        let Some(value) = value else {
            return true;
        };
        let ours = is_our_value(key, value, routing);
        if key == "model" && ours {
            chosen = Some(value.to_owned());
        }
        !ours
    });
    let dropped = before - edits.len();
    (out, dropped, chosen)
}

/// 平铺的单条写入（`config/value/write`）。形状里没有可减的余地，但**路由字段那几种
/// 值我们仍然要拦**：`model_provider = "gptswitch"` 或网关模型落进原生配置，
/// 与 `config/batchWrite` 那条是同一个病。拦下整条请求而不是改内容——改写单条写入会
/// 留下一个「宿主以为写了、其实没写」的谎，不如让它带着原值走空写入。
fn sanitize_config_value_write(
    message: &Value,
    routing: &Routing,
) -> (Value, usize, Option<String>) {
    let mut out = message.clone();
    let Some(params) = out.get_mut("params").and_then(Value::as_object_mut) else {
        return (out, 0, None);
    };
    let key = params
        .get("keyPath")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let value = params
        .get("value")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let (Some(key), Some(value)) = (key, value) else {
        return (out, 0, None);
    };
    if !is_managed_key_path(&key) || !is_our_value(&key, &value, routing) {
        return (out, 0, None);
    }
    let chosen = (key == "model").then(|| value.clone());
    // 摘成一笔空写入：值置空、并请求宿主重读。空写的回执形状与普通写一致。
    params.insert("value".to_owned(), Value::String(String::new()));
    (out, 1, chosen)
}

/// 这个值是不是「我们的」。
///
/// 判据**按键区分**，因为这几个键装的不是同一种东西：`model` 装模型标识（`gs/…`），
/// `model_provider` 装 provider 名（`gptswitch`），`model_catalog_json` 装我们目录里
/// 那份 json 的**路径**。拿一个统一的字符串前缀去套所有键，会漏掉后两种——真机
/// 10-13 那次正是 `model_provider` 这类值漏进原生配置。
fn is_our_value(key: &str, value: &str, routing: &Routing) -> bool {
    if value.is_empty() {
        // 空值 = 宿主在删这个键。我们的路由字段不许被它从配置里抹掉。
        return true;
    }
    match key {
        "model" => routing.owns_model(value),
        "model_provider" => value == PROVIDER_ID,
        "model_catalog_json" => value.contains("gptswitch") || value.contains("/catalogs/"),
        _ => false,
    }
}

/// `config/batchWrite` 里 `model` 那笔的值（不管是不是我们的）。用来判断宿主这次
/// 是不是在「选回原生模型」——那种情况要把记得的选择清掉。
fn batch_write_model(message: &Value) -> Option<String> {
    message
        .get("params")?
        .get("edits")?
        .as_array()?
        .iter()
        .find_map(|edit| {
            let key = edit.get("keyPath").and_then(Value::as_str)?;
            let value = edit.get("value").and_then(Value::as_str)?;
            (key == "model").then(|| value.to_owned())
        })
}

/// 把 `config/read` 结果里的路由字段**盖回宿主选中的那个网关模型**。
///
/// 这是整条修复链上最后、也是不可省的一环。只拦写入是不够的：宿主写完立刻重读，
/// 读到的还是原生的旧值，选择器就自己勾回去（真机日志里 `config/batchWrite` 后面
/// 永远紧跟 `config/read`，5 次点击 5 次回滚）。所以「摘掉写入」必须配一个
/// 「读回时把选择还给它」，这两件事合起来才等价于一次成功的写入。
///
/// **`model` 与 `model_provider` 必须一起盖，只盖一个比不盖更糟。**
/// 宿主把这一对当成「我要用哪个上游」的完整答案，开线程时原样发给子进程
/// （真机 `thread/start` 的 params 里 `model` 与 `modelProvider` 相邻出现）。
/// 只盖 `model` 的话，原生那根的 provider（用户配置里是 `custom`）会跟着网关模型
/// 一起进托管子进程，而托管 profile 里只有 `[model_providers.gptswitch]`——
/// 子进程当场报 "Model provider `custom` not found"，用户看到的是「选是选上了，
/// 一发就错」。已按报文实测：`modelProvider=custom` 必失败，`gptswitch` 才开得出线程。
///
/// 只改这两个字段，其余原样透传；`origins` 里对应的那两条也要跟上，
/// 否则宿主可能拿 origins 判定「值来自哪个文件」，看到原生文件却对不上号。
/// `origins.model` 是对象（`{name:{type,file,profile}, version}`，已按真机报文核对），
/// 不是字符串；形状不认识时整块 origins 不动——宁可少改一处，也不要把结构写坏。
fn virtualize_config_read(result: &mut Value, routing: &Routing) -> bool {
    let Some(chosen) = routing.selection().map(str::to_owned) else {
        return false;
    };
    let Some(config) = result.get_mut("config").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut overlaid: Vec<&'static str> = Vec::new();
    if config.get("model").and_then(Value::as_str) != Some(chosen.as_str()) {
        config.insert("model".to_owned(), Value::String(chosen.clone()));
        overlaid.push("model");
    }
    if config.get("model_provider").and_then(Value::as_str) != Some(PROVIDER_ID) {
        config.insert(
            "model_provider".to_owned(),
            Value::String(PROVIDER_ID.to_owned()),
        );
        overlaid.push("model_provider");
    }
    if overlaid.is_empty() {
        // 原生那根本来就是这个值：不必改，也不该记一次「改写」。
        return false;
    }
    if let Some(origins) = result.get_mut("origins").and_then(Value::as_object_mut) {
        for key in overlaid {
            if let Some(version) = origins
                .get_mut(key)
                .and_then(|origin| origin.get_mut("version"))
            {
                *version = json!("bridge/selection");
            }
        }
    }
    true
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

#[cfg(test)]
mod config_write_tests {
    use super::*;

    /// 菜单合并记下的名单：`gs/` 开头那几条是我们的。
    fn routing_with_managed() -> Routing {
        let mut routing = Routing::default();
        routing.remember_managed(&[
            json!({"id": "gs/prov/model-a"}),
            json!({"id": "gs/prov/model-b", "model": "gs/prov/model-b"}),
        ]);
        routing
    }

    fn write(edits: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": "w1",
            "method": "config/batchWrite",
            "params": {
                "edits": edits,
                "expectedVersion": null,
                "filePath": "/Users/someone/.codex/config.toml",
                "reloadUserConfig": true,
            },
        })
    }

    /// 网关模型那笔摘掉，其余键原样留下——原生配置不能被写成「账号 provider + 网关模型」。
    #[test]
    fn a_managed_model_edit_is_dropped_and_the_rest_keep_their_order() {
        let message = write(json!([
            {"keyPath": "model", "value": "gs/prov/model-a", "mergeStrategy": "replace"},
            {"keyPath": "model_reasoning_effort", "value": "high", "mergeStrategy": "replace"},
        ]));
        let (out, dropped, chosen) = sanitize_config_write(&message, &routing_with_managed());
        assert_eq!(dropped, 1);
        assert_eq!(
            chosen.as_deref(),
            Some("gs/prov/model-a"),
            "摘掉的那次选择必须被交出来，否则界面会回滚"
        );
        let edits = out["params"]["edits"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["keyPath"], json!("model_reasoning_effort"));
        // 请求的其余部分一个字节都不动。
        assert_eq!(out["params"]["filePath"], message["params"]["filePath"]);
        assert_eq!(out["params"]["reloadUserConfig"], json!(true));
    }

    /// 全被减光时保留一条合法的空写入（空 edits 是合法请求，宿主拿的是子进程真实回执）。
    #[test]
    fn everything_dropped_leaves_an_empty_but_valid_write() {
        let message = write(json!([
            {"keyPath": "model", "value": "gs/prov/model-b", "mergeStrategy": "replace"},
        ]));
        let (out, dropped, chosen) = sanitize_config_write(&message, &routing_with_managed());
        assert_eq!(dropped, 1);
        assert_eq!(chosen.as_deref(), Some("gs/prov/model-b"));
        assert_eq!(out["params"]["edits"], json!([]));
    }

    /// 别的路由字段也归我们：`model_provider = "gptswitch"` 落进原生配置是同一个病。
    ///
    /// 值放的是真机上的形状（见 `~/Library/Application Support/app.gptswitch.desktop/
    /// codex-home/config.toml`）：provider 名与目录 json 的路径，都不是 `gs/` 开头的模型标识。
    #[test]
    fn other_managed_keys_are_dropped_too() {
        let message = write(json!([
            {"keyPath": "model_provider", "value": "gptswitch", "mergeStrategy": "replace"},
            {
                "keyPath": "model_catalog_json",
                "value": "/Users/someone/Library/Application Support/app.gptswitch.desktop/catalogs/rev_abc/models.json",
                "mergeStrategy": "replace",
            },
            {"keyPath": "approval_policy", "value": "never", "mergeStrategy": "replace"},
        ]));
        let (out, dropped, chosen) = sanitize_config_write(&message, &routing_with_managed());
        assert_eq!(dropped, 2, "两个路由字段都该被摘掉");
        assert!(chosen.is_none(), "被摘的不是 model，就没有选择可记");
        let edits = out["params"]["edits"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["keyPath"], json!("approval_policy"));

        // 原生那根的 provider / 目录不进我们的判定，一个字节都不动。
        let native = write(json!([
            {"keyPath": "model_provider", "value": "custom", "mergeStrategy": "replace"},
            {"keyPath": "model_catalog_json", "value": "/Users/someone/.codex/models.json", "mergeStrategy": "replace"},
        ]));
        assert_eq!(sanitize_config_write(&native, &routing_with_managed()).1, 0);
    }

    /// 原生模型、无关键、供应商子路径都不许碰。
    #[test]
    fn native_models_and_other_edits_pass_through() {
        let routing = routing_with_managed();
        let native = write(json!([
            {"keyPath": "model", "value": "gpt-5.6-sol", "mergeStrategy": "replace"},
        ]));
        let (_, dropped, chosen) = sanitize_config_write(&native, &routing);
        assert_eq!(dropped, 0);
        assert!(chosen.is_none(), "原生模型不是我们的选择");
        let unrelated = write(json!([
            {"keyPath": "model_providers.custom.name", "value": "gs/prov/model-a", "mergeStrategy": "replace"},
        ]));
        assert_eq!(
            sanitize_config_write(&unrelated, &routing).1,
            0,
            "只有顶层键才是路由字段；供应商表的子路径改的是别的东西"
        );
    }

    /// 平铺写入（`config/value/write`）是同一个病的另一个口子，也要拦。
    #[test]
    fn flat_writes_are_sanitized_too() {
        let routing = routing_with_managed();
        let flat = json!({
            "jsonrpc": "2.0", "id": "w2", "method": "config/value/write",
            "params": {"keyPath": "model", "value": "gs/prov/model-a", "mergeStrategy": "replace"},
        });
        let (out, dropped, chosen) = sanitize_config_value_write(&flat, &routing);
        assert_eq!(dropped, 1, "网关模型的平铺写入必须被拦下");
        assert_eq!(chosen.as_deref(), Some("gs/prov/model-a"));
        assert_eq!(
            out["params"]["value"],
            json!(""),
            "拦下 = 写空值，宿主拿到的是子进程的真实回执，不是我们编的"
        );
        // 原生模型的平铺写入原样放过。
        let native = json!({
            "jsonrpc": "2.0", "id": "w3", "method": "config/value/write",
            "params": {"keyPath": "model", "value": "gpt-5.6-sol", "mergeStrategy": "replace"},
        });
        let (out, dropped, _) = sanitize_config_value_write(&native, &routing);
        assert_eq!(dropped, 0);
        assert_eq!(out, native);
    }

    /// 名单还没建立（菜单没取过）时也不能误伤——`gs/` 前缀本身就说明是我们的。
    ///
    /// 这条是拿真机日志换来的：10-13 20:07:55，菜单还没来，`config/batchWrite` 原样
    /// 落盘，网关模型进了原生配置。只靠菜单名单判定的写法在那一刻是瞎的。
    #[test]
    fn a_gateway_prefix_is_ours_even_before_the_menu_arrives() {
        let message = write(json!([
            {"keyPath": "model", "value": "gs/prov/model-a", "mergeStrategy": "replace"},
        ]));
        let (_, dropped, chosen) = sanitize_config_write(&message, &Routing::default());
        assert_eq!(dropped, 1, "没有菜单时也要认出网关模型");
        assert_eq!(chosen.as_deref(), Some("gs/prov/model-a"));
    }

    /// 冷启动时菜单还没到，模型选择也必须路由进托管那根，而不是被当成原生模型。
    #[test]
    fn a_gateway_model_routes_to_managed_without_a_menu() {
        let message = json!({"method": "thread/start", "params": {"model": "gs/prov/model-a"}});
        assert_eq!(
            Routing::default().direct(&message),
            (Side::Managed, Why::ModelIsOurs)
        );
    }

    /// **显式模型优先于线程归属**：在原生线程里换成网关模型，必须路由进托管那根。
    ///
    /// 真机日志里 `thread/settings/update` 带着 `gs/…` 却被 `thread-pinned` 送进原生，
    /// 那根没有这个模型——用户以为换了模型，其实整条请求都发错了地方。
    #[test]
    fn an_explicit_model_beats_a_thread_pin() {
        let mut routing = routing_with_managed();
        routing.pin("t-1", Side::Native);
        let switch_to_ours = json!({
            "method": "thread/settings/update",
            "params": {"threadId": "t-1", "model": "gs/prov/model-a"},
        });
        assert_eq!(
            routing.direct(&switch_to_ours),
            (Side::Managed, Why::ModelIsOurs),
            "在这次请求里点名了网关模型，就该按模型走"
        );
        // 没带模型时，线程归属仍然是唯一线索。
        let no_model = json!({"method": "turn/start", "params": {"threadId": "t-1"}});
        assert_eq!(routing.direct(&no_model), (Side::Native, Why::ThreadPinned));
    }

    /// 没带模型时，线程归属仍然是唯一线索。
    ///
    /// 真机上 `thread/resume` 会**声明** `model` 这个键但值是 null（15 次实测都是
    /// `model= None` 而 paramKeys 里有 `model`）。`model_of` 只认非空字符串，所以那种
    /// 形状不会误触发上面的「显式模型优先」，续接照旧跟着线程走。
    #[test]
    fn a_null_model_key_does_not_override_the_thread_pin() {
        let mut routing = routing_with_managed();
        routing.pin("t-managed", Side::Managed);
        let resume = json!({
            "method": "thread/resume",
            "params": {
                "threadId": "t-managed",
                "model": Value::Null,
                "modelProvider": "gptswitch",
            },
        });
        assert_eq!(
            routing.direct(&resume),
            (Side::Managed, Why::ThreadPinned),
            "声明了 model 键但值是 null，不能当成「换模型」"
        );
        // 空串同理：那是「没填」，不是「选了空模型」。
        let empty =
            json!({"method": "thread/resume", "params": {"threadId": "t-managed", "model": ""}});
        assert_eq!(routing.direct(&empty), (Side::Managed, Why::ThreadPinned));
    }

    /// 读配置时把路由字段盖回选中的网关模型——这是「选择不再回滚」的那一半。
    #[test]
    fn config_read_gives_the_selection_back() {
        let mut routing = Routing::default();
        let mut result = json!({
            "config": {
                "model": "gpt-5.6-sol",
                "model_provider": "custom",
                "model_reasoning_effort": "high",
            },
            "origins": {
                "model": {"name": {"type": "user"}, "version": "sha256:abc"},
                "model_provider": {"name": {"type": "user"}, "version": "sha256:abc"},
            },
        });
        // 没选过网关模型：一个字节都不改。
        assert!(!virtualize_config_read(&mut result, &routing));
        assert_eq!(result["config"]["model"], json!("gpt-5.6-sol"));
        assert_eq!(result["config"]["model_provider"], json!("custom"));

        routing.set_selection(Some("gs/prov/model-a".to_owned()));
        assert!(virtualize_config_read(&mut result, &routing));
        assert_eq!(result["config"]["model"], json!("gs/prov/model-a"));
        assert_eq!(
            result["config"]["model_reasoning_effort"],
            json!("high"),
            "只改路由字段，别的字段原样透传"
        );
        assert_eq!(
            result["origins"]["model"]["version"],
            json!("bridge/selection")
        );
        // 已经是我们的值：不必改写，也不该记一次「改写」。
        assert!(!virtualize_config_read(&mut result, &routing));

        // 形态不符（没有 config 对象）时不能慌，也不能写坏。
        let mut odd = json!({"unexpected": true});
        assert!(!virtualize_config_read(&mut odd, &routing));
        assert_eq!(odd, json!({"unexpected": true}));
    }

    /// **provider 也要一起盖**：只把 model 换成网关模型、provider 留着原生那根的
    /// `custom`，宿主会把这一对原样发给托管子进程，而那根只有 `[model_providers.gptswitch]`。
    ///
    /// 报文实测（`scripts/g0/selection-check.mjs`）：`modelProvider=custom` 必失败
    /// （`Model provider `custom` not found`），`gptswitch` 才开得出线程。
    #[test]
    fn config_read_hands_back_the_provider_that_can_serve_our_model() {
        let routing = {
            let mut routing = Routing::default();
            routing.set_selection(Some("gs/prov/model-a".to_owned()));
            routing
        };
        let mut result = json!({
            "config": {"model": "gpt-6-luna", "model_provider": "custom"},
            "origins": {"model_provider": {"name": {"type": "user"}, "version": "sha256:abc"}},
        });
        assert!(virtualize_config_read(&mut result, &routing));
        assert_eq!(result["config"]["model"], json!("gs/prov/model-a"));
        assert_eq!(
            result["config"]["model_provider"],
            json!("gptswitch"),
            "provider 不跟着换，托管那根会报 provider not found"
        );
        assert_eq!(
            result["origins"]["model_provider"]["version"],
            json!("bridge/selection")
        );

        // 只有 provider 不对（model 已经是我们的）时也要盖，且算一次改写。
        let mut partial = json!({
            "config": {"model": "gs/prov/model-a", "model_provider": "custom"},
        });
        assert!(virtualize_config_read(&mut partial, &routing));
        assert_eq!(partial["config"]["model_provider"], json!("gptswitch"));

        // 两个键都对 = 没有可改的，不该记一次「改写」。
        let mut aligned = json!({
            "config": {"model": "gs/prov/model-a", "model_provider": "gptswitch"},
        });
        assert!(!virtualize_config_read(&mut aligned, &routing));
    }

    /// 选择记忆要能跨进程：同一个状态文件，一个实例写、另一个读。
    #[test]
    fn the_selection_survives_a_restart_through_the_state_file() {
        let dir =
            std::env::temp_dir().join(format!("gptswitch-bridge-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("bridge.log");
        let path = Routing::state_path_for(Some(&log)).unwrap();
        assert_eq!(path.file_name().unwrap(), "bridge-model-state");

        let mut writer = Routing::default();
        writer.load_state(Some(path.clone()));
        writer.set_selection(Some("gs/prov/model-a".to_owned()));
        assert!(path.exists(), "选择要落盘，别的 bridge 实例才看得见");

        // 另一个实例（宿主会给每根 app-server 各起一个）起来时要读到同一个选择。
        let mut reader = Routing::default();
        reader.load_state(Some(path.clone()));
        assert_eq!(reader.selection(), Some("gs/prov/model-a"));

        // 选回原生 = 清掉，别人读到的也必须是「没选过」。
        reader.set_selection(None);
        let mut third = Routing::default();
        third.load_state(Some(path.clone()));
        assert_eq!(third.selection(), None);

        // 状态文件坏了当「没选过」，不猜。
        std::fs::write(&path, "{ 这不是 JSON").unwrap();
        let mut broken = Routing::default();
        broken.load_state(Some(path.clone()));
        assert_eq!(broken.selection(), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// bridge 的名单必须与 switch-core 的 `MANAGED_KEYS` 一致。
    ///
    /// 两份名单分开演化会让同一份配置在两个方向上被改写：一份去掉了某些键当托管 profile,
    /// 另一份却认为那些键可以落进原生配置。
    #[test]
    fn managed_keys_match_switch_core() {
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../switch-core/src/codex/config.rs"
        ))
        .expect("读不到 switch-core 的 config.rs");
        for key in MANAGED_KEYS {
            assert!(
                source.contains(&format!("\"{key}\"")),
                "switch-core 的 MANAGED_KEYS 里没有 {key}，两份名单已经不一致了"
            );
        }
        // provider 名同理：它是托管 profile 里 `[model_providers.<名字>]` 的那个名字，
        // 也是宿主开线程时带给子进程的 `modelProvider`。名字对不上，子进程会
        // 报 provider not found——配置里写的是 A，我们回报给宿主的是 B。
        assert!(
            source.contains(&format!("PROVIDER_ID: &str = \"{PROVIDER_ID}\"")),
            "switch-core 的 PROVIDER_ID 已经不是 {PROVIDER_ID} 了"
        );
    }
}
