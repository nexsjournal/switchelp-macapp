//! 凭据解析：把凭据元数据解析为一次性可用的秘密值。
//!
//! 关键约束（来自安全文档）：
//! - 秘密只在请求所需时间存在于内存，不进入 UI DTO、日志或诊断包。
//! - 替换 Key 产生新版本，在途请求继续使用旧版本。
//! - 凭据库条目缺失与 API 401 是不同状态，给不同修复动作。

use super::SecretVault;
use crate::domain::credential::{Credential, CredentialStatus};
use crate::domain::error::{CoreError, ErrorCode};
use std::collections::HashMap;
use std::sync::Mutex;
use zeroize::Zeroize;

/// 解析出的秘密值。`Drop` 时清空缓冲区，减少内存驻留时间。
pub struct ResolvedSecret {
    value: String,
    pub credential_id: String,
    pub secret_version: u32,
}

impl ResolvedSecret {
    /// 只在构造请求头等必要位置暴露，调用方不得写入日志或 DTO。
    pub fn expose(&self) -> &str {
        &self.value
    }

    pub fn masked(&self) -> String {
        crate::domain::credential::mask_secret(&self.value)
    }
}

impl std::fmt::Debug for ResolvedSecret {
    /// 手写 Debug：避免秘密被 `{:?}` 意外打印到日志。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedSecret")
            .field("credential_id", &self.credential_id)
            .field("secret_version", &self.secret_version)
            .field("value", &"••••••••")
            .finish()
    }
}

impl Drop for ResolvedSecret {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

/// 凭据解析器：带一个有界的内存缓存，退出/锁定/轮换时清空。
pub struct CredentialResolver<'a> {
    vault: &'a dyn SecretVault,
    cache: Mutex<HashMap<String, (u32, String)>>,
    cache_enabled: bool,
}

impl<'a> CredentialResolver<'a> {
    pub fn new(vault: &'a dyn SecretVault) -> Self {
        Self {
            vault,
            cache: Mutex::new(HashMap::new()),
            cache_enabled: true,
        }
    }

    /// 关闭缓存后每次解析都回到凭据库，供锁定/轮换场景使用。
    pub fn without_cache(vault: &'a dyn SecretVault) -> Self {
        Self {
            vault,
            cache: Mutex::new(HashMap::new()),
            cache_enabled: false,
        }
    }

    /// 清空缓存。凭据库锁定、系统休眠唤醒、Key 轮换后必须调用。
    pub fn clear_cache(&self) {
        self.cache.lock().expect("锁未被污染").clear();
    }

    pub fn cache_len(&self) -> usize {
        self.cache.lock().expect("锁未被污染").len()
    }

    /// 解析指定版本的秘密。
    ///
    /// - 凭据状态不可选择时拒绝，不尝试“也许还能用”。
    /// - 条目缺失返回 `CREDENTIAL_MISSING`，与 401 区分。
    pub fn resolve(&self, credential: &Credential) -> Result<ResolvedSecret, CoreError> {
        if !credential.status.is_selectable() {
            return Err(CoreError::new(
                ErrorCode::CredentialMissing,
                "error.credentialNotSelectable",
            )
            .with_detail(format!(
                "凭据状态为 {}，不能用于新请求",
                credential.status.label_key()
            )));
        }

        let key = credential.id.as_str().to_owned();
        if self.cache_enabled {
            if let Some((version, value)) = self.cache.lock().expect("锁未被污染").get(&key) {
                if *version == credential.secret_version {
                    return Ok(ResolvedSecret {
                        value: value.clone(),
                        credential_id: credential.id.as_str().to_owned(),
                        secret_version: *version,
                    });
                }
            }
        }

        let secret = self.vault.load(&credential.secret_ref)?.ok_or_else(|| {
            CoreError::new(ErrorCode::CredentialMissing, "error.credentialMissing")
                .with_detail("此 Key 的安全记录不存在，需要重新填写".to_owned())
                .with_recovery("reenter", "action.reenterSecret")
        })?;

        if self.cache_enabled {
            self.cache
                .lock()
                .expect("锁未被污染")
                .insert(key, (credential.secret_version, secret.clone()));
        }

        Ok(ResolvedSecret {
            value: secret,
            credential_id: credential.id.as_str().to_owned(),
            secret_version: credential.secret_version,
        })
    }

