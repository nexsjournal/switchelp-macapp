//! SSE 流解析与生命周期。
//!
//! 规则来自 [网关与协议](../../../../docs/architecture/03-gateway-and-protocols.md)：
//! 不假定一个网络 chunk 等于一个事件，也不假定 UTF-8 字符完整落在同一块；
//! 处理多行 data、空事件、keepalive、CRLF、截断与背压。

use serde::{Deserialize, Serialize};

/// 一个完整的 SSE 事件。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SseEvent {
    /// `event:` 字段；缺省表示默认消息事件。
    pub event: Option<String>,
    /// 多行 `data:` 以 `\n` 连接后的内容。
    pub data: String,
    /// `id:` 字段，用于续接游标。
    pub id: Option<String>,
    /// `retry:` 字段（毫秒）。
    pub retry: Option<u64>,
}

impl SseEvent {
    /// 空事件（只有空行分隔）没有语义，不应向下游提交。
    pub fn is_empty(&self) -> bool {
        self.event.is_none() && self.data.is_empty() && self.id.is_none() && self.retry.is_none()
    }
}

/// 按字节解析的 SSE 解析器。
///
/// 只按 `\n`（0x0A）切行：0x0A 不会出现在 UTF-8 多字节序列的续字节中，
/// 因此字节级切行永远安全；行内容保持为字节缓冲，凑齐完整一行后才解码，
/// 天然支持一个 UTF-8 字符被拆到两个网络 chunk。
#[derive(Debug, Default)]
pub struct SseParser {
    pending: Vec<u8>,
    event: Option<String>,
    data_lines: Vec<String>,
    id: Option<String>,
    retry: Option<u64>,
    /// 最近一次解码失败标记，供调用方判断流是否可信。
    invalid_utf8: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// 尚未组成完整事件的字节数。
    pub fn pending_bytes(&self) -> usize {
        self.pending.len()
    }

    /// 是否存在未 dispatch 的半截事件。
    pub fn has_incomplete(&self) -> bool {
        !self.pending.is_empty()
            || self.event.is_some()
            || !self.data_lines.is_empty()
            || self.id.is_some()
            || self.retry.is_some()
    }

    /// 是否遇到过无法解码的字节。
    pub fn saw_invalid_utf8(&self) -> bool {
        self.invalid_utf8
    }

    /// 送入一个网络 chunk，返回本次解析出的完整事件（可能有多个）。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.pending.drain(..=index).collect();
            // drain 包含分隔符本身。
            line.pop();
            // 剥掉 CRLF 的 \r；孤立的 \r 不剥，避免破坏数据。
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.consume_line(&line) {
                events.push(event);
            }
        }

        events
    }

    /// 流结束：把缓冲区中未以换行结束的残行当作最后一行处理，并 dispatch。
    ///
    /// 返回 `None` 表示没有可提交的事件（例如只有 keepalive）。
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.pending.is_empty() {
            let mut line = std::mem::take(&mut self.pending);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            // 残行可能已构成一个字段，但仍需空行才会 dispatch；
            // 这里先记录字段，再统一收尾。
            self.apply_field_line(&line);
        }
        self.dispatch()
    }

    fn consume_line(&mut self, line: &[u8]) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        self.apply_field_line(line);
        None
    }

    fn apply_field_line(&mut self, line: &[u8]) {
        // keepalive 与注释行以 ':' 开头，直接忽略。
        if line.first() == Some(&b':') {
            return;
        }

        let text = match std::str::from_utf8(line) {
            Ok(text) => text,
            Err(_) => {
                // 解码失败不静默丢弃：记录标记，字段内容按原样忽略，
                // 由调用方决定是否终止流而不是把乱码送进模型上下文。
                self.invalid_utf8 = true;
                return;
            }
        };

        let (field, value) = match text.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (text, ""),
        };

        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => self.data_lines.push(value.to_owned()),
            "id" => self.id = Some(value.to_owned()),
            "retry" => {
                if let Ok(millis) = value.trim().parse::<u64>() {
                    self.retry = Some(millis);
                }
            }
            _ => {}
        }
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.event.is_none()
            && self.data_lines.is_empty()
            && self.id.is_none()
            && self.retry.is_none()
        {
            return None;
        }
        let data = self.data_lines.join("\n");
        let event = self.event.take();
        self.data_lines.clear();
        let id = self.id.take();
        let retry = self.retry.take();
        let parsed = SseEvent {
            event,
            data,
            id,
            retry,
        };
        if parsed.is_empty() {
            None
        } else {
            Some(parsed)
        }
    }
}

