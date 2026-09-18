use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// 生成新的随机标识。
            pub fn generate() -> Self {
                Self(format!("{}", uuid::Uuid::new_v4()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }

            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(ProviderId);
id_type!(CredentialId);
id_type!(ModelId);
id_type!(InstanceId);
id_type!(RevisionId);
id_type!(OperationId);
id_type!(PlanId);
id_type!(ProbeId);

/// 目录 alias：全局唯一、稳定，不含 API Key、URL 或邮箱。
///
/// 形态为 `gs/{provider}/{model}`，只允许 `[a-z0-9_-]` 与 `/` 分隔符。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CatalogAlias(String);

impl CatalogAlias {
    pub fn parse(value: &str) -> Result<Self, super::error::CoreError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(super::error::CoreError::validation("alias 不能为空"));
        }
        if trimmed.len() > 128 {
            return Err(super::error::CoreError::validation("alias 不能超过 128 字符"));
        }
        if !trimmed.starts_with("gs/") {
            return Err(super::error::CoreError::validation("alias 必须以 gs/ 开头"));
        }
        for segment in trimmed.split('/') {
            if segment.is_empty() {
                return Err(super::error::CoreError::validation("alias 不能包含空段"));
            }
            if !segment
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                return Err(super::error::CoreError::validation(
                    "alias 只允许小写字母、数字、_ 和 -",
                ));
            }
        }
        if trimmed.contains('@') || trimmed.contains("://") {
            return Err(super::error::CoreError::validation("alias 不能包含邮箱或 URL"));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// 由供应商和模型标识推导稳定 alias。
    pub fn from_parts(provider: &str, model: &str) -> Result<Self, super::error::CoreError> {
        Self::parse(&format!(
            "gs/{}/{}",
            sanitize_segment(provider),
            sanitize_segment(model)
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 将任意展示值映射为 alias 段：保留 ASCII 字母数字，其余按十六进制折叠以保持稳定且可读。
fn sanitize_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
            out.push_str(&format!("{:x}", ch as u32));
        }
    }
    if out.is_empty() {
        out.push_str("x");
    }
    out
}

impl fmt::Display for CatalogAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_namespaced_alias() {
        assert!(CatalogAlias::parse("gs/p_a/m_1").is_ok());
    }

    #[test]
    fn rejects_alias_without_prefix() {
        assert!(CatalogAlias::parse("p_a/m_1").is_err());
    }

    #[test]
    fn rejects_alias_with_uppercase_or_url() {
        assert!(CatalogAlias::parse("gs/P_A/m_1").is_err());
        assert!(CatalogAlias::parse("gs/p_a/https://x").is_err());
    }

    #[test]
    fn derives_stable_alias_from_unicode_names() {
        let alias = CatalogAlias::from_parts("供应商 A", "Model/X").unwrap();
        assert!(alias.as_str().starts_with("gs/"));
        // 大小写与分隔符归一，重复调用结果一致。
        assert_eq!(alias, CatalogAlias::from_parts("供应商 A", "Model/X").unwrap());
        assert!(!alias.as_str().contains('A'));
    }
}
