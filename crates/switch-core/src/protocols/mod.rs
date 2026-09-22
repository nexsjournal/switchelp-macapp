//! 协议适配：把宿主的 Responses 请求转成上游协议，再把上游流还原成 Responses 事件。
//!
//! 规则来自 [网关与协议](../../../../docs/architecture/03-gateway-and-protocols.md)：
//! 转换必须**显式记录损失**，不能假装上游支持它没有的能力。PDF 与视频永不进入宿主目录，
//! 网关侧也不做隐式转写（例如把 PDF 渲染成图片再冒充原生输入）。
//!
//! 宿主只说 Responses；`chat_completions` 供应商由本模块负责双向翻译。

pub mod chat;
pub mod responses;

use crate::domain::capability::Support;
use serde::{Deserialize, Serialize};

/// 适配器标识。路由快照里的 `protocol_id` 决定用哪一个。
pub const RESPONSES_V1: &str = "responses.v1";
pub const CHAT_COMPLETIONS_V1: &str = "chat_completions.v1";

/// 一次转换中明确记录的能力损失，供诊断与界面展示。
///
/// 不允许“静默降级”：丢掉的字段必须在这里出现，并被写入诊断事件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptationLoss {
    /// 上游协议里没有对应表达的字段或能力。
    pub feature: String,
    pub message_key: String,
    pub detail: String,
}

impl AdaptationLoss {
    pub fn new(feature: &str, message_key: &str, detail: impl Into<String>) -> Self {
        Self {
            feature: feature.to_owned(),
            message_key: message_key.to_owned(),
            detail: detail.into(),
        }
    }
}

/// 上游调用的准备结果：目标地址、鉴权头与请求体。
///
/// 上游 Key 只出现在 `headers` 里，永不写入请求体、日志或错误详情。
pub struct PreparedRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub losses: Vec<AdaptationLoss>,
}

/// 一次请求要执行的模型策略。全部来自发布时冻结的路由快照，
/// 不在请求期读取当前表单值——否则热改表单会改变在途请求的行为。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteLimits {
    /// 该模型声明的输出上限。宿主请求更小时取宿主的，更大时收口到这里。
    pub output_limit: Option<u64>,
    /// 声明可用的思考档位。非空表示该模型已声明档位且映射已版本化。
    pub reasoning_efforts: Vec<String>,
    /// 该模型是否声明了上游自己的服务端内置工具（`web_search` 等）。
    ///
    /// 默认未知＝不转发：宿主发来的内置工具在这个模型上没有依据，而把一件上游没实现的
    /// 功能转过去，代价不是「能力少一点」而是整条请求被上游 400 拒掉。
    pub builtin_tools: Support,
}

impl RouteLimits {
    /// 内置工具是否允许转发。只有明确声明支持才转发。
    pub fn forwards_builtin_tools(&self) -> bool {
        self.builtin_tools == Support::Supported
    }

    /// 收口输出上限：返回实际要发送的值，以及是否因为模型策略被下调。
    pub fn clamp_output_limit(&self, requested: Option<u64>) -> (Option<u64>, bool) {
        match (requested, self.output_limit) {
            (Some(requested), Some(limit)) if requested > limit => (Some(limit), true),
            (None, Some(limit)) => (Some(limit), false),
            (requested, _) => (requested, false),
        }
    }

    /// 该档位是否属于已声明集合。未声明任何档位时一律为 false。
    pub fn allows_effort(&self, effort: &str) -> bool {
        self.reasoning_efforts.iter().any(|value| value == effort)
    }
}

/// 请求里实际用到的输入模态。只认宿主会真的发送内容的那几种。
pub fn requested_modalities(request: &serde_json::Value) -> Vec<&'static str> {
    let mut found = Vec::new();
    let Some(items) = request.get("input").and_then(|input| input.as_array()) else {
        return found;
    };
    for item in items {
        let Some(parts) = item.get("content").and_then(|content| content.as_array()) else {
            continue;
        };
        for part in parts {
            let modality = match part.get("type").and_then(|kind| kind.as_str()) {
                Some("input_image") | Some("image_url") => "image",
                Some("input_audio") | Some("audio") => "audio",
                Some("input_file") | Some("input_pdf") => "pdf",
                _ => continue,
            };
            if !found.contains(&modality) {
                found.push(modality);
            }
        }
    }
    found
}

/// 请求用到、但该模型未声明可原生发送的模态。
///
/// 非空时必须拒绝：把图片悄悄塞给一个只声明文本的模型属于模态虚报，
/// 也等于偷偷借用了另一个模型的能力。
pub fn unsupported_modalities(request: &serde_json::Value, native: &[String]) -> Vec<&'static str> {
    requested_modalities(request)
        .into_iter()
        .filter(|modality| !native.iter().any(|value| value == modality))
        .collect()
}

/// 一个待写出的 Responses SSE 事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsesEvent {
    pub name: &'static str,
    pub payload: serde_json::Value,
}

impl ResponsesEvent {
    pub fn new(name: &'static str, payload: serde_json::Value) -> Self {
        Self { name, payload }
    }

    /// SSE 文本帧。事件名与数据分两行，空行结束。
    pub fn to_frame(&self) -> String {
        format!("event: {}\ndata: {}\n\n", self.name, self.payload)
    }
}

/// 上游流里出现的一帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamFrame {
    /// 解析出的事件（`data:` 的 JSON）。
    Data(serde_json::Value),
    /// 上游显式结束标记（`[DONE]`）。
    Done,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn image_request() -> serde_json::Value {
        json!({
            "input": [{"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "看这张图"},
                {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}
            ]}]
        })
    }

    #[test]
    fn requested_modalities_are_detected_from_content_parts() {
        assert_eq!(requested_modalities(&image_request()), vec!["image"]);
        assert!(requested_modalities(&json!({"input": []})).is_empty());
        assert!(requested_modalities(&json!({})).is_empty());
        assert!(requested_modalities(&json!({"input": [{"type": "message"}]})).is_empty());
    }

    #[test]
    fn a_text_only_model_refuses_image_input_instead_of_borrowing_another_model() {
        let unsupported = unsupported_modalities(&image_request(), &["text".to_owned()]);
        assert_eq!(unsupported, vec!["image"], "必须显式拒绝，不能转发");
    }

    #[test]
    fn declared_modalities_pass_and_are_deduplicated() {
        let native = vec!["text".to_owned(), "image".to_owned()];
        assert!(unsupported_modalities(&image_request(), &native).is_empty());

        let twice = json!({"input": [
            {"type": "message", "content": [{"type": "input_image", "image_url": "a"}]},
            {"type": "message", "content": [{"type": "input_image", "image_url": "b"}]}
        ]});
        assert_eq!(requested_modalities(&twice), vec!["image"]);
    }

    #[test]
    fn text_only_requests_are_never_blocked() {
        let text = json!({"input": [{"type": "message", "content": [{"type": "input_text", "text": "hi"}]}]});
        assert!(unsupported_modalities(&text, &[]).is_empty());
    }
}
