//! Chat Completions 适配器：把宿主的 Responses 请求翻译成 OpenAI 兼容的 chat 请求，
//! 再把 chat 的流式分片还原成 Responses 事件。
//!
//! 为什么需要它：本机第三方供应商多数只提供 `/chat/completions`（例如
//! `model_protocols` 标为 `chat` 的模型），而 Codex 只说 Responses。
//!
//! 翻译原则（[网关与协议](../../../../docs/architecture/03-gateway-and-protocols.md)）：
//! - 上游没有对应表达的字段一律进 `losses`，不假装生效；
//! - 不重放已提交的流，不在流中更换凭据；
//! - 生成的 `call_id` 必须可回传，供宿主把 function result 对回来。

use super::{AdaptationLoss, PreparedRequest, ResponsesEvent, RouteLimits};
use crate::domain::error::CoreError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// 上游 chat 端点。
pub fn endpoint(base: &str) -> String {
    format!("{}/chat/completions", base.trim_end_matches('/'))
}

/// 准备 chat 请求：把 Responses 请求体翻译成 chat 请求体。
pub fn prepare(
    base: &str,
    upstream_id: &str,
    request: &Value,
    limits: &RouteLimits,
) -> Result<PreparedRequest, CoreError> {
    let object = request
        .as_object()
        .ok_or_else(|| CoreError::validation("请求体必须是 JSON 对象"))?;
    let mut losses = Vec::new();

    let mut body = Map::new();
    body.insert("model".to_owned(), json!(upstream_id));
    body.insert(
        "messages".to_owned(),
        Value::Array(messages(request, &mut losses)?),
    );
    body.insert("stream".to_owned(), json!(true));
    // 让上游在最后一帧给出用量；不支持时只是少一次统计，不影响内容。
    body.insert("stream_options".to_owned(), json!({"include_usage": true}));

    if let Some(tools) = object.get("tools").and_then(Value::as_array) {
        let translated: Vec<Value> = tools
            .iter()
            .filter_map(|tool| translate_tool(tool, &mut losses))
            .collect();
        if !translated.is_empty() {
            body.insert("tools".to_owned(), Value::Array(translated));
        }
    }
    if let Some(choice) = object.get("tool_choice") {
        if let Some(translated) = translate_tool_choice(choice, &mut losses) {
            body.insert("tool_choice".to_owned(), translated);
        }
    }
    if let Some(parallel) = object.get("parallel_tool_calls") {
        body.insert("parallel_tool_calls".to_owned(), parallel.clone());
    }
    // 输出上限由模型策略收口：宿主可以要得更少，但不能超过该模型声明的上限。
    let requested = object.get("max_output_tokens").and_then(Value::as_u64);
    let (effective, capped) = limits.clamp_output_limit(requested);
    match effective {
        Some(limit) => {
            // chat 的通用字段名；个别供应商只认 max_completion_tokens，属于端点差异，
            // 由真实请求验证，不在这里猜。
            body.insert("max_tokens".to_owned(), json!(limit));
            if capped {
                losses.push(AdaptationLoss::new(
                    "max_output_tokens",
                    "loss.outputLimitCapped",
                    format!(
                        "宿主请求 {} Token，超过该模型声明的上限 {}，已下调到上限",
                        requested.unwrap_or(0),
                        limit
                    ),
                ));
            }
        }
        None => losses.push(AdaptationLoss::new(
            "max_output_tokens",
            "loss.outputLimitNotSpecified",
            "宿主要求上限未声明，且该模型也没有声明上限，将使用上游默认值",
        )),
    }

    for (field, message_key, detail) in [
        (
            "store",
            "loss.storeUnsupported",
            "chat 端点没有等价的 store 语义",
        ),
        (
            "include",
            "loss.includeUnsupported",
            "chat 端点不支持 include 选择",
        ),
        (
            "prompt_cache_key",
            "loss.promptCacheKeyUnsupported",
            "chat 端点没有提示缓存键",
        ),
    ] {
        if object.contains_key(field) {
            losses.push(AdaptationLoss::new(field, message_key, detail));
        }
    }
    // 思考档位：只有该模型声明了档位（且映射已版本化为 reasoning.effort.v1）才发送。
    // 未声明就不发，并记录为损失，而不是假装生效。
    if let Some(effort) = object
        .get("reasoning")
        .and_then(|value| value.get("effort"))
    {
        let requested = effort.as_str().unwrap_or_default();
        if limits.reasoning_efforts.is_empty() {
            losses.push(AdaptationLoss::new(
                "reasoning.effort",
                "loss.reasoningEffortNotDeclared",
                format!("该模型未声明思考档位，{requested} 未发送"),
            ));
        } else if limits.allows_effort(requested) {
            body.insert("reasoning_effort".to_owned(), json!(requested));
        } else {
            losses.push(AdaptationLoss::new(
                "reasoning.effort",
                "loss.reasoningEffortOutOfRange",
                format!(
                    "思考档位 {} 不在该模型声明的集合 {:?} 内，未发送",
                    requested, limits.reasoning_efforts
                ),
            ));
        }
    }

    let bytes = serde_json::to_vec(&Value::Object(body))
        .map_err(|_| CoreError::internal("请求体序列化失败"))?;
    Ok(PreparedRequest {
        url: endpoint(base),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: bytes,
        losses,
    })
}