    /// 保存新秘密并返回更新后的凭据元数据。旧版本条目保留给在途请求。
    pub fn store_new_version(
        &self,
        credential: &mut Credential,
        secret: &str,
        now: impl Into<String>,
    ) -> Result<(), CoreError> {
        credential.replace_secret(secret, now)?;
        let reference = crate::credentials::secret_ref(
            credential.provider_id.as_str(),
            credential.id.as_str(),
            credential.secret_version,
        );
        self.vault.store(&reference, secret)?;
        credential.secret_ref = reference;
        // 轮换后缓存必须失效，否则新请求仍可能拿到旧秘密。
        self.cache
            .lock()
            .expect("锁未被污染")
            .remove(credential.id.as_str());
        Ok(())
    }

    /// 首次保存。
    pub fn store_initial(
        &self,
        credential: &mut Credential,
        secret: &str,
    ) -> Result<(), CoreError> {
        let reference = crate::credentials::secret_ref(
            credential.provider_id.as_str(),
            credential.id.as_str(),
            credential.secret_version,
        );
        self.vault.store(&reference, secret)?;
        credential.secret_ref = reference;
        Ok(())
    }

    /// 凭据库记录是否存在。用于区分“记录缺失”与“认证失败”。
    pub fn entry_present(&self, credential: &Credential) -> Result<bool, CoreError> {
        self.vault.exists(&credential.secret_ref)
    }

    /// 撤销：先停止分配新请求，再清理安全存储。
    pub fn revoke(&self, credential: &Credential) -> Result<(), CoreError> {
        self.vault.delete(&credential.secret_ref)?;
        self.cache
            .lock()
            .expect("锁未被污染")
            .remove(credential.id.as_str());
        Ok(())
    }
}

