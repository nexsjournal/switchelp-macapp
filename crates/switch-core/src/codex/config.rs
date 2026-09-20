//! Codex `config.toml` 的保真读写与字段所有权。
//!
//! 规则来自 [配置生命周期](../../../../docs/architecture/02-configuration-lifecycle.md)：
//! 只管理被计划声明的字段；未知项、注释、引号与换行尽可能保留；解析失败立即停止。

use crate::domain::error::{CoreError, ErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

/// 本工具在用户 providers 表中使用的固定 ID。占用检测以该 ID 为边界。
pub const PROVIDER_ID: &str = "gptswitch";
/// 本工具生成的 provider 显示名。
pub const PROVIDER_NAME: &str = "Switchelp";
/// 宿主调用 auth helper 时固定传入的 `--instance` 参数值。
///
/// 宿主不接受参数化命令，只会照字面拼出 `["--instance", <本常量>]`。
/// helper 安装时必须用同一个值，否则双方对不上，宿主拿不到令牌。
pub const AUTH_HELPER_INSTANCE: &str = "local-main";

/// 本工具允许管理的顶层键路径（不含 `[model_providers.gptswitch]` 子表）。
pub const MANAGED_KEYS: [&str; 5] = [
    "model",
    "model_provider",
    "model_catalog_json",
    "model_context_window",
    "model_reasoning_effort",
];

/// 配置文件快照：原始字节、摘要、换行风格与解析后的语法树。
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub path: PathBuf,
    pub existed: bool,
    pub content_hash: String,
    /// 原始换行风格，写入时保持一致，避免整文件行尾被改写。
    pub line_ending: String,
    document: DocumentMut,
}

/// 检测文件的换行风格。默认 LF。
pub fn detect_line_ending(text: &str) -> String {
    if text.contains("\r\n") {
        "\r\n".to_owned()
    } else {
        "\n".to_owned()
    }
}

/// 按指定换行风格渲染语法树。toml_edit 只输出 LF，因此这里做一次统一转换。
fn render(document: &DocumentMut, line_ending: &str) -> String {
    let text = document.to_string();
    if line_ending == "\r\n" {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.replace("\r\n", "\n")
    }
}

