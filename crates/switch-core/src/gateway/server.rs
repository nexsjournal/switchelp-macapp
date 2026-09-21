//! 本机网关服务：受认证的 loopback 推理入口。
//!
//! 请求路径形如 `/i/{instanceId}/c/{catalogRevision}/v1/responses`：前缀决定目录版本，
//! 请求本身不能覆盖。首个字节之前就完成的判定（方法、来源、令牌、体积）来自
//! [`RequestGuard`]；路由来自已发布的不可变 [`GatewayRouter`] 快照。
//!
//! 并发模型：每连接一个线程。本机单用户工具不需要异步运行时的复杂度，
//! 而阻塞读取让上游流式转发与超时判定都保持直白。所有连接都只绑 loopback。
//!
//! 超时分三层：连接、首事件由 `TimeoutPolicy` 直接映射到 HTTP 客户端；
//! 流空闲在读取循环里按两次读之间的间隔判定（下一次读返回后才生效），
//! 上游挂死时的最终兜底是宿主自己的 `stream_idle_timeout_ms`。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::auth::{GatewayToken, InboundHeaders, RequestGuard, MAX_REQUEST_BYTES};
use super::routing::{AdmissionError, GatewayRouter};
use super::sse::SseParser;
use super::timeouts::TimeoutPolicy;
use crate::credentials::{CredentialResolver, SecretVault};
use crate::diagnostics::{DiagnosticEvent, DiagnosticLog, LogLevel};
use crate::domain::error::{CoreError, ErrorCode};
use crate::domain::ids::InstanceId;
use crate::protocols::{chat, responses, ResponsesEvent, CHAT_COMPLETIONS_V1};
use crate::storage::Repository;

/// 请求头区上限：单行 8 KiB，最多 64 行。
const MAX_HEADER_LINE: usize = 8 * 1024;
const MAX_HEADER_LINES: usize = 64;
/// 转发上游错误正文的上限：足够定位，又不至于把整个响应体搬给宿主。
const MAX_UPSTREAM_ERROR_CHARS: usize = 2_000;

/// 网关装配参数。
pub struct GatewayConfig {
    /// 本机身份标识，用于认证前的来源声明校验与状态展示。
    ///
    /// 推理请求的实际归属由 URL 前缀里的实例决定（见 `handle_inference`），
    /// 所以一个进程可以服务本工具发布过的多个实例前缀。
    pub instance_id: InstanceId,
    pub token: GatewayToken,
    pub port: u16,
    pub timeouts: TimeoutPolicy,
    /// 诊断日志。网关把每次推理的结论写进去，界面据此定位失败。
    pub diagnostics: Arc<DiagnosticLog>,
}

/// 本机网关。`bind` 之后用 `local_addr` 读取真实端口（测试用 0 端口）。
pub struct Gateway {
    config: GatewayConfig,
    repository: Arc<dyn Repository>,
    vault: Arc<dyn SecretVault>,
    router: Arc<GatewayRouter>,
    agent: ureq::Agent,
    listener: Mutex<Option<TcpListener>>,
    local_addr: Mutex<Option<SocketAddr>>,
    running: AtomicBool,
    served: AtomicU64,
    /// 暂停新推理请求。用于“先停新的，再换 Key”，不影响在途请求。
    paused: AtomicBool,
}

