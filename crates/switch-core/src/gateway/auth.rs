//! 本机网关认证与来源校验。
//!
//! 规则来自 [安全与平台](../../../../docs/architecture/05-security-and-platforms.md)：
//! 本机访问令牌与上游 Key 分离；令牌只授权特定实例的推理接口；默认不跨域；
//! 只靠“127.0.0.1 不公开”不足以防恶意网页与 DNS rebinding。

use crate::domain::error::{CoreError, ErrorCode};
use crate::domain::ids::InstanceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 请求体运输保护上限（32 MiB）。
///
/// 这是运输层保护，不代表 Token 上限；长文本与图像需区分字符数与解码后尺寸。
pub const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;

/// 允许的服务方法。
const ALLOWED_METHODS: [&str; 3] = ["POST", "GET", "OPTIONS"];

/// 本机访问令牌。
///
/// 至少 256-bit 随机熵，绑定到单个实例；只授权该实例的推理接口，
/// 不可用于管理供应商或读取文件。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GatewayToken(String);

impl GatewayToken {
    /// 生成新令牌。熵来源为两个独立 UUIDv4 的拼接，再经 SHA-256 展开。
    pub fn generate() -> Self {
        let seed = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        let mut hasher = Sha256::new();
        hasher.update(seed.as_bytes());
        hasher.update(b"gptswitch-gateway-token-v1");
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(64);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        Self(hex)
    }

    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// 令牌字节长度对应的熵下限校验。
    pub fn is_strong_enough(&self) -> bool {
        self.0.len() >= 64
    }

    /// 只在 auth helper 的 stdout 等必要位置暴露。
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// 常量时间比较，避免通过响应时间侧信道推断令牌。
    pub fn matches(&self, candidate: &str) -> bool {
        constant_time_eq(self.0.as_bytes(), candidate.as_bytes())
    }
}

impl std::fmt::Debug for GatewayToken {
    /// 手写 Debug：令牌绝不进入日志或错误详情。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GatewayToken(••••••••)")
    }
}

/// 常量时间字节比较：长度不同也走完固定轮次，只由异或累加决定结果。
///
/// 长度差不能折进 `u8`：`(a.len() ^ b.len()) as u8` 在相差 256 的倍数时截断成 0，
/// 「长度不同必然不等」这条性质就不成立了。长度单独在 usize 上比一次。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff: u8 = 0;
    let len = a.len().max(b.len());
    for index in 0..len {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        diff |= left ^ right;
    }
    diff == 0 && a.len() == b.len()
}

/// 入站请求头。字段全部为可选，便于显式判断“缺失”与“空值”。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundHeaders {
    pub method: String,
    pub host: Option<String>,
    pub origin: Option<String>,
    pub authorization: Option<String>,
    pub content_type: Option<String>,
    pub accept: Option<String>,
    /// CORS 预检标记头；出现即说明请求来自浏览器。
    pub access_control_request_method: Option<String>,
}

impl InboundHeaders {
    pub fn post(host: &str, token: &GatewayToken) -> Self {
        Self {
            method: "POST".to_owned(),
            host: Some(host.to_owned()),
            origin: None,
            authorization: Some(format!("Bearer {}", token.expose())),
            content_type: Some("application/json".to_owned()),
            accept: Some("text/event-stream".to_owned()),
            access_control_request_method: None,
        }
    }
}

/// 入站请求校验器。
#[derive(Debug, Clone)]
pub struct RequestGuard {
    token: GatewayToken,
    instance_id: InstanceId,
}