impl ConfigSnapshot {
    /// 解析配置文本。row/col 只用于定位，不携带用户内容。
    pub fn parse(path: impl Into<PathBuf>, text: &str) -> Result<Self, CoreError> {
        let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| {
            let mut message = "配置文件无法解析".to_owned();
            if let Some(span) = error.span() {
                message.push_str(&format!("，错误位置偏移 {}", span.start));
            }
            CoreError::new(ErrorCode::ConfigParseFailed, "error.configParseFailed")
                .with_detail(message)
                .with_recovery("openCopy", "action.openConfigCopy")
        })?;
        Ok(Self {
            path: path.into(),
            existed: true,
            content_hash: hash(text),
            line_ending: detect_line_ending(text),
            document,
        })
    }

    /// 读取文件并解析；文件不存在时返回空文档（首次应用场景）。
    pub fn read(path: impl Into<PathBuf>) -> Result<Self, CoreError> {
        let path = path.into();
        if !path.exists() {
            return Ok(Self {
                path,
                existed: false,
                content_hash: hash(""),
                line_ending: "\n".to_owned(),
                document: DocumentMut::new(),
            });
        }
        let text = std::fs::read_to_string(&path)?;
        let mut snapshot = Self::parse(path, &text)?;
        snapshot.existed = true;
        Ok(snapshot)
    }

    pub fn to_text(&self) -> String {
        render(&self.document, &self.line_ending)
    }

    pub fn document(&self) -> &DocumentMut {
        &self.document
    }

    /// 读取本工具管理的字段当前值（用于差异与恢复）。
    pub fn managed_value(&self, key: &str) -> Option<String> {
        // provider 子表统一走“读回语义 -> 规范化序列化”，
        // 避免直接对语法树节点取字符串导致比较口径漂移。
        if key == "model_providers.gptswitch" {
            return self.read_provider().and_then(|p| serialized_provider(&p));
        }
        self.document.get(key).map(|item| {
            if let Some(text) = item.as_str() {
                return text.to_owned();
            }
            // 整数按十进制规范化：语法树保留用户原始写法（128_000），直接取字符串会把
            // 「数值相同、写法不同」误判成外部修改，从让还原被无谓挡下。
            if let Some(number) = item.as_integer() {
                return number.to_string();
            }
            item.to_string().trim().to_owned()
        })
    }

    /// 是否存在 `[model_providers.gptswitch]` 子表。
    pub fn has_gateway_provider(&self) -> bool {
        self.document
            .get("model_providers")
            .and_then(|item| item.get(PROVIDER_ID))
            .is_some()
    }

    /// 把 `[model_providers.gptswitch]` 读回为结构体；不存在或结构不符时返回 None。
    pub fn read_provider(&self) -> Option<ManagedProvider> {
        let table = self
            .document
            .get("model_providers")?
            .get(PROVIDER_ID)?
            .as_table()?;

        let base_url = table.get("base_url")?.as_str()?.to_owned();
        let wire_api = table
            .get("wire_api")
            .and_then(|i| i.as_str())
            .unwrap_or("responses")
            .to_owned();
        let auth_table = table.get("auth").and_then(|i| i.as_table());
        let auth = match auth_table {
            Some(auth) => {
                if let Some(command) = auth.get("command").and_then(|i| i.as_str()) {
                    ProviderAuth::Command {
                        command: command.to_owned(),
                        timeout_ms: auth
                            .get("timeout_ms")
                            .and_then(|i| i.as_integer())
                            .unwrap_or(5000)
                            .max(0) as u64,
                        refresh_interval_ms: auth
                            .get("refresh_interval_ms")
                            .and_then(|i| i.as_integer())
                            .unwrap_or(300_000)
                            .max(0) as u64,
                    }
                } else if let Some(env_key) = auth.get("env_key").and_then(|i| i.as_str()) {
                    ProviderAuth::EnvKey {
                        env_key: env_key.to_owned(),
                    }
                } else {
                    return None;
                }
            }
            None => return None,
        };

        Some(ManagedProvider {
            base_url,
            wire_api,
            auth,
        })
    }

    /// 是否存在其他工具已经占用的同名 provider 或非本工具的目录配置。
    pub fn foreign_managers(&self) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        if let Some(providers) = self
            .document
            .get("model_providers")
            .and_then(|i| i.as_table())
        {
            for (id, item) in providers.iter() {
                if id == PROVIDER_ID {
                    continue;
                }
                let name = item
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !name.is_empty() {
                    found.push(id.to_owned());
                }
            }
        }
        found
    }

    /// 脱敏预览：秘密字段替换为掩码，供 UI 只读展示。
    ///
    /// 先按字段名替换，再对整段文本做一次“密钥形状”扫描，避免秘密藏在
    /// `args`、`headers` 等列表里被原样导出。扫描只做替换，不做任何网络动作。
    pub fn redacted_preview(&self) -> String {
        let mut preview = self.document.clone();
        let secret_keys = [
            "key",
            "api_key",
            "token",
            "password",
            "secret",
            "authorization",
        ];
        redact_table(preview.as_table_mut(), &secret_keys, None);
        redact_secret_shapes(&preview.to_string())
    }
}

fn redact_table(table: &mut Table, secrets: &[&str], parent: Option<&str>) {
    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_owned()).collect();
    for key in keys {
        let lowered = key.to_ascii_lowercase();
        let path = match parent {
            Some(parent) => format!("{parent}.{lowered}"),
            None => lowered.clone(),
        };
        let is_secret = secrets.iter().any(|s| lowered.contains(s))
            || lowered == "env_key"
            || path.ends_with("auth.env_key");
        if is_secret {
            if let Some(item) = table.get_mut(&key) {
                *item = value("••••••••");
            }
            continue;
        }
        if let Some(item) = table.get_mut(&key) {
            if let Some(child) = item.as_table_mut() {
                redact_table(child, secrets, Some(&path));
            }
        }
    }
}

/// 已知密钥前缀形状。只覆盖可识别的公开前缀，不做启发式猜测。
const SECRET_PREFIXES: [&str; 8] = ["sk-", "sk_", "rk-", "api-", "key-", "pk-", "ghp_", "xoxb-"];

/// 把形如 `sk-xxxx` 的连续秘密片段替换为掩码，保留前缀以便用户识别来源。
fn redact_secret_shapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in split_keep_delimiters(text) {
        let trimmed =
            token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
        let is_secret = SECRET_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
            && trimmed.chars().count() >= 12;
        if is_secret {
            let prefix_len = SECRET_PREFIXES
                .iter()
                .find(|prefix| trimmed.starts_with(**prefix))
                .map(|prefix| prefix.len())
                .unwrap_or(0);
            let prefix = &trimmed[..prefix_len];
            out.push_str(&token.replace(trimmed, &format!("{prefix}••••••••")));
        } else {
            out.push_str(&token);
        }
    }
    out
}

