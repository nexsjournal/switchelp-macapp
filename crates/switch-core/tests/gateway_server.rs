//! 本机网关的端到端测试：真实 TCP、真实 HTTP、真实 SSE 转发。
//!
//! 覆盖的不变量（来自 [安全与跨平台](../../docs/architecture/05-security-and-platforms.md)
//! 与 [网关与协议](../../docs/architecture/03-gateway-and-protocols.md)）：
//! - 未认证请求绝不触达上游；
//! - 未发布的目录版本与 alias 不回落、不猜测；
//! - 上游是 chat 协议时，宿主仍然只看到合法的 Responses 事件序列；
//! - 上游 Key 不出现在任何回给宿主的内容里。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use switch_core::{
    application::{ModelDraft, ProviderDraft, WorkspaceService},
    credentials::MemoryVault,
    diagnostics::{DiagnosticLog, LogLevel},
    domain::{
        ids::{InstanceId, RevisionId},
        model::ModelPolicy,
        provider::{AuthKind, Protocol},
        tokens::TokenCount,
    },
    gateway::{
        Gateway, GatewayConfig, GatewayRouter, GatewayToken, TimeoutPolicy, MAX_REQUEST_BYTES,
    },
    protocols::{CHAT_COMPLETIONS_V1, RESPONSES_V1},
    storage::{
        snapshot::{RouteEntry, RouteSnapshot},
        Repository, SqliteRepository,
    },
};

const SECRET: &str = "synthetic-upstream-key-0123456789";
const INSTANCE: &str = "inst_test";
const REVISION: &str = "rev_test";

/// 合成上游：按脚本回答，并记录收到的请求体。
struct MockUpstream {
    endpoint: String,
    received: Arc<Mutex<Vec<String>>>,
}

enum MockReply {
    Sse(&'static str),
    Status {
        code: u16,
        body: String,
    },
    /// 先读请求、停一会儿再回：用来观察「请求进行中」的中间状态。
    SseDelayed(&'static str, Duration),
}

impl MockUpstream {
    fn start(reply: MockReply) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();
        std::thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(stream) = incoming else { break };
                let sink = sink.clone();
                let reply = match &reply {
                    MockReply::Sse(body) => MockReply::Sse(body),
                    MockReply::Status { code, body } => MockReply::Status {
                        code: *code,
                        body: body.clone(),
                    },
                    MockReply::SseDelayed(body, delay) => MockReply::SseDelayed(body, *delay),
                };
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut length = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            break;
                        }
                        if line == "\r\n" || line == "\n" {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; length];
                    if length > 0 {
                        let _ = reader.read_exact(&mut body);
                    }
                    sink.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&body).into_owned());

                    let mut stream = stream;
                    if let MockReply::SseDelayed(_, delay) = reply {
                        std::thread::sleep(delay);
                    }
                    let response = match reply {
                        MockReply::SseDelayed(body, _) | MockReply::Sse(body) => format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
                        ),
                        MockReply::Status { code, body } => format!(
                            "HTTP/1.1 {code} Error\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        ),
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                });
            }
        });
        Self {
            endpoint: format!("http://127.0.0.1:{port}/v1"),
            received,
        }
    }

    fn requests(&self) -> usize {
        self.received.lock().unwrap().len()
    }

    fn last_body(&self) -> Value {
        let bodies = self.received.lock().unwrap();
        serde_json::from_str(bodies.last().expect("上游应至少收到一次请求")).unwrap()
    }
}

struct Harness {
    gateway: Arc<Gateway>,
    alias: String,
    port: u16,
    token: GatewayToken,
    upstream: MockUpstream,
    /// 用例据此制造「路由快照还在、底层实体已经没了」这类竞态。
    repository: Arc<dyn Repository>,
    /// 检查请求结束后目录版本的引用计数有没有回到 0。
    router: Arc<GatewayRouter>,
}

/// 路由上冻结的模型策略。
#[derive(Default, Clone)]
struct TestPolicy {
    credential_version_offset: u32,
    output_limit: Option<u64>,
    reasoning_efforts: Vec<String>,
    native_modalities: Vec<String>,
}

impl TestPolicy {
    fn declaring(modalities: &[&str]) -> Self {
        Self {
            native_modalities: modalities.iter().map(|value| (*value).to_owned()).collect(),
            ..Self::default()
        }
    }
}

impl Harness {
    /// `protocol_id` 决定网关用哪个适配器。
    fn start(protocol_id: &str, reply: MockReply, provider_protocol: Protocol) -> Self {
        Self::start_with(protocol_id, reply, provider_protocol, TestPolicy::default())
    }

