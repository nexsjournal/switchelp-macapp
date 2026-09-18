//! 原生凭据库。只访问本应用命名空间，不枚举其他应用条目。
use super::SecretVault;
use crate::domain::error::{CoreError, ErrorCode};

pub struct SystemVault {
    service: String,
}

impl SystemVault {
    pub fn new(service: impl Into<String>) -> Result<Self, CoreError> {
        let service = service.into();
        if service.trim().is_empty() {
            return Err(CoreError::validation("凭据服务名不能为空"));
        }
        Ok(Self { service })
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn entry(&self, reference: &str) -> Result<keyring::Entry, CoreError> {
        if !reference.starts_with("gptswitch/")
            || reference.len() > 512
            || reference.chars().any(char::is_control)
        {
            return Err(CoreError::validation("凭据引用无效"));
        }
        keyring::Entry::new(&self.service, reference).map_err(vault_error)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn vault_error(_: keyring::Error) -> CoreError {
    // OS 错误可能含账户、路径等信息；不直接转发到 renderer。
    CoreError::new(ErrorCode::KeystoreLocked, "error.keystoreLocked")
        .with_detail("系统凭据库不可用或访问被拒绝".to_owned())
        .with_recovery("unlock", "action.unlockKeystore")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SecretVault for SystemVault {
    fn store(&self, reference: &str, secret: &str) -> Result<(), CoreError> {
        if secret.trim().is_empty() || secret.len() > 4096 {
            return Err(CoreError::validation("API Key 为空或过长"));
        }
        self.entry(reference)?
            .set_password(secret)
            .map_err(vault_error)
    }
    fn load(&self, reference: &str) -> Result<Option<String>, CoreError> {
        match self.entry(reference)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(vault_error(error)),
        }
    }
    fn delete(&self, reference: &str) -> Result<(), CoreError> {
        match self.entry(reference)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(vault_error(error)),
        }
    }
    fn exists(&self, reference: &str) -> Result<bool, CoreError> {
        use zeroize::Zeroizing;
        Ok(self.load(reference)?.map(Zeroizing::new).is_some())
    }
}

// keyring 在未启用原生后端的平台会退回 mock；这里显式失败，避免报告假保存。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl SecretVault for SystemVault {
    fn store(&self, _: &str, _: &str) -> Result<(), CoreError> {
        Err(unsupported())
    }
    fn load(&self, _: &str) -> Result<Option<String>, CoreError> {
        Err(unsupported())
    }
    fn delete(&self, _: &str) -> Result<(), CoreError> {
        Err(unsupported())
    }
    fn exists(&self, _: &str) -> Result<bool, CoreError> {
        Err(unsupported())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn unsupported() -> CoreError {
    CoreError::new(ErrorCode::KeystoreLocked, "error.keystoreUnsupported")
}
