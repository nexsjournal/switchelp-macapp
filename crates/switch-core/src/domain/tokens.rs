use super::error::CoreError;
use serde::{Deserialize, Serialize};

/// 宿主整数范围内的 Token 上限（i64 安全区），避免超出 Codex 配置可表示范围。
pub const MAX_TOKEN_VALUE: u64 = 2_147_483_647;

/// 解析用户输入的 Token 数值。
///
/// 规则来自设计规范：`32k` 必须精确解析为 32000；`Ki` 不与 `k` 混用；空值视为 null。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TokenCount(u64);

impl TokenCount {
    pub fn new(value: u64) -> Result<Self, CoreError> {
        if value > MAX_TOKEN_VALUE {
            return Err(CoreError::validation(format!(
                "Token 数值超过上限 {MAX_TOKEN_VALUE}"
            )));
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// 解析文本输入：支持千分位逗号、`k`/`m` 十进制后缀与 `Ki`/`Mi` 二进制后缀。
    pub fn parse(input: &str) -> Result<Option<Self>, CoreError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let lowered = trimmed.to_ascii_lowercase().replace(['_', ',', ' '], "");
        let (digits, multiplier) = if let Some(head) = lowered.strip_suffix("ki") {
            (head, 1024_u64)
        } else if let Some(head) = lowered.strip_suffix("mi") {
            (head, 1024_u64 * 1024)
        } else if let Some(head) = lowered.strip_suffix('k') {
            (head, 1000_u64)
        } else if let Some(head) = lowered.strip_suffix('m') {
            (head, 1_000_000_u64)
        } else {
            (lowered.as_str(), 1_u64)
        };

        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return Err(CoreError::validation("Token 数值必须是正整数"));
        }
        let base: u64 = digits
            .parse()
            .map_err(|_| CoreError::validation("Token 数值超出可表示范围"))?;
        let total = base
            .checked_mul(multiplier)
            .ok_or_else(|| CoreError::validation("Token 数值超出可表示范围"))?;
        Self::new(total).map(Some)
    }

    /// 展示形态：千分位精确值 + 可选简写，避免 `128` 被误读成 `128k`。
    pub fn display(self) -> String {
        let exact = group_thousands(self.0);
        if self.0 >= 1000 && self.0 % 1000 == 0 {
            format!("{} ({})", exact, short(self.0))
        } else {
            exact
        }
    }
}

fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn short(value: u64) -> String {
    if value % 1_000_000 == 0 {
        format!("{}m", value / 1_000_000)
    } else {
        format!("{}k", value / 1000)
    }
}

/// 上下文与输出预算的校验结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetCheck {
    /// 压缩建议阈值；无法给出（结果非正数）时为 None。
    pub compact_suggestion: Option<u64>,
    /// 阻止应用的原因列表。
    pub blocking_issues: Vec<String>,
}

/// 按网关文档的预算关系校验：输出不得超过上下文；压缩建议取
/// `min(floor(C*0.8), C - O - S)`，结果非正数则阻止应用。
pub fn check_budget(
    context: Option<TokenCount>,
    output: Option<TokenCount>,
    safety_margin: u64,
) -> BudgetCheck {
    let mut blocking_issues = Vec::new();
    let context_value = context.map(|c| c.value());
    let output_value = output.map(|o| o.value());

    if let (Some(c), Some(o)) = (context_value, output_value) {
        if o >= c {
            blocking_issues.push(format!("最大输出 {o} 不能大于等于上下文 {c}"));
        }
    }

    let compact_suggestion = match (context_value, output_value) {
        (Some(c), Some(o)) => {
            let by_ratio = (c as f64 * 0.8).floor() as u64;
            let by_reserve = c
                .checked_sub(o)
                .and_then(|rest| rest.checked_sub(safety_margin));
            match by_reserve {
                Some(reserve) if reserve > 0 => Some(by_ratio.min(reserve)),
                _ => {
                    blocking_issues.push(
                        "上下文不足以同时容纳输出预留与工具裕度，压缩阈值无有效值".to_owned(),
                    );
                    None
                }
            }
        }
        (Some(c), None) => Some((c as f64 * 0.8).floor() as u64),
        _ => None,
    };

    BudgetCheck {
        compact_suggestion,
        blocking_issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_and_binary_suffixes_distinctly() {
        assert_eq!(TokenCount::parse("32k").unwrap().unwrap().value(), 32_000);
        assert_eq!(TokenCount::parse("32K").unwrap().unwrap().value(), 32_000);
        assert_eq!(TokenCount::parse("32Ki").unwrap().unwrap().value(), 32_768);
        assert_eq!(TokenCount::parse("1m").unwrap().unwrap().value(), 1_000_000);
        assert_eq!(
            TokenCount::parse("1Mi").unwrap().unwrap().value(),
            1_048_576
        );
    }

    #[test]
    fn parses_grouped_digits_and_rejects_noise() {
        assert_eq!(
            TokenCount::parse("128,000").unwrap().unwrap().value(),
            128_000
        );
        assert_eq!(
            TokenCount::parse("  8192  ").unwrap().unwrap().value(),
            8192
        );
        assert!(TokenCount::parse("12.5k").is_err());
        assert!(TokenCount::parse("-1").is_err());
        assert!(TokenCount::parse("abc").is_err());
    }

    #[test]
    fn empty_input_is_unknown_not_zero() {
        assert_eq!(TokenCount::parse("").unwrap(), None);
        assert_eq!(TokenCount::parse("   ").unwrap(), None);
    }

    #[test]
    fn display_keeps_unit_and_exact_value() {
        assert_eq!(
            TokenCount::new(128_000).unwrap().display(),
            "128,000 (128k)"
        );
        assert_eq!(TokenCount::new(8192).unwrap().display(), "8,192");
    }

    #[test]
    fn rejects_value_above_host_range() {
        assert!(TokenCount::new(MAX_TOKEN_VALUE + 1).is_err());
    }

    #[test]
    fn output_must_stay_below_context() {
        let check = check_budget(
            TokenCount::parse("128k").unwrap(),
            TokenCount::parse("128k").unwrap(),
            0,
        );
        assert!(!check.blocking_issues.is_empty());
    }

    #[test]
    fn compact_suggestion_respects_reserve() {
        let check = check_budget(
            TokenCount::parse("128000").unwrap(),
            TokenCount::parse("8192").unwrap(),
            4096,
        );
        // min(floor(128000*0.8)=102400, 128000-8192-4096=115712) => 102400
        assert_eq!(check.compact_suggestion, Some(102_400));
        assert!(check.blocking_issues.is_empty());
    }

    #[test]
    fn compact_suggestion_blocks_when_reserve_is_exhausted() {
        let check = check_budget(
            TokenCount::parse("10000").unwrap(),
            TokenCount::parse("9000").unwrap(),
            2000,
        );
        assert_eq!(check.compact_suggestion, None);
        assert_eq!(check.blocking_issues.len(), 1);
    }

    #[test]
    fn unknown_context_yields_no_suggestion() {
        let check = check_budget(None, None, 0);
        assert_eq!(check.compact_suggestion, None);
        assert!(check.blocking_issues.is_empty());
    }
}