/// Responses 的 `input` 数组翻译成 chat 的 `messages`。
///
/// 系统级内容的两个来源——顶层 `instructions` 与 `input` 里的 `developer` 消息——合并成
/// **一条**开头的 system 消息。Codex 现在把开发者指令放在 `input` 里发（本机实测 0.155：
/// 一条 role 为 `developer` 的 message，1.3 万字符），而 chat 协议没有 `developer` 角色：
/// 原样转发会被只认 system/user/assistant/tool 的上游直接拒绝（moonshot 的
/// `/chat/completions` 回 `400 role 'developer' is not allowed`）。也不能拆成两条 system——
/// 上游普遍要求 system 位于首位。
fn messages(request: &Value, losses: &mut Vec<AdaptationLoss>) -> Result<Vec<Value>, CoreError> {
    let mut system_parts: Vec<String> = Vec::new();
    if let Some(instructions) = request.get("instructions").and_then(Value::as_str) {
        if !instructions.trim().is_empty() {
            system_parts.push(instructions.to_owned());
        }
    }

    let mut messages: Vec<Value> = Vec::new();
    let Some(items) = request.get("input").and_then(Value::as_array) else {
        return Ok(with_system(system_parts, messages));
    };
    let mut developer_after_conversation = false;
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                if item.get("role").and_then(Value::as_str) == Some("developer") {
                    let (texts, images) = message_parts(item, losses);
                    if !images.is_empty() {
                        losses.push(AdaptationLoss::new(
                            "input.developer.image",
                            "loss.contentPartDropped",
                            "developer 消息里的图片无法并入 system 消息，未发送",
                        ));
                    }
                    // 合并会把它的位置提前到开头；已经不在一起了就得记一笔。
                    if !messages.is_empty() {
                        developer_after_conversation = true;
                    }
                    system_parts.extend(texts);
                } else {
                    push_message(&mut messages, item, losses);
                }
            }
            Some("function_call") => push_function_call(&mut messages, item, losses),
            Some("function_call_output") => push_tool_output(&mut messages, item),
            Some("reasoning") => losses.push(AdaptationLoss::new(
                "input.reasoning",
                "loss.reasoningItemDropped",
                "上游为 chat 协议，无法回传推理条目",
            )),
            Some(other) => losses.push(AdaptationLoss::new(
                other,
                "loss.inputItemDropped",
                format!("chat 协议没有 {other} 这类输入条目"),
            )),
            None => losses.push(AdaptationLoss::new(
                "input.unknown",
                "loss.inputItemDropped",
                "输入条目缺少 type，未发送",
            )),
        }
    }
    if developer_after_conversation {
        losses.push(AdaptationLoss::new(
            "input.developer",
            "loss.developerInstructionReordered",
            "developer 消息出现在对话之后，已并入开头的 system 消息",
        ));
    }
    Ok(with_system(system_parts, messages))
}

/// 系统级内容恒在首位：上游普遍要求 system 打头，也不接受夹在对话中间的 system。
fn with_system(system_parts: Vec<String>, mut messages: Vec<Value>) -> Vec<Value> {
    if !system_parts.is_empty() {
        messages.insert(
            0,
            json!({"role": "system", "content": system_parts.join("\n\n")}),
        );
    }
    messages
}

/// 一条 message 的内容分片：文本按原顺序、图片保留原始 URL。
fn message_parts(item: &Value, losses: &mut Vec<AdaptationLoss>) -> (Vec<String>, Vec<Value>) {
    let mut texts: Vec<String> = Vec::new();
    let mut images: Vec<Value> = Vec::new();
    let Some(parts) = item.get("content").and_then(Value::as_array) else {
        return (texts, images);
    };
    for part in parts {
        match part.get("type").and_then(Value::as_str) {
            Some("input_text") | Some("output_text") | Some("text") => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    texts.push(text.to_owned());
                }
            }
            Some("input_image") | Some("image_url") => {
                let url =
                    part.get("image_url")
                        .and_then(|value| {
                            value.as_str().map(str::to_owned).or_else(|| {
                                value.get("url").and_then(Value::as_str).map(str::to_owned)
                            })
                        })
                        .or_else(|| {
                            part.get("image_url")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                        });
                match url {
                    Some(url) => {
                        images.push(json!({"type": "image_url", "image_url": {"url": url}}))
                    }
                    None => losses.push(AdaptationLoss::new(
                        "input.image",
                        "loss.imageWithoutUrl",
                        "图片输入没有可用 URL，未发送",
                    )),
                }
            }
            Some(other) => losses.push(AdaptationLoss::new(
                other,
                "loss.contentPartDropped",
                format!("chat 协议没有 {other} 这类内容分片"),
            )),
            None => {}
        }
    }
    (texts, images)
}