impl Gateway {
    pub fn new(
        repository: Arc<dyn Repository>,
        vault: Arc<dyn SecretVault>,
        router: Arc<GatewayRouter>,
        config: GatewayConfig,
    ) -> Self {
        // 连接与首事件超时来自策略；正文读取不设总时限，否则长回复会被截断
        // （ureq 的 recv_body 是总预算，不是每次读的空闲预算）。
        // 正文预算只在策略显式设置时启用：ureq 的 `recv_body` 是**总预算**而非每次读的
        // 空闲预算，默认打开会截断长回复。空闲超时因此只有粗粒度保护，见模块说明。
        let body_budget = match config.timeouts.total_ms {
            0 => None,
            total => Some(Duration::from_millis(total)),
        };
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_millis(config.timeouts.connect_ms)))
                .timeout_recv_response(Some(Duration::from_millis(config.timeouts.first_event_ms)))
                .timeout_recv_body(body_budget)
                .proxy(ureq::Proxy::try_from_env())
                .build(),
        );
        Self {
            config,
            repository,
            vault,
            router,
            agent,
            listener: Mutex::new(None),
            local_addr: Mutex::new(None),
            running: AtomicBool::new(false),
            served: AtomicU64::new(0),
            paused: AtomicBool::new(false),
        }
    }

    /// 仅绑定 loopback。端口被占用时返回 `PortInUse`，由装配层决定改端口而不是静默换。
    pub fn bind(&self) -> Result<u16, CoreError> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.config.port));
        let listener = TcpListener::bind(address).map_err(|error| {
            // 最可能的成因就是另一个 Switchelp 已经占着端口（一个进程一份网关）。
            // 这句提示会一路显示到界面上，所以直接说出来，别让用户去猜。
            CoreError::new(ErrorCode::PortInUse, "error.portInUse").with_detail(format!(
                "无法绑定 {address}：{error}。端口被占用最常见的原因是已经开着另一个 Switchelp 实例；\
                 也可能被别的程序占用——本工具不会自动换端口，因为 Codex 配置里写的就是这个地址。"
            ))
        })?;
        let local = listener
            .local_addr()
            .map_err(|_| CoreError::internal("无法读取网关端口"))?;
        *self
            .listener
            .lock()
            .map_err(|_| CoreError::internal("监听锁不可用"))? = Some(listener);
        *self
            .local_addr
            .lock()
            .map_err(|_| CoreError::internal("地址锁不可用"))? = Some(local);
        Ok(local.port())
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr.lock().ok().and_then(|value| *value)
    }

    pub fn served_requests(&self) -> u64 {
        self.served.load(Ordering::Relaxed)
    }

    /// 诊断日志。界面读它来定位失败。
    pub fn diagnostics(&self) -> Arc<DiagnosticLog> {
        self.config.diagnostics.clone()
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.config.instance_id
    }

    /// 在后台线程接受连接，直到进程退出或调用 `shutdown`。
    pub fn spawn(self: &Arc<Self>) -> Result<(), CoreError> {
        let listener = self
            .listener
            .lock()
            .map_err(|_| CoreError::internal("监听锁不可用"))?
            .take()
            .ok_or_else(|| CoreError::internal("网关尚未绑定端口"))?;
        self.running.store(true, Ordering::Relaxed);
        let gateway = self.clone();
        std::thread::Builder::new()
            .name("gptswitch-gateway".to_owned())
            .spawn(move || {
                for incoming in listener.incoming() {
                    if !gateway.running.load(Ordering::Relaxed) {
                        break;
                    }
                    match incoming {
                        Ok(stream) => {
                            let gateway = gateway.clone();
                            // 单连接失败不能拖垮监听循环。
                            let _ = std::thread::Builder::new()
                                .name("gptswitch-conn".to_owned())
                                .spawn(move || {
                                    let _ = gateway.handle(stream);
                                });
                        }
                        Err(_) => continue,
                    }
                }
            })
            .map_err(|_| CoreError::internal("无法启动网关线程"))?;
        Ok(())
    }

    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// 暂停 / 继续接受**新**推理请求。
    ///
    /// 暂停只影响新请求：在途请求继续跑完，符合“不默认中断正在生成的任务”。
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// 处理一个连接。错误在这里被翻译成 HTTP 响应，绝不把 `CoreError` 细节直接吐出。
    ///
    /// **不变量：这条连接要么收到一个 HTTP 响应，要么在写出第一个字节之后收到流内的 error 事件。**
    /// 两者都不会的第三种情况——判定阶段失败、却直接关掉连接——过去真实存在：若分支里用了 `?`，
    /// 错误会从 `handle` 冒到接受循环，而那里是 `let _ = handle(...)`，于是宿主只看到
    /// `Empty reply`，既没有状态码也没有原因。下面 `outcome` 之后的兜底就是为它准备的。
    fn handle(&self, stream: TcpStream) -> Result<(), CoreError> {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_nodelay(true);
        let mut writer = stream
            .try_clone()
            .map_err(|_| CoreError::internal("复制连接失败"))?;
        let mut reader = BufReader::new(stream);

        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            // 客户端只是断开，不记为失败。
            Ok(None) => return Ok(()),
            Err(error) => {
                self.served.fetch_add(1, Ordering::Relaxed);
                return write_error(&mut writer, &error);
            }
        };
        self.served.fetch_add(1, Ordering::Relaxed);

        let guard = RequestGuard::new(self.config.token.clone(), self.config.instance_id.clone());
        if let Err(error) = guard.check(&request.headers, request.body.len()) {
            self.config.diagnostics.record(
                DiagnosticEvent::new(
                    crate::diagnostics::now_rfc3339(),
                    LogLevel::Warning,
                    "gateway",
                    request.path.clone(),
                    "result.rejectedBeforeRouting",
                )
                .with_metadata("error_code", format!("{:?}", error.code)),
            );
            return write_error(&mut writer, &error);
        }

        let mut response = Response::new(&mut writer);
        let outcome = match (request.method.as_str(), route_kind(&request.path)) {
            ("GET", RouteKind::Health) => response.write_json(
                200,
                &json!({"status": "ok", "served": self.served_requests()}),
            ),
            ("GET", RouteKind::Models { revision }) => {
                self.handle_models(&mut response, &request.path, &revision)
            }
            ("POST", RouteKind::Responses) => self.handle_inference(&mut response, &request),
            ("POST", RouteKind::Realtime) => response.write_error(
                &CoreError::new(
                    ErrorCode::CapabilityUnsupported,
                    "error.realtimeUnsupported",
                )
                .with_detail("本机网关不支持实时语音通道；请使用普通对话。".to_owned()),
            ),
            (_, RouteKind::Unknown) => response.write_error(&CoreError::not_found("网关接口")),
            _ => response.write_error(&CoreError::validation("该路径不支持此方法")),
        };

        if let Err(error) = &outcome {
            // 一个字节都还没写出去，说明失败发生在判定阶段：补一个可读错误再关。
            // 已经开流的失败由 `pipe_stream` 自己用流内 error 事件收尾，那里不能再写 HTTP 响应头，
            // 所以这一步必须看 `started`，不能无条件补。
            if !response.started() {
                self.config.diagnostics.record(
                    DiagnosticEvent::new(
                        crate::diagnostics::now_rfc3339(),
                        LogLevel::Error,
                        "gateway",
                        request.path.clone(),
                        "result.unansweredRequest",
                    )
                    .with_metadata("error_code", format!("{:?}", error.code)),
                );
                let _ = response.write_error(error);
            }
        }
        outcome
    }

    /// 当前目录版本里可用的 alias。宿主用它做诊断，不用于路由决策。
    ///
    /// 实例归属按 `admission` 同样的标准校验：这个接口显示的是「谁能被调用」，
    /// 不该比推理接口更松，否则一个错误的前缀就能列出别人的模型。
    fn handle_models(
        &self,
        response: &mut Response,
        path: &str,
        revision: &str,
    ) -> Result<(), CoreError> {
        let instance = crate::storage::snapshot::RuntimePublication::parse_prefix(path)
            .map(|(instance, _)| instance)
            .ok_or_else(|| {
                CoreError::new(ErrorCode::RouteMismatch, "error.unknownCatalogRevision")
                    .with_detail(format!("无法从路径解析出实例与目录版本：{path}"))
            })?;
        let aliases = match self
            .router
            .aliases_checked(revision, &InstanceId::new(instance))
        {
            Ok(aliases) => aliases,
            Err(error) => {
                let error = AdmissionError::to_core_error(&error);
                self.config.diagnostics.record(
                    DiagnosticEvent::new(
                        crate::diagnostics::now_rfc3339(),
                        LogLevel::Warning,
                        "gateway",
                        path.to_owned(),
                        "result.routeRejected",
                    )
                    .with_metadata("revision_id", revision.to_owned())
                    .with_metadata("error_code", format!("{:?}", error.code)),
                );
                return response.write_error(&error);
            }
        };
        let data: Vec<Value> = aliases
            .iter()
            .map(|alias| json!({"id": alias, "object": "model", "owned_by": "gptswitch"}))
            .collect();
        response.write_json(200, &json!({"object": "list", "data": data}))
    }

    /// 推理主路径：准入 → 取凭据 → 转换为上游请求 → 流式还原 → 回写宿主。
    fn handle_inference(
        &self,
        response: &mut Response,
        request: &IncomingRequest,
    ) -> Result<(), CoreError> {
        let payload: Value = match serde_json::from_slice(&request.body) {
            Ok(value) => value,
            Err(_) => return response.write_error(&CoreError::validation("请求体不是合法 JSON")),
        };
        let alias = match payload.get("model").and_then(Value::as_str) {
            Some(alias) => alias,
            None => {
                return response.write_error(&CoreError::validation(
                    "请求缺少 model；本机网关按 alias 路由，不接受空模型",
                ))
            }
        };

        // 实例取自 URL 前缀，而不是启动时写死的实例：一个网关可以服务本工具发布过的
        // 任意实例前缀，而前缀本身确定目录版本。`admission` 仍会校验该目录修订
        // 确实属于前缀里声明的实例，任一侧对不上都拒绝。
        let admission =
            match crate::storage::snapshot::RuntimePublication::parse_prefix(&request.path)
                .ok_or_else(|| AdmissionError::UnknownPrefix {
                    catalog_revision: request.path.clone(),
                })
                .and_then(|(instance, revision)| {
                    self.router
                        .admission(&revision, alias, &InstanceId::new(instance))
                }) {
                Ok(admission) => admission,
                Err(error) => {
                    let error = AdmissionError::to_core_error(&error);
                    self.config.diagnostics.record(
                        DiagnosticEvent::new(
                            crate::diagnostics::now_rfc3339(),
                            LogLevel::Warning,
                            "gateway",
                            alias.to_owned(),
                            "result.routeRejected",
                        )
                        .with_metadata("alias", alias)
                        .with_metadata("error_code", format!("{:?}", error.code)),
                    );
                    return response.write_error(&error);
                }
            };
        let route = admission.route;
        // 从这一刻起这个目录版本被引用。没有这一步，旧版本在任何时候都「可回收」，
        // 而回收策略就只能在「留得太多」和「敢不敢删」之间瞎猜。
        // 配对释放放在下面 `InferenceGuard` 的 Drop 里：本函数有十几条提前 return，
        // 靠人肉在每条出口上写 release 迟早会漏。
        self.router.retain(&route.catalog_revision, 0);
        let _held = InferenceGuard {
            router: self.router.clone(),
            revision: route.catalog_revision.clone(),
        };

        let provider = self
            .repository
            .get_provider(&route.provider_id)?
            .ok_or_else(|| {
                CoreError::not_found("供应商").with_detail("该路由引用的供应商已被删除".to_owned())
            })?;
        let credential = self
            .repository
            .get_credential(&route.credential_id)?
            .ok_or_else(|| {
                CoreError::new(ErrorCode::CredentialMissing, "error.credentialMissing")
                    .with_detail("该路由引用的 Key 不存在".to_owned())
            })?;
        // 目录发布后 Key 被替换：拒绝而不是静默换成新 Key。
        // 续接与在途请求必须绑定发布时的凭据版本。
        if credential.secret_version != route.credential_version {
            return response.write_error(
                &CoreError::new(
                    ErrorCode::ContinuationBound,
                    "error.credentialVersionChanged",
                )
                .with_detail("该模型发布后 Key 已更换，请重新生成应用计划".to_owned()),
            );
        }
        let resolver = CredentialResolver::new(self.vault.as_ref());
        let secret = match resolver.resolve(&credential) {
            Ok(secret) => secret,
            Err(error) => return response.write_error(&error),
        };

        if self.is_paused() {
            let error = CoreError::new(ErrorCode::Internal, "error.gatewayPaused")
                .with_detail("本机网关已暂停接受新请求；在途请求不受影响。".to_owned());
            return response.write_error(&error);
        }

        // 模态在执行前判定：把图片塞给只声明文本的模型属于模态虚报，
        // 也等于偷偷借用另一个模型的能力，必须显式拒绝而不是转发。
        let unsupported =
            crate::protocols::unsupported_modalities(&payload, &route.native_modalities);
        if !unsupported.is_empty() {
            let error = CoreError::new(
                ErrorCode::CapabilityUnsupported,
                "error.modalityNotDeclared",
            )
            .with_detail(format!(
                "该模型未声明 {} 输入，本次请求包含这类内容，已拒绝转发",
                unsupported.join("、")
            ));
            self.config.diagnostics.record(
                DiagnosticEvent::new(
                    crate::diagnostics::now_rfc3339(),
                    LogLevel::Warning,
                    "gateway",
                    alias.to_owned(),
                    "result.modalityRejected",
                )
                .with_metadata("alias", alias)
                .with_metadata("model_id", route.model_id.as_str())
                .with_metadata("error_code", unsupported.join("+")),
            );
            return response.write_error(&error);
        }

        let limits = route.limits();
        let prepared = match route.protocol_id.as_str() {
            CHAT_COMPLETIONS_V1 => {
                chat::prepare(&provider.endpoint, &route.upstream_id, &payload, &limits)
            }
            _ => responses::prepare(&provider.endpoint, &route.upstream_id, &payload, &limits),
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return response.write_error(&error),
        };
        if !prepared.losses.is_empty() {
            // 转换损失必须可见：宿主以为生效的参数如果被丢弃，用户只能从这里看出来。
            self.config.diagnostics.record(
                DiagnosticEvent::new(
                    crate::diagnostics::now_rfc3339(),
                    LogLevel::Warning,
                    "gateway",
                    alias.to_owned(),
                    "result.adaptationLoss",
                )
                .with_metadata("alias", alias)
                .with_metadata("protocol_id", route.protocol_id.clone())
                .with_metadata(
                    "error_code",
                    prepared
                        .losses
                        .iter()
                        .map(|loss| loss.feature.as_str())
                        .collect::<Vec<_>>()
                        .join("+"),
                ),
            );
        }

        let mut call = self
            .agent
            .post(&prepared.url)
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", secret.expose()));
        for (name, value) in &prepared.headers {
            if name.eq_ignore_ascii_case("content-type") {
                call = call.header(name, value);
            }
        }
        let upstream = match call.send(prepared.body.as_slice()) {
            Ok(response) => response,
            Err(error) => {
                return response.write_error(&upstream_transport_error(&error));
            }
        };

        let status = upstream.status().as_u16();
        if !(200..300).contains(&status) {
            let mut upstream = upstream;
            let text = upstream
                .body_mut()
                .with_config()
                .limit((MAX_UPSTREAM_ERROR_CHARS * 4) as u64)
                .read_to_string()
                .unwrap_or_default();
            let detail = redact(&text, secret.expose());
            let error = upstream_status_error(status, &detail);
            self.config.diagnostics.record(
                DiagnosticEvent::new(
                    crate::diagnostics::now_rfc3339(),
                    LogLevel::Error,
                    "gateway",
                    alias.to_owned(),
                    "result.upstreamFailed",
                )
                .with_metadata("alias", alias)
                .with_metadata("provider_id", route.provider_id.as_str())
                .with_metadata("model_id", route.model_id.as_str())
                .with_metadata("http_status", status.to_string())
                .with_metadata("error_code", format!("{:?}", error.code)),
            );
            return response.write_error(&error);
        }

        self.config.diagnostics.record(
            DiagnosticEvent::new(
                crate::diagnostics::now_rfc3339(),
                LogLevel::Info,
                "gateway",
                alias.to_owned(),
                "result.upstreamAccepted",
            )
            .with_metadata("alias", alias)
            .with_metadata("provider_id", route.provider_id.as_str())
            .with_metadata("model_id", route.model_id.as_str())
            .with_metadata("protocol_id", route.protocol_id.clone())
            .with_metadata("http_status", status.to_string())
            .with_metadata("revision_id", route.catalog_revision.clone())
            .with_metadata("credential_version", route.credential_version.to_string())
            .with_metadata("app_version", env!("CARGO_PKG_VERSION")),
        );

        let translator = if route.protocol_id == CHAT_COMPLETIONS_V1 {
            Translator::Chat {
                state: chat::ChatStream::new(alias),
            }
        } else {
            Translator::Passthrough {
                alias: alias.to_owned(),
            }
        };
        // 从这里开始是原始字节：守卫记为「已开始」，之后不可能再改成 HTTP 错误响应。
        self.pipe_stream(
            response.stream_mut(),
            upstream.into_body(),
            translator,
            secret.expose(),
        )
    }

    /// 上游 SSE → 宿主 SSE。逐事件转发并立即 flush，保证宿主能增量看到输出。
    ///
    /// 拿到的必须是**已经开过流**的连接：本函数第一件事就是写响应头，
    /// 之后所有失败都只能靠流内的 error 事件表达。
    fn pipe_stream(
        &self,
        writer: &mut TcpStream,
        body: ureq::Body,
        mut translator: Translator,
        secret: &str,
    ) -> Result<(), CoreError> {
        write_head(writer, 200, "OK", "text/event-stream")?;
        let mut chunked = Chunked::new(writer);
        let mut parser = SseParser::new();
        let mut reader = body.into_reader();
        let mut buffer = vec![0u8; 16 * 1024];
        let idle = Duration::from_millis(self.config.timeouts.idle_ms);
        let mut last_frame = Instant::now();
        let mut saw_done = false;

        // 宿主断开就是取消信号：写不出去立即停止读取上游。`Body` 随函数返回被释放，
        // 上游连接随之关闭，不再继续消耗供应商额度。检测延迟取决于下一次写——
        // 流式响应每来一个上游事件就写一次，因此延迟有界。
        let mut client_gone = false;
        // 已经用 error 事件收尾：之后不能再补完成事件，否则截断会看起来像完整回复。
        let mut upstream_failed = false;

        if let Some(events) = translator.start() {
            for event in events {
                if chunked.write_frame(event.name, &event.payload).is_err() {
                    client_gone = true;
                }
            }
        }

        'stream: while !client_gone {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) => {
                    // 上游在流中途出错：给宿主一个明确的终止事件，而不是静默断流。
                    let error = stream_read_error(&error);
                    let _ = chunked.write_frame("error", &error_payload(&error));
                    client_gone = chunked.failed();
                    upstream_failed = true;
                    break;
                }
            };
            if last_frame.elapsed() > idle {
                let error = CoreError::internal("上游流空闲超时");
                let _ = chunked.write_frame("error", &error_payload(&error));
                client_gone = chunked.failed();
                upstream_failed = true;
                break;
            }
            last_frame = Instant::now();
            for event in parser.feed(&buffer[..read]) {
                if event.data.trim() == "[DONE]" {
                    saw_done = true;
                    break;
                }
                let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
                    continue;
                };
                // 上游会把错误也当成流里的数据帧发出来（限流、内容过滤、内部错误）。
                // 翻译器只认 choices/delta，这种帧过去被当成「没有内容」而继续，
                // 最后照发 response.completed —— 半句话被伪装成完整回复。
                if let Some(payload) = data.get("error").filter(|value| !value.is_null()) {
                    let error = upstream_stream_error(payload, secret);
                    let _ = chunked.write_frame("error", &error_payload(&error));
                    client_gone = chunked.failed();
                    upstream_failed = true;
                    break 'stream;
                }
                for frame in translator.feed(&data) {
                    if chunked.write_frame(&frame.name, &frame.payload).is_err() {
                        client_gone = true;
                        break 'stream;
                    }
                }
            }
            if saw_done {
                break;
            }
        }

        if client_gone {
            // 连接已经没了：不写结束块，也没有可交付的完整响应。这不是网关失败。
            self.config.diagnostics.record(
                DiagnosticEvent::new(
                    crate::diagnostics::now_rfc3339(),
                    LogLevel::Info,
                    "gateway",
                    "stream".to_owned(),
                    "result.clientDisconnected",
                )
                .with_metadata("error_code", "CLIENT_GONE"),
            );
            return Ok(());
        }

        if upstream_failed {
            // 终止事件已经发出（上游报错、读到一半断开、空闲超时）。这里只把分块编码收尾，
            // 不补 response.completed：那会把「半句话」伪装成一次完整回复。
            return chunked.finish();
        }

        // 上游干净断开（对端 FIN）却没给任何终止标记：既没有 `[DONE]`，也没有非 null
        // 的 `finish_reason`。判据取两者之一而不是只认 `[DONE]`——只发 `finish_reason`
        // 的兼容实现确实存在，只认 `[DONE]` 会把它们误判成截断，那是拿假警报换假成功。
        if !saw_done && !translator.saw_terminal() {
            let error = CoreError::new(ErrorCode::Internal, "error.upstreamStreamIncomplete")
                .with_detail("上游在给出完成标记前结束了连接，这次回复不完整".to_owned());
            let _ = chunked.write_frame("error", &error_payload(&error));
            self.config.diagnostics.record(
                DiagnosticEvent::new(
                    crate::diagnostics::now_rfc3339(),
                    LogLevel::Warning,
                    "gateway",
                    "stream".to_owned(),
                    "result.upstreamStreamIncomplete",
                )
                .with_metadata("error_code", "UPSTREAM_STREAM_INCOMPLETE")
                .with_metadata("pending_bytes", parser.pending_bytes().to_string()),
            );
            return chunked.finish();
        }

        for event in parser.finish().into_iter() {
            if let Ok(data) = serde_json::from_str::<Value>(&event.data) {
                for frame in translator.feed(&data) {
                    let _ = chunked.write_frame(&frame.name, &frame.payload);
                }
            }
        }
        for frame in translator.finish() {
            let _ = chunked.write_frame(&frame.name, &frame.payload);
        }
        chunked.finish()
    }
}