/// 按 TOML 语法边界切分，保证替换不跨越引号或结构符号。
fn split_keep_delimiters(text: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            current.push(ch);
        } else {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            parts.push(ch.to_string());
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// 本工具生成的 provider 表内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProvider {
    pub base_url: String,
    /// 宿主侧 wire_api；当前版本只接受 responses。
    pub wire_api: String,
    /// 认证方式：`command` 或 `env_key`，具体值必须显式给出。
    pub auth: ProviderAuth,
}

/// 构造 provider 子表。apply 与 diff 必须共用同一构造逻辑，否则差异判定会漂移。
fn provider_table(provider: &ManagedProvider) -> Table {
    let mut table = Table::new();
    table["name"] = value(PROVIDER_NAME);
    table["base_url"] = value(provider.base_url.clone());
    table["wire_api"] = value(provider.wire_api.clone());
    let mut auth = Table::new();
    match &provider.auth {
        ProviderAuth::Command {
            command,
            timeout_ms,
            refresh_interval_ms,
        } => {
            auth["command"] = value(command.clone());
            let mut args: toml_edit::Array = toml_edit::Array::new();
            args.push("--instance");
            args.push(AUTH_HELPER_INSTANCE);
            auth["args"] = value(args);
            auth["timeout_ms"] = value(*timeout_ms as i64);
            auth["refresh_interval_ms"] = value(*refresh_interval_ms as i64);
        }
        ProviderAuth::EnvKey { env_key } => {
            auth["env_key"] = value(env_key.clone());
        }
    }
    table["auth"] = Item::Table(auth);
    table
}

/// provider 子表的规范化字符串。
///
/// `Table::to_string()` 只渲染本级标量，嵌套的 `auth` 会作为独立 section 出现在
/// 文档里而被丢掉；这里显式拼出完整语义，保证 diff 与所有权记录口径一致。
fn serialized_provider(provider: &ManagedProvider) -> Option<String> {
    let mut text = format!(
        "name = \"{}\"\nbase_url = \"{}\"\nwire_api = \"{}\"",
        PROVIDER_NAME, provider.base_url, provider.wire_api
    );
    match &provider.auth {
        ProviderAuth::Command {
            command,
            timeout_ms,
            refresh_interval_ms,
        } => {
            text.push_str(&format!(
                "\n\n[auth]\ncommand = \"{command}\"\nargs = [\"--instance\", \"{AUTH_HELPER_INSTANCE}\"]\ntimeout_ms = {timeout_ms}\nrefresh_interval_ms = {refresh_interval_ms}"
            ));
        }
        ProviderAuth::EnvKey { env_key } => {
            text.push_str(&format!("\n\n[auth]\nenv_key = \"{env_key}\""));
        }
    }
    Some(text)
}

/// provider 认证投影。core 只接受本机令牌，不接受上游 Key。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProviderAuth {
    /// 通过 helper 输出本机访问令牌。
    Command {
        command: String,
        timeout_ms: u64,
        refresh_interval_ms: u64,
    },
    /// 旧宿主使用环境变量注入本机令牌。
    EnvKey { env_key: String },
}

impl ProviderAuth {
    /// 该认证配置是否会泄露上游秘密。
    pub fn carries_upstream_secret(&self) -> bool {
        false
    }

    fn validate(&self) -> Result<(), CoreError> {
        match self {
            ProviderAuth::Command {
                command,
                timeout_ms,
                refresh_interval_ms,
            } => {
                if command.trim().is_empty() {
                    return Err(CoreError::validation("auth helper 路径不能为空"));
                }
                if *timeout_ms == 0 || *timeout_ms > 60_000 {
                    return Err(CoreError::validation("auth helper 超时必须为 1-60000 ms"));
                }
                if *refresh_interval_ms < 1_000 {
                    return Err(CoreError::validation("令牌刷新间隔不能小于 1 秒"));
                }
            }
            ProviderAuth::EnvKey { env_key } => {
                if env_key.trim().is_empty() {
                    return Err(CoreError::validation("env_key 不能为空"));
                }
                if env_key.to_ascii_lowercase().contains("key") && env_key.contains("sk-") {
                    return Err(CoreError::validation("env_key 不能是上游 Key 明文"));
                }
            }
        }
        Ok(())
    }
}