    fn start_with(
        protocol_id: &str,
        reply: MockReply,
        provider_protocol: Protocol,
        route_policy: TestPolicy,
    ) -> Self {
        let upstream = MockUpstream::start(reply);
        let repository: Arc<dyn Repository> = Arc::new(SqliteRepository::in_memory().unwrap());
        let vault = Arc::new(MemoryVault::new());
        let workspace = WorkspaceService::new(repository.clone(), vault.clone());
        let provider = workspace
            .save_provider(
                ProviderDraft {
                    id: None,
                    name: "测试供应商".into(),
                    endpoint: upstream.endpoint.clone(),
                    protocol: provider_protocol,
                    auth_kind: AuthKind::ApiKey,
                    preset_id: None,
                    notes: None,
                    enabled: true,
                },
                0,
            )
            .unwrap();
        let credential = workspace
            .add_credential(provider.id.as_str(), "日常", SECRET.into())
            .unwrap();
        workspace
            .select_credential(provider.id.as_str(), credential.id.as_str())
            .unwrap();
        let policy = ModelPolicy {
            context_limit: Some(TokenCount::new(128_000).unwrap()),
            output_limit: Some(TokenCount::new(8_192).unwrap()),
            ..Default::default()
        };
        let model = workspace
            .save_model(
                ModelDraft {
                    id: None,
                    provider_id: provider.id.as_str().to_owned(),
                    upstream_id: "vendor/Upstream-Model".into(),
                    catalog_alias: String::new(),
                    display_name: "测试模型".into(),
                    policy,
                    in_catalog: true,
                    display_name_overridden: true,
                    protocol_override: None,
                },
                0,
            )
            .unwrap();

        let router = Arc::new(GatewayRouter::new());
        router
            .publish(
                RouteSnapshot::new(
                    RevisionId::new(REVISION),
                    InstanceId::new(INSTANCE),
                    REVISION,
                    vec![RouteEntry {
                        alias: model.catalog_alias.as_str().to_owned(),
                        provider_id: provider.id.clone(),
                        model_id: model.id.clone(),
                        upstream_id: model.upstream_id.clone(),
                        credential_id: credential.id.clone(),
                        credential_version: credential.secret_version
                            + route_policy.credential_version_offset,
                        protocol_id: protocol_id.to_owned(),
                        output_limit: route_policy.output_limit,
                        reasoning_efforts: route_policy.reasoning_efforts.clone(),
                        native_modalities: route_policy.native_modalities.clone(),
                    }],
                    "2026-09-18T00:00:00Z",
                )
                .unwrap(),
            )
            .unwrap();

        let token = GatewayToken::from_raw("t".repeat(64));
        let gateway = Arc::new(Gateway::new(
            repository.clone(),
            vault,
            router.clone(),
            GatewayConfig {
                instance_id: InstanceId::new(INSTANCE),
                token: token.clone(),
                port: 0,
                timeouts: TimeoutPolicy::default(),
                diagnostics: Arc::new(DiagnosticLog::default()),
            },
        ));
        let port = gateway.bind().unwrap();
        gateway.spawn().unwrap();
        Self {
            gateway,
            router,
            repository,
            alias: model.catalog_alias.as_str().to_owned(),
            port,
            token,
            upstream,
        }
    }

    fn url(&self, suffix: &str) -> String {
        format!(
            "http://127.0.0.1:{}/i/{INSTANCE}/c/{REVISION}/v1/{suffix}",
            self.port
        )
    }

    fn post(&self, suffix: &str, token: Option<&str>, body: &Value) -> (u16, String) {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut call = agent
            .post(&self.url(suffix))
            .header("content-type", "application/json");
        if let Some(token) = token {
            call = call.header("authorization", format!("Bearer {token}"));
        }
        let response = call.send(body.to_string().as_bytes()).unwrap();
        let status = response.status().as_u16();
        let text = response.into_body().read_to_string().unwrap_or_default();
        (status, text)
    }

    fn request_body(&self) -> Value {
        request_body(&self.alias)
    }

    /// 任意路径上的 GET，用来看错误响应本身（而不是只看状态码）。
    fn get_path(path: &str, token: Option<&str>) -> (u16, String) {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut call = agent.get(path).header("host", "127.0.0.1");
        if let Some(token) = token {
            call = call.header("authorization", format!("Bearer {token}"));
        }
        // 判定阶段失败的旧行为是**直接关掉连接**，于是这里会 `Err` 而不是拿到响应。
        // 用例直接 unwrap：拿不到响应本身就是失败，报错信息比断言状态码更直白。
        let response = call.call().expect("网关必须给出响应，而不是关掉连接");
        let status = response.status().as_u16();
        let text = response.into_body().read_to_string().unwrap_or_default();
        (status, text)
    }
}