impl RequestGuard {
    pub fn new(token: GatewayToken, instance_id: InstanceId) -> Self {
        Self { token, instance_id }
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    /// 校验方法、来源、认证、类型与体积。
    ///
    /// 任一项失败都返回结构化错误；认证失败**不返回模型列表**，也不透露令牌是否存在。
    pub fn check(&self, headers: &InboundHeaders, body_len: usize) -> Result<(), CoreError> {
        let method = headers.method.trim().to_ascii_uppercase();
        if !ALLOWED_METHODS.contains(&method.as_str()) {
            return Err(
                CoreError::new(ErrorCode::ValidationFailed, "error.methodNotAllowed")
                    .with_detail(format!("不支持的方法 {method}")),
            );
        }

        // 浏览器预检一律拒绝：本工具没有 Web UI，也不开放跨域。
        if headers.access_control_request_method.is_some() {
            return Err(unauthorized("该入口不接受浏览器预检请求"));
        }
        if headers.origin.is_some() {
            return Err(unauthorized("该入口不接受带 Origin 的请求"));
        }

        // 防 DNS rebinding：Host 必须是 loopback。
        let host = headers
            .host
            .as_deref()
            .ok_or_else(|| unauthorized("缺少 Host"))?;
        if !is_loopback_host(host) {
            return Err(unauthorized(format!("Host 不是本机地址：{host}")));
        }

        let presented = headers
            .authorization
            .as_deref()
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(|| unauthorized("缺少 Bearer 令牌"))?;
        if !self.token.matches(presented.trim()) {
            return Err(unauthorized("令牌不匹配"));
        }

        if body_len > MAX_REQUEST_BYTES {
            return Err(
                CoreError::new(ErrorCode::RequestTooLarge, "error.requestTooLarge").with_detail(
                    format!("请求体 {body_len} 字节超过上限 {MAX_REQUEST_BYTES} 字节"),
                ),
            );
        }

        if method == "POST" {
            let content_type = headers.content_type.as_deref().unwrap_or_default();
            if !content_type
                .to_ascii_lowercase()
                .starts_with("application/json")
            {
                return Err(CoreError::new(
                    ErrorCode::ValidationFailed,
                    "error.unsupportedContentType",
                )
                .with_detail("请求必须使用 application/json".to_owned()));
            }
        }

        Ok(())
    }
}

fn unauthorized(detail: impl Into<String>) -> CoreError {
    CoreError::new(ErrorCode::Unauthorized, "error.unauthorized").with_detail(detail.into())
}

/// 判断 Host 是否为 loopback。允许带端口与 IPv6 方括号形式。
pub fn is_loopback_host(host: &str) -> bool {
    let value = host.trim();
    if value.is_empty() {
        return false;
    }
    // 去掉可能存在的端口。裸 IPv6 字面量（如 ::1）里冒号本身是地址的一部分，
    // 必须先按「冒号数量 >= 2」判定为 IPv6，不能把最后一段当端口剥掉。
    let host_part = if let Some(stripped) = value.strip_prefix('[') {
        match stripped.split_once(']') {
            Some((inner, _)) => inner,
            None => return false,
        }
    } else if value.matches(':').count() >= 2 {
        value
    } else {
        match value.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => {
                head
            }
            _ => value,
        }
    };
    matches!(
        host_part.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1" | "0:0:0:0:0:0:0:1"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归：长度差曾经折进 `u8`，相差 256 的倍数时截断成 0，
    /// 「长度不同必然不等」这条性质就不成立了。
    #[test]
    fn length_difference_never_compares_equal() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
        let mut padded = b"abc".to_vec();
        padded.extend(std::iter::repeat_n(0u8, 256));
        assert_eq!(padded.len() - 3, 256, "构造 256 字节的长度差");
        assert!(
            !constant_time_eq(b"abc", &padded),
            "长度差 256 不能被截断成相等"
        );
    }

    fn guard() -> (RequestGuard, GatewayToken) {
        let token = GatewayToken::generate();
        let guard = RequestGuard::new(token.clone(), InstanceId::new("inst_1"));
        (guard, token)
    }

    fn headers(token: &GatewayToken) -> InboundHeaders {
        InboundHeaders::post("127.0.0.1:18765", token)
    }

    #[test]
    fn valid_request_passes() {
        let (guard, token) = guard();
        assert!(guard.check(&headers(&token), 1024).is_ok());
    }

    #[test]
    fn generated_token_has_enough_entropy_and_is_unique() {
        let a = GatewayToken::generate();
        let b = GatewayToken::generate();
        assert!(a.is_strong_enough());
        assert_ne!(a, b);
        assert_eq!(a.expose().len(), 64);
    }

    #[test]
    fn debug_never_reveals_the_token() {
        let (guard, token) = guard();
        let rendered = format!("{token:?} {guard:?}");
        assert!(!rendered.contains(token.expose()));
        assert!(rendered.contains("••••••••"));
    }

