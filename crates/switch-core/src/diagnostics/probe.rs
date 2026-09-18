//! 连接探测：把“能不能用”拆成可判定的阶段，而不是一个笼统的成功/失败。
//!
//! 阶段划分（对应 [产品需求](../../../../docs/01-product-requirements.md) R04）：
//! 1. `connect`：地址可达、TLS 完成；
//! 2. `credential`：Key 是否被接受；
//! 3. `model`：该模型是否出现在上游模型列表里；
//! 4. `generate`：真实发一次最小请求（**有网络副作用与费用**，默认不执行）。
//!
//! 探测只读取，不写 Codex 配置、不改动已保存的实体。取消在阶段之间生效：
//! 正在进行的阻塞 HTTP 调用无法中途打断，这一点在结果里如实标注。

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::domain::error::ErrorCode;
use crate::domain::provider::Protocol;
use crate::protocols::{chat, responses};

/// 单个阶段的判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProbeState {
    Running,
    Passed,
    Failed,
    Skipped,
}

/// 一个阶段的结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageOutcome {
    pub stage_key: String,
    pub status: ProbeState,
    pub elapsed_ms: Option<u64>,
    pub message_key: String,
    pub error_code: Option<ErrorCode>,
}

impl StageOutcome {
    fn new(stage_key: &str, state: ProbeState, message_key: &str, elapsed_ms: Option<u64>) -> Self {
        Self {
            stage_key: stage_key.to_owned(),
            status: state,
            elapsed_ms,
            message_key: message_key.to_owned(),
            error_code: None,
        }
    }

    fn failed(
        stage_key: &str,
        code: ErrorCode,
        message_key: &str,
        elapsed_ms: Option<u64>,
    ) -> Self {
        Self {
            error_code: Some(code),
            ..Self::new(stage_key, ProbeState::Failed, message_key, elapsed_ms)
        }
    }
}

/// 探测目标。只带标识，不带秘密——秘密由调用方临时解析后传入。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeTarget {
    pub provider_id: String,
    pub credential_id: String,
    pub model_id: Option<String>,
    pub upstream_id: Option<String>,
    /// 界面展示用的可读名称。
    pub label: String,
}

/// 探测计划：是否包含真实生成、以及用哪个协议发。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbePlan {
    pub protocol: Protocol,
    pub include_generate: bool,
}

impl ProbePlan {
    pub fn read_only(protocol: Protocol) -> Self {
        Self {
            protocol,
            include_generate: false,
        }
    }
}

/// 探测结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub id: String,
    pub target_label: String,
    pub stages: Vec<StageOutcome>,
    pub started_at: String,
    /// 是否被用户取消；被取消时后续阶段标记为 skipped。
    pub cancelled: bool,
    /// 是否真的向供应商发起了生成请求（有费用）。
    pub generated: bool,
}

impl ProbeReport {
    /// 是否全部通过（跳过的阶段不算失败）。
    pub fn passed(&self) -> bool {
        !self
            .stages
            .iter()
            .any(|stage| stage.status == ProbeState::Failed)
    }
}

/// 探测执行器。持有一个可取消集合与复用的 HTTP 代理。
pub struct Probes {
    agent: ureq::Agent,
    cancelled: Mutex<HashSet<String>>,
}

impl Default for Probes {
    fn default() -> Self {
        Self::new()
    }
}

