use super::capability::Support;
use super::error::CoreError;
use super::tokens::TokenCount;
use serde::{Deserialize, Serialize};

/// 思考控制方式。供应商只支持开关或预算时仍然可以记录，但映射必须经过兼容验证。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningControl {
    Effort,
    Toggle,
    Budget,
    None,
}

impl ReasoningControl {
    /// 是否可以投影到 Codex 目录的档位字段。
    pub fn projects_to_levels(self) -> bool {
        matches!(self, ReasoningControl::Effort)
    }
}

/// 每模型的推理策略。
///
/// `default_value = None` 表示“不指定”，不能向上游发送字符串 `auto`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningPolicy {
    pub support: Support,
    pub control: ReasoningControl,
    pub allowed_values: Vec<String>,
    pub default_value: Option<String>,
    pub budget_tokens: Option<u64>,
    /// 已实现、版本化的适配器映射 ID。
    pub mapping_id: Option<String>,
}

impl Default for ReasoningPolicy {
    fn default() -> Self {
        Self {
            support: Support::Unknown,
            control: ReasoningControl::None,
            allowed_values: Vec::new(),
            default_value: None,
            budget_tokens: None,
            mapping_id: None,
        }
    }
}

impl ReasoningPolicy {
    /// 构造一个明确声明的档位策略。
    pub fn effort(values: Vec<String>, default: Option<String>) -> Self {
        Self {
            support: Support::Supported,
            control: ReasoningControl::Effort,
            allowed_values: values,
            default_value: default,
            budget_tokens: None,
            mapping_id: Some("reasoning.effort.v1".to_owned()),
        }
    }

    /// 默认档位必须属于已声明集合；支持未知时只允许“不指定”。
    pub fn validate(&self) -> Result<(), CoreError> {
        if let Some(default) = &self.default_value {
            if self.support == Support::Unknown {
                return Err(CoreError::validation("思考支持状态未知时只能选择“不指定”"));
            }
            if !self.allowed_values.iter().any(|v| v == default) {
                return Err(CoreError::validation(format!(
                    "默认思考档位 {default} 不在已声明集合内"
                )));
            }
        }
        if matches!(self.control, ReasoningControl::Toggle) && self.allowed_values.len() > 2 {
            return Err(CoreError::validation("开关型推理控制不能声明超过两个取值"));
        }
        Ok(())
    }

    /// 是否可以在 Codex 原生选择器里切换。
    ///
    /// 只有档位型且映射经过验证时才返回 true；其余情况只在网关固定策略里生效。
    pub fn host_selectable(&self) -> bool {
        self.support == Support::Supported
            && self.control.projects_to_levels()
            && !self.allowed_values.is_empty()
            && self.mapping_id.is_some()
    }

    /// 目录投影：档位列表加默认档位。
    pub fn catalog_levels(&self) -> Vec<&str> {
        if self.host_selectable() {
            self.allowed_values.iter().map(|v| v.as_str()).collect()
        } else {
            Vec::new()
        }
    }

    /// 预算与输出上限冲突时阻止发送，不悄悄调整。
    pub fn check_budget_conflict(&self, output_limit: Option<TokenCount>) -> Result<(), CoreError> {
        if let (Some(budget), Some(output)) = (self.budget_tokens, output_limit) {
            if budget > output.value() {
                return Err(CoreError::validation(format!(
                    "推理预算 {} 超过输出上限 {}",
                    budget,
                    output.value()
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_must_belong_to_allowed_values() {
        let policy = ReasoningPolicy::effort(
            vec!["low".into(), "medium".into(), "high".into()],
            Some("high".into()),
        );
        assert!(policy.validate().is_ok());

        let policy = ReasoningPolicy::effort(vec!["low".into()], Some("max".into()));
        assert!(policy.validate().is_err());
    }

    #[test]
    fn unknown_support_cannot_declare_a_default() {
        let policy = ReasoningPolicy {
            default_value: Some("low".into()),
            ..Default::default()
        };
        assert!(policy.validate().is_err());
    }

    #[test]
    fn unverified_mapping_is_not_host_selectable() {
        let mut policy = ReasoningPolicy::effort(vec!["low".into()], None);
        policy.mapping_id = None;
        assert!(!policy.host_selectable());
        assert!(policy.catalog_levels().is_empty());
    }

    #[test]
    fn non_effort_control_is_not_projected_to_levels() {
        let policy = ReasoningPolicy {
            support: Support::Supported,
            control: ReasoningControl::Toggle,
            allowed_values: vec!["on".into(), "off".into()],
            default_value: None,
            budget_tokens: None,
            mapping_id: Some("m".into()),
        };
        assert!(policy.validate().is_ok());
        assert!(!policy.host_selectable());
        assert!(policy.catalog_levels().is_empty());
    }

    #[test]
    fn budget_above_output_limit_is_blocked() {
        let policy = ReasoningPolicy {
            budget_tokens: Some(9000),
            ..ReasoningPolicy::default()
        };
        let limit = TokenCount::parse("8192").unwrap();
        assert!(policy.check_budget_conflict(limit).is_err());
        assert!(policy.check_budget_conflict(None).is_ok());
    }

    #[test]
    fn toggle_rejects_more_than_two_values() {
        let policy = ReasoningPolicy {
            support: Support::Supported,
            control: ReasoningControl::Toggle,
            allowed_values: vec!["a".into(), "b".into(), "c".into()],
            default_value: None,
            budget_tokens: None,
            mapping_id: None,
        };
        assert!(policy.validate().is_err());
    }
}
