//! 分层超时策略。
//!
//! 文档初始建议：连接 10 s、首事件 90 s、流空闲 180 s；每供应商可覆盖，
//! 但 UI 必须显示实际值。推理模型允许更长首事件预算，但不是无限等待。

use serde::{Deserialize, Serialize};

/// 分层超时（毫秒）。每一层单独判定，避免“总超时”掩盖真实瓶颈。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeoutPolicy {
    /// 建立连接与 TLS 握手。
    pub connect_ms: u64,
    /// 从请求提交到首个 SSE 事件（或首个响应字节）。
    pub first_event_ms: u64,
    /// 流开始后两个事件之间的最大间隔。
    pub idle_ms: u64,
    /// 上游请求整体上限（0 表示不设总时限）。
    pub total_ms: u64,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            connect_ms: 10_000,
            first_event_ms: 90_000,
            idle_ms: 180_000,
            total_ms: 0,
        }
    }
}

/// 超时违规的分类，供错误契约映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutKind {
    Connect,
    FirstEvent,
    Idle,
    Total,
}

impl TimeoutKind {
    pub fn code(self) -> &'static str {
        match self {
            TimeoutKind::Connect => "TIMEOUT_CONNECT",
            TimeoutKind::FirstEvent => "TIMEOUT_FIRST_EVENT",
            TimeoutKind::Idle => "TIMEOUT_IDLE",
            TimeoutKind::Total => "TIMEOUT_TOTAL",
        }
    }

    /// 连接失败可重试；已经流过数据后的空闲超时不再重试（可能已产生副作用）。
    pub fn retryable(self) -> bool {
        matches!(self, TimeoutKind::Connect)
    }
}

impl TimeoutPolicy {
    /// 校验下限，避免配置成 0 或负数语义。
    pub fn validate(&self) -> Result<(), crate::domain::error::CoreError> {
        for (name, value) in [
            ("连接超时", self.connect_ms),
            ("首事件超时", self.first_event_ms),
            ("流空闲超时", self.idle_ms),
        ] {
            if value == 0 {
                return Err(crate::domain::error::CoreError::validation(format!(
                    "{name}必须是正整数"
                )));
            }
        }
        Ok(())
    }

    /// 给定已流逝时间，判断触发了哪一层超时。
    ///
    /// `streaming` 为 true 表示已经进入流式阶段，此时首事件预算不再适用。
    pub fn check(
        &self,
        elapsed_ms: u64,
        since_last_event_ms: u64,
        streaming: bool,
    ) -> Option<TimeoutKind> {
        if !streaming && elapsed_ms > self.first_event_ms {
            return Some(TimeoutKind::FirstEvent);
        }
        if streaming && since_last_event_ms > self.idle_ms {
            return Some(TimeoutKind::Idle);
        }
        if self.total_ms > 0 && elapsed_ms > self.total_ms {
            return Some(TimeoutKind::Total);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_documented_initial_values() {
        let policy = TimeoutPolicy::default();
        assert_eq!(policy.connect_ms, 10_000);
        assert_eq!(policy.first_event_ms, 90_000);
        assert_eq!(policy.idle_ms, 180_000);
        assert_eq!(policy.total_ms, 0);
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn first_event_budget_applies_before_streaming_only() {
        let policy = TimeoutPolicy::default();
        assert_eq!(policy.check(89_000, 0, false), None);
        assert_eq!(
            policy.check(91_000, 0, false),
            Some(TimeoutKind::FirstEvent)
        );
        // 已进入流式后，首事件预算不再适用。
        assert_eq!(policy.check(200_000, 5_000, true), None);
    }

    #[test]
    fn idle_budget_applies_only_while_streaming() {
        let policy = TimeoutPolicy::default();
        assert_eq!(policy.check(1_000, 200_000, false), None);
        assert_eq!(policy.check(1_000, 200_000, true), Some(TimeoutKind::Idle));
    }

    #[test]
    fn total_budget_is_opt_in() {
        let mut policy = TimeoutPolicy::default();
        assert_eq!(policy.check(10_000_000, 0, true), None);
        policy.total_ms = 5_000;
        assert_eq!(policy.check(6_000, 100, true), Some(TimeoutKind::Total));
    }

    #[test]
    fn only_connect_timeout_is_retryable() {
        assert!(TimeoutKind::Connect.retryable());
        assert!(!TimeoutKind::Idle.retryable());
        assert!(!TimeoutKind::FirstEvent.retryable());
        assert_ne!(TimeoutKind::Idle.code(), TimeoutKind::FirstEvent.code());
    }

    #[test]
    fn zero_timeouts_are_rejected() {
        let policy = TimeoutPolicy {
            connect_ms: 0,
            ..TimeoutPolicy::default()
        };
        assert!(policy.validate().is_err());
    }
}