/// 路径类别。前缀解析放在这里，避免在每个分支里各写一遍。
#[derive(Debug, PartialEq, Eq)]
enum RouteKind {
    Health,
    Models { revision: String },
    Responses,
    Realtime,
    Unknown,
}

fn route_kind(path: &str) -> RouteKind {
    // 查询串与片段不属于路径：`/health?x=1` 与 `/health` 是同一路由。
    let path = path.split(['?', '#']).next().unwrap_or(path);
    if path == "/health" {
        return RouteKind::Health;
    }
    let Some((_, revision)) = crate::storage::snapshot::RuntimePublication::parse_prefix(path)
    else {
        return RouteKind::Unknown;
    };
    // 前缀之后必须恰好是 `v1/<端点>`——路径形状是 `/i/{实例}/c/{版本}/v1/{端点}`。
    // 只看最后一段的话，`/i/x/c/rev/v1/anything/responses` 也会被当成推理端点。
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    match segments.as_slice() {
        ["i", _, "c", _, "v1", "responses"] => RouteKind::Responses,
        ["i", _, "c", _, "v1", "models"] => RouteKind::Models { revision },
        ["i", _, "c", _, "v1", "realtime"] => RouteKind::Realtime,
        _ => RouteKind::Unknown,
    }
}

/// 流式翻译的两条路径。透传保留上游事件名，chat 走状态机重建事件。
enum Translator {
    Passthrough { alias: String },
    Chat { state: chat::ChatStream },
}