fn push_message(messages: &mut Vec<Value>, item: &Value, losses: &mut Vec<AdaptationLoss>) {
    let Some(role) = item.get("role").and_then(Value::as_str) else {
        losses.push(AdaptationLoss::new(
            "input.message.role",
            "loss.inputItemDropped",
            "消息缺少 role，未发送",
        ));
        return;
    };
    if item.get("content").and_then(Value::as_array).is_none() {
        messages.push(json!({"role": role, "content": ""}));
        return;
    }
    let (texts, images) = message_parts(item, losses);
    // 纯文本用字符串形式：兼容性最好，不必让每个供应商都接受分片数组。
    let content = if images.is_empty() {
        json!(texts.join("\n"))
    } else {
        let mut blocks: Vec<Value> = texts
            .iter()
            .map(|text| json!({"type": "text", "text": text}))
            .collect();
        blocks.extend(images);
        Value::Array(blocks)
    };
    messages.push(json!({"role": role, "content": content}));
}

/// 助手发起的工具调用。连续的调用合并进同一条 assistant 消息，符合 chat 的惯例。
fn push_function_call(messages: &mut Vec<Value>, item: &Value, losses: &mut Vec<AdaptationLoss>) {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (Some(name), Some(arguments)) = (
        item.get("name").and_then(Value::as_str),
        item.get("arguments").and_then(Value::as_str),
    ) else {
        losses.push(AdaptationLoss::new(
            "input.function_call",
            "loss.functionCallIncomplete",
            "工具调用缺少 name 或 arguments，未发送",
        ));
        return;
    };
    let call = json!({"id": call_id, "type": "function", "function": {"name": name, "arguments": arguments}});
    let appended = messages
        .last_mut()
        .filter(|last| last.get("role").and_then(Value::as_str) == Some("assistant"))
        .and_then(|last| last.get_mut("tool_calls"))
        .and_then(|calls| calls.as_array_mut())
        .map(|calls| calls.push(call.clone()))
        .is_some();
    if !appended {
        messages.push(json!({"role": "assistant", "content": null, "tool_calls": [call]}));
    }
}

fn push_tool_output(messages: &mut Vec<Value>, item: &Value) {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output = match item.get("output") {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    messages.push(json!({"role": "tool", "tool_call_id": call_id, "content": output}));
}

fn translate_tool(tool: &Value, losses: &mut Vec<AdaptationLoss>) -> Option<Value> {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        losses.push(AdaptationLoss::new(
            "tools",
            "loss.nonFunctionToolDropped",
            "chat 协议只映射 function 工具，其余宿主内置工具未发送",
        ));
        return None;
    }
    let name = tool.get("name").and_then(Value::as_str)?;
    let mut function = Map::new();
    function.insert("name".to_owned(), json!(name));
    if let Some(description) = tool.get("description") {
        function.insert("description".to_owned(), description.clone());
    }
    function.insert(
        "parameters".to_owned(),
        tool.get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
    );
    Some(json!({"type": "function", "function": Value::Object(function)}))
}

fn translate_tool_choice(choice: &Value, losses: &mut Vec<AdaptationLoss>) -> Option<Value> {
    match choice {
        Value::String(mode) if matches!(mode.as_str(), "auto" | "none" | "required") => {
            Some(json!(mode))
        }
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("function") => {
            match object.get("name").and_then(Value::as_str) {
                Some(name) => Some(json!({"type": "function", "function": {"name": name}})),
                None => {
                    losses.push(AdaptationLoss::new(
                        "tool_choice",
                        "loss.toolChoiceDropped",
                        "tool_choice 指定了 function 但缺少 name，改为上游默认",
                    ));
                    None
                }
            }
        }
        _ => {
            losses.push(AdaptationLoss::new(
                "tool_choice",
                "loss.toolChoiceDropped",
                "无法翻译的 tool_choice，改为上游默认",
            ));
            None
        }
    }
}

/// 流式翻译状态机：chat 分片 → Responses 事件。
///
/// 事件顺序按宿主已验证的形状发出：`response.created` →
/// `output_item.added` → `content_part.added` → `output_text.delta` →
/// `output_text.done` → `content_part.done` → `output_item.done` → `response.completed`。
pub struct ChatStream {
    alias: String,
    response_id: String,
    message_id: String,
    sequence: u64,
    next_output_index: u64,
    message_index: Option<u64>,
    message_opened: bool,
    text: String,
    tool_calls: BTreeMap<u64, ToolCall>,
    output: Vec<Value>,
    usage: Option<Value>,
    created: bool,
    completed: bool,
    /// 上游是否在 delta 里给过非 null 的 `finish_reason`。
    ///
    /// `[DONE]` 虽被 OpenAI 兼容协议要求，但确有只发 `finish_reason` 的实现；
    /// 上游干净断开时用它区分「正常结束」与「半截被截断」。
    saw_finish_reason: bool,
}