/// 期望写入的受管配置。字段为 None 表示不纳入本次事务。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedConfig {
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub model_catalog_json: Option<String>,
    pub provider: Option<ManagedProvider>,
    pub model_context_window: Option<u64>,
    pub model_reasoning_effort: Option<String>,
}

/// 单个字段的所有权记录：基线原值、最后一次写入值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldOwnership {
    pub key_path: String,
    /// 基线时字段是否存在。
    pub baseline_presence: bool,
    /// 基线原值；原本不存在时为空。
    pub baseline_value: Option<String>,
    /// 本工具最后一次写入的值。
    pub last_written_value: Option<String>,
}

impl FieldOwnership {
    pub fn new(key_path: impl Into<String>, baseline_value: Option<String>) -> Self {
        Self {
            key_path: key_path.into(),
            baseline_presence: baseline_value.is_some(),
            baseline_value: baseline_value.clone(),
            last_written_value: baseline_value,
        }
    }
}

/// 这个受管字段的当前值是否**明确是本工具写进去的**。
///
/// 为什么需要它：基线记录的是「接管之前用户原本的值」，但旧版本装过、或另一个工具把我们写的
/// 行圈进它自己的托管块时，配置里已经带着我们的值。此时若把「我们自己的值」记成基线，
/// 还原就会忠实地把它再写回去——用户以为点了还原，Codex 却仍然走本工具（真机上正是如此）。
///
/// 只认**有本工具特征**的值，泛化的数值（上下文窗口、思考档位）在这里一律返回 false：
/// 它们单独看不出作者，交给 [`baseline_is_our_own_work`] 做整组判断。
pub fn looks_like_our_value(key_path: &str, value: Option<&str>) -> bool {
    let Some(value) = value else { return false };
    let trimmed = value.trim();
    match key_path {
        "model_provider" => trimmed == PROVIDER_ID,
        // 本工具的目录别名一律以 `gs/` 开头（见 domain::ids::CatalogAlias）。
        "model" => trimmed.starts_with("gs/"),
        // 目录文件只可能落在本工具的数据目录里：`<appData>/catalogs/<rev>/models.json`。
        // 判据用「catalogs 子目录 + models.json」而不是绝对路径，跨平台成立。
        "model_catalog_json" => {
            let normalized = trimmed.replace('\\', "/");
            normalized.contains("/catalogs/") && normalized.ends_with("models.json")
        }
        // 这个子表要按**内容**认：键名是本工具的固定 ID，但用户也可能自己在同名表里放了别的
        // 东西。本工具写的表带自己的显示名，base_url 是本机网关的实例路由
        // （`…/i/<instance>/c/<revision>/v1`）——两条认一条即可。
        "model_providers.gptswitch" => {
            value.contains(PROVIDER_NAME) || (value.contains("/i/") && value.contains("/c/"))
        }
        _ => false,
    }
}

/// 记下来的基线本身就出自本工具吗？
///
/// 判据取两条最强的证据：基线里的 `model_provider` 是本工具的 ID，且基线里存在本工具的
/// providers 子表——别人的配置不会碰这两样。成立就意味着「没有可还原的用户原值」，
/// 还原应当**删除**这些字段（于是 Codex 回到原生登录与原生模型列表），而不是把它们写回基线。
pub fn baseline_is_our_own_work(ownership: &[FieldOwnership]) -> bool {
    let baseline_of = |key: &str| {
        ownership
            .iter()
            .find(|record| record.key_path == key)
            .and_then(|record| record.baseline_value.as_deref())
    };
    let provider_is_ours = baseline_of("model_provider")
        // 本工具的 ID 或本工具的 providers 子表出现在基线里，就是最强的证据：
        // 别人的配置不会碰这两样。
        .map(|value| looks_like_our_value("model_provider", Some(value)))
        .unwrap_or(false);
    let table_is_ours = baseline_of("model_providers.gptswitch")
        .map(|value| looks_like_our_value("model_providers.gptswitch", Some(value)))
        .unwrap_or(false);
    provider_is_ours || table_is_ours
}

/// 首次接管某个字段时要记的基线。
///
/// 正常情况下基线就是当前值（那是用户原本的配置）。但当前值已经带着本工具特征时，
/// 说明这是**我们自己早先写下的**，正确的基线是「原本不存在」。
fn initial_ownership(key_path: &str, current: Option<String>) -> FieldOwnership {
    let baseline = current.filter(|value| !looks_like_our_value(key_path, Some(value)));
    FieldOwnership::new(key_path, baseline)
}