impl Translator {
    fn start(&mut self) -> Option<Vec<ResponsesEvent>> {
        match self {
            // 透传：上游自己会发 response.created，网关不额外插入事件。
            Translator::Passthrough { .. } => None,
            Translator::Chat { state, .. } => Some(state.starting()),
        }
    }

    fn feed(&mut self, data: &Value) -> Vec<OwnedEvent> {
        match self {
            Translator::Passthrough { alias } => {
                let mut payload = data.clone();
                responses::rewrite_model(&mut payload, alias);
                // 透传时事件名由上游决定，用载荷里的 type 保持一致。
                let name = payload
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("message")
                    .to_owned();
                vec![OwnedEvent { name, payload }]
            }
            Translator::Chat { state, .. } => state
                .feed(data)
                .into_iter()
                .map(|event| OwnedEvent {
                    name: event.name.to_owned(),
                    payload: event.payload,
                })
                .collect(),
        }
    }

    fn finish(&mut self) -> Vec<OwnedEvent> {
        match self {
            Translator::Passthrough { .. } => Vec::new(),
            Translator::Chat { state, .. } => state
                .finish()
                .into_iter()
                .map(|event| OwnedEvent {
                    name: event.name.to_owned(),
                    payload: event.payload,
                })
                .collect(),
        }
    }

