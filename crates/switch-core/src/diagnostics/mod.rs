//! 诊断事件、脱敏与诊断包。
//!
//! 规则来自 [安全与跨平台](../../../../docs/architecture/05-security-and-platforms.md) 第 5 节：
//! - **allowlist 结构化提取为主，字符串脱敏为辅**：字段名不在白名单里就不落盘，
//!   而不是先存下来再想办法擦掉；
//! - 永不记录 prompt、completion、工具参数、文件内容、Authorization、cookie、完整 URL query；
//! - 诊断包先给预览（列出内容、敏感项处理与大小），再由用户决定保存；
//! - 默认保留 7 天或 20 MiB，先到为准。

mod discovery;
mod export;
mod probe;
mod update;

pub use discovery::{fetch as fetch_models, DiscoveredModel};
pub use export::{build_export, preview_export, write_export, ExportPreview, PreviewItem};
pub use update::{check as check_update, UpdateStatus, DEFAULT_ENDPOINT as UPDATE_ENDPOINT};
pub use probe::{ProbePlan, ProbeReport, ProbeState, ProbeTarget, Probes, StageOutcome};

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 默认保留时长（天）。
pub const RETENTION_DAYS: i64 = 7;
/// 默认保留体积上限（20 MiB）。
pub const MAX_RETAINED_BYTES: usize = 20 * 1024 * 1024;

/// 允许进入诊断事件的元数据键。
///
/// 这是**唯一**的准入清单：这里没有的键在 `with_metadata` 里被直接丢弃。
/// 新增字段必须同时在这里登记，避免顺手把请求正文塞进日志。
pub const ALLOWED_METADATA_KEYS: [&str; 19] = [
    "instance_id",
    "revision_id",
    "operation_id",
    "provider_id",
    "model_id",
    "alias",
    "protocol_id",
    "credential_version",
    "phase",
    "attempt",
    "http_status",
    "ttft_ms",
    "elapsed_ms",
    "error_code",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "byte_size",
    "app_version",
];

/// 诊断事件的级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn label_key(self) -> &'static str {
        match self {
            LogLevel::Info => "log.info",
            LogLevel::Warning => "log.warning",
            LogLevel::Error => "log.error",
        }
    }
}

/// 一条诊断事件。字段本身就是 allowlist——没有“自由文本”槽位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub timestamp: String,
    pub level: LogLevel,
    /// 事件类别（`gateway`、`apply`、`credential`、`discovery`…）。
    pub category_key: String,
    /// 目标的可读标识：alias、供应商名或实例标识。
    pub target_label: String,
    pub result_key: String,
    pub elapsed_ms: Option<u64>,
    pub safe_metadata: BTreeMap<String, String>,
}

impl DiagnosticEvent {
    pub fn new(
        timestamp: impl Into<String>,
        level: LogLevel,
        category_key: &str,
        target_label: impl Into<String>,
        result_key: &str,
    ) -> Self {
        Self {
            timestamp: timestamp.into(),
            level,
            category_key: category_key.to_owned(),
            target_label: target_label.into(),
            result_key: result_key.to_owned(),
            elapsed_ms: None,
            safe_metadata: BTreeMap::new(),
        }
    }

    /// 只接受白名单内的键；值仍然会被脱敏。
    pub fn with_metadata(mut self, key: &str, value: impl Into<String>) -> Self {
        if ALLOWED_METADATA_KEYS.contains(&key) {
            self.safe_metadata
                .insert(key.to_owned(), redact_value(&value.into()));
        }
        self
    }

    pub fn with_elapsed(mut self, elapsed_ms: u64) -> Self {
        self.elapsed_ms = Some(elapsed_ms);
        self
    }

    /// 该事件是否属于某个导出范围。范围按类别前缀匹配，空范围表示全部。
    pub fn in_scope(&self, scopes: &[String]) -> bool {
        scopes.is_empty()
            || scopes
                .iter()
                .any(|scope| self.category_key.starts_with(scope.as_str()))
    }

    /// 序列化后的近似体积，用于保留策略与预览。
    pub fn byte_size(&self) -> usize {
        serde_json::to_string(self)
            .map(|text| text.len())
            .unwrap_or(0)
    }
}