/// 字段级差异。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub key_path: String,
    pub before: Option<String>,
    pub after: Option<String>,
    /// 修改原因 key，供 UI 说明为什么写这个字段。
    pub reason_key: String,
}

impl FieldChange {
    pub fn is_removal(&self) -> bool {
        self.after.is_none()
    }
}

/// 三方还原结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum RestoreOutcome {
    /// 当前值仍等于本工具写入值，可安全恢复基线。
    Restore { key_path: String },
    /// 基线原本不存在，恢复即删除该键。
    Delete { key_path: String },
    /// 外部已修改，保留当前值并进入冲突处理。
    Conflict {
        key_path: String,
        current: Option<String>,
    },
    /// 无需处理。
    Unchanged { key_path: String },
}

impl RestoreOutcome {
    pub fn key_path(&self) -> &str {
        match self {
            RestoreOutcome::Restore { key_path }
            | RestoreOutcome::Delete { key_path }
            | RestoreOutcome::Conflict { key_path, .. }
            | RestoreOutcome::Unchanged { key_path } => key_path,
        }
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, RestoreOutcome::Conflict { .. })
    }
}

/// 应用受管字段，返回新文本与更新后的所有权记录。
///
/// 不做隐式执行：调用方负责在写入前重新比对文件摘要（CAS）。
pub fn apply_managed(
    snapshot: &ConfigSnapshot,
    managed: &ManagedConfig,
    ownership: &[FieldOwnership],
) -> Result<(String, Vec<FieldOwnership>), CoreError> {
    if let Some(provider) = &managed.provider {
        provider.auth.validate()?;
        if provider.wire_api != "responses" {
            return Err(CoreError::validation(
                "Codex 侧 wire_api 只支持 responses；Chat 在网关转译",
            ));
        }
    }
    if let Some(model) = &managed.model {
        if model.trim().is_empty() {
            return Err(CoreError::validation("默认模型不能为空字符串"));
        }
    }
    if let Some(context) = managed.model_context_window {
        if context == 0 {
            return Err(CoreError::validation("上下文窗口必须是正整数"));
        }
    }

    let mut document = snapshot.document().clone();
    let mut updated: Vec<FieldOwnership> = Vec::new();

    let existing: std::collections::HashMap<&str, &FieldOwnership> =
        ownership.iter().map(|o| (o.key_path.as_str(), o)).collect();

    let record = |document: &DocumentMut,
                  key: &str,
                  new_value: Option<String>,
                  updated: &mut Vec<FieldOwnership>| {
        let baseline = match existing.get(key) {
            Some(record) => (*record).clone(),
            None => initial_ownership(key, snapshot.managed_value(key)),
        };
        let _ = document;
        updated.push(FieldOwnership {
            last_written_value: new_value,
            ..baseline
        });
    };

    // 顶层标量字段
    let scalars: [(&str, Option<String>); 4] = [
        ("model", managed.model.clone()),
        ("model_provider", managed.model_provider.clone()),
        ("model_catalog_json", managed.model_catalog_json.clone()),
        (
            "model_reasoning_effort",
            managed.model_reasoning_effort.clone(),
        ),
    ];
    for (key, new_value) in scalars {
        match new_value {
            Some(new_value) => {
                document[key] = value(new_value.clone());
                record(&document, key, Some(new_value), &mut updated);
            }
            None => {
                if document.contains_key(key) && ownership.iter().any(|o| o.key_path == key) {
                    document.remove(key);
                    record(&document, key, None, &mut updated);
                }
            }
        }
    }

    // 上下文窗口是唯一的整数受管字段：写成字符串宿主读不出来，必须按整数写。
    match managed.model_context_window {
        Some(window) => {
            document["model_context_window"] = value(window as i64);
            record(
                &document,
                "model_context_window",
                Some(window.to_string()),
                &mut updated,
            );
        }
        None => {
            if document.contains_key("model_context_window")
                && ownership
                    .iter()
                    .any(|o| o.key_path == "model_context_window")
            {
                document.remove("model_context_window");
                record(&document, "model_context_window", None, &mut updated);
            }
        }
    }

    // provider 子表：仅在明确接管时写入
    if let Some(provider) = &managed.provider {
        let key_path = "model_providers.gptswitch";
        set_gateway_provider(&mut document, Some(&provider_table(provider)))?;
        record(
            &document,
            key_path,
            serialized_provider(provider),
            &mut updated,
        );
    }

    Ok((render(&document, &snapshot.line_ending), updated))
}