    /// 上游是否给过终止标记。
    ///
    /// 直通路径的事件由上游自己发（包括 `response.completed`），网关无从、
    /// 也不该在这里判定，必须返回「是」，否则会把 responses 直通误判成截断。
    fn saw_terminal(&self) -> bool {
        match self {
            Translator::Passthrough { .. } => true,
            Translator::Chat { state } => state.saw_finish_reason(),
        }
    }
}

/// 事件名可能是上游给的（透传），所以不能复用 `&'static str`。
struct OwnedEvent {
    name: String,
    payload: Value,
}

/// 分块传输编码写出器：每个事件一个 chunk，写完立即 flush。
struct Chunked<'a> {
    stream: &'a mut TcpStream,
    /// 是否已经向宿主写出失败；失败即视为对方断开。
    failed: bool,
}

impl<'a> Chunked<'a> {
    fn new(stream: &'a mut TcpStream) -> Self {
        Self {
            stream,
            failed: false,
        }
    }

    /// 上一次写是否失败（宿主断开）。
    fn failed(&self) -> bool {
        self.failed
    }

    fn write_frame(&mut self, name: &str, payload: &Value) -> Result<(), CoreError> {
        let frame = format!("event: {name}\ndata: {payload}\n\n");
        self.write_chunk(frame.as_bytes())
    }

    fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), CoreError> {
        let header = format!("{:x}\r\n", bytes.len());
        self.stream
            .write_all(header.as_bytes())
            .and_then(|_| self.stream.write_all(bytes))
            .and_then(|_| self.stream.write_all(b"\r\n"))
            .and_then(|_| self.stream.flush())
            .map_err(|_| {
                // 记下来：调用方据此区分“宿主断开”和“网关内部错误”。
                self.failed = true;
                CoreError::internal("向宿主写出流式响应失败")
            })
    }

    fn finish(self) -> Result<(), CoreError> {
        self.stream
            .write_all(b"0\r\n\r\n")
            .and_then(|_| self.stream.flush())
            .map_err(|_| CoreError::internal("结束流式响应失败"))
    }
}

/// 推理请求对某个目录版本的引用，随请求结束自动释放。
///
/// 用的是 `Drop` 而不是在每个 `return` 前手写 `release`：这条路径上提前返回的分支有十几条
/// （缺 model、准入失败、缺凭据、暂停、模态拒绝、转换失败、上游失败……），漏掉任何一条
/// 都会让那个版本永远不可回收——正好是要修的那个 bug 的另一种形态。
struct InferenceGuard {
    router: Arc<GatewayRouter>,
    revision: String,
}

impl Drop for InferenceGuard {
    fn drop(&mut self) {
        self.router.release(&self.revision, 0);
    }
}

/// 连接写出的守卫，记录「响应是否已经开始」。
///
/// 这条连接上存在两类失败，它们在返回 `Err` 时长得一模一样：
/// - **判定阶段失败**（令牌不对、目录版本不认识、请求缺 model）：一个字节都没写出去，
///   完全可以回一个可读的 HTTP 错误；
/// - **已经开流之后失败**（上游中途断开、空闲超时）：响应头早发出去了，只剩流内的 error 事件。
///
/// 少了这个区分，`handle` 的调用方就没法安全兜底——无条件补错误会把流式响应写坏，
/// 不补则让判定阶段的失败变成「连接被关掉、宿主看到 Empty reply」，查不出原因。
struct Response<'a> {
    stream: &'a mut TcpStream,
    started: bool,
}