impl Probes {
    pub fn new() -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                .proxy(ureq::Proxy::try_from_env())
                .build(),
        );
        Self {
            agent,
            cancelled: Mutex::new(HashSet::new()),
        }
    }

    /// 请求取消。返回是否命中一个仍在登记的探测。
    pub fn cancel(&self, probe_id: &str) -> bool {
        self.cancelled
            .lock()
            .expect("取消集合锁未被污染")
            .insert(probe_id.to_owned())
    }

    pub fn is_cancelled(&self, probe_id: &str) -> bool {
        self.cancelled
            .lock()
            .expect("取消集合锁未被污染")
            .contains(probe_id)
    }

    /// 执行探测。`secret` 只用于请求头，绝不进入结果。
    pub fn run(
        &self,
        probe_id: &str,
        endpoint: &str,
        target: &ProbeTarget,
        plan: &ProbePlan,
        secret: &str,
    ) -> ProbeReport {
        let started_at = super::now_rfc3339();
        let base = endpoint.trim_end_matches('/').to_owned();
        let mut stages = Vec::new();
        let mut generated = false;

        // 阶段 1+2+3 共用一次 /models 调用：少发一次请求就少一次被限流的可能。
        let listing = self.fetch_models(&base, secret);
        let (connect, credential, model) = judge_listing(&listing, target);
        stages.push(connect);
        let models_ok = credential.status == ProbeState::Passed;
        stages.push(credential);
        stages.push(model);

        if plan.include_generate && !self.is_cancelled(probe_id) {
            let Some(upstream_id) = target.upstream_id.as_deref() else {
                stages.push(StageOutcome::new(
                    "generate",
                    ProbeState::Skipped,
                    "probe.skippedNoModel",
                    None,
                ));
                return self.finish(probe_id, target, stages, started_at, generated);
            };
            generated = true;
            stages.push(self.generate(&base, plan.protocol, upstream_id, secret));
        } else if plan.include_generate {
            stages.push(StageOutcome::new(
                "generate",
                ProbeState::Skipped,
                "probe.cancelled",
                None,
            ));
        }

        let _ = models_ok;
        self.finish(probe_id, target, stages, started_at, generated)
    }

    fn finish(
        &self,
        probe_id: &str,
        target: &ProbeTarget,
        stages: Vec<StageOutcome>,
        started_at: String,
        generated: bool,
    ) -> ProbeReport {
        let cancelled = self.is_cancelled(probe_id);
        self.cancelled
            .lock()
            .expect("取消集合锁未被污染")
            .remove(probe_id);
        ProbeReport {
            id: probe_id.to_owned(),
            target_label: target.label.clone(),
            stages,
            started_at,
            cancelled,
            generated,
        }
    }

    /// 读取上游模型列表。返回值只包含三者之一：成功解析的 id 列表、HTTP 状态、传输错误。
    fn fetch_models(&self, base: &str, secret: &str) -> Listing {
        let started = Instant::now();
        let call = self
            .agent
            .get(&format!("{base}/models"))
            .header("accept", "application/json")
            .header("authorization", format!("Bearer {secret}"));
        match call.call() {
            Ok(response) => {
                let status = response.status().as_u16();
                let elapsed = started.elapsed().as_millis() as u64;
                let mut response = response;
                let text = response
                    .body_mut()
                    .with_config()
                    .limit(512 * 1024)
                    .read_to_string()
                    .unwrap_or_default();
                Listing::Status {
                    status,
                    elapsed,
                    ids: parse_model_ids(&text),
                }
            }
            Err(error) => Listing::Transport {
                elapsed: started.elapsed().as_millis() as u64,
                detail: error.to_string(),
            },
        }
    }

    /// 真实生成一次最小请求。这是唯一有副作用与费用的阶段。
    fn generate(
        &self,
        base: &str,
        protocol: Protocol,
        upstream_id: &str,
        secret: &str,
    ) -> StageOutcome {
        let (url, body) = match protocol {
            Protocol::Responses => (
                responses::endpoint(base),
                serde_json::json!({
                    "model": upstream_id,
                    "input": [{"type": "message", "role": "user",
                               "content": [{"type": "input_text", "text": "ping"}]}],
                    "max_output_tokens": 16,
                    "stream": false,
                }),
            ),
            Protocol::ChatCompletions => (
                chat::endpoint(base),
                serde_json::json!({
                    "model": upstream_id,
                    "messages": [{"role": "user", "content": "ping"}],
                    "max_tokens": 16,
                    "stream": false,
                }),
            ),
        };
        let started = Instant::now();
        let bytes = match serde_json::to_vec(&body) {
            Ok(bytes) => bytes,
            Err(_) => {
                return StageOutcome::failed(
                    "generate",
                    ErrorCode::Internal,
                    "probe.generateFailed",
                    None,
                )
            }
        };
        let call = self
            .agent
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {secret}"))
            .send(bytes.as_slice());
        let elapsed = started.elapsed().as_millis() as u64;
        match call {
            Ok(response) if response.status().is_success() => StageOutcome::new(
                "generate",
                ProbeState::Passed,
                "probe.generatePassed",
                Some(elapsed),
            ),
            Ok(response) => {
                let status = response.status().as_u16();
                let (code, key) = classify_status(status);
                StageOutcome::failed("generate", code, key, Some(elapsed))
            }
            Err(_) => StageOutcome::failed(
                "generate",
                ErrorCode::Internal,
                "probe.upstreamUnreachable",
                Some(elapsed),
            ),
        }
    }
}