/// 接收诊断事件。网关与配置事务共用一个日志。
pub trait DiagnosticSink: Send + Sync {
    fn record(&self, event: DiagnosticEvent);
}

/// 有界的事件环形缓冲：按条数与字节数双重限制，并按时长淘汰旧事件。
pub struct DiagnosticLog {
    capacity: usize,
    max_bytes: usize,
    retention_days: i64,
    events: Mutex<VecDeque<DiagnosticEvent>>,
    bytes: Mutex<usize>,
    dropped: Mutex<u64>,
}

impl Default for DiagnosticLog {
    fn default() -> Self {
        Self::new(2_000, MAX_RETAINED_BYTES, RETENTION_DAYS)
    }
}

impl DiagnosticLog {
    pub fn new(capacity: usize, max_bytes: usize, retention_days: i64) -> Self {
        Self {
            capacity: capacity.max(1),
            max_bytes: max_bytes.max(1),
            retention_days: retention_days.max(1),
            events: Mutex::new(VecDeque::new()),
            bytes: Mutex::new(0),
            dropped: Mutex::new(0),
        }
    }

    /// 记录一条事件。超出条数或字节上限时从最早的事件开始丢弃。
    pub fn record(&self, event: DiagnosticEvent) {
        let size = event.byte_size();
        if size > self.max_bytes {
            // 单条就超过总预算：整条丢弃，避免一条巨物挤掉全部历史。
            *self.dropped.lock().expect("诊断锁未被污染") += 1;
            return;
        }
        let mut events = self.events.lock().expect("诊断锁未被污染");
        let mut bytes = self.bytes.lock().expect("诊断锁未被污染");
        events.push_back(event);
        *bytes += size;
        while events.len() > self.capacity || *bytes > self.max_bytes {
            let Some(evicted) = events.pop_front() else {
                break;
            };
            *bytes = bytes.saturating_sub(evicted.byte_size());
        }
    }

    /// 丢弃早于 `now_unix - retention_days` 的事件。返回丢弃条数。
    pub fn prune(&self, now_unix: i64) -> usize {
        let cutoff = now_unix - self.retention_days * 86_400;
        let mut events = self.events.lock().expect("诊断锁未被污染");
        let mut bytes = self.bytes.lock().expect("诊断锁未被污染");
        let mut removed = 0;
        // 本身事件时间戳由 `now_rfc3339()` 生成，固定为等长的 UTC 文本；
        // 等长 RFC3339 文本的字典序即时间序，因此不需要解析。长度不一致的
        // 外部时间戳一律不淘汰——宁可多留，不可误删。
        let cutoff_text = time::OffsetDateTime::from_unix_timestamp(cutoff)
            .ok()
            .and_then(|stamp| {
                stamp
                    .format(&time::format_description::well_known::Rfc3339)
                    .ok()
            });
        while let Some(front) = events.front() {
            let expired = cutoff_text
                .as_deref()
                .map(|cutoff_text| {
                    front.timestamp.len() == cutoff_text.len()
                        && front.timestamp.as_str() < cutoff_text
                })
                .unwrap_or(false);
            if !expired {
                break;
            }
            let Some(evicted) = events.pop_front() else {
                break;
            };
            *bytes = bytes.saturating_sub(evicted.byte_size());
            removed += 1;
        }
        removed
    }