impl<'a> Response<'a> {
    fn new(stream: &'a mut TcpStream) -> Self {
        Self {
            stream,
            started: false,
        }
    }

    /// 是否已经向宿主写出过任何响应字节。
    fn started(&self) -> bool {
        self.started
    }

    /// 取原始连接。调用方从这一刻起自己负责写出，守卫记为已开始。
    fn stream_mut(&mut self) -> &mut TcpStream {
        self.started = true;
        self.stream
    }

    fn write_json(&mut self, status: u16, payload: &Value) -> Result<(), CoreError> {
        self.started = true;
        write_json(self.stream, status, payload)
    }

    fn write_error(&mut self, error: &CoreError) -> Result<(), CoreError> {
        let status = status_for(error.code);
        self.write_json(status, &error_payload(error))
    }
}

struct IncomingRequest {
    method: String,
    path: String,
    headers: InboundHeaders,
    body: Vec<u8>,
}

/// 解析请求行、头部与正文。`Ok(None)` 表示对端在发请求前就关闭了连接。
fn read_request(reader: &mut BufReader<TcpStream>) -> Result<Option<IncomingRequest>, CoreError> {
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .map_err(|_| CoreError::validation("读取请求行失败"))?
        == 0
    {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let path = parts.next().unwrap_or_default().to_owned();
    if method.is_empty() || path.is_empty() {
        return Err(CoreError::validation("请求行不完整"));
    }

    let mut headers = InboundHeaders {
        method: method.clone(),
        ..InboundHeaders::default()
    };
    let mut content_length: Option<usize> = None;
    for _ in 0..MAX_HEADER_LINES {
        let mut raw = String::new();
        let read = reader
            .read_line(&mut raw)
            .map_err(|_| CoreError::validation("读取请求头失败"))?;
        if read == 0 || raw == "\r\n" || raw == "\n" {
            break;
        }
        if raw.len() > MAX_HEADER_LINE {
            return Err(CoreError::validation("请求头过长"));
        }
        let Some((name, value)) = raw.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        match name.as_str() {
            "host" => headers.host = Some(value),
            "origin" => headers.origin = Some(value),
            "authorization" => headers.authorization = Some(value),
            "content-type" => headers.content_type = Some(value),
            "accept" => headers.accept = Some(value),
            "access-control-request-method" => headers.access_control_request_method = Some(value),
            "content-length" => content_length = value.parse::<usize>().ok(),
            "transfer-encoding" => {
                return Err(CoreError::validation(
                    "本机网关不接受分块请求体，请使用 Content-Length",
                ))
            }
            _ => {}
        }
    }

    let length = content_length.unwrap_or(0);
    // 先按声明长度拒绝，避免为了测量而把超大请求读进内存。
    if length > MAX_REQUEST_BYTES {
        return Err(
            CoreError::new(ErrorCode::RequestTooLarge, "error.requestTooLarge").with_detail(
                format!("请求体 {length} 字节超过上限 {MAX_REQUEST_BYTES} 字节"),
            ),
        );
    }
    let mut body = vec![0u8; length];
    if length > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|_| CoreError::validation("请求体不完整"))?;
    }
    Ok(Some(IncomingRequest {
        method,
        path,
        headers,
        body,
    }))
}

fn write_head(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
) -> Result<(), CoreError> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncache-control: no-cache\r\nconnection: close\r\ntransfer-encoding: chunked\r\n\r\n"
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|_| CoreError::internal("写出响应头失败"))
}

fn write_json(stream: &mut TcpStream, status: u16, payload: &Value) -> Result<(), CoreError> {
    let body = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    let head = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        reason_for(status),
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(&body))
        .and_then(|_| stream.flush())
        .map_err(|_| CoreError::internal("写出响应失败"))
}

fn write_error(stream: &mut TcpStream, error: &CoreError) -> Result<(), CoreError> {
    let status = status_for(error.code);
    write_json(stream, status, &error_payload(error))
}

fn error_payload(error: &CoreError) -> Value {
    json!({
        "error": {
            "code": error.code,
            "message_key": error.message_key,
            "details": error.safe_details,
            "retryable": error.code.retryable(),
        }
    })
}

/// 错误码到 HTTP 状态的映射。宿主按 HTTP 语义决定是否重试，必须稳定。
fn status_for(code: ErrorCode) -> u16 {
    match code {
        ErrorCode::Unauthorized | ErrorCode::CredentialMissing => 401,
        ErrorCode::ModelPermissionDenied => 403,
        ErrorCode::NotFound | ErrorCode::RouteMismatch => 404,
        ErrorCode::RequestTooLarge => 413,
        ErrorCode::Conflict | ErrorCode::ConfigChanged => 409,
        ErrorCode::CapabilityUnsupported | ErrorCode::ValidationFailed => 400,
        ErrorCode::KeystoreLocked | ErrorCode::PortInUse => 503,
        ErrorCode::CatalogSchemaMismatch | ErrorCode::DesktopReloadRequired => 409,
        ErrorCode::ContinuationBound => 409,
        ErrorCode::ConfigParseFailed | ErrorCode::Internal => 500,
    }
}