/// 等目录版本的引用计数排空，返回最终值。
///
/// 释放靠 `InferenceGuard` 的 `Drop`，而客户端拿到完整响应体可能**早于**服务端任务析构那个
/// 守卫。所以直接 `assert_eq!(refs, 0)` 是一个时序假设：本机连跑 40 次全过，CI 上却偶发红灯
/// 一次（2026-09-22 实测，失败的是「上游失败的分支也要释放引用」那条）。这里给一个有界的
/// 等待窗口——到期仍不为 0 才是真泄漏，并把实际值带进断言消息。
fn refs_after_drain(router: &GatewayRouter, revision: &str) -> usize {
    let started = std::time::Instant::now();
    loop {
        let live = router.refs(revision).total();
        if live == 0 || started.elapsed() > Duration::from_secs(5) {
            return live;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn request_body(alias: &str) -> Value {
    json!({
        "model": alias,
        "instructions": "你是编码助手",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "你好"}]}],
        "max_output_tokens": 1024,
        "stream": true,
    })
}

const CHAT_SSE: &str = concat!(
    "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"你好\"}}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"世界\"},\"finish_reason\":\"stop\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2,\"total_tokens\":9}}\n\n",
    "data: [DONE]\n\n",
);

const RESPONSES_SSE: &str = concat!(
    "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_1\",\"model\":\"vendor/Upstream-Model\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
    "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":1,\"response\":{\"id\":\"resp_1\",\"model\":\"vendor/Upstream-Model\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
);

#[test]
fn missing_or_wrong_token_is_rejected() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let body = request_body("whatever");

    let (status, text) = harness.post("responses", None, &body);
    assert_eq!(status, 401, "缺少令牌必须拒绝");
    assert!(text.contains("UNAUTHORIZED"));

    let (status, _) = harness.post("responses", Some("wrong"), &body);
    assert_eq!(status, 401, "错误令牌必须拒绝");
    assert_eq!(harness.upstream.requests(), 0, "认证失败绝不能触发上游请求");
}

#[test]
fn unknown_alias_is_rejected_without_reaching_the_upstream() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &request_body("gs/not-published"));

    assert_eq!(status, 404);
    assert!(
        text.contains("ROUTE_MISMATCH"),
        "必须给出路由不匹配而不是通用错误：{text}"
    );
    assert_eq!(harness.upstream.requests(), 0);
}

#[test]
fn unknown_catalog_revision_is_rejected() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();
    let url = format!(
        "http://127.0.0.1:{}/i/{INSTANCE}/c/rev_missing/v1/responses",
        harness.port
    );
    let response = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
        .post(&url)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .send(request_body("x").to_string().as_bytes())
        .unwrap();

    assert_eq!(
        response.status().as_u16(),
        404,
        "未发布的目录版本不能回落到最新"
    );
}

#[test]
fn chat_upstream_is_translated_into_responses_events() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());

    assert_eq!(status, 200);
    for expected in [
        "event: response.created",
        "event: response.output_item.added",
        "event: response.content_part.added",
        "event: response.output_text.delta",
        "event: response.output_text.done",
        "event: response.output_item.done",
        "event: response.completed",
    ] {
        assert!(text.contains(expected), "缺少事件 {expected}：{text}");
    }
    assert!(text.contains("你好"), "增量文本必须转发");
    assert!(text.contains("世界"));
    assert!(
        text.contains("\"total_tokens\":9"),
        "用量必须翻译成 Responses 形状：{text}"
    );
    assert!(text.contains(&harness.alias), "响应身份必须是 alias");
    assert!(
        !text.contains("vendor/Upstream-Model"),
        "上游模型 ID 不得泄漏给宿主"
    );

    let upstream_body = harness.upstream.last_body();
    assert_eq!(upstream_body["model"], "vendor/Upstream-Model");
    assert_eq!(upstream_body["messages"][0]["role"], "system");
    assert_eq!(upstream_body["messages"][1]["content"], "你好");
    assert_eq!(upstream_body["max_tokens"], 1024);
    assert_eq!(upstream_body["stream"], true);
}

/// 回归：Codex 把开发者指令放在 `input` 里（role 为 `developer`，实测 0.155 有 1.3 万字符），
/// 而 chat 上游只认 system/user/assistant/tool。原样转发会被上游 400，用户在宿主里看到的
/// 只是「上游拒绝了请求」，无从知道是哪一处翻译漏了。
#[test]
fn codex_developer_instructions_never_reach_a_chat_upstream() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();
    let mut body = harness.request_body();
    body["input"] = json!([
        {"type": "message", "role": "developer", "content": [
            {"type": "input_text", "text": "开发者指令"}]},
        {"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": "你好"}]},
    ]);

    let (status, _) = harness.post("responses", Some(&token), &body);

    assert_eq!(status, 200);
    let upstream = harness.upstream.last_body();
    let roles: Vec<&str> = upstream["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, vec!["system", "user"], "上游不得收到 developer 角色");
    assert_eq!(
        upstream["messages"][0]["content"], "你是编码助手\n\n开发者指令",
        "两条系统级内容合并成一条 system"
    );
}

