//! Codex 接入层：实例检测、配置保真读写、目录编译与兼容性判断。
//!
//! 本模块只处理文件与 schema，不发起网络请求，也不读取会话正文。

pub mod backup;
pub mod catalog;
pub mod coexist;
pub mod config;
pub mod detect;
pub mod plan;

pub use backup::{BackupEntry, BackupStore};
pub use config::{ConfigSnapshot, ManagedConfig, ManagedProvider};
