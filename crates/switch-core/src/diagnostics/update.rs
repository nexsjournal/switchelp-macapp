//! 更新检查：查询公开仓库的最新发布并与当前版本比较。
//!
//! 只做**比较**，不下载、不安装：自动更新需要签名身份与更新通道，
//! 这两样当前都没有，所以这里给出结论与发布页链接，由用户自己决定。
//!
//! 查询失败时返回 `error`，**不能**把失败说成“已是最新”。

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 更新检查结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    /// 能拿到的最新发布版本；查询失败时为空。
    pub latest: Option<String>,
    pub has_update: bool,
    /// 发布页地址，供用户自己去下载。
    pub release_url: Option<String>,
    pub published_at: Option<String>,
    /// 查询失败的原因（网络、限流、尚无发布等）。
    pub error: Option<String>,
}

/// 默认查询地址：本项目的公开 Release。
pub const DEFAULT_ENDPOINT: &str =
    "https://api.github.com/repos/nexsjournal/gptswitch-macapp/releases/latest";

/// 查询最新发布。`current` 是当前版本（不带 `v` 前缀）。
pub fn check(current: &str, endpoint: &str) -> UpdateStatus {
    let mut status = UpdateStatus {
        current: current.to_owned(),
        latest: None,
        has_update: false,
        release_url: None,
        published_at: None,
        error: None,
    };
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(8)))
            .timeout_recv_response(Some(Duration::from_secs(15)))
            .proxy(ureq::Proxy::try_from_env())
            .build(),
    );
    let response = match agent
        .get(endpoint)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "gptswitch-update-check")
        .call()
    {
        Ok(response) => response,
        Err(error) => {
            status.error = Some(format!("无法查询发布信息：{error}"));
            return status;
        }
    };
    let code = response.status().as_u16();
    if code == 404 {
        status.error = Some("该仓库还没有发布版本".to_owned());
        return status;
    }
    if !(200..300).contains(&code) {
        status.error = Some(format!("查询发布信息失败：HTTP {code}"));
        return status;
    }
    let mut response = response;
    let text = match response.body_mut().with_config().limit(256 * 1024).read_to_string() {
        Ok(text) => text,
        Err(_) => {
            status.error = Some("读取发布信息失败".to_owned());
            return status;
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => {
            status.error = Some("发布信息不是合法 JSON".to_owned());
            return status;
        }
    };

    let latest = value
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .map(|tag| tag.trim_start_matches('v').to_owned())
        .filter(|tag| !tag.is_empty());
    let Some(latest) = latest else {
        status.error = Some("发布信息缺少版本号".to_owned());
        return status;
    };
    status.has_update = latest != current;
    status.latest = Some(latest);
    status.release_url = value
        .get("html_url")
        .and_then(|url| url.as_str())
        .map(str::to_owned);
    status.published_at = value
        .get("published_at")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Ipv4Addr, TcpListener};

    /// 合成发布接口：按脚本回答一次。
    fn endpoint(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Some(Ok(stream)) = listener.incoming().next() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut agent = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if line.to_ascii_lowercase().starts_with("user-agent:") {
                        agent = line["user-agent:".len()..].trim().to_owned();
                    }
                }
                assert!(!agent.is_empty(), "查询必须带 user-agent");
                let mut stream = stream;
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
                let _ = stream.flush();
            }
        });
        format!("http://127.0.0.1:{port}/latest")
    }

    #[test]
    fn a_newer_release_is_reported_with_its_page() {
        let status = check(
            "0.1.0",
            &endpoint(
                200,
                "{\"tag_name\":\"v0.2.0\",\"html_url\":\"https://example.test/releases/v0.2.0\",\"published_at\":\"2026-09-18T00:00:00Z\"}",
            ),
        );

        assert!(status.has_update);
        assert_eq!(status.latest.as_deref(), Some("0.2.0"), "tag 的 v 前缀要剥掉");
        assert_eq!(status.current, "0.1.0");
        assert_eq!(status.release_url.as_deref(), Some("https://example.test/releases/v0.2.0"));
        assert!(status.error.is_none());
    }

    #[test]
    fn the_same_version_is_not_an_update() {
        let status = check("0.2.0", &endpoint(200, "{\"tag_name\":\"v0.2.0\"}"));
        assert!(!status.has_update);
        assert_eq!(status.latest.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn a_repository_without_releases_says_so_instead_of_claiming_up_to_date() {
        let status = check("0.1.0", &endpoint(404, "{\"message\":\"Not Found\"}"));
        assert!(!status.has_update);
        assert!(status.latest.is_none(), "没有发布版本时不能报一个版本出来");
        assert!(status.error.as_deref().unwrap_or_default().contains("还没有发布版本"));
    }

    #[test]
    fn rate_limiting_is_reported_rather_than_treated_as_no_update() {
        // GitHub 限流返回 403：必须报错，不能显示“已是最新”。
        let status = check("0.1.0", &endpoint(403, "{\"message\":\"rate limited\"}"));
        assert!(status.latest.is_none());
        assert!(status.error.as_deref().unwrap_or_default().contains("403"));
    }

    #[test]
    fn an_unreachable_endpoint_reports_the_failure() {
        let status = check("0.1.0", "http://127.0.0.1:1/latest");
        assert!(status.error.is_some());
        assert!(!status.has_update);
    }

    #[test]
    fn a_payload_without_a_tag_is_an_error_not_a_version() {
        let status = check("0.1.0", &endpoint(200, "{\"name\":\"latest\"}"));
        assert!(status.latest.is_none());
        assert!(status.error.as_deref().unwrap_or_default().contains("版本号"));
    }
}