/// 上游把错误当成流里的数据帧发出来（限流、内容过滤、内部错误）。
///
/// 回归：翻译器只认 `choices`/`delta`，这种帧过去被当成「没有内容」继续，
/// 最后照发 `response.completed` —— 半句话被伪装成完整回复。
#[test]
fn mid_stream_error_frame_is_surfaced_instead_of_reported_as_completed() {
    const SSE_WITH_ERROR: &str = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"前半句\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"type\":\"rate_limit_exceeded\",\"message\":\"too many requests\"}}\n\n",
        "data: [DONE]\n\n",
    );
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(SSE_WITH_ERROR),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());

    assert_eq!(status, 200, "响应头早已发出，只能用事件收尾");
    assert!(
        text.contains("event: error"),
        "必须给出明确的终止事件：\n{text}"
    );
    assert!(
        !text.contains("response.completed"),
        "不得把截断伪装成完整回复：\n{text}"
    );
    assert!(
        text.contains("rate_limit_exceeded"),
        "错误原因要可读：\n{text}"
    );
    // 上游地址里的 Key 不得出现在外发内容里。
    assert!(!text.contains(SECRET), "错误详情不得泄漏上游 Key");
}

/// 上游把连接干净关掉（对端 FIN），却**没有任何**终止标记：既没发 `[DONE]`，
/// 也没在 delta 里给过非 null 的 `finish_reason`。
///
/// 回归：读循环的 `Ok(0) => break` 不带失败标志，收尾既不进 `client_gone`
/// 也不进 `upstream_failed`，于是照走 `translator.finish()` —— 半句话被
/// 伪装成一次完整回复。
#[test]
fn truncated_chat_stream_without_terminal_marker_is_reported_as_error() {
    const SSE_TRUNCATED: &str = concat!(
        "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"前半句\"},\"finish_reason\":null}]}\n\n",
    );
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(SSE_TRUNCATED),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());

    assert_eq!(status, 200, "响应头早已发出，只能用事件收尾");
    assert!(
        text.contains("event: error"),
        "截断必须给出明确的终止事件：\n{text}"
    );
    assert!(
        !text.contains("response.completed"),
        "不得把截断伪装成完整回复：\n{text}"
    );
    assert!(
        text.contains("upstreamStreamIncomplete"),
        "错误要指明上游在给出完成标记前结束了连接：\n{text}"
    );
    assert!(
        text.contains("前半句"),
        "截断前已经收到的内容仍要交给宿主：\n{text}"
    );
    assert!(
        harness
            .gateway
            .diagnostics()
            .list(None)
            .iter()
            .any(|event| event.result_key == "result.upstreamStreamIncomplete"),
        "截断必须留痕"
    );
}

/// 上游只发 `finish_reason` 而不发 `[DONE]`：OpenAI 兼容协议里确实存在这种
/// 实现。只认 `[DONE]` 会把它们误判成截断——用一个假警报换一个假成功不划算。
#[test]
fn chat_stream_with_finish_reason_but_no_done_still_completes() {
    const SSE_FINISH_ONLY: &str = concat!(
        "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"你好\"},\"finish_reason\":\"stop\"}]}\n\n",
    );
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(SSE_FINISH_ONLY),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());

    assert_eq!(status, 200);
    assert!(
        text.contains("event: response.completed"),
        "上游给过 finish_reason 就不算截断：\n{text}"
    );
    assert!(
        text.contains("\"status\":\"completed\""),
        "完成事件必须是 completed 状态：\n{text}"
    );
    assert!(!text.contains("event: error"), "不得误报截断：\n{text}");
}

#[test]
fn responses_upstream_passes_through_and_hides_the_upstream_id() {
    let harness = Harness::start(
        RESPONSES_V1,
        MockReply::Sse(RESPONSES_SSE),
        Protocol::Responses,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());

    assert_eq!(status, 200);
    assert!(text.contains("event: response.created"));
    assert!(text.contains("event: response.completed"));
    assert!(text.contains(&harness.alias), "透传也要把身份改写回 alias");
    assert!(!text.contains("vendor/Upstream-Model"));

    let upstream_body = harness.upstream.last_body();
    assert_eq!(upstream_body["model"], "vendor/Upstream-Model");
    assert!(upstream_body["input"].is_array(), "透传必须保留 input 结构");
    assert!(upstream_body.get("messages").is_none());
}