struct ToolCall {
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    output_index: u64,
}

impl ChatStream {
    pub fn new(alias: &str) -> Self {
        let response_id = format!("resp_{}", uuid::Uuid::new_v4().simple());
        let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
        Self {
            alias: alias.to_owned(),
            response_id,
            message_id,
            sequence: 0,
            next_output_index: 0,
            message_index: None,
            message_opened: false,
            text: String::new(),
            tool_calls: BTreeMap::new(),
            output: Vec::new(),
            usage: None,
            created: false,
            completed: false,
            saw_finish_reason: false,
        }
    }

    /// 上游已接受请求：先把 `response.created` 交给宿主，缩短首事件等待。
    pub fn starting(&mut self) -> Vec<ResponsesEvent> {
        if self.created {
            return Vec::new();
        }
        self.created = true;
        vec![self.event(
            "response.created",
            json!({"response": self.response_object("in_progress", json!([]), Value::Null)}),
        )]
    }

    /// 消费一个 chat 分片，产出对应的 Responses 事件。
    pub fn feed(&mut self, chunk: &Value) -> Vec<ResponsesEvent> {
        let mut events = Vec::new();
        if let Some(id) = chunk.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                self.response_id = format!("resp_{id}");
            }
        }
        if let Some(usage) = chunk.get("usage").filter(|value| !value.is_null()) {
            self.usage = Some(usage.clone());
        }
        let Some(choices) = chunk.get("choices").and_then(Value::as_array) else {
            return events;
        };
        for choice in choices {
            // `finish_reason` 与 `delta` 平级；即使该帧没有 delta 也要记账。
            if choice
                .get("finish_reason")
                .is_some_and(|reason| !reason.is_null())
            {
                self.saw_finish_reason = true;
            }
            let Some(delta) = choice.get("delta") else {
                continue;
            };
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    self.absorb_tool_call(call, &mut events);
                }
            }
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                if !text.is_empty() {
                    self.absorb_text(text, &mut events);
                }
            }
        }
        events
    }

    fn absorb_text(&mut self, text: &str, events: &mut Vec<ResponsesEvent>) {
        if !self.message_opened {
            let index = self.next_output_index;
            self.next_output_index += 1;
            self.message_index = Some(index);
            self.message_opened = true;
            events.push(self.event(
                "response.output_item.added",
                json!({
                    "output_index": index,
                    "item": {"id": self.message_id, "type": "message", "role": "assistant",
                             "status": "in_progress", "content": []},
                }),
            ));
            events.push(self.event(
                "response.content_part.added",
                json!({
                    "item_id": self.message_id, "output_index": index, "content_index": 0,
                    "part": {"type": "output_text", "text": "", "annotations": []},
                }),
            ));
        }
        self.text.push_str(text);
        events.push(self.event(
            "response.output_text.delta",
            json!({
                "item_id": self.message_id,
                "output_index": self.message_index.unwrap_or(0),
                "content_index": 0,
                "delta": text,
            }),
        ));
    }

    fn absorb_tool_call(&mut self, call: &Value, events: &mut Vec<ResponsesEvent>) {
        let key = call.get("index").and_then(Value::as_u64).unwrap_or(0);
        let function = call.get("function").cloned().unwrap_or(Value::Null);
        if !self.tool_calls.contains_key(&key) {
            let output_index = self.next_output_index;
            self.next_output_index += 1;
            let call_id = call
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()));
            let entry = ToolCall {
                item_id: format!("fc_{}", uuid::Uuid::new_v4().simple()),
                call_id,
                name: function
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                arguments: String::new(),
                output_index,
            };
            events.push(self.event(
                "response.output_item.added",
                json!({
                    "output_index": output_index,
                    "item": {"id": entry.item_id, "type": "function_call", "call_id": entry.call_id,
                             "name": entry.name, "arguments": "", "status": "in_progress"},
                }),
            ));
            self.tool_calls.insert(key, entry);
        }
        let entry = self.tool_calls.get_mut(&key).expect("刚插入或已存在");
        if let Some(id) = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            entry.call_id = id.to_owned();
        }
        if let Some(name) = function
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        {
            entry.name = name.to_owned();
        }
        if let Some(fragment) = function.get("arguments").and_then(Value::as_str) {
            if !fragment.is_empty() {
                entry.arguments.push_str(fragment);
                let (item_id, output_index) = (entry.item_id.clone(), entry.output_index);
                events.push(self.event(
                    "response.function_call_arguments.delta",
                    json!({
                        "item_id": item_id, "output_index": output_index, "delta": fragment,
                    }),
                ));
            }
        }
    }

    /// 上游是否给过协议层终止标记（非 null 的 `finish_reason`）。
    pub fn saw_finish_reason(&self) -> bool {
        self.saw_finish_reason
    }

    /// 上游流结束：补齐未关闭的条目并给出 `response.completed`。
    pub fn finish(&mut self) -> Vec<ResponsesEvent> {
        if self.completed {
            return Vec::new();
        }
        self.completed = true;
        let mut events = Vec::new();
        if self.message_opened {
            let index = self.message_index.unwrap_or(0);
            events.push(self.event(
                "response.output_text.done",
                json!({"item_id": self.message_id, "output_index": index, "content_index": 0, "text": self.text}),
            ));
            events.push(self.event(
                "response.content_part.done",
                json!({
                    "item_id": self.message_id, "output_index": index, "content_index": 0,
                    "part": {"type": "output_text", "text": self.text, "annotations": []},
                }),
            ));
            let item = json!({
                "id": self.message_id, "type": "message", "role": "assistant", "status": "completed",
                "content": [{"type": "output_text", "text": self.text, "annotations": []}],
            });
            events.push(self.event(
                "response.output_item.done",
                json!({"output_index": index, "item": item}),
            ));
            self.output.push(item);
        }
        // 先取快照再发事件：event() 需要 &mut self，不能在遍历 tool_calls 时借用。
        let tool_items: Vec<(u64, Value)> = self
            .tool_calls
            .values()
            .map(|call| {
                (
                    call.output_index,
                    json!({
                        "id": call.item_id, "type": "function_call", "call_id": call.call_id,
                        "name": call.name, "arguments": call.arguments, "status": "completed",
                    }),
                )
            })
            .collect();
        for (output_index, item) in tool_items {
            events.push(self.event(
                "response.output_item.done",
                json!({"output_index": output_index, "item": item}),
            ));
            self.output.push(item);
        }
        let output = Value::Array(self.output.clone());
        let usage = self.usage_object();
        events.push(self.event(
            "response.completed",
            json!({"response": self.response_object("completed", output, usage)}),
        ));
        events
    }

    fn usage_object(&self) -> Value {
        let source = self.usage.clone().unwrap_or(Value::Null);
        let input = source
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output = source
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let total = source
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(input + output);
        let cached = source
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let reasoning = source
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        json!({
            "input_tokens": input, "output_tokens": output, "total_tokens": total,
            "input_tokens_details": {"cached_tokens": cached},
            "output_tokens_details": {"reasoning_tokens": reasoning},
        })
    }

    fn response_object(&self, status: &str, output: Value, usage: Value) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": 0,
            "model": self.alias,
            "status": status,
            "output": output,
            "usage": usage,
        })
    }

    fn event(&mut self, name: &'static str, payload: Value) -> ResponsesEvent {
        let mut payload = payload;
        if let Some(object) = payload.as_object_mut() {
            object.insert("type".to_owned(), json!(name));
            object.insert("sequence_number".to_owned(), json!(self.sequence));
        }
        self.sequence += 1;
        ResponsesEvent::new(name, payload)
    }
}