/// `/models` 调用的三种结果。
enum Listing {
    Status {
        status: u16,
        elapsed: u64,
        ids: Option<Vec<String>>,
    },
    Transport {
        elapsed: u64,
        detail: String,
    },
}

/// 把一次 `/models` 调用拆成 connect / credential / model 三个结论。
fn judge_listing(
    listing: &Listing,
    target: &ProbeTarget,
) -> (StageOutcome, StageOutcome, StageOutcome) {
    match listing {
        Listing::Transport { elapsed, detail } => {
            let (code, message_key) = classify_transport(detail);
            (
                StageOutcome::failed("connect", code, message_key, Some(*elapsed)),
                StageOutcome::new(
                    "credential",
                    ProbeState::Skipped,
                    "probe.skippedUnreachable",
                    None,
                ),
                StageOutcome::new(
                    "model",
                    ProbeState::Skipped,
                    "probe.skippedUnreachable",
                    None,
                ),
            )
        }
        Listing::Status {
            status,
            elapsed,
            ids,
        } => {
            let connect = StageOutcome::new(
                "connect",
                ProbeState::Passed,
                "probe.connected",
                Some(*elapsed),
            );
            match status {
                200..=299 => {
                    let credential = StageOutcome::new(
                        "credential",
                        ProbeState::Passed,
                        "probe.credentialAccepted",
                        Some(*elapsed),
                    );
                    let model = judge_model(ids.as_deref(), target);
                    (connect, credential, model)
                }
                401 => (
                    connect,
                    StageOutcome::failed(
                        "credential",
                        ErrorCode::CredentialMissing,
                        "probe.credentialRejected",
                        Some(*elapsed),
                    ),
                    StageOutcome::new(
                        "model",
                        ProbeState::Skipped,
                        "probe.skippedNoCredential",
                        None,
                    ),
                ),
                403 => (
                    connect,
                    StageOutcome::failed(
                        "credential",
                        ErrorCode::ModelPermissionDenied,
                        "probe.credentialForbidden",
                        Some(*elapsed),
                    ),
                    StageOutcome::new(
                        "model",
                        ProbeState::Skipped,
                        "probe.skippedNoCredential",
                        None,
                    ),
                ),
                404 => (
                    connect,
                    StageOutcome::new(
                        "credential",
                        ProbeState::Skipped,
                        "probe.credentialUnknown",
                        Some(*elapsed),
                    ),
                    StageOutcome::new(
                        "model",
                        ProbeState::Skipped,
                        "probe.modelsUnsupported",
                        Some(*elapsed),
                    ),
                ),
                other => {
                    let (code, key) = classify_status(*other);
                    (
                        connect,
                        StageOutcome::failed("credential", code, key, Some(*elapsed)),
                        StageOutcome::new(
                            "model",
                            ProbeState::Skipped,
                            "probe.skippedNoCredential",
                            None,
                        ),
                    )
                }
            }
        }
    }
}