/// 依据探测结果更新凭据状态。401 与凭据库缺失必须区分。
pub fn status_from_probe(
    http_status: Option<u16>,
    keystore_error: Option<ErrorCode>,
) -> CredentialStatus {
    if let Some(code) = keystore_error {
        return match code {
            ErrorCode::KeystoreLocked => CredentialStatus::KeystoreLocked,
            ErrorCode::CredentialMissing => CredentialStatus::Missing,
            _ => CredentialStatus::Saved,
        };
    }
    match http_status {
        Some(200..=299) => CredentialStatus::Verified,
        Some(401) => CredentialStatus::AuthFailed,
        Some(403) => CredentialStatus::ScopeLimited,
        _ => CredentialStatus::Saved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryVault;
    use crate::domain::ids::{CredentialId, ProviderId};

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

    fn seeded() -> (MemoryVault, Credential) {
        let vault = MemoryVault::new();
        let mut credential = credential();
        let resolver = CredentialResolver::new(&vault);
        resolver
            .store_initial(&mut credential, "sk-live-0123456789abcdef")
            .unwrap();
        (vault, credential)
    }

    #[test]
    fn resolves_secret_and_exposes_masked_view() {
        let (vault, credential) = seeded();
        let resolver = CredentialResolver::new(&vault);
        let secret = resolver.resolve(&credential).unwrap();
        assert_eq!(secret.expose(), "sk-live-0123456789abcdef");
        assert_eq!(secret.masked(), "••••••••cdef");
        assert_eq!(secret.secret_version, 1);
    }

    #[test]
    fn debug_output_never_contains_the_secret() {
        let (vault, credential) = seeded();
        let resolver = CredentialResolver::new(&vault);
        let secret = resolver.resolve(&credential).unwrap();
        let rendered = format!("{secret:?}");
        assert!(!rendered.contains("sk-live"));
        assert!(rendered.contains("••••••••"));
    }

    #[test]
    fn caching_avoids_repeated_vault_reads() {
        let (vault, credential) = seeded();
        let resolver = CredentialResolver::new(&vault);
        resolver.resolve(&credential).unwrap();
        assert_eq!(resolver.cache_len(), 1);
        // 缓存命中时即使凭据库被锁定也能继续服务在途请求。
        vault.set_locked(true);
        assert!(resolver.resolve(&credential).is_ok());

        // 清空缓存后立即暴露锁定状态。
        resolver.clear_cache();
        assert_eq!(
            resolver.resolve(&credential).unwrap_err().code,
            ErrorCode::KeystoreLocked
        );
    }

    #[test]
    fn replacement_bumps_version_and_invalidates_cache() {
        let (vault, mut credential) = seeded();
        let resolver = CredentialResolver::new(&vault);
        let old = resolver.resolve(&credential).unwrap();
        assert_eq!(old.secret_version, 1);

        resolver
            .store_new_version(
                &mut credential,
                "sk-live-ffffffffffffffff",
                "2026-09-18T01:00:00Z",
            )
            .unwrap();
        assert_eq!(credential.secret_version, 2);

        let new = resolver.resolve(&credential).unwrap();
        assert_eq!(new.expose(), "sk-live-ffffffffffffffff");
        // 旧版本条目仍存在，供在途请求使用。
        assert!(vault.load("gptswitch/p_1/c_1/v1").unwrap().is_some());
    }

    #[test]
    fn missing_keystore_entry_is_distinct_from_auth_failure() {
        let vault = MemoryVault::new();
        let mut credential = credential();
        credential.secret_ref = "gptswitch/p_1/c_1/v1".to_owned();
        let resolver = CredentialResolver::new(&vault);
        let error = resolver.resolve(&credential).unwrap_err();
        assert_eq!(error.code, ErrorCode::CredentialMissing);
        assert!(!error.recovery_actions.is_empty());
    }

    #[test]
    fn disabled_credential_is_refused_even_if_present_in_vault() {
        let (vault, mut credential) = seeded();
        let resolver = CredentialResolver::new(&vault);
        credential.status = CredentialStatus::Disabled;
        let error = resolver.resolve(&credential).unwrap_err();
        assert_eq!(error.code, ErrorCode::CredentialMissing);
        // 条目仍然存在，说明拒绝来自状态而不是存储。
        assert!(resolver.entry_present(&credential).unwrap());
    }

    #[test]
    fn revoke_removes_secret_and_cache() {
        let (vault, credential) = seeded();
        let resolver = CredentialResolver::new(&vault);
        resolver.resolve(&credential).unwrap();
        resolver.revoke(&credential).unwrap();
        assert_eq!(resolver.cache_len(), 0);
        assert!(!resolver.entry_present(&credential).unwrap());
    }

    #[test]
    fn uncached_resolver_always_hits_the_vault() {
        let (vault, credential) = seeded();
        let resolver = CredentialResolver::without_cache(&vault);
        resolver.resolve(&credential).unwrap();
        assert_eq!(resolver.cache_len(), 0);
    }

    #[test]
    fn probe_status_mapping_separates_401_from_missing_record() {
        assert_eq!(
            status_from_probe(Some(200), None),
            CredentialStatus::Verified
        );
        assert_eq!(
            status_from_probe(Some(401), None),
            CredentialStatus::AuthFailed
        );
        assert_eq!(
            status_from_probe(Some(403), None),
            CredentialStatus::ScopeLimited
        );
        assert_eq!(status_from_probe(Some(500), None), CredentialStatus::Saved);
        assert_eq!(
            status_from_probe(None, Some(ErrorCode::KeystoreLocked)),
            CredentialStatus::KeystoreLocked
        );
        assert_eq!(
            status_from_probe(None, Some(ErrorCode::CredentialMissing)),
            CredentialStatus::Missing
        );
        assert_ne!(
            status_from_probe(Some(401), None),
            status_from_probe(None, Some(ErrorCode::CredentialMissing))
        );
    }
}