/// 把网关 provider 子表写进文档（`None` 表示删掉）。
///
/// 不能用 `document["model_providers.gptswitch"] = ...`：那按**字面量键**插入，会得到一条
/// `"model_providers.gptswitch" = "..."` 的垃圾键，而真正的子表原封不动——既不生效又污染
/// 用户文件。用户把 `model_providers` 写成内联表时同理：内联表只能装值，塞进去的表会被
/// 静默丢弃，所以先把它还原成普通表。
fn set_gateway_provider(
    document: &mut DocumentMut,
    provider: Option<&Table>,
) -> Result<(), CoreError> {
    if document
        .get("model_providers")
        .map(|item| item.is_inline_table())
        .unwrap_or(false)
    {
        let inline = document
            .remove("model_providers")
            .and_then(|item| item.into_value().ok())
            .and_then(|value| value.as_inline_table().cloned())
            .ok_or_else(|| CoreError::internal("model_providers 内联表无法转换"))?;
        document["model_providers"] = Item::Table(inline.into_table());
    }

    match provider {
        Some(table) => {
            if !document.contains_key("model_providers") {
                document["model_providers"] = Item::Table(Table::new());
            }
            let providers = document["model_providers"]
                .as_table_mut()
                .ok_or_else(|| CoreError::internal("model_providers 不是表"))?;
            providers.insert(PROVIDER_ID, Item::Table(table.clone()));
        }
        None => {
            if let Some(providers) = document["model_providers"].as_table_mut() {
                providers.remove(PROVIDER_ID);
                if providers.is_empty() {
                    document.remove("model_providers");
                }
            }
        }
    }
    Ok(())
}

/// 把「provider 子表的规范化片段」解析回表结构。
///
/// 所有权里记的是 `serialized_provider()` 产出的片段（字段行 + `[auth]` 子节），不带 section 头。
/// 直接补一个 `[model_providers.gptswitch]` 再解析是不行的：片段里的 `[auth]` 会变成**顶层**同名
/// 节而不是子表。这里先按片段本身解析出顶层键，再整体搬进一张表——子节自然成为子表。
fn parse_gateway_provider(fragment: &str) -> Result<Table, CoreError> {
    let document: DocumentMut = fragment
        .parse()
        .map_err(|error| CoreError::internal(format!("provider 基线无法解析：{error}")))?;
    let mut table = Table::new();
    for (key, item) in document.iter() {
        table.insert(key, item.clone());
    }
    if table
        .get("base_url")
        .and_then(|item| item.as_str())
        .is_none()
    {
        return Err(CoreError::internal("provider 基线缺少 base_url"));
    }
    Ok(table)
}

/// 计算字段级差异，供“查看差异”页面使用。
pub fn diff_managed(snapshot: &ConfigSnapshot, managed: &ManagedConfig) -> Vec<FieldChange> {
    let mut changes = Vec::new();
    let mut push = |key: &str, after: Option<String>, reason: &str| {
        let before = snapshot.managed_value(key);
        if before != after {
            changes.push(FieldChange {
                key_path: key.to_owned(),
                before,
                after,
                reason_key: reason.to_owned(),
            });
        }
    };

    push("model", managed.model.clone(), "reason.defaultModel");
    push(
        "model_provider",
        managed.model_provider.clone(),
        "reason.providerRoute",
    );
    push(
        "model_catalog_json",
        managed.model_catalog_json.clone(),
        "reason.catalog",
    );
    push(
        "model_context_window",
        managed.model_context_window.map(|v| v.to_string()),
        "reason.contextOverride",
    );
    push(
        "model_reasoning_effort",
        managed.model_reasoning_effort.clone(),
        "reason.reasoningDefault",
    );
    if let Some(provider) = &managed.provider {
        let serialized = serialized_provider(provider).unwrap_or_default();
        let before = snapshot.managed_value("model_providers.gptswitch");
        if before.as_deref() != Some(serialized.as_str()) {
            changes.push(FieldChange {
                key_path: "model_providers.gptswitch".to_owned(),
                before,
                after: Some(serialized),
                reason_key: "reason.gatewayProvider".to_owned(),
            });
        }
    }
    changes
}

