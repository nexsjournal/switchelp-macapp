use super::error::CoreError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 供应商 Base URL 的规范化解析结果。
///
/// 规则来自安全文档：远程 endpoint 默认 HTTPS，仅显式 loopback 允许 HTTP；
/// 保留大小写敏感路径与 query，不允许 userinfo 携带秘密。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrl {
    raw: String,
    normalized: String,
    origin: String,
    path: String,
    query: Option<String>,
    loopback: bool,
}

/// 用户可能粘贴完整接口地址；这里给出建议 Base URL，而不是悄悄拼成重复路径。
const KNOWN_ENDPOINT_SUFFIXES: [&str; 4] =
    ["/chat/completions", "/responses", "/completions", "/models"];

impl BaseUrl {
    pub fn parse(input: &str) -> Result<Self, CoreError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(CoreError::validation("Base URL 不能为空"));
        }
        if trimmed.chars().any(|c| c.is_whitespace()) {
            return Err(CoreError::validation("Base URL 不能包含空白字符"));
        }

        let (scheme, rest) = trimmed
            .split_once("://")
            .ok_or_else(|| CoreError::validation("Base URL 必须包含 http:// 或 https://"))?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(CoreError::validation("Base URL 只支持 http 或 https"));
        }

        let (authority, remainder) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, ""),
        };
        if authority.is_empty() {
            return Err(CoreError::validation("Base URL 缺少主机名"));
        }
        if authority.contains('@') {
            return Err(CoreError::validation("Base URL 不能在 userinfo 中携带凭据"));
        }
        if authority.chars().any(|c| c.is_ascii_whitespace()) {
            return Err(CoreError::validation("Base URL 主机名非法"));
        }

        let (host, port) = split_host_port(authority)?;
        if host.is_empty() {
            return Err(CoreError::validation("Base URL 缺少主机名"));
        }
        let loopback = is_loopback_host(host);
        if scheme == "http" && !loopback {
            return Err(CoreError::validation(
                "远程地址必须使用 HTTPS；http 仅允许本机 loopback",
            ));
        }

        let (path_part, query) = match remainder.split_once('?') {
            Some((path, query)) => (path, Some(query.to_owned())),
            None => (remainder, None),
        };
        // 去尾斜杠但保留大小写：路径大小写敏感，不能整体小写化。
        let path = if path_part == "/" {
            String::new()
        } else {
            path_part.trim_end_matches('/').to_owned()
        };

        let origin = match port {
            Some(port) => format!("{scheme}://{host}:{port}"),
            None => format!("{scheme}://{host}"),
        };
        let mut normalized = format!("{origin}{path}");
        if let Some(query) = &query {
            normalized.push('?');
            normalized.push_str(query);
        }

        Ok(Self {
            raw: trimmed.to_owned(),
            normalized,
            origin,
            path,
            query,
            loopback,
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn normalized(&self) -> &str {
        &self.normalized
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// 归一化路径，空字符串表示根路径。
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn is_loopback(&self) -> bool {
        self.loopback
    }

    /// 在现有路径后拼接相对片段，用于生成具体接口地址。
    pub fn join(&self, segment: &str) -> String {
        let segment = segment.trim_start_matches('/');
        format!("{}/{}", self.normalized.trim_end_matches('/'), segment)
    }

    /// 若用户粘贴的是具体接口地址，返回建议的 Base URL。
    pub fn suggest_base(&self) -> Option<String> {
        let lowered = self.path.to_ascii_lowercase();
        for suffix in KNOWN_ENDPOINT_SUFFIXES {
            if lowered.ends_with(suffix) {
                let head = &self.path[..self.path.len() - suffix.len()];
                return Some(format!("{}{}", self.origin, head.trim_end_matches('/')));
            }
        }
        None
    }
}

fn split_host_port(authority: &str) -> Result<(&str, Option<u16>), CoreError> {
    // IPv6 字面量形如 [::1]:8080
    if let Some(stripped) = authority.strip_prefix('[') {
        let (host, rest) = stripped
            .split_once(']')
            .ok_or_else(|| CoreError::validation("IPv6 地址缺少右括号"))?;
        return match rest.strip_prefix(':') {
            Some(port) => Ok((host, Some(parse_port(port)?))),
            None if rest.is_empty() => Ok((host, None)),
            None => Err(CoreError::validation("IPv6 地址格式非法")),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Ok((host, Some(parse_port(port)?))),
        None => Ok((authority, None)),
    }
}

fn parse_port(value: &str) -> Result<u16, CoreError> {
    value
        .parse::<u16>()
        .map_err(|_| CoreError::validation("端口必须是 0-65535 的整数"))
}

fn is_loopback_host(host: &str) -> bool {
    let lowered = host.to_ascii_lowercase();
    lowered == "localhost"
        || lowered == "127.0.0.1"
        || lowered == "::1"
        || lowered == "0:0:0:0:0:0:0:1"
}

impl fmt::Display for BaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_trailing_slash_only() {
        let url = BaseUrl::parse("https://API.Example.com/V1/").unwrap();
        assert_eq!(url.normalized(), "https://API.Example.com/V1");
        assert_eq!(url.path(), "/V1");
    }

    #[test]
    fn rejects_plain_http_for_remote_hosts() {
        assert!(BaseUrl::parse("http://api.example.com/v1").is_err());
    }

    #[test]
    fn allows_http_for_loopback_only() {
        assert!(BaseUrl::parse("http://127.0.0.1:18765/v1")
            .unwrap()
            .is_loopback());
        assert!(BaseUrl::parse("http://localhost:8080")
            .unwrap()
            .is_loopback());
        assert!(BaseUrl::parse("http://[::1]:8080").unwrap().is_loopback());
        assert!(!BaseUrl::parse("https://api.example.com")
            .unwrap()
            .is_loopback());
    }

    #[test]
    fn rejects_userinfo_credentials() {
        assert!(BaseUrl::parse("https://user:key@api.example.com").is_err());
    }

    #[test]
    fn rejects_missing_scheme_and_relative_url() {
        assert!(BaseUrl::parse("api.example.com/v1").is_err());
        assert!(BaseUrl::parse("ftp://api.example.com").is_err());
        assert!(BaseUrl::parse("").is_err());
    }

    #[test]
    fn keeps_query_and_path_case() {
        let url = BaseUrl::parse("https://api.example.com/OpenAI/V1?api-version=2024").unwrap();
        assert_eq!(url.path(), "/OpenAI/V1");
        assert_eq!(url.query(), Some("api-version=2024"));
        assert_eq!(
            url.normalized(),
            "https://api.example.com/OpenAI/V1?api-version=2024"
        );
    }

    #[test]
    fn parses_port_and_rejects_invalid_port() {
        let url = BaseUrl::parse("https://api.example.com:8443/v1").unwrap();
        assert_eq!(url.origin(), "https://api.example.com:8443");
        assert!(BaseUrl::parse("https://api.example.com:99999").is_err());
        assert!(BaseUrl::parse("https://api.example.com:abc").is_err());
    }

    #[test]
    fn join_appends_endpoint_without_double_slash() {
        let url = BaseUrl::parse("https://api.example.com/v1/").unwrap();
        assert_eq!(url.join("/models"), "https://api.example.com/v1/models");
        assert_eq!(
            url.join("chat/completions"),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn suggests_base_url_when_user_pastes_endpoint() {
        let url = BaseUrl::parse("https://api.example.com/v1/chat/completions").unwrap();
        assert_eq!(
            url.suggest_base().as_deref(),
            Some("https://api.example.com/v1")
        );
        let url = BaseUrl::parse("https://api.example.com/v1").unwrap();
        assert_eq!(url.suggest_base(), None);
    }
}