    pub fn len(&self) -> usize {
        self.events.lock().expect("诊断锁未被污染").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bytes(&self) -> usize {
        *self.bytes.lock().expect("诊断锁未被污染")
    }

    pub fn dropped(&self) -> u64 {
        *self.dropped.lock().expect("诊断锁未被污染")
    }

    /// 清空本工具自己的诊断事件。返回清掉的条数。
    ///
    /// 只影响本工具的诊断记录，不触碰 Codex 历史、不触碰配置事务记录。
    pub fn clear(&self) -> usize {
        let removed = self.events.lock().expect("诊断锁未被污染").len();
        self.events.lock().expect("诊断锁未被污染").clear();
        *self.bytes.lock().expect("诊断锁未被污染") = 0;
        removed
    }

    /// 按级别过滤的事件列表，最新在后。
    pub fn list(&self, level: Option<LogLevel>) -> Vec<DiagnosticEvent> {
        self.events
            .lock()
            .expect("诊断锁未被污染")
            .iter()
            .filter(|event| level.map(|level| event.level >= level).unwrap_or(true))
            .cloned()
            .collect()
    }
}

impl DiagnosticSink for DiagnosticLog {
    fn record(&self, event: DiagnosticEvent) {
        DiagnosticLog::record(self, event)
    }
}

/// 值级脱敏：allowlist 之后的第二道防线。
///
/// 上游错误可能把密钥回显进任何字段，所以即使键合法，值也要过一遍。
pub fn redact_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for token in split_keep_separators(value) {
        if token.starts_with("Bearer ") {
            out.push_str("Bearer ••••");
        } else if looks_like_secret(&token) {
            out.push_str("••••");
        } else {
            out.push_str(&token);
        }
    }
    out
}

/// 把文本切成可判定的片段：按空白与常见分隔符断开，保留分隔符本身。
fn split_keep_separators(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if ch.is_whitespace() || matches!(ch, ',' | ';' | '"' | '\'' | '=' | '&' | '?') {
            parts.push(std::mem::take(&mut current));
            parts.push(ch.to_string());
        } else {
            current.push(ch);
        }
    }
    parts.push(current);
    parts.retain(|part| !part.is_empty());
    parts
}