/// 流式生命周期。
///
/// 向下游提交状态头也是可观察承诺：`headersAccepted` 之后不能无痕重试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SseStreamState {
    /// 已被准入，尚未向上游发出。
    Admitted,
    /// 已向上游发出请求。
    Sent,
    /// 下游已收到状态头。
    HeadersAccepted,
    /// 正在传输事件。
    Streaming,
    /// 正常完成。
    Completed,
    /// 上游提前结束（缺少结束事件）。
    Incomplete,
    /// 传输失败。
    Failed,
    /// 客户端或策略取消。
    Cancelled,
}

impl SseStreamState {
    pub fn can_transition_to(self, next: SseStreamState) -> bool {
        use SseStreamState::*;
        match self {
            Admitted => matches!(next, Sent | Failed | Cancelled),
            Sent => matches!(next, HeadersAccepted | Failed | Cancelled),
            HeadersAccepted => matches!(next, Streaming | Completed | Incomplete | Failed | Cancelled),
            Streaming => matches!(next, Completed | Incomplete | Failed | Cancelled),
            Completed | Incomplete | Failed | Cancelled => false,
        }
    }

    /// 终态：不再变化。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SseStreamState::Completed
                | SseStreamState::Incomplete
                | SseStreamState::Failed
                | SseStreamState::Cancelled
        )
    }

    /// 是否已向下游提交过状态头。已提交后不得跨 Key 重试。
    pub fn has_committed_headers(self) -> bool {
        !matches!(self, SseStreamState::Admitted | SseStreamState::Sent)
    }

    /// 是否仍可安全重试（尚未产生可观察副作用）。
    pub fn is_retryable(self) -> bool {
        matches!(self, SseStreamState::Admitted | SseStreamState::Sent)
    }

    pub fn message_key(self) -> &'static str {
        match self {
            SseStreamState::Admitted => "stream.admitted",
            SseStreamState::Sent => "stream.sent",
            SseStreamState::HeadersAccepted => "stream.headersAccepted",
            SseStreamState::Streaming => "stream.streaming",
            SseStreamState::Completed => "stream.completed",
            SseStreamState::Incomplete => "stream.incomplete",
            SseStreamState::Failed => "stream.failed",
            SseStreamState::Cancelled => "stream.cancelled",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_event() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"event: response.output_text.delta\ndata: {\"delta\":\"hi\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("response.output_text.delta"));
        assert_eq!(events[0].data, "{\"delta\":\"hi\"}");
        assert!(!parser.has_incomplete());
    }

    #[test]
    fn parses_multiple_events_in_one_chunk() {
        let mut parser = SseParser::new();
        let chunk = b"data: a\n\ndata: b\n\ndata: c\n\n";
        let events = parser.feed(chunk);
        let payloads: Vec<&str> = events.iter().map(|e| e.data.as_str()).collect();
        assert_eq!(payloads, vec!["a", "b", "c"]);
    }

    #[test]
    fn joins_multi_line_data_with_newline() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"data: line1\ndata: line2\ndata: line3\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2\nline3");
    }

    #[test]
    fn decodes_utf8_character_split_across_chunks() {
        let mut parser = SseParser::new();
        let text = "中文增量";
        let payload = format!("data: {}\n\n", text);
        let bytes = payload.as_bytes();
        // 在中文字符中间切开，模拟网络分片。
        let split = bytes
            .iter()
            .position(|b| *b >= 0x80)
            .expect("应包含多字节字符")
            + 1;

        let first = parser.feed(&bytes[..split]);
        assert!(first.is_empty(), "半截字符不应产出事件");
        assert!(parser.pending_bytes() > 0);

        let second = parser.feed(&bytes[split..]);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].data, text);
        assert!(!parser.saw_invalid_utf8());
    }

    #[test]
    fn handles_crlf_line_endings() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"event: ping\r\ndata: pong\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("ping"));
        assert_eq!(events[0].data, "pong");
    }

    #[test]
    fn ignores_comment_and_keepalive_lines() {
        let mut parser = SseParser::new();
        let events = parser.feed(b": keepalive\n\n: another comment\ndata: real\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "real");
    }

    #[test]
    fn parses_id_and_retry_fields() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"id: 42\nretry: 1500\ndata: x\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_deref(), Some("42"));
        assert_eq!(events[0].retry, Some(1500));
    }

    #[test]
    fn rejects_malformed_retry_without_dropping_event() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"retry: soon\ndata: x\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].retry, None);
        assert_eq!(events[0].data, "x");
    }

    #[test]
    fn truncated_stream_reports_incomplete() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"data: partial");
        assert!(events.is_empty());
        assert!(parser.has_incomplete());
        assert!(parser.pending_bytes() > 0);

        // finish 提交残行，供上层判定 Incomplete。
        let tail = parser.finish();
        assert_eq!(tail.map(|e| e.data), Some("partial".to_owned()));
        assert!(!parser.has_incomplete());
    }

    #[test]
    fn finish_without_pending_data_returns_none() {
        let mut parser = SseParser::new();
        parser.feed(b"data: done\n\n");
        assert_eq!(parser.finish(), None);
    }

    #[test]
    fn event_without_data_is_still_reported() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"event: response.completed\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("response.completed"));
        assert!(events[0].data.is_empty());
    }

    #[test]
    fn blank_separator_alone_produces_no_event() {
        let mut parser = SseParser::new();
        assert!(parser.feed(b"\n\n\n").is_empty());
        assert!(!parser.has_incomplete());
    }

    #[test]
    fn invalid_utf8_is_flagged_not_silently_lost() {
        let mut parser = SseParser::new();
        let events = parser.feed(&[b'd', b'a', b't', b'a', b':', b' ', 0xFF, 0xFE, b'\n', b'\n']);
        assert!(events.is_empty());
        assert!(parser.saw_invalid_utf8(), "解码失败必须留下标记");
    }

    #[test]
    fn value_colon_is_preserved() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"data: {\"url\":\"https://example.com\"}\n\n");
        assert_eq!(events[0].data, "{\"url\":\"https://example.com\"}");
    }

    #[test]
    fn field_without_space_after_colon_is_supported() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"data:tight\n\n");
        assert_eq!(events[0].data, "tight");
    }

    #[test]
    fn large_payload_across_many_chunks_is_reassembled() {
        let mut parser = SseParser::new();
        let payload = "x".repeat(10_000);
        let wire = format!("data: {}\n\n", payload);
        let bytes = wire.as_bytes();
        let mut collected: Vec<SseEvent> = Vec::new();
        for chunk in bytes.chunks(97) {
            collected.extend(parser.feed(chunk));
        }
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].data, payload);
    }

    #[test]
    fn stream_state_transitions_follow_contract() {
        use SseStreamState::*;
        assert!(Admitted.can_transition_to(Sent));
        assert!(Sent.can_transition_to(HeadersAccepted));
        assert!(HeadersAccepted.can_transition_to(Streaming));
        assert!(Streaming.can_transition_to(Completed));
        assert!(Streaming.can_transition_to(Incomplete));
        assert!(Streaming.can_transition_to(Failed));
        assert!(Streaming.can_transition_to(Cancelled));

        assert!(!Admitted.can_transition_to(Streaming), "不能跳过状态头提交");
        assert!(!Completed.can_transition_to(Streaming), "终态不可回退");
        assert!(!Failed.can_transition_to(Admitted));
    }

    #[test]
    fn committed_headers_disable_retry_and_credential_swap() {
        assert!(SseStreamState::Admitted.is_retryable());
        assert!(SseStreamState::Sent.is_retryable());
        assert!(!SseStreamState::HeadersAccepted.is_retryable());
        assert!(SseStreamState::HeadersAccepted.has_committed_headers());
        assert!(!SseStreamState::Sent.has_committed_headers());
        assert!(SseStreamState::Completed.is_terminal());
        assert!(SseStreamState::Incomplete.is_terminal());
    }

    #[test]
    fn stream_states_have_distinct_message_keys() {
        let states = [
            SseStreamState::Admitted,
            SseStreamState::Sent,
            SseStreamState::HeadersAccepted,
            SseStreamState::Streaming,
            SseStreamState::Completed,
            SseStreamState::Incomplete,
            SseStreamState::Failed,
            SseStreamState::Cancelled,
        ];
        let mut keys: Vec<&str> = states.iter().map(|s| s.message_key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), states.len(), "每个状态必须有独立文案 key");
    }

    #[test]
    fn event_serializes_with_camel_case_field_names() {
        let event = SseEvent {
            event: Some("e".to_owned()),
            data: "d".to_owned(),
            id: Some("1".to_owned()),
            retry: Some(10),
        };
        let json = serde_json::to_value(&event).unwrap();
        for key in ["event", "data", "id", "retry"] {
            assert!(json.get(key).is_some(), "缺少字段 {}", key);
        }
    }
}