fn reason_for(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// 上游非 2xx：归类后交给宿主，正文脱敏并截断。
fn upstream_status_error(status: u16, detail: &str) -> CoreError {
    let (code, message_key) = match status {
        401 | 403 => (
            ErrorCode::ModelPermissionDenied,
            "error.modelPermissionDenied",
        ),
        404 => (ErrorCode::NotFound, "error.upstreamModelMissing"),
        429 => (ErrorCode::Internal, "error.upstreamRateLimited"),
        400..=499 => (ErrorCode::ValidationFailed, "error.upstreamRejected"),
        _ => (ErrorCode::Internal, "error.upstreamFailed"),
    };
    CoreError::new(code, message_key).with_detail(format!("上游返回 {status}：{detail}"))
}

/// 传输层失败：连接被拒、TLS 失败、首事件超时等。
fn upstream_transport_error(error: &ureq::Error) -> CoreError {
    let text = error.to_string();
    let (code, message_key) = match &text {
        value if value.contains("timeout") || value.contains("timed out") => {
            (ErrorCode::Internal, "error.upstreamTimeout")
        }
        value if value.contains("dns") || value.contains("resolve") => {
            (ErrorCode::ValidationFailed, "error.upstreamUnresolvable")
        }
        _ => (ErrorCode::Internal, "error.upstreamUnreachable"),
    };
    CoreError::new(code, message_key).with_detail(format!("无法完成上游请求：{text}"))
}

/// 上游在流中途用数据帧报错。
///
/// 只取结构化的 `type`/`code`/`message` 并脱敏、截断：整帧正文有时会回显请求内容，
/// 而错误详情会出现在界面、日志与用户截图里。
fn upstream_stream_error(payload: &Value, secret: &str) -> CoreError {
    let kind = payload
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| payload.get("code").and_then(Value::as_str))
        .unwrap_or("upstream_error");
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("上游在流中途返回错误");
    let detail = redact(&format!("{kind}: {message}"), secret);
    CoreError::new(ErrorCode::Internal, "error.upstreamFailed")
        .with_detail(format!("上游流中途出错：{detail}"))
}

/// 流已开始后读取中断。此时响应头已经发出，只能用事件收尾。
fn stream_read_error(error: &std::io::Error) -> CoreError {
    CoreError::internal("上游流在传输中断开").with_detail(format!("读取上游流失败：{error}"))
}

/// 把已知秘密从要外发的文本里抹掉，再截断。
///
/// 上游错误正文有时会回显请求头；这是唯一可能让上游 Key 出现在响应里的路径。
fn redact(text: &str, secret: &str) -> String {
    let mut cleaned = text.replace(['\n', '\r'], " ");
    if !secret.is_empty() && cleaned.contains(secret) {
        cleaned = cleaned.replace(secret, "••••redacted");
    }
    if cleaned.chars().count() > MAX_UPSTREAM_ERROR_CHARS {
        cleaned = cleaned
            .chars()
            .take(MAX_UPSTREAM_ERROR_CHARS)
            .collect::<String>()
            + "…";
    }
    cleaned
}

/// 网关可观测状态，供壳层展示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayStatus {
    pub running: bool,
    /// 是否已暂停接受新请求。
    pub paused: bool,
    pub port: Option<u16>,
    pub served: u64,
    pub instance_id: String,
    /// 已发布、正在服务的目录版本。空列表表示尚未应用过任何配置。
    pub revisions: Vec<String>,
    pub token_fingerprint: String,
}

impl Gateway {
    pub fn status(&self) -> GatewayStatus {
        let address = self.local_addr();
        GatewayStatus {
            running: self.running.load(Ordering::Relaxed),
            paused: self.is_paused(),
            port: address.map(|value| value.port()),
            served: self.served_requests(),
            instance_id: self.config.instance_id.as_str().to_owned(),
            revisions: self.router.revisions(),
            // 只暴露指纹，便于人工核对 helper 是否指向同一令牌，不泄露令牌本身。
            token_fingerprint: token_fingerprint(self.config.token.expose()),
        }
    }
}

/// 令牌指纹：取前 8 位十六进制，足够核对、不足以反推。
pub fn token_fingerprint(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 路由形状是 `/i/{实例}/c/{版本}/v1/{端点}`，`/health` 是唯一的例外。
    /// 回归：过去只取最后一段，且不剥查询串——`/v1/x/responses` 会被当成推理端点，
    /// `/health?x=1` 反而变成 404。
    #[test]
    fn route_kind_requires_the_documented_shape_and_ignores_query() {
        let base = "/i/inst_1/c/rev_1";
        assert_eq!(
            route_kind(&format!("{base}/v1/responses")),
            RouteKind::Responses
        );
        assert_eq!(
            route_kind(&format!("{base}/v1/realtime")),
            RouteKind::Realtime
        );
        assert_eq!(
            route_kind(&format!("{base}/v1/models")),
            RouteKind::Models {
                revision: "rev_1".to_owned()
            }
        );
        assert_eq!(route_kind("/health"), RouteKind::Health);
        assert_eq!(route_kind("/health?probe=1"), RouteKind::Health);
        assert_eq!(
            route_kind(&format!("{base}/v1/models?limit=1")),
            RouteKind::Models {
                revision: "rev_1".to_owned()
            }
        );

        // 多一段尾路径不是本网关的路由，不能因为最后一段叫 responses 就当推理端点。
        assert_eq!(
            route_kind(&format!("{base}/v1/anything/responses")),
            RouteKind::Unknown
        );
        // 少了 v1 段、或没有实例前缀，同样不认。
        assert_eq!(route_kind(&format!("{base}/responses")), RouteKind::Unknown);
        assert_eq!(route_kind("/v1/responses"), RouteKind::Unknown);
        assert_eq!(route_kind("/"), RouteKind::Unknown);
    }
}
