use serde::{Deserialize, Serialize};

/// 版本状态：官网“支持 macOS/Windows”不能代替兼容矩阵。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Unverified,
    Experimental,
    Stable,
    Unsupported,
}

impl CompatibilityStatus {
    /// 仅 stable 允许显示“已加载”等确定性结论。
    pub fn allows_verified_claims(self) -> bool {
        matches!(self, CompatibilityStatus::Stable)
    }

    pub fn label_key(self) -> &'static str {
        match self {
            CompatibilityStatus::Unverified => "compat.unverified",
            CompatibilityStatus::Experimental => "compat.experimental",
            CompatibilityStatus::Stable => "compat.stable",
            CompatibilityStatus::Unsupported => "compat.unsupported",
        }
    }
}

/// Codex 实例指纹的一部分：用来判断适配器是否仍然适用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFingerprint {
    pub desktop_version: Option<String>,
    pub cli_version: Option<String>,
    pub schema_hash: Option<String>,
}

impl VersionFingerprint {
    pub fn unknown() -> Self {
        Self {
            desktop_version: None,
            cli_version: None,
            schema_hash: None,
        }
    }

    /// 指纹变化进入“未验证”，不自动修改配置。
    pub fn status_against(&self, known: &VersionFingerprint) -> CompatibilityStatus {
        if self.cli_version.is_some() && self.cli_version == known.cli_version {
            CompatibilityStatus::Stable
        } else if self.schema_hash.is_some() && self.schema_hash == known.schema_hash {
            CompatibilityStatus::Experimental
        } else {
            CompatibilityStatus::Unverified
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_stable_allows_verified_claims() {
        assert!(CompatibilityStatus::Stable.allows_verified_claims());
        assert!(!CompatibilityStatus::Experimental.allows_verified_claims());
    }

    #[test]
    fn unknown_version_is_unverified() {
        let current = VersionFingerprint::unknown();
        let known = VersionFingerprint {
            desktop_version: Some("26.908.70816".into()),
            cli_version: Some("0.154.0-alpha.6.2".into()),
            schema_hash: Some("abc".into()),
        };
        assert_eq!(
            current.status_against(&known),
            CompatibilityStatus::Unverified
        );
    }

    #[test]
    fn matching_cli_version_is_stable_and_schema_only_is_experimental() {
        let known = VersionFingerprint {
            desktop_version: Some("1".into()),
            cli_version: Some("0.154.0".into()),
            schema_hash: Some("abc".into()),
        };
        let mut current = known.clone();
        assert_eq!(current.status_against(&known), CompatibilityStatus::Stable);

        current.cli_version = Some("0.155.0".into());
        assert_eq!(
            current.status_against(&known),
            CompatibilityStatus::Experimental
        );

        current.schema_hash = Some("def".into());
        assert_eq!(
            current.status_against(&known),
            CompatibilityStatus::Unverified
        );
    }
}