#[test]
fn upstream_error_body_never_leaks_the_upstream_key() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Status {
            code: 502,
            body: format!("{{\"error\":{{\"message\":\"bad key {SECRET}\"}}}}"),
        },
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());

    assert_eq!(status, 500, "上游 5xx 映射为内部错误");
    assert!(
        !text.contains(SECRET),
        "上游错误正文里的 Key 必须被抹掉：{text}"
    );
    assert!(
        text.contains("redacted"),
        "应留下脱敏痕迹，便于排查：{text}"
    );
    assert!(
        text.contains("INTERNAL"),
        "5xx 归类为可重试的内部错误：{text}"
    );
    assert!(
        text.contains("error.upstreamFailed"),
        "必须保留“失败来自上游”这一区分：{text}"
    );
}

#[test]
fn upstream_permission_error_is_classified() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Status {
            code: 401,
            body: "{\"error\":{\"message\":\"no permission\"}}".to_owned(),
        },
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());

    assert_eq!(status, 403);
    assert!(text.contains("MODEL_PERMISSION_DENIED"), "{text}");
}

#[test]
fn models_endpoint_lists_only_the_published_revision() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let response = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
        .get(&harness.url("models"))
        .header("authorization", format!("Bearer {token}"))
        .call()
        .unwrap();
    let text = response.into_body().read_to_string().unwrap();

    assert!(text.contains(&harness.alias));
    assert_eq!(harness.upstream.requests(), 0, "列模型不应触达上游");
}

#[test]
fn health_requires_the_token_too() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let response = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
        .get(&format!("http://127.0.0.1:{}/health", harness.port))
        .call()
        .unwrap();
    assert_eq!(response.status().as_u16(), 401, "健康检查不开放匿名访问");
}

#[test]
fn realtime_path_reports_unsupported_instead_of_hanging() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("realtime", Some(&token), &json!({}));

    assert_eq!(status, 400);
    assert!(
        text.contains("CAPABILITY_UNSUPPORTED"),
        "必须显式告知不支持：{text}"
    );
}

#[test]
fn oversized_body_is_rejected_before_reading_it_all() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();
    let declared = MAX_REQUEST_BYTES + 1;

    let mut stream = TcpStream::connect(("127.0.0.1", harness.port)).unwrap();
    let head = format!(
        "POST /i/{INSTANCE}/c/{REVISION}/v1/responses HTTP/1.1\r\nhost: 127.0.0.1\r\nauthorization: Bearer {token}\r\ncontent-type: application/json\r\ncontent-length: {declared}\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);

    assert!(
        response.starts_with("HTTP/1.1 413"),
        "应返回 413：{response}"
    );
    assert!(response.contains("REQUEST_TOO_LARGE"));
}

#[test]
fn chunked_request_bodies_are_refused_with_a_clear_error() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let mut stream = TcpStream::connect(("127.0.0.1", harness.port)).unwrap();
    let head = format!(
        "POST /i/{INSTANCE}/c/{REVISION}/v1/responses HTTP/1.1\r\nhost: 127.0.0.1\r\nauthorization: Bearer {token}\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);

    assert!(
        response.starts_with("HTTP/1.1 400"),
        "应返回 400：{response}"
    );
    assert!(response.contains("Content-Length"));
}

#[test]
fn credential_version_change_blocks_the_request() {
    // 路由引用的凭据版本比存储里的更旧：相当于“目录发布后又换了 Key”。
    let harness = Harness::start_with(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
        TestPolicy {
            credential_version_offset: 1,
            ..TestPolicy::default()
        },
    );
    let token = harness.token.expose().to_owned();

    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());

    assert_eq!(status, 409);
    assert!(text.contains("CONTINUATION_BOUND"), "{text}");
    assert_eq!(harness.upstream.requests(), 0, "版本不符不得触达上游");
}

#[test]
fn binding_reports_the_actual_port_and_status_reflects_it() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let status = harness.gateway.status();

    assert!(status.running);
    assert_eq!(status.port, Some(harness.port));
    assert_eq!(status.instance_id, INSTANCE);
    assert_eq!(harness.upstream.requests(), 0, "网关启动本身不应发请求");
}

/// 网关必须把每次推理的结论写进诊断日志，否则界面无从定位失败。
#[test]
fn the_gateway_records_diagnostics_for_success_and_rejection() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();
    let log = harness.gateway.diagnostics();

    harness.post("responses", Some(&token), &harness.request_body());
    let (status, _) = harness.post("responses", None, &harness.request_body());
    assert_eq!(status, 401);

    let events = log.list(None);
    let results: Vec<&str> = events
        .iter()
        .map(|event| event.result_key.as_str())
        .collect();
    assert!(
        results.contains(&"result.upstreamAccepted"),
        "成功请求必须留痕：{results:?}"
    );
    assert!(
        results.contains(&"result.rejectedBeforeRouting"),
        "认证失败必须留痕：{results:?}"
    );
    assert!(
        events.iter().any(|event| event.level == LogLevel::Warning),
        "拒绝类事件应为警告级别"
    );

    // 日志里不得出现网关令牌或上游 Key。
    let rendered = serde_json::to_string(&events).unwrap();
    assert!(!rendered.contains(&token), "诊断事件不得包含网关令牌");
    assert!(!rendered.contains(SECRET), "诊断事件不得包含上游 Key");
}