fn judge_model(ids: Option<&[String]>, target: &ProbeTarget) -> StageOutcome {
    let Some(upstream_id) = target.upstream_id.as_deref() else {
        return StageOutcome::new("model", ProbeState::Skipped, "probe.skippedNoModel", None);
    };
    let Some(ids) = ids else {
        return StageOutcome::new("model", ProbeState::Skipped, "probe.modelsUnparsable", None);
    };
    if ids.iter().any(|id| id == upstream_id) {
        StageOutcome::new("model", ProbeState::Passed, "probe.modelFound", None)
    } else {
        StageOutcome::failed("model", ErrorCode::NotFound, "probe.modelMissing", None)
    }
}

/// 解析 OpenAI 兼容的 `/models` 响应，只取 id。
fn parse_model_ids(text: &str) -> Option<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let data = value.get("data")?.as_array()?;
    Some(
        data.iter()
            .filter_map(|entry| {
                entry
                    .get("id")
                    .and_then(|id| id.as_str())
                    .map(str::to_owned)
            })
            .collect(),
    )
}

fn classify_status(status: u16) -> (ErrorCode, &'static str) {
    match status {
        401 => (ErrorCode::CredentialMissing, "probe.credentialRejected"),
        403 => (
            ErrorCode::ModelPermissionDenied,
            "probe.credentialForbidden",
        ),
        404 => (ErrorCode::NotFound, "probe.modelMissing"),
        429 => (ErrorCode::Internal, "probe.rateLimited"),
        400..=499 => (ErrorCode::ValidationFailed, "probe.upstreamRejected"),
        _ => (ErrorCode::Internal, "probe.upstreamFailed"),
    }
}