/// 非流式响应翻译：chat 补全 → Responses 对象。
pub fn translate_completion(completion: &Value, alias: &str) -> Value {
    let message = completion
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .cloned()
        .unwrap_or(Value::Null);

    let mut output = Vec::new();
    let text = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !text.is_empty() {
        output.push(json!({
            "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
            "type": "message", "role": "assistant", "status": "completed",
            "content": [{"type": "output_text", "text": text, "annotations": []}],
        }));
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let function = call.get("function").cloned().unwrap_or(Value::Null);
            output.push(json!({
                "id": format!("fc_{}", uuid::Uuid::new_v4().simple()),
                "type": "function_call",
                "call_id": call.get("id").and_then(Value::as_str).unwrap_or_default(),
                "name": function.get("name").and_then(Value::as_str).unwrap_or_default(),
                "arguments": function.get("arguments").and_then(Value::as_str).unwrap_or_default(),
                "status": "completed",
            }));
        }
    }

    let usage_source = completion.get("usage").cloned().unwrap_or(Value::Null);
    let input = usage_source
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let out = usage_source
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "id": completion.get("id").and_then(Value::as_str).map(|id| format!("resp_{id}"))
            .unwrap_or_else(|| format!("resp_{}", uuid::Uuid::new_v4().simple())),
        "object": "response",
        "created_at": completion.get("created").and_then(Value::as_u64).unwrap_or(0),
        "model": alias,
        "status": "completed",
        "output": Value::Array(output),
        "usage": {
            "input_tokens": input,
            "output_tokens": out,
            "total_tokens": usage_source.get("total_tokens").and_then(Value::as_u64).unwrap_or(input + out),
            "input_tokens_details": {"cached_tokens": 0},
            "output_tokens_details": {"reasoning_tokens": 0},
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body_of(prepared: &PreparedRequest) -> Value {
        serde_json::from_slice(&prepared.body).unwrap()
    }

    fn loss_features(losses: &[AdaptationLoss]) -> Vec<&str> {
        losses.iter().map(|loss| loss.feature.as_str()).collect()
    }

    fn responses_request() -> Value {
        json!({
            "model": "gs/p_a/m_1",
            "instructions": "你是编码助手",
            "input": [{"type": "message", "role": "user",
                       "content": [{"type": "input_text", "text": "你好"}]}],
            "max_output_tokens": 4096,
            "stream": true,
        })
    }

    #[test]
    fn prepare_translates_the_basic_request() {
        let prepared = prepare(
            "https://host/v1/",
            "vendor/Model-X",
            &responses_request(),
            &RouteLimits::default(),
        )
        .unwrap();

        assert_eq!(prepared.url, "https://host/v1/chat/completions");
        let body = body_of(&prepared);
        assert_eq!(body["model"], "vendor/Model-X");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "你是编码助手");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "你好");
    }

    #[test]
    fn prepare_maps_function_tools_and_choice() {
        let mut request = responses_request();
        request["tools"] = json!([
            {"type": "function", "name": "read_file", "description": "读文件",
             "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}},
            {"type": "web_search"},
        ]);
        request["tool_choice"] = json!({"type": "function", "name": "read_file"});

        let prepared = prepare("https://host/v1", "m", &request, &RouteLimits::default()).unwrap();
        let body = body_of(&prepared);

        assert_eq!(
            body["tools"].as_array().unwrap().len(),
            1,
            "非 function 工具不应被发送"
        );
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
        assert_eq!(body["tools"][0]["function"]["parameters"]["type"], "object");
        assert_eq!(body["tool_choice"]["function"]["name"], "read_file");
        assert!(loss_features(&prepared.losses).contains(&"tools"));
    }

    #[test]
    fn prepare_merges_consecutive_tool_calls_and_passes_results_back() {
        let mut request = responses_request();
        request["input"] = json!([
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "看两个文件"}]},
            {"type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\":\"a\"}"},
            {"type": "function_call", "call_id": "call_2", "name": "read_file", "arguments": "{\"path\":\"b\"}"},
            {"type": "function_call_output", "call_id": "call_1", "output": "内容 A"},
        ]);

        let prepared = prepare("https://host/v1", "m", &request, &RouteLimits::default()).unwrap();
        let body = body_of(&prepared);
        let messages = body["messages"].as_array().unwrap();

        assert_eq!(
            messages.len(),
            4,
            "system + user + 合并后的 assistant + tool"
        );
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(messages[2]["tool_calls"][1]["id"], "call_2");
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call_1");
        assert_eq!(messages[3]["content"], "内容 A");
    }

    #[test]
    fn prepare_keeps_images_as_content_parts() {
        let mut request = responses_request();
        request["input"] = json!([{"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": "看这张图"},
            {"type": "input_image", "image_url": "data:image/png;base64,AAAA"},
        ]}]);

        let prepared = prepare("https://host/v1", "m", &request, &RouteLimits::default()).unwrap();
        let content = &body_of(&prepared)["messages"][1]["content"];

        assert!(content.is_array(), "带图片时必须用分片数组");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,AAAA");
    }

    /// Codex 真实发出的形状：顶层 `instructions`，`input` 里还有一条 role 为 `developer`
    /// 的开发者指令（0.155 实测，1.3 万字符）。
    fn codex_request() -> Value {
        json!({
            "instructions": "顶层指令",
            "input": [
                {"type": "message", "role": "developer", "content": [
                    {"type": "input_text", "text": "开发者指令"}]},
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "你好"}]},
            ],
        })
    }

    #[test]
    fn developer_instructions_are_folded_into_the_system_message() {
        let prepared = prepare(
            "https://api.moonshot.cn/v1",
            "kimi-k3",
            &codex_request(),
            &RouteLimits::default(),
        )
        .unwrap();
        let body = body_of(&prepared);
        let messages = body["messages"].as_array().unwrap();

        assert!(
            messages
                .iter()
                .all(|message| message["role"] != "developer"),
            "chat 上游不认识 developer，发出去就是 400"
        );
        assert_eq!(messages.len(), 2, "system + user：系统级内容只留一条");
        assert_eq!(messages[0]["role"], "system");
        let system = messages[0]["content"].as_str().unwrap();
        assert_eq!(
            system, "顶层指令\n\n开发者指令",
            "顶层指令在前、开发者指令在后，顺序固定"
        );
        assert_eq!(messages[1]["role"], "user");
        assert!(
            !loss_features(&prepared.losses).contains(&"input.developer"),
            "开发者指令本来就在最前面，合并没有改变语义"
        );
    }

    #[test]
    fn a_developer_message_after_the_conversation_is_folded_and_marked() {
        let request = json!({
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "你好"}]},
                {"type": "message", "role": "developer", "content": [
                    {"type": "input_text", "text": "补充指令"}]},
            ],
        });
        let prepared = prepare("https://host/v1", "m", &request, &RouteLimits::default()).unwrap();
        let body = body_of(&prepared);
        let messages = body["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "补充指令");
        assert_eq!(messages[1]["role"], "user");
        assert!(
            loss_features(&prepared.losses).contains(&"input.developer"),
            "位置被提前了，必须记为损失"
        );
    }

    #[test]
    fn prepare_records_every_field_it_cannot_send() {
        let mut request = responses_request();
        request["store"] = json!(true);
        request["include"] = json!(["reasoning.encrypted_content"]);
        request["prompt_cache_key"] = json!("cache-1");
        request["reasoning"] = json!({"effort": "high"});

        let prepared = prepare("https://host/v1", "m", &request, &RouteLimits::default()).unwrap();
        let features = loss_features(&prepared.losses);

        for expected in ["store", "include", "prompt_cache_key", "reasoning.effort"] {
            assert!(features.contains(&expected), "{expected} 必须被记录为损失");
        }
        // 损失只记录，不写入请求体。
        let body = body_of(&prepared);
        assert!(body.get("store").is_none());
        assert!(body.get("reasoning").is_none());
    }

    fn limits(output: Option<u64>, efforts: &[&str]) -> RouteLimits {
        RouteLimits {
            output_limit: output,
            reasoning_efforts: efforts.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn the_declared_output_limit_wins_over_a_larger_host_request() {
        let mut request = responses_request();
        request["max_output_tokens"] = json!(32_000);

        let prepared =
            prepare("https://host/v1", "m", &request, &limits(Some(8_192), &[])).unwrap();

        assert_eq!(
            body_of(&prepared)["max_tokens"],
            8192,
            "必须收口到模型声明的上限"
        );
        assert!(
            loss_features(&prepared.losses).contains(&"max_output_tokens"),
            "下调必须被记录，不能静默发生"
        );
        assert!(prepared.losses[0].detail.contains("32000"));
    }

    #[test]
    fn a_smaller_host_request_is_kept_and_not_raised_to_the_declared_limit() {
        let mut request = responses_request();
        request["max_output_tokens"] = json!(1_024);

        let prepared =
            prepare("https://host/v1", "m", &request, &limits(Some(8_192), &[])).unwrap();

        assert_eq!(body_of(&prepared)["max_tokens"], 1024, "宿主可以要得更少");
        assert!(prepared.losses.is_empty());
    }

    #[test]
    fn the_declared_limit_applies_even_when_the_host_asks_for_nothing() {
        let mut request = responses_request();
        request.as_object_mut().unwrap().remove("max_output_tokens");

        let prepared =
            prepare("https://host/v1", "m", &request, &limits(Some(4_096), &[])).unwrap();

        assert_eq!(body_of(&prepared)["max_tokens"], 4096);
        assert!(prepared.losses.is_empty(), "这是模型策略，不算损失");
    }

    #[test]
    fn a_declared_effort_is_sent_as_a_versioned_mapping() {
        let mut request = responses_request();
        request["reasoning"] = json!({"effort": "high"});

        let prepared = prepare(
            "https://host/v1",
            "m",
            &request,
            &limits(None, &["low", "high"]),
        )
        .unwrap();

        assert_eq!(body_of(&prepared)["reasoning_effort"], "high");
        assert!(prepared.losses.is_empty());
    }

    #[test]
    fn an_undeclared_effort_is_dropped_and_reported_instead_of_guessed() {
        let mut request = responses_request();
        request["reasoning"] = json!({"effort": "max"});

        // 模型完全没声明档位。
        let prepared = prepare("https://host/v1", "m", &request, &limits(None, &[])).unwrap();
        assert!(
            body_of(&prepared)["reasoning_effort"].is_null(),
            "未声明就不得发送"
        );
        assert!(loss_features(&prepared.losses).contains(&"reasoning.effort"));

        // 声明了档位但不含请求值。
        let prepared = prepare(
            "https://host/v1",
            "m",
            &request,
            &limits(None, &["low", "high"]),
        )
        .unwrap();
        assert!(body_of(&prepared)["reasoning_effort"].is_null());
        assert!(prepared.losses[0].message_key == "loss.reasoningEffortOutOfRange");
    }

    #[test]
    fn clamp_output_limit_has_three_distinct_outcomes() {
        let declared = limits(Some(8_192), &[]);
        assert_eq!(
            declared.clamp_output_limit(Some(32_000)),
            (Some(8_192), true)
        );
        assert_eq!(
            declared.clamp_output_limit(Some(1_024)),
            (Some(1_024), false)
        );
        assert_eq!(declared.clamp_output_limit(None), (Some(8_192), false));

        let undeclared = RouteLimits::default();
        assert_eq!(
            undeclared.clamp_output_limit(Some(2_048)),
            (Some(2_048), false)
        );
        assert_eq!(undeclared.clamp_output_limit(None), (None, false));
    }

    #[test]
    fn prepare_rejects_non_object_bodies() {
        assert!(prepare(
            "https://host/v1",
            "m",
            &json!("nope"),
            &RouteLimits::default()
        )
        .is_err());
    }

    fn event_names(events: &[ResponsesEvent]) -> Vec<&'static str> {
        events.iter().map(|event| event.name).collect()
    }

    #[test]
    fn stream_emits_the_verified_event_order_for_text() {
        let mut stream = ChatStream::new("gs/p_a/m_1");
        let mut all = stream.starting();
        assert_eq!(event_names(&all), vec!["response.created"]);

        all.extend(stream.feed(
            &json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {"role": "assistant"}}]}),
        ));
        all.extend(stream.feed(&json!({"choices": [{"index": 0, "delta": {"content": "你"}}]})));
        all.extend(stream.feed(&json!({"choices": [{"index": 0, "delta": {"content": "好"}, "finish_reason": "stop"}]})));
        all.extend(stream.feed(&json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 3, "total_tokens": 15}})));
        all.extend(stream.finish());

        assert_eq!(
            event_names(&all),
            vec![
                "response.created",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.completed",
            ],
            "事件顺序必须与宿主已验证的形状一致"
        );

        let completed = all.last().unwrap();
        let response = &completed.payload["response"];
        assert_eq!(response["status"], "completed");
        assert_eq!(
            response["model"], "gs/p_a/m_1",
            "响应身份必须是 alias，不是上游 ID"
        );
        assert_eq!(response["usage"]["input_tokens"], 12);
        assert_eq!(response["usage"]["output_tokens"], 3);
        assert_eq!(response["usage"]["total_tokens"], 15);
        assert_eq!(response["output"][0]["content"][0]["text"], "你好");
    }

    #[test]
    fn stream_unwraps_an_upstream_error_carried_in_the_chunk() {
        let mut stream = ChatStream::new("gs/p_a/m_1");
        stream.starting();
        // data 帧带 error 而不是 choices 时不能产生半截会话。
        let events = stream.feed(&json!({"error": {"message": "上游拒绝"}}));
        assert!(events.is_empty());
    }

    #[test]
    fn stream_accumulates_tool_call_argument_fragments() {
        let mut stream = ChatStream::new("gs/p_a/m_1");
        let mut all = stream.starting();
        all.extend(stream.feed(&json!({"choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": 0, "id": "call_9", "type": "function", "function": {"name": "read_file", "arguments": "{\"pa"}}
        ]}}]})));
        all.extend(
            stream.feed(&json!({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "th\":\"a\"}"}}
            ]}}]})),
        );
        all.extend(stream.finish());

        let added = all
            .iter()
            .find(|event| event.name == "response.output_item.added")
            .unwrap();
        assert_eq!(added.payload["item"]["type"], "function_call");
        assert_eq!(added.payload["item"]["name"], "read_file");
        assert_eq!(added.payload["item"]["call_id"], "call_9");

        let deltas: Vec<&str> = all
            .iter()
            .filter(|event| event.name == "response.function_call_arguments.delta")
            .map(|event| event.payload["delta"].as_str().unwrap())
            .collect();
        assert_eq!(deltas, vec!["{\"pa", "th\":\"a\"}"]);

        let completed = all.last().unwrap();
        let item = &completed.payload["response"]["output"][0];
        assert_eq!(
            item["arguments"], "{\"path\":\"a\"}",
            "分片必须拼成完整参数"
        );
        assert_eq!(item["call_id"], "call_9", "call_id 必须可回传以对回结果");
    }

    #[test]
    fn sequence_numbers_are_monotonic_across_the_whole_stream() {
        let mut stream = ChatStream::new("gs/p_a/m_1");
        let mut all = stream.starting();
        all.extend(stream.feed(&json!({"choices": [{"index": 0, "delta": {"content": "x"}}]})));
        all.extend(stream.feed(&json!({"choices": [], "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}})));
        all.extend(stream.finish());

        for (index, event) in all.iter().enumerate() {
            assert_eq!(event.payload["sequence_number"], index as u64);
            assert_eq!(event.payload["type"], event.name);
        }
    }

    #[test]
    fn finish_is_idempotent() {
        let mut stream = ChatStream::new("gs/p_a/m_1");
        stream.starting();
        stream.feed(&json!({"choices": [{"index": 0, "delta": {"content": "x"}}]}));
        assert!(!stream.finish().is_empty());
        assert!(stream.finish().is_empty(), "重复收尾不得再发一次 completed");
    }

    #[test]
    fn translate_completion_maps_text_tools_and_usage() {
        let completion = json!({
            "id": "chatcmpl-7",
            "created": 1_700_000_000,
            "choices": [{"index": 0, "message": {
                "role": "assistant",
                "content": "好的",
                "tool_calls": [{"id": "call_1", "type": "function",
                                "function": {"name": "read_file", "arguments": "{}"}}],
            }, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 7, "total_tokens": 12},
        });

        let response = translate_completion(&completion, "gs/p_a/m_1");
        assert_eq!(response["model"], "gs/p_a/m_1");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["output"].as_array().unwrap().len(), 2);
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][1]["type"], "function_call");
        assert_eq!(response["output"][1]["call_id"], "call_1");
        assert_eq!(response["usage"]["input_tokens"], 5);
        assert_eq!(response["usage"]["total_tokens"], 12);
    }
}