/// GW-05：未声明图片输入的模型必须被显式拒绝，且不得触达上游。
#[test]
fn an_undeclared_modality_is_rejected_before_reaching_the_upstream() {
    let harness = Harness::start_with(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
        TestPolicy::declaring(&["text"]),
    );
    let token = harness.token.expose().to_owned();
    let mut body = harness.request_body();
    body["input"] = json!([{"type": "message", "role": "user", "content": [
        {"type": "input_text", "text": "看这张图"},
        {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}
    ]}]);

    let (status, text) = harness.post("responses", Some(&token), &body);

    assert_eq!(status, 400);
    assert!(text.contains("CAPABILITY_UNSUPPORTED"), "{text}");
    assert!(
        text.contains("未声明"),
        "错误详情要说明是未声明模态：{text}"
    );
    assert_eq!(harness.upstream.requests(), 0, "拒绝必须发生在上游调用之前");
    assert!(
        harness
            .gateway
            .diagnostics()
            .list(None)
            .iter()
            .any(|event| event.result_key == "result.modalityRejected"),
        "模态拒绝必须留痕"
    );
}

/// GW-05：声明了图片的模型可以正常收到图片输入。
#[test]
fn a_declared_modality_is_forwarded() {
    let harness = Harness::start_with(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
        TestPolicy::declaring(&["text", "image"]),
    );
    let token = harness.token.expose().to_owned();
    let mut body = harness.request_body();
    body["input"] = json!([{"type": "message", "role": "user", "content": [
        {"type": "input_text", "text": "看这张图"},
        {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}
    ]}]);

    let (status, _) = harness.post("responses", Some(&token), &body);

    assert_eq!(status, 200);
    assert_eq!(harness.upstream.requests(), 1);
    let upstream_body = harness.upstream.last_body();
    assert_eq!(
        upstream_body["messages"][1]["content"][1]["type"],
        "image_url"
    );
}

/// GW-05：模型声明的输出上限必须落到真实的上游请求参数上。
#[test]
fn the_declared_output_limit_reaches_the_upstream_request() {
    let harness = Harness::start_with(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
        TestPolicy {
            output_limit: Some(2_048),
            ..TestPolicy::default()
        },
    );
    let token = harness.token.expose().to_owned();
    let mut body = harness.request_body();
    body["max_output_tokens"] = json!(32_000);

    let (status, _) = harness.post("responses", Some(&token), &body);

    assert_eq!(status, 200);
    let upstream_body = harness.upstream.last_body();
    assert_eq!(
        upstream_body["max_tokens"], 2048,
        "宿主请求 32000，必须被模型声明的上限收口"
    );
}

