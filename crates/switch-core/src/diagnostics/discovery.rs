//! 模型发现：读取上游 `/models`，结果作为**发现值**，永不覆盖用户手工填写的值。
//!
//! 分层规则来自 [产品需求](../../../../docs/01-product-requirements.md) R07 与
//! [数据与契约](../../../../docs/architecture/04-data-and-contracts.md)：
//! 发现值与用户值是两条独立的层，刷新只更新发现层；用户覆盖过就继续用用户值。
//! 发现失败是正常情况（很多兼容端点没有 `/models`），必须给出可读原因而不是空列表。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::domain::error::{CoreError, ErrorCode};

/// 一个被发现的模型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredModel {
    /// 上游精确 ID，保留大小写与斜杠。
    pub upstream_id: String,
    /// 上游给出的显示名；上游没给就用 ID。
    pub display_name: String,
    /// 是否已经在本地保存过（界面据此显示“已添加”）。
    pub already_saved: bool,
}

/// 读取上游模型列表。
///
/// `saved_upstream_ids` 是本地已保存的模型 ID，用于标记 `already_saved`。
pub fn fetch(
    endpoint: &str,
    secret: &str,
    saved_upstream_ids: &[String],
) -> Result<Vec<DiscoveredModel>, CoreError> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .proxy(ureq::Proxy::try_from_env())
            .build(),
    );
    let url = format!("{}/models", endpoint.trim_end_matches('/'));
    let response = agent
        .get(&url)
        .header("accept", "application/json")
        .header("authorization", format!("Bearer {secret}"))
        .call()
        .map_err(|error| {
            CoreError::new(ErrorCode::Internal, "error.discoveryUnreachable")
                .with_detail(format!("无法读取上游模型列表：{error}"))
        })?;

    let status = response.status().as_u16();
    let mut response = response;
    let text = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .unwrap_or_default();

    if status == 404 {
        return Err(
            CoreError::new(ErrorCode::CapabilityUnsupported, "error.discoveryUnsupported")
                .with_detail("此接口未提供模型列表，请手动填写模型 ID".to_owned()),
        );
    }
    if !(200..300).contains(&status) {
        let (code, key) = match status {
            401 => (ErrorCode::CredentialMissing, "error.discoveryRejected"),
            403 => (ErrorCode::ModelPermissionDenied, "error.discoveryForbidden"),
            _ => (ErrorCode::Internal, "error.discoveryFailed"),
        };
        return Err(CoreError::new(code, key)
            .with_detail(format!("上游返回 {}，未能读取模型列表", status)));
    }

    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
        CoreError::new(ErrorCode::CapabilityUnsupported, "error.discoveryUnparsable")
            .with_detail("上游模型列表不是合法 JSON，请手动填写模型 ID".to_owned())
    })?;
    let data = value.get("data").and_then(|data| data.as_array()).ok_or_else(|| {
        CoreError::new(ErrorCode::CapabilityUnsupported, "error.discoveryUnparsable")
            .with_detail("上游模型列表缺少 data 字段，请手动填写模型 ID".to_owned())
    })?;

    let mut models: Vec<DiscoveredModel> = data
        .iter()
        .filter_map(|entry| entry.get("id").and_then(|id| id.as_str()))
        .filter(|id| !id.trim().is_empty())
        .map(|id| DiscoveredModel {
            upstream_id: id.to_owned(),
            display_name: data
                .iter()
                .find(|entry| entry.get("id").and_then(|value| value.as_str()) == Some(id))
                .and_then(|entry| entry.get("name").and_then(|name| name.as_str()))
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(id)
                .to_owned(),
            already_saved: saved_upstream_ids.iter().any(|saved| saved == id),
        })
        .collect();
    models.sort_by(|a, b| a.upstream_id.cmp(&b.upstream_id));
    models.dedup_by(|a, b| a.upstream_id == b.upstream_id);
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Ipv4Addr, TcpListener};

    const SECRET: &str = "synthetic-discovery-key-0123456789";

    fn mock(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(stream) = incoming else { break };
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut auth = String::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if line.to_ascii_lowercase().starts_with("authorization:") {
                            auth = line["authorization:".len()..].trim().to_owned();
                        }
                    }
                    assert!(auth.starts_with("Bearer "), "发现必须带上凭据");
                    let mut stream = stream;
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    );
                    let _ = stream.flush();
                });
            }
        });
        format!("http://127.0.0.1:{port}/v1")
    }

    #[test]
    fn discovered_models_are_sorted_deduplicated_and_flagged() {
        let endpoint = mock(
            200,
            "{\"data\":[{\"id\":\"vendor/b\"},{\"id\":\"vendor/a\",\"name\":\"模型 A\"},{\"id\":\"vendor/b\"}]}",
        );

        let models = fetch(&endpoint, SECRET, &["vendor/b".to_owned()]).unwrap();

        assert_eq!(models.len(), 2, "重复 ID 只保留一条");
        assert_eq!(models[0].upstream_id, "vendor/a");
        assert_eq!(models[0].display_name, "模型 A");
        assert!(!models[0].already_saved);
        assert_eq!(models[1].upstream_id, "vendor/b");
        assert!(models[1].already_saved);
        assert_eq!(models[1].display_name, "vendor/b", "上游没给名字就回落到 ID");
    }

    #[test]
    fn a_404_is_reported_as_unsupported_rather_than_empty() {
        let endpoint = mock(404, "{\"error\":{\"message\":\"not found\"}}");
        let error = fetch(&endpoint, SECRET, &[]).unwrap_err();

        assert_eq!(error.code, ErrorCode::CapabilityUnsupported);
        assert_eq!(error.message_key, "error.discoveryUnsupported");
        assert!(error.safe_details.iter().any(|detail| detail.contains("手动填写")));
    }

    #[test]
    fn a_rejected_key_is_classified() {
        let endpoint = mock(401, "{\"error\":{\"message\":\"bad key\"}}");
        assert_eq!(
            fetch(&endpoint, SECRET, &[]).unwrap_err().code,
            ErrorCode::CredentialMissing
        );
    }

    #[test]
    fn a_non_json_body_is_reported_as_unparsable() {
        let endpoint = mock(200, "<html>nope</html>");
        let error = fetch(&endpoint, SECRET, &[]).unwrap_err();
        assert_eq!(error.code, ErrorCode::CapabilityUnsupported);
        assert_eq!(error.message_key, "error.discoveryUnparsable");
    }

    #[test]
    fn a_payload_without_data_is_rejected_instead_of_returning_nothing() {
        let endpoint = mock(200, "{\"object\":\"list\"}");
        assert_eq!(
            fetch(&endpoint, SECRET, &[]).unwrap_err().message_key,
            "error.discoveryUnparsable"
        );
    }

    #[test]
    fn an_unreachable_endpoint_reports_the_transport_failure() {
        let error = fetch("http://127.0.0.1:1/v1", SECRET, &[]).unwrap_err();
        assert_eq!(error.message_key, "error.discoveryUnreachable");
    }
}