fn classify_transport(detail: &str) -> (ErrorCode, &'static str) {
    if detail.contains("timeout") || detail.contains("timed out") {
        (ErrorCode::Internal, "probe.timedOut")
    } else if detail.contains("dns")
        || detail.contains("resolve")
        || detail.contains("Name or service")
    {
        (ErrorCode::ValidationFailed, "probe.unresolvable")
    } else {
        (ErrorCode::Internal, "probe.upstreamUnreachable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{Ipv4Addr, TcpListener};

    const SECRET: &str = "synthetic-probe-key-0123456789";

    /// 可编排的合成上游：按序返回预设响应。
    struct Mock {
        endpoint: String,
        seen: std::sync::Arc<Mutex<Vec<(String, String)>>>,
    }

    impl Mock {
        fn start(replies: Vec<(u16, &'static str)>) -> Self {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
            let sink = seen.clone();
            std::thread::spawn(move || {
                let mut replies = replies.into_iter();
                for incoming in listener.incoming() {
                    let Ok(stream) = incoming else { break };
                    let Some((status, body)) = replies.next() else {
                        break;
                    };
                    let sink = sink.clone();
                    std::thread::spawn(move || {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut path = String::new();
                        let mut auth = String::new();
                        let mut length = 0usize;
                        loop {
                            let mut line = String::new();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                                break;
                            }
                            let lower = line.to_ascii_lowercase();
                            if lower.starts_with("content-length:") {
                                length =
                                    line["content-length:".len()..].trim().parse().unwrap_or(0);
                            } else if lower.starts_with("authorization:") {
                                auth = line["authorization:".len()..].trim().to_owned();
                            } else if lower.starts_with("get ") || lower.starts_with("post ") {
                                path = line
                                    .split_whitespace()
                                    .nth(1)
                                    .unwrap_or_default()
                                    .to_owned();
                            }
                        }
                        let mut body_bytes = vec![0u8; length];
                        if length > 0 {
                            let _ = reader.read_exact(&mut body_bytes);
                        }
                        sink.lock().unwrap().push((path, auth));
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
            Self {
                endpoint: format!("http://127.0.0.1:{port}/v1"),
                seen,
            }
        }

        fn paths(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|(path, _)| path.clone())
                .collect()
        }

        fn secrets(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|(_, auth)| auth.clone())
                .collect()
        }
    }

    fn target(upstream: Option<&str>) -> ProbeTarget {
        ProbeTarget {
            provider_id: "p_a".into(),
            credential_id: "c_a".into(),
            model_id: Some("m_a".into()),
            upstream_id: upstream.map(str::to_owned),
            label: "测试供应商".into(),
        }
    }

    fn stage<'a>(report: &'a ProbeReport, key: &str) -> &'a StageOutcome {
        report
            .stages
            .iter()
            .find(|stage| stage.stage_key == key)
            .expect("阶段应存在")
    }

    #[test]
    fn a_healthy_provider_passes_connect_credential_and_model() {
        let mock = Mock::start(vec![(
            200,
            "{\"data\":[{\"id\":\"vendor/Model-X\"},{\"id\":\"vendor/other\"}]}",
        )]);
        let probes = Probes::new();

        let report = probes.run(
            "probe_1",
            &mock.endpoint,
            &target(Some("vendor/Model-X")),
            &ProbePlan::read_only(Protocol::ChatCompletions),
            SECRET,
        );

        assert!(report.passed());
        assert_eq!(stage(&report, "connect").status, ProbeState::Passed);
        assert_eq!(stage(&report, "credential").status, ProbeState::Passed);
        assert_eq!(stage(&report, "model").status, ProbeState::Passed);
        assert!(!report.generated, "只读探测不得发起生成请求");
        assert_eq!(mock.paths(), vec!["/v1/models"], "只读探测只打一次 /models");
        assert_eq!(
            mock.secrets()[0],
            format!("Bearer {SECRET}"),
            "凭据只在请求头里"
        );
    }

    #[test]
    fn a_missing_model_fails_the_model_stage_but_not_the_credential_stage() {
        let mock = Mock::start(vec![(200, "{\"data\":[{\"id\":\"vendor/other\"}]}")]);
        let probes = Probes::new();

        let report = probes.run(
            "probe_2",
            &mock.endpoint,
            &target(Some("vendor/Model-X")),
            &ProbePlan::read_only(Protocol::ChatCompletions),
            SECRET,
        );

        assert!(!report.passed());
        assert_eq!(stage(&report, "credential").status, ProbeState::Passed);
        assert_eq!(stage(&report, "model").status, ProbeState::Failed);
        assert_eq!(
            stage(&report, "model").error_code,
            Some(ErrorCode::NotFound)
        );
    }

    #[test]
    fn a_rejected_key_is_reported_as_a_credential_failure() {
        let mock = Mock::start(vec![(401, "{\"error\":{\"message\":\"bad key\"}}")]);
        let probes = Probes::new();

        let report = probes.run(
            "probe_3",
            &mock.endpoint,
            &target(Some("vendor/Model-X")),
            &ProbePlan::read_only(Protocol::ChatCompletions),
            SECRET,
        );

        assert_eq!(stage(&report, "connect").status, ProbeState::Passed);
        assert_eq!(stage(&report, "credential").status, ProbeState::Failed);
        assert_eq!(
            stage(&report, "credential").error_code,
            Some(ErrorCode::CredentialMissing)
        );
        assert_eq!(stage(&report, "model").status, ProbeState::Skipped);
    }

    #[test]
    fn an_unreachable_endpoint_fails_connect_and_skips_the_rest() {
        let probes = Probes::new();
        // 127.0.0.1:1 基本不可能有监听。
        let report = probes.run(
            "probe_4",
            "http://127.0.0.1:1/v1",
            &target(Some("vendor/Model-X")),
            &ProbePlan::read_only(Protocol::ChatCompletions),
            SECRET,
        );

        assert_eq!(stage(&report, "connect").status, ProbeState::Failed);
        assert_eq!(stage(&report, "credential").status, ProbeState::Skipped);
        assert_eq!(stage(&report, "model").status, ProbeState::Skipped);
    }

    #[test]
    fn endpoints_without_a_models_list_skip_rather_than_fail() {
        let mock = Mock::start(vec![(404, "{\"error\":{\"message\":\"not found\"}}")]);
        let probes = Probes::new();

        let report = probes.run(
            "probe_5",
            &mock.endpoint,
            &target(Some("vendor/Model-X")),
            &ProbePlan::read_only(Protocol::ChatCompletions),
            SECRET,
        );

        assert_eq!(stage(&report, "connect").status, ProbeState::Passed);
        assert_eq!(stage(&report, "credential").status, ProbeState::Skipped);
        assert_eq!(stage(&report, "model").status, ProbeState::Skipped);
        assert!(report.passed(), "跳过的阶段不算失败");
    }

    #[test]
    fn including_generate_sends_one_real_request_and_marks_the_side_effect() {
        let mock = Mock::start(vec![
            (200, "{\"data\":[{\"id\":\"vendor/Model-X\"}]}"),
            (200, "{\"choices\":[{\"message\":{\"content\":\"pong\"}}]}"),
        ]);
        let probes = Probes::new();

        let report = probes.run(
            "probe_6",
            &mock.endpoint,
            &target(Some("vendor/Model-X")),
            &ProbePlan {
                protocol: Protocol::ChatCompletions,
                include_generate: true,
            },
            SECRET,
        );

        assert!(report.generated, "必须标注发生过真实生成");
        assert_eq!(stage(&report, "generate").status, ProbeState::Passed);
        assert_eq!(mock.paths(), vec!["/v1/models", "/v1/chat/completions"]);
    }

    #[test]
    fn generate_uses_the_responses_endpoint_for_responses_providers() {
        let mock = Mock::start(vec![
            (200, "{\"data\":[{\"id\":\"vendor/Model-X\"}]}"),
            (200, "{\"id\":\"resp_1\",\"output\":[]}"),
        ]);
        let probes = Probes::new();

        probes.run(
            "probe_7",
            &mock.endpoint,
            &target(Some("vendor/Model-X")),
            &ProbePlan {
                protocol: Protocol::Responses,
                include_generate: true,
            },
            SECRET,
        );

        assert_eq!(mock.paths(), vec!["/v1/models", "/v1/responses"]);
    }

    #[test]
    fn cancelling_before_generate_skips_it_instead_of_running_it() {
        let mock = Mock::start(vec![(200, "{\"data\":[{\"id\":\"vendor/Model-X\"}]}")]);
        let probes = Probes::new();
        probes.cancel("probe_8");

        let report = probes.run(
            "probe_8",
            &mock.endpoint,
            &target(Some("vendor/Model-X")),
            &ProbePlan {
                protocol: Protocol::ChatCompletions,
                include_generate: true,
            },
            SECRET,
        );

        assert!(report.cancelled);
        assert_eq!(stage(&report, "generate").status, ProbeState::Skipped);
        assert_eq!(stage(&report, "generate").message_key, "probe.cancelled");
        assert!(!report.generated, "被取消时不得真的发出生成请求");
        assert_eq!(mock.paths(), vec!["/v1/models"]);
    }

    #[test]
    fn the_report_never_contains_the_secret() {
        let mock = Mock::start(vec![(200, "{\"data\":[{\"id\":\"vendor/Model-X\"}]}")]);
        let probes = Probes::new();

        let report = probes.run(
            "probe_9",
            &mock.endpoint,
            &target(Some("vendor/Model-X")),
            &ProbePlan::read_only(Protocol::ChatCompletions),
            SECRET,
        );

        let rendered = serde_json::to_string(&report).unwrap();
        assert!(!rendered.contains(SECRET));
        assert!(rendered.contains("probe.connected"));
    }

    #[test]
    fn model_ids_parse_defensively() {
        assert_eq!(
            parse_model_ids("{\"data\":[{\"id\":\"a\"},{\"id\":\"b\"}]}").unwrap(),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(
            parse_model_ids("{\"data\":[]}").unwrap(),
            Vec::<String>::new()
        );
        assert!(parse_model_ids("not json").is_none());
        assert!(
            parse_model_ids("{\"object\":\"list\"}").is_none(),
            "缺少 data 时不应假装拿到了空列表"
        );
    }
}
