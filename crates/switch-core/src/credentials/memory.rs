//! 内存凭据库：供单元测试与离线演示使用，不用于生产路径。

use super::SecretVault;
use crate::domain::error::{CoreError, ErrorCode};
use std::collections::HashMap;
use std::sync::Mutex;

/// 内存实现。可注入故障以覆盖凭据库不可用分支。
#[derive(Debug, Default)]
pub struct MemoryVault {
    entries: Mutex<HashMap<String, String>>,
    locked: Mutex<bool>,
}

impl MemoryVault {
    pub fn new() -> Self {
        Self::default()
    }

    /// 模拟系统凭据库被锁定。锁定时所有操作返回 `KEYSTORE_LOCKED`。
    pub fn set_locked(&self, locked: bool) {
        *self.locked.lock().expect("锁未被污染") = locked;
    }

    pub fn len(&self) -> usize {
        self.entries.lock().expect("锁未被污染").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn guard(&self) -> Result<(), CoreError> {
        if *self.locked.lock().expect("锁未被污染") {
            return Err(CoreError::new(ErrorCode::KeystoreLocked, "error.keystoreLocked")
                .with_recovery("unlock", "action.unlockKeystore"));
        }
        Ok(())
    }
}

impl SecretVault for MemoryVault {
    fn store(&self, secret_ref: &str, secret: &str) -> Result<(), CoreError> {
        self.guard()?;
        if secret.trim().is_empty() {
            return Err(CoreError::validation("秘密不能为空"));
        }
        self.entries
            .lock()
            .expect("锁未被污染")
            .insert(secret_ref.to_owned(), secret.to_owned());
        Ok(())
    }

    fn load(&self, secret_ref: &str) -> Result<Option<String>, CoreError> {
        self.guard()?;
        Ok(self
            .entries
            .lock()
            .expect("锁未被污染")
            .get(secret_ref)
            .cloned())
    }

    fn delete(&self, secret_ref: &str) -> Result<(), CoreError> {
        self.guard()?;
        self.entries.lock().expect("锁未被污染").remove(secret_ref);
        Ok(())
    }

    fn exists(&self, secret_ref: &str) -> Result<bool, CoreError> {
        self.guard()?;
        Ok(self
            .entries
            .lock()
            .expect("锁未被污染")
            .contains_key(secret_ref))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_loads_secret_by_reference() {
        let vault = MemoryVault::new();
        vault.store("ref-1", "sk-test-0123456789abcdef").unwrap();
        assert_eq!(
            vault.load("ref-1").unwrap().as_deref(),
            Some("sk-test-0123456789abcdef")
        );
        assert!(vault.exists("ref-1").unwrap());
    }

    #[test]
    fn load_missing_entry_returns_none_not_error() {
        let vault = MemoryVault::new();
        assert_eq!(vault.load("missing").unwrap(), None);
        assert!(!vault.exists("missing").unwrap());
    }

    #[test]
    fn delete_is_idempotent() {
        let vault = MemoryVault::new();
        vault.store("ref", "sk-0123456789abcdef").unwrap();
        vault.delete("ref").unwrap();
        vault.delete("ref").unwrap();
        assert_eq!(vault.load("ref").unwrap(), None);
    }

    #[test]
    fn locked_keystore_reports_dedicated_code() {
        let vault = MemoryVault::new();
        vault.set_locked(true);
        let error = vault.store("ref", "sk-0123456789abcdef").unwrap_err();
        assert_eq!(error.code, ErrorCode::KeystoreLocked);
        assert!(!error.recovery_actions.is_empty());
        assert!(vault.load("ref").unwrap_err().code == ErrorCode::KeystoreLocked);
        // 锁定期间绝不写入任何内容，更不落明文。
        assert!(vault.is_empty());
    }

    #[test]
    fn rejects_blank_secret() {
        let vault = MemoryVault::new();
        assert!(vault.store("ref", "   ").is_err());
    }
}