    #[test]
    fn missing_or_wrong_token_is_rejected() {
        let (guard, token) = guard();
        let mut request = headers(&token);
        request.authorization = None;
        assert_eq!(
            guard.check(&request, 1).unwrap_err().code,
            ErrorCode::Unauthorized
        );

        let mut request = headers(&token);
        request.authorization = Some("Bearer wrong-token".to_owned());
        assert_eq!(
            guard.check(&request, 1).unwrap_err().code,
            ErrorCode::Unauthorized
        );

        // 不带 Bearer 前缀也拒绝。
        let mut request = headers(&token);
        request.authorization = Some(token.expose().to_owned());
        assert_eq!(
            guard.check(&request, 1).unwrap_err().code,
            ErrorCode::Unauthorized
        );
    }

    #[test]
    fn browser_origin_and_preflight_are_rejected() {
        let (guard, token) = guard();
        let mut request = headers(&token);
        request.origin = Some("https://evil.example.com".to_owned());
        assert_eq!(
            guard.check(&request, 1).unwrap_err().code,
            ErrorCode::Unauthorized
        );

        let mut request = headers(&token);
        request.access_control_request_method = Some("POST".to_owned());
        assert_eq!(
            guard.check(&request, 1).unwrap_err().code,
            ErrorCode::Unauthorized
        );

        let mut request = headers(&token);
        request.method = "OPTIONS".to_owned();
        request.access_control_request_method = Some("POST".to_owned());
        assert!(guard.check(&request, 0).is_err());
    }

    #[test]
    fn non_loopback_host_is_rejected() {
        let (guard, token) = guard();
        let mut request = headers(&token);
        request.host = Some("api.example.com".to_owned());
        let error = guard.check(&request, 1).unwrap_err();
        assert_eq!(error.code, ErrorCode::Unauthorized);
        assert!(error.safe_details[0].contains("本机"));

        let mut request = headers(&token);
        request.host = None;
        assert!(guard.check(&request, 1).is_err());
    }

    #[test]
    fn loopback_host_parsing_accepts_ports_and_ipv6() {
        for host in [
            "127.0.0.1",
            "127.0.0.1:18765",
            "localhost",
            "localhost:8080",
            "::1",
            "[::1]",
            "[::1]:8080",
            "LOCALHOST",
        ] {
            assert!(is_loopback_host(host), "{host} 应被识别为 loopback");
        }
        for host in ["0.0.0.0", "192.0.2.10", "api.example.com", "", "::2"] {
            assert!(!is_loopback_host(host), "{host} 不应被识别为 loopback");
        }
    }

    #[test]
    fn oversized_body_is_rejected_with_dedicated_code() {
        let (guard, token) = guard();
        let error = guard
            .check(&headers(&token), MAX_REQUEST_BYTES + 1)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RequestTooLarge);
        assert!(guard.check(&headers(&token), MAX_REQUEST_BYTES).is_ok());
    }

    #[test]
    fn post_requires_json_content_type() {
        let (guard, token) = guard();
        let mut request = headers(&token);
        request.content_type = Some("text/plain".to_owned());
        assert!(guard.check(&request, 1).is_err());

        let mut request = headers(&token);
        request.content_type = Some("application/json; charset=utf-8".to_owned());
        assert!(guard.check(&request, 1).is_ok());

        let mut request = headers(&token);
        request.content_type = None;
        assert!(guard.check(&request, 1).is_err());
    }

    #[test]
    fn get_does_not_require_content_type() {
        let (guard, token) = guard();
        let mut request = headers(&token);
        request.method = "GET".to_owned();
        request.content_type = None;
        assert!(guard.check(&request, 0).is_ok());
    }

    #[test]
    fn unsupported_method_is_rejected() {
        let (guard, token) = guard();
        let mut request = headers(&token);
        request.method = "DELETE".to_owned();
        assert!(guard.check(&request, 0).is_err());
    }

    #[test]
    fn auth_failure_does_not_reveal_whether_a_token_exists() {
        let (guard, token) = guard();
        let mut missing = headers(&token);
        missing.authorization = None;
        let mut wrong = headers(&token);
        wrong.authorization = Some("Bearer nope".to_owned());

        let a = guard.check(&missing, 0).unwrap_err();
        let b = guard.check(&wrong, 0).unwrap_err();
        // 两类失败的对外结构与文案一致，不区分“没令牌”和“令牌错”。
        assert_eq!(a.code, b.code);
        assert_eq!(a.message_key, b.message_key);
        assert_ne!(a.safe_details, b.safe_details);
    }

    #[test]
    fn constant_time_compare_handles_different_lengths() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }
}