/// 三方比较：基线 B、本工具写入 W、当前值 C。
pub fn plan_restore(
    snapshot: &ConfigSnapshot,
    ownership: &[FieldOwnership],
) -> Vec<RestoreOutcome> {
    // 基线本身就是本工具写的（旧版本、或另一个工具把我们写的行圈进了它的托管块）：
    // 此时「还原成基线」等于把我们自己的值再写回去，用户根本回不到原生 Codex。
    // 正确的动作是删掉这些字段——那才是「原本不存在」的语义。
    let baseline_is_ours = baseline_is_our_own_work(ownership);
    ownership
        .iter()
        .map(|record| {
            let current = snapshot.managed_value(&record.key_path);
            if current == record.last_written_value {
                if record.baseline_presence && !baseline_is_ours {
                    RestoreOutcome::Restore {
                        key_path: record.key_path.clone(),
                    }
                } else {
                    RestoreOutcome::Delete {
                        key_path: record.key_path.clone(),
                    }
                }
            } else if current == record.baseline_value {
                RestoreOutcome::Unchanged {
                    key_path: record.key_path.clone(),
                }
            } else {
                RestoreOutcome::Conflict {
                    key_path: record.key_path.clone(),
                    current,
                }
            }
        })
        .collect()
}

/// 执行还原：只撤销与外部修改无冲突的字段，无关字段全部保留。
pub fn execute_restore(
    snapshot: &ConfigSnapshot,
    ownership: &[FieldOwnership],
) -> Result<(String, Vec<RestoreOutcome>), CoreError> {
    let outcomes = plan_restore(snapshot, ownership);
    let mut document = snapshot.document().clone();
    for (record, outcome) in ownership.iter().zip(outcomes.iter()) {
        // provider 子表是唯一「键路径不是字面量键」的受管字段：写入与删除都必须按表操作，
        // 走下面的字面量分支会得到 `"model_providers.gptswitch" = "…"` 这种垃圾键。
        if record.key_path == "model_providers.gptswitch" {
            match outcome {
                RestoreOutcome::Restore { .. } => {
                    let table = parse_gateway_provider(
                        record.baseline_value.as_deref().unwrap_or_default(),
                    )?;
                    set_gateway_provider(&mut document, Some(&table))?;
                }
                RestoreOutcome::Delete { .. } => set_gateway_provider(&mut document, None)?,
                RestoreOutcome::Conflict { .. } | RestoreOutcome::Unchanged { .. } => {}
            }
            continue;
        }
        match outcome {
            RestoreOutcome::Restore { .. } => match &record.baseline_value {
                Some(baseline) => {
                    document[record.key_path.as_str()] = value(baseline.clone());
                }
                None => {
                    document.remove(record.key_path.as_str());
                }
            },
            RestoreOutcome::Delete { .. } => {
                document.remove(record.key_path.as_str());
            }
            RestoreOutcome::Conflict { .. } | RestoreOutcome::Unchanged { .. } => {}
        }
    }
    Ok((render(&document, &snapshot.line_ending), outcomes))
}

/// 内容摘要，用于 CAS 比对。
pub fn hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 原子写入：同目录临时文件 + fsync + rename。
pub fn write_atomic(path: &Path, content: &str) -> Result<(), CoreError> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| CoreError::validation("配置路径缺少父目录"))?;
    std::fs::create_dir_all(parent)?;
    // 临时名必须**每个写入者唯一**。写死成 `.{name}.gptswitch.tmp` 时，两个并发写入会共用
    // 同一个临时文件：一个刚 create 完、另一个就把它截断，于是前者 rename 过去的是别人的
    // 半截内容。实测同一路径并发写 320 次里有 223 次失败，而且失败的是「写坏了」而不是
    // 「被拒绝」——这比失败更糟。进程号 + 进程内自增序号 + 纳秒，足以在本机区分任何两个写入者。
    let temp = parent.join(format!(
        ".{}.gptswitch.{}.{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("config.toml"),
        std::process::id(),
        next_temp_serial(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.subsec_nanos())
            .unwrap_or(0),
    ));
    {
        let mut file = std::fs::File::create(&temp)?;
        // 目标已存在时沿用它的权限。config.toml 可能被用户或其它工具设成 0600（里面可能有
        // 别的工具写入的密钥），而 create + rename 会把它换成 umask 推导值（通常 0644），
        // 等于悄悄把「只有本人可读」放宽给同机其他用户。权限复制失败就宁可写入失败。
        if let Ok(metadata) = std::fs::metadata(path) {
            std::fs::set_permissions(&temp, metadata.permissions())?;
        }
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)?;
    // rename 只是目录项变更，掉电可能丢；同步父目录才算落盘。
    #[cfg(unix)]
    {
        let parent_handle = std::fs::File::open(parent)?;
        parent_handle.sync_all()?;
    }
    Ok(())
}