/// GW-03：宿主中途断开时，网关必须停止读取上游，而不是把整个流跑完。
#[test]
fn a_client_disconnect_stops_the_upstream_stream() {
    // 大量且较大的分片：宿主的接收缓冲不可能全部吸收，写入必然失败。
    let filler = "x".repeat(1_024);
    let mut sse = String::new();
    for _ in 0..200 {
        sse.push_str(&format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{filler}\"}}}}]}}\n\n"
        ));
    }
    sse.push_str("data: [DONE]\n\n");
    let leaked: &'static str = Box::leak(sse.into_boxed_str());

    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(leaked),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let mut stream = TcpStream::connect(("127.0.0.1", harness.port)).unwrap();
    let body = harness.request_body().to_string();
    let head = format!(
        "POST /i/{INSTANCE}/c/{REVISION}/v1/responses HTTP/1.1\r\nhost: 127.0.0.1\r\nauthorization: Bearer {token}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    // 只读一点点就断开，模拟宿主取消正在生成的任务。
    let mut head_buffer = [0u8; 256];
    let _ = stream.read(&mut head_buffer);
    let _ = stream.shutdown(std::net::Shutdown::Both);
    drop(stream);

    // 异步收尾：等网关在下一次写时发现对方已经离开。
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut observed = false;
    while std::time::Instant::now() < deadline {
        if harness
            .gateway
            .diagnostics()
            .list(None)
            .iter()
            .any(|event| event.result_key == "result.clientDisconnected")
        {
            observed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        observed,
        "宿主断开必须被识别为取消并留痕，实际事件：{:?}",
        harness
            .gateway
            .diagnostics()
            .list(None)
            .iter()
            .map(|event| event.result_key.clone())
            .collect::<Vec<_>>()
    );
}

/// 暂停只拦新请求：在途请求不受影响，新请求得到明确的 503 而不是等待超时。
#[test]
fn pausing_the_gateway_rejects_new_requests_without_affecting_in_flight_ones() {
    let harness = Harness::start(
        CHAT_COMPLETIONS_V1,
        MockReply::Sse(CHAT_SSE),
        Protocol::ChatCompletions,
    );
    let token = harness.token.expose().to_owned();

    let (status, _) = harness.post("responses", Some(&token), &harness.request_body());
    assert_eq!(status, 200, "暂停前应正常");
    assert_eq!(harness.upstream.requests(), 1);

    harness.gateway.set_paused(true);
    assert!(harness.gateway.status().paused);
    let (status, text) = harness.post("responses", Some(&token), &harness.request_body());
    assert_eq!(status, 500);
    assert!(
        text.contains("error.gatewayPaused"),
        "必须给出暂停原因：{text}"
    );
    assert_eq!(harness.upstream.requests(), 1, "暂停时不得触达上游");

    harness.gateway.set_paused(false);
    let (status, _) = harness.post("responses", Some(&token), &harness.request_body());
    assert_eq!(status, 200, "恢复后应可继续");
}

/// 判定阶段的失败**必须**给一个可读的 HTTP 错误。
///
/// 回归：这些分支过去用 `?` 把错误冒到接受循环，而那里是 `let _ = handle(...)`，
/// 于是连接被直接关掉——宿主看到的是 `Empty reply`，既没有状态码也没有原因，
/// 用户只能看到界面「没反应」。项目自己在风险文档里写死了「失败必须可判定」，
/// 这条用例就是那句话的可执行版本。
mod every_failure_is_answerable {
    use super::*;
    use switch_core::domain::provider::Provider;

    #[test]
    fn models_with_an_unpublished_revision_answers_with_a_readable_error() {
        let harness = Harness::start(
            CHAT_COMPLETIONS_V1,
            MockReply::Sse(CHAT_SSE),
            Protocol::ChatCompletions,
        );
        let token = harness.token.expose().to_owned();
        let url = format!(
            "http://127.0.0.1:{}/i/{INSTANCE}/c/rev_missing/v1/models",
            harness.port
        );

        let (status, body) = Harness::get_path(&url, Some(&token));
        assert_eq!(status, 404, "未发布的目录版本不回落、也不该静默断开");
        let payload: Value = serde_json::from_str(&body).expect("错误必须是 JSON 正文");
        assert_eq!(payload["error"]["code"], "ROUTE_MISMATCH");
        assert!(
            payload["error"]["details"][0]
                .as_str()
                .unwrap_or_default()
                .contains("rev_missing"),
            "错误正文要说清是哪个版本，否则用户无从下手：{body}"
        );
    }

    /// 请求进行中必须**真的**持有引用。
    ///
    /// 上一条用例只断言「结束后归零」——`retain` 从来没被调用时它同样成立（缺条目时
    /// `release` 是空操作）。所以还需要这条：在请求还没结束时，引用数必须大于 0，
    /// 否则回收策略会在别人正用着这个版本时把它删掉。
    #[test]
    fn an_in_flight_request_holds_a_reference_to_its_revision() {
        let harness = Harness::start(
            CHAT_COMPLETIONS_V1,
            MockReply::SseDelayed(CHAT_SSE, Duration::from_millis(700)),
            Protocol::ChatCompletions,
        );
        let token = harness.token.expose().to_owned();
        let body = harness.request_body();

        std::thread::scope(|scope| {
            let request = scope.spawn(|| harness.post("responses", Some(&token), &body));
            std::thread::sleep(Duration::from_millis(250));
            let during = harness.router.refs(REVISION).total();
            let (status, _) = request.join().unwrap();
            assert_eq!(status, 200);
            assert!(during > 0, "请求进行中必须持有引用，实际为 {during}");
            assert_eq!(
                refs_after_drain(&harness.router, REVISION),
                0,
                "请求结束后引用必须归还"
            );
        });
    }

    /// 引用计数必须配对：请求期间 >0，请求结束回到 0。
    ///
    /// 这条保证的是回收策略能工作。`retain` 过去根本没被调用过，于是「有没有人正在用
    /// 这个版本」永远是未知的——回收就只能在「留得太多」和「敢不敢删」之间瞎猜。
    /// 配对释放靠 `Drop`，所以**每一条提前返回的分支**都必须走到它。
    #[test]
    fn a_finished_request_releases_the_revision_it_referenced() {
        let harness = Harness::start(
            CHAT_COMPLETIONS_V1,
            MockReply::Sse(CHAT_SSE),
            Protocol::ChatCompletions,
        );
        let token = harness.token.expose().to_owned();

        // 正常完成的一条。
        let (status, _) = harness.post("responses", Some(&token), &harness.request_body());
        assert_eq!(status, 200);
        assert_eq!(
            refs_after_drain(&harness.router, REVISION),
            0,
            "请求结束后引用必须归零，否则这个版本永远无法回收"
        );

        // 提前返回的一条：缺 model 走的是「判定阶段失败」的出口。
        let (status, _) = harness.post("responses", Some(&token), &json!({"input": []}));
        assert_eq!(status, 400);
        assert_eq!(
            refs_after_drain(&harness.router, REVISION),
            0,
            "被拒绝的请求同样要释放引用"
        );

        // 上游报错的一条：错误发生在已经开流之后。
        let failing = Harness::start(
            CHAT_COMPLETIONS_V1,
            MockReply::Status {
                code: 403,
                body: "{\"error\":\"forbidden\"}".to_owned(),
            },
            Protocol::ChatCompletions,
        );
        let failing_token = failing.token.expose().to_owned();
        let (status, _) = failing.post("responses", Some(&failing_token), &failing.request_body());
        assert_eq!(status, 403);
        assert_eq!(
            refs_after_drain(&failing.router, REVISION),
            0,
            "上游失败的分支也要释放引用"
        );
    }

    #[test]
    fn models_refuses_an_instance_that_does_not_own_the_revision() {
        // /v1/models 过去只查版本在不在、不查实例归属，于是换个前缀就能列出别人的模型。
        let harness = Harness::start(
            CHAT_COMPLETIONS_V1,
            MockReply::Sse(CHAT_SSE),
            Protocol::ChatCompletions,
        );
        let token = harness.token.expose().to_owned();
        let url = format!(
            "http://127.0.0.1:{}/i/inst_somebody_else/c/{REVISION}/v1/models",
            harness.port
        );

        let (status, body) = Harness::get_path(&url, Some(&token));
        assert_ne!(status, 200, "展示接口不该比推理接口更松");
        assert_eq!(status, 401);
        let payload: Value = serde_json::from_str(&body).expect("错误必须是 JSON 正文");
        assert_eq!(payload["error"]["code"], "UNAUTHORIZED");
    }

    #[test]
    fn a_request_without_a_model_answers_instead_of_hanging_up() {
        let harness = Harness::start(
            CHAT_COMPLETIONS_V1,
            MockReply::Sse(CHAT_SSE),
            Protocol::ChatCompletions,
        );
        let token = harness.token.expose().to_owned();
        // 有合法 alias 之外的形状：完全没有 model 字段。
        let (status, body) = harness.post("responses", Some(&token), &json!({"input": []}));

        assert_eq!(status, 400);
        let payload: Value = serde_json::from_str(&body).expect("错误必须是 JSON 正文");
        assert_eq!(payload["error"]["code"], "VALIDATION_FAILED");
        assert!(
            payload["error"]["details"][0]
                .as_str()
                .unwrap_or_default()
                .contains("alias"),
            "要说清这里是按 alias 路由：{body}"
        );
    }

    #[test]
    fn a_deleted_provider_is_reported_as_an_error_not_a_dropped_connection() {
        // 路由快照在发布时冻结，而供应商可以在那之后被删掉。这是真实竞态，
        // 过去它会静默断连。
        let harness = Harness::start(
            CHAT_COMPLETIONS_V1,
            MockReply::Sse(CHAT_SSE),
            Protocol::ChatCompletions,
        );
        let token = harness.token.expose().to_owned();
        // 按外键顺序拆掉：模型与 Key 引用供应商，所以先删它们。路由快照是内存里的，
        // 不会被这轮删除碰到——这正是真实竞态的形状：快照冻结在发布那一刻。
        for model in harness.repository.list_models().unwrap() {
            harness
                .repository
                .delete_model(&model.id)
                .expect("删除模型");
        }
        for provider in harness.repository.list_providers().unwrap() {
            // 当前 Key 受保护（删它就等于悄悄换路由），先解除选中状态。
            let cleared = Provider {
                active_credential_id: None,
                ..provider.clone()
            };
            harness
                .repository
                .save_provider(cleared, provider.version)
                .expect("解除当前 Key");
            for credential in harness.repository.list_credentials(&provider.id).unwrap() {
                harness
                    .repository
                    .delete_credential(&credential.id)
                    .expect("删除 Key");
            }
            harness
                .repository
                .delete_provider(&provider.id)
                .expect("删除供应商");
        }

        let (status, body) = harness.post("responses", Some(&token), &harness.request_body());
        assert_eq!(status, 404);
        let payload: Value = serde_json::from_str(&body).expect("错误必须是 JSON 正文");
        assert_eq!(payload["error"]["code"], "NOT_FOUND");
        assert_eq!(harness.upstream.requests(), 0, "不该触达上游");
    }
}
