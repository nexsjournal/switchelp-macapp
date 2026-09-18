//! 凭据存储端口与系统实现适配层。
//!
//! 规则来自 [安全与跨平台](../../../../docs/architecture/05-security-and-platforms.md)：
//! 明文 Key 只写入系统凭据库，SQLite 只存引用与掩码；凭据库不可用时明确失败，
//! 绝不降级为明文 JSON。

pub mod memory;
pub mod resolver;
pub mod system;

pub use memory::MemoryVault;
pub use resolver::{CredentialResolver, ResolvedSecret};
pub use system::SystemVault;

use crate::domain::error::CoreError;

/// 系统凭据库抽象。实现必须保证：写入失败不留下半条记录，读取失败给出可分类原因。
pub trait SecretVault: Send + Sync {
    /// 写入或覆盖一个秘密，返回条目引用。
    fn store(&self, secret_ref: &str, secret: &str) -> Result<(), CoreError>;

    /// 读取秘密。条目不存在返回 `None`，凭据库锁定返回 `KEYSTORE_LOCKED`。
    fn load(&self, secret_ref: &str) -> Result<Option<String>, CoreError>;

    /// 删除秘密。条目不存在视为成功（幂等）。
    fn delete(&self, secret_ref: &str) -> Result<(), CoreError>;

    /// 条目是否存在。只检查存在性，不返回明文。
    fn exists(&self, secret_ref: &str) -> Result<bool, CoreError>;
}

/// 生成凭据库条目引用。引用本身不含秘密，可以安全落库。
pub fn secret_ref(provider_id: &str, credential_id: &str, version: u32) -> String {
    format!("gptswitch/{provider_id}/{credential_id}/v{version}")
}
