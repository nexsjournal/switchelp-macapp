//! 本机网关：受认证的 loopback 推理入口。
//!
//! 路由原则（[网关与协议](../../../../docs/architecture/03-gateway-and-protocols.md)）：
//! 一个稳定 alias 对应 `(providerId, modelId)`；请求开始即固定
//! `routeRevision + credentialVersion + protocolVersion`；目录前缀决定可用 alias 集合，
//! 旧前缀不会回落到最新版本。

pub mod auth;
pub mod helper;
pub mod routing;
pub mod server;
pub mod sse;
pub mod timeouts;

pub use auth::{GatewayToken, InboundHeaders, RequestGuard, MAX_REQUEST_BYTES};
pub use helper::{helper_path, install as install_auth_helper, revoke as revoke_auth_helper};
pub use routing::{Admission, AdmissionError, GatewayRouter, RequestRoute};
pub use server::{token_fingerprint, Gateway, GatewayConfig, GatewayStatus};
pub use sse::{SseEvent, SseParser, SseStreamState};
pub use timeouts::TimeoutPolicy;

/// 默认监听端口。文档明确 18765 不是强制端口：端口归属由实例探测决定，
/// 被占用时生成改端口计划，而不是静默换端口。
pub const DEFAULT_PORT: u16 = 18765;

/// 仅绑定 loopback，不监听 `0.0.0.0`。
pub const LOOPBACK_HOST: &str = "127.0.0.1";

/// 该端口的监听来源。只有本工具自己占用的端口才允许发布。
pub fn origin(port: u16) -> String {
    format!("http://{}:{}", LOOPBACK_HOST, port)
}
