//! Switchelp 控制核心。
//!
//! 本 crate 不依赖 Tauri、窗口框架或任何 UI 类型：`src-tauri` 只做装配，
//! React 只通过类型化 DesktopClient 访问同一批 DTO。

pub mod application;
pub mod codex;
pub mod content;
pub mod credentials;
pub mod diagnostics;
pub mod domain;
pub mod gateway;
pub mod platform;
pub mod plugins;
pub mod protocols;
pub mod storage;
pub mod toolhub;
pub mod usage;

pub use domain::error::{CoreError, ErrorCode, RecoveryAction};

/// 当前 UTC 时间（Unix 秒）。壳与核心用同一个时钟来源，避免各处各取一次系统时间。
pub fn time_now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
