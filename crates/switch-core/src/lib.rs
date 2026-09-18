//! GPTSwitch 控制核心。
//!
//! 本 crate 不依赖 Tauri、窗口框架或任何 UI 类型：`src-tauri` 只做装配，
//! React 只通过类型化 DesktopClient 访问同一批 DTO。

pub mod application;
pub mod codex;
pub mod credentials;
pub mod diagnostics;
pub mod domain;
pub mod gateway;
pub mod platform;
pub mod protocols;
pub mod storage;

pub use domain::error::{CoreError, ErrorCode, RecoveryAction};