/// 判定一个片段像不像密钥。
///
/// 只认高置信度的形态，宁可漏一个也不要把正常内容打成星号——
/// 诊断日志被涂成一片星号就等于没有诊断。
fn looks_like_secret(token: &str) -> bool {
    let trimmed = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    if trimmed.len() < 20 {
        return false;
    }
    if trimmed.starts_with("sk-") || trimmed.starts_with("ghp_") || trimmed.starts_with("xoxb-") {
        return true;
    }
    // 纯十六进制且足够长（网关令牌即 64 位十六进制）。
    if trimmed.len() >= 32 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // 长随机串：无明显结构但足够长且字符集混合。
    trimmed.len() >= 40
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 生成事件时间戳。
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(target: &str) -> DiagnosticEvent {
        DiagnosticEvent::new(
            "2026-09-18T00:00:00Z",
            LogLevel::Info,
            "gateway",
            target,
            "result.ok",
        )
    }

    #[test]
    fn clear_removes_only_retained_events_and_resets_the_budget() {
        let log = DiagnosticLog::default();
        log.record(event("a"));
        log.record(event("b"));
        assert!(log.bytes() > 0);

        assert_eq!(log.clear(), 2);
        assert!(log.is_empty());
        assert_eq!(log.bytes(), 0);
        assert_eq!(log.clear(), 0, "重复清空不报错");
    }

    #[test]
    fn allowlist_drops_unknown_keys_instead_of_storing_them() {
        let recorded = event("gs/p_a/m_1")
            .with_metadata("alias", "gs/p_a/m_1")
            .with_metadata("prompt", "用户刚才问的完整问题")
            .with_metadata("authorization", "Bearer sk-live-abcdefghijklmnopqrstuvwxyz")
            .with_metadata("http_status", "200");

        assert_eq!(recorded.safe_metadata.len(), 2, "只有白名单键能落盘");
        assert!(recorded.safe_metadata.contains_key("alias"));
        assert!(recorded.safe_metadata.contains_key("http_status"));
        let rendered = serde_json::to_string(&recorded).unwrap();
        assert!(!rendered.contains("完整问题"));
        assert!(!rendered.contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn values_are_redacted_even_under_allowed_keys() {
        let recorded = event("gs/p_a/m_1")
            .with_metadata("error_code", "upstream said: token sk-live-0123456789abcdefghij")
            .with_metadata("alias", "Bearer 0123456789abcdef0123456789abcdef");

        let rendered = serde_json::to_string(&recorded).unwrap();
        assert!(!rendered.contains("sk-live-0123456789abcdefghij"));
        assert!(!rendered.contains("0123456789abcdef0123456789abcdef"));
        assert!(rendered.contains("••••"));
    }

    #[test]
    fn short_or_structured_text_is_not_mangled() {
        assert_eq!(redact_value("gs/p_a/m_1"), "gs/p_a/m_1");
        assert_eq!(
            redact_value("https://api.example.com/v1"),
            "https://api.example.com/v1"
        );
        assert_eq!(redact_value("200 is fine"), "200 is fine");
        let sentence = "上游返回 502，模型不存在或权限不足";
        assert_eq!(redact_value(sentence), sentence);
    }

    #[test]
    fn gateway_token_shaped_values_are_masked() {
        let token = "a".repeat(64);
        assert!(redact_value(&token).contains("••••"));
    }

    #[test]
    fn ring_buffer_respects_the_entry_capacity() {
        let log = DiagnosticLog::new(3, MAX_RETAINED_BYTES, RETENTION_DAYS);
        for index in 0..10 {
            log.record(event(&format!("target-{index}")));
        }
        assert_eq!(log.len(), 3);
        let targets: Vec<String> = log
            .list(None)
            .into_iter()
            .map(|event| event.target_label)
            .collect();
        assert_eq!(
            targets,
            vec!["target-7".to_owned(), "target-8".to_owned(), "target-9".to_owned()],
            "保留最新的事件"
        );
    }

    #[test]
    fn ring_buffer_respects_the_byte_budget() {
        let log = DiagnosticLog::new(1_000, 600, RETENTION_DAYS);
        for index in 0..50 {
            log.record(event(&format!("target-{index}")));
        }
        assert!(log.bytes() <= 600, "字节预算必须生效，实际 {}", log.bytes());
        assert!(log.len() < 50);
    }

    #[test]
    fn a_single_oversized_event_is_dropped_not_retained() {
        let log = DiagnosticLog::new(100, 64, RETENTION_DAYS);
        log.record(event("huge"));
        assert_eq!(log.len(), 0);
        assert_eq!(log.dropped(), 1);
    }

    #[test]
    fn level_filter_returns_that_level_and_above() {
        let log = DiagnosticLog::default();
        log.record(event("info"));
        log.record(DiagnosticEvent::new(
            "2026-09-18T00:00:01Z",
            LogLevel::Warning,
            "gateway",
            "warn",
            "result.warn",
        ));
        log.record(DiagnosticEvent::new(
            "2026-09-18T00:00:02Z",
            LogLevel::Error,
            "gateway",
            "error",
            "result.error",
        ));

        assert_eq!(log.list(None).len(), 3);
        assert_eq!(log.list(Some(LogLevel::Warning)).len(), 2);
        assert_eq!(log.list(Some(LogLevel::Error)).len(), 1);
    }

    #[test]
    fn scope_matching_uses_category_prefix() {
        let gateway = event("alias");
        assert!(gateway.in_scope(&[]));
        assert!(gateway.in_scope(&["gateway".to_owned()]));
        assert!(!gateway.in_scope(&["apply".to_owned()]));
    }

    #[test]
    fn prune_drops_events_older_than_the_retention_window() {
        let log = DiagnosticLog::new(100, MAX_RETAINED_BYTES, 7);
        // 2026-09-18T00:00:00Z；保留 7 天后边界落在 2026-09-11。
        let now = 1_789_699_200;
        log.record(DiagnosticEvent::new(
            "2026-09-01T00:00:00Z",
            LogLevel::Info,
            "gateway",
            "old",
            "result.ok",
        ));
        log.record(DiagnosticEvent::new(
            "2026-09-17T00:00:00Z",
            LogLevel::Info,
            "gateway",
            "recent",
            "result.ok",
        ));

        assert_eq!(log.prune(now), 1);
        assert_eq!(log.len(), 1);
        assert_eq!(log.list(None)[0].target_label, "recent");
    }

    #[test]
    fn unexpected_timestamp_shapes_are_kept_rather_than_assumed_expired() {
        let log = DiagnosticLog::new(100, MAX_RETAINED_BYTES, 1);
        log.record(DiagnosticEvent::new(
            "not-a-timestamp",
            LogLevel::Info,
            "gateway",
            "unknown",
            "result.ok",
        ));
        assert_eq!(log.prune(1_800_000_000), 0, "长度异常的时间戳不应被误删");
        assert_eq!(log.len(), 1);
    }
}
