use super::error::CoreError;
use super::ids::{CredentialId, ProviderId};
use serde::{Deserialize, Serialize};

/// 掩码策略：短 Key 不暴露任何尾号，避免整串被反推。
pub const MASK_MIN_LEN_FOR_SUFFIX: usize = 12;
pub const MASK_SUFFIX_LEN: usize = 4;

/// 固定掩码：`sk-••••••••1234`；短 Key 只显示固定点。
pub fn mask_secret(secret: &str) -> String {
    let trimmed = secret.trim();
    if trimmed.chars().count() < MASK_MIN_LEN_FOR_SUFFIX {
        return "••••••••".to_owned();
    }
    let suffix: String = trimmed
        .chars()
        .rev()
        .take(MASK_SUFFIX_LEN)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••••••{suffix}")
}

/// 凭据可用状态。Keychain 记录缺失与 API 401 是两个不同状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    /// 已保存，尚未检测。
    Saved,
    /// 认证通过（只对指定接口与模型成立）。
    Verified,
    /// 认证失败（401）。
    AuthFailed,
    /// 权限/地区/模型范围待判定（403）。
    ScopeLimited,
    /// 系统凭据库暂时不可用。
    KeystoreLocked,
    /// 安全记录不存在，需要重新填写。
    Missing,
    /// 用户主动停用。
    Disabled,
}

impl CredentialStatus {
    /// 该状态是否适合成为新请求的默认 Key。
    pub fn is_selectable(self) -> bool {
        matches!(
            self,
            CredentialStatus::Saved | CredentialStatus::Verified | CredentialStatus::ScopeLimited
        )
    }

    pub fn label_key(self) -> &'static str {
        match self {
            CredentialStatus::Saved => "credential.saved",
            CredentialStatus::Verified => "credential.verified",
            CredentialStatus::AuthFailed => "credential.authFailed",
            CredentialStatus::ScopeLimited => "credential.scopeLimited",
            CredentialStatus::KeystoreLocked => "credential.keystoreLocked",
            CredentialStatus::Missing => "credential.missing",
            CredentialStatus::Disabled => "credential.disabled",
        }
    }
}

/// 凭据元数据。明文 Key 永不进入 SQLite，只保存引用与掩码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: CredentialId,
    pub provider_id: ProviderId,
    pub label: String,
    /// 系统凭据库中的条目引用；不含明文。
    pub secret_ref: String,
    /// 替换 Key 递增版本，在途请求保留旧版本。
    pub secret_version: u32,
    pub masked_suffix: String,
    pub status: CredentialStatus,
    pub scope: Option<String>,
    pub last_verified_at: Option<String>,
    pub version: u64,
    pub created_at: String,
}

impl Credential {
    pub const LABEL_MAX: usize = 64;

    /// 保存新 Key：只接收一次秘密值，返回掩码与引用。
    pub fn create(
        id: CredentialId,
        provider_id: ProviderId,
        label: impl Into<String>,
        secret: &str,
        now: impl Into<String>,
    ) -> Result<Self, CoreError> {
        let label = label.into().trim().to_owned();
        validate_label(&label)?;
        validate_secret(secret)?;
        Ok(Self {
            id,
            provider_id,
            label,
            secret_ref: String::new(),
            secret_version: 1,
            masked_suffix: mask_secret(secret),
            status: CredentialStatus::Saved,
            scope: None,
            last_verified_at: None,
            version: 1,
            created_at: now.into(),
        })
    }

    /// 替换秘密：创建新 secret_version，不覆写在途版本。
    pub fn replace_secret(
        &mut self,
        secret: &str,
        now: impl Into<String>,
    ) -> Result<(), CoreError> {
        validate_secret(secret)?;
        self.secret_version += 1;
        self.masked_suffix = mask_secret(secret);
        self.status = CredentialStatus::Saved;
        self.last_verified_at = None;
        self.version += 1;
        // created_at 保持首次创建时间；替换时间由调用方通过 last_verified_at/审计记录表达。
        let _ = now.into();
        Ok(())
    }

    pub fn rename(&mut self, label: impl Into<String>) -> Result<(), CoreError> {
        let label = label.into().trim().to_owned();
        validate_label(&label)?;
        self.label = label;
        self.version += 1;
        Ok(())
    }