/// 进程内唯一的临时文件序号。
fn next_temp_serial() -> u64 {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_provider() -> ManagedProvider {
        ManagedProvider {
            base_url: "http://127.0.0.1:18765/i/inst_1/c/rev_1/v1".to_owned(),
            wire_api: "responses".to_owned(),
            auth: ProviderAuth::Command {
                command: "/tmp/helper".to_owned(),
                timeout_ms: 5_000,
                refresh_interval_ms: 300_000,
            },
        }
    }

    /// 宿主只会照字面拼 `--instance <固定值>`；渲染与 helper 安装必须用同一个常量，
    /// 一旦漂移，宿主就再也取不到本机令牌。
    #[test]
    fn auth_command_args_use_the_shared_instance_constant() {
        let rendered = serialized_provider(&command_provider()).unwrap();
        assert!(
            rendered.contains("--instance") && rendered.contains(AUTH_HELPER_INSTANCE),
            "渲染出的 args 必须引用共享常量，实际：{rendered}"
        );
        assert_eq!(AUTH_HELPER_INSTANCE, "local-main");
    }

    /// 上游 Key 永远不能出现在受管 provider 表里，只能出现本机 helper。
    #[test]
    fn command_auth_never_carries_an_upstream_secret() {
        let provider = command_provider();
        assert!(!provider.auth.carries_upstream_secret());
        let rendered = serialized_provider(&provider).unwrap();
        assert!(rendered.contains("command = \"/tmp/helper\""));
        assert!(!rendered.contains("api_key"));
        assert!(!rendered.contains("sk-"));
    }

    /// 空 helper 路径必须在写入前被拒绝，而不是写进配置让宿主启动失败。
    #[test]
    fn empty_auth_command_is_rejected_before_writing() {
        let provider = ManagedProvider {
            auth: ProviderAuth::Command {
                command: "   ".to_owned(),
                timeout_ms: 5_000,
                refresh_interval_ms: 300_000,
            },
            ..command_provider()
        };
        assert!(provider.auth.validate().is_err());
    }
}

/// 原子写入的并发安全性。
///
/// 回归：临时文件名写死成 `.{name}.gptswitch.tmp` 时，同一个路径上的两个并发写入共用
/// 同一个临时文件——一个刚 create 完、另一个就把它截断，于是 rename 过去的是别人的
/// 半截内容，或者 rename 到一半发现临时文件已经不见了。实测 320 次并发写里有 223 次失败，
/// 而且失败形态是「写坏了」而不是「被拒绝」，这比报错更危险：config.toml 是用户唯一的配置。
#[cfg(test)]
mod write_atomic_tests {
    use super::*;

    #[test]
    fn concurrent_writers_never_produce_a_torn_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let payloads: Vec<String> = (0..4)
            .map(|index| format!("model = \"payload-{index}\"\n# {}\n", "x".repeat(64 * 1024)))
            .collect();

        std::thread::scope(|scope| {
            for index in 0..32usize {
                let path = path.clone();
                let payload = payloads[index % payloads.len()].clone();
                scope.spawn(move || {
                    write_atomic(&path, &payload).expect("并发写入不应失败");
                });
            }
        });

        let landed = std::fs::read_to_string(&path).unwrap();
        assert!(
            payloads.contains(&landed),
            "落盘内容必须是某一次写入的**完整**内容；长度 {} 与任何一次输入都不相等，说明发生了截断或拼接",
            landed.len()
        );

        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "config.toml")
            .collect();
        assert!(
            leftovers.is_empty(),
            "临时文件必须被清理干净：{leftovers:?}"
        );
    }

    #[test]
    fn a_write_failure_leaves_no_temp_file_behind() {
        // 临时文件建在目标目录里，所以目标目录不可写时写入必须失败——而不是悄悄写到别处。
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nope");
        let path = nested.join("config.toml");
        // 父目录是个文件而不是目录：create_dir_all 会失败。
        std::fs::write(&nested, "我只是一个文件").unwrap();
        assert!(write_atomic(&path, "model = \"x\"\n").is_err());
    }
}