    /// 停用 / 重新启用。
    ///
    /// 重新启用回到「已保存、尚未检测」而不是停用前的状态：停用期间上游可能已经换过政策，
    /// 把旧的「已验证」带回来等于替用户确认了一件没验证过的事。
    pub fn set_disabled(&mut self, disabled: bool) {
        self.status = if disabled {
            CredentialStatus::Disabled
        } else {
            CredentialStatus::Saved
        };
        self.version += 1;
    }

    /// 设为新请求使用：仅可选择的凭据能被固定。
    pub fn mark_active(&self) -> Result<(), CoreError> {
        if !self.status.is_selectable() {
            return Err(CoreError::validation(format!(
                "凭据状态 {} 不能设为新请求使用",
                self.status.label_key()
            )));
        }
        Ok(())
    }

    /// 删除前检查：仍被续接绑定时只能进入“停用并等待释放”。
    pub fn deletion_plan(&self, active_bindings: usize) -> DeletionPlan {
        if active_bindings > 0 {
            DeletionPlan::DeactivateAndWait { active_bindings }
        } else {
            DeletionPlan::DeleteNow
        }
    }
}

/// 删除计划：有活跃续接引用时延迟实际删除。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum DeletionPlan {
    DeleteNow,
    DeactivateAndWait { active_bindings: usize },
}

fn validate_label(label: &str) -> Result<(), CoreError> {
    if label.is_empty() {
        return Err(CoreError::validation("Key 备注不能为空"));
    }
    if label.chars().count() > Credential::LABEL_MAX {
        return Err(CoreError::validation(format!(
            "Key 备注不能超过 {} 字符",
            Credential::LABEL_MAX
        )));
    }
    Ok(())
}

fn validate_secret(secret: &str) -> Result<(), CoreError> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return Err(CoreError::validation("API Key 不能为空"));
    }
    if trimmed.len() > 4096 {
        return Err(CoreError::validation("API Key 长度异常"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential() -> Credential {
        Credential::create(
            CredentialId::new("c_1"),
            ProviderId::new("p_1"),
            "工作 Key",
            "sk-live-0123456789abcdef",
            "2026-09-18T00:00:00Z",
        )
        .unwrap()
    }

    #[test]
    fn mask_keeps_only_tail_suffix_for_long_keys() {
        assert_eq!(mask_secret("sk-live-0123456789abcdef"), "••••••••cdef");
    }

    #[test]
    fn mask_hides_short_keys_entirely() {
        assert_eq!(mask_secret("short"), "••••••••");
        assert_eq!(mask_secret(""), "••••••••");
    }

    #[test]
    fn replace_creates_new_secret_version() {
        let mut credential = credential();
        let original_mask = credential.masked_suffix.clone();
        credential
            .replace_secret("sk-live-ffffffffffffffff", "2026-09-18T01:00:00Z")
            .unwrap();
        assert_eq!(credential.secret_version, 2);
        assert_ne!(credential.masked_suffix, original_mask);
        assert_eq!(credential.status, CredentialStatus::Saved);
    }

    #[test]
    fn rejects_blank_label_or_secret() {
        assert!(Credential::create(
            CredentialId::new("c"),
            ProviderId::new("p"),
            "  ",
            "secret",
            "now"
        )
        .is_err());
        assert!(Credential::create(
            CredentialId::new("c"),
            ProviderId::new("p"),
            "工作 Key",
            "   ",
            "now"
        )
        .is_err());
    }

    #[test]
    fn disabled_credential_cannot_become_active() {
        let mut credential = credential();
        credential.status = CredentialStatus::Disabled;
        assert!(credential.mark_active().is_err());
        credential.status = CredentialStatus::Verified;
        assert!(credential.mark_active().is_ok());
    }

    #[test]
    fn keystore_locked_is_distinct_from_auth_failed() {
        assert_ne!(
            CredentialStatus::KeystoreLocked.label_key(),
            CredentialStatus::AuthFailed.label_key()
        );
        assert!(!CredentialStatus::KeystoreLocked.is_selectable());
        assert!(!CredentialStatus::Missing.is_selectable());
    }

    #[test]
    fn deletion_waits_while_bindings_are_active() {
        let credential = credential();
        assert_eq!(credential.deletion_plan(0), DeletionPlan::DeleteNow);
        assert_eq!(
            credential.deletion_plan(3),
            DeletionPlan::DeactivateAndWait { active_bindings: 3 }
        );
    }
}
