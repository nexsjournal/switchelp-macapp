//! Responses 透传适配器。
//!
//! 上游本身就支持 Responses 时不做任何结构改写，只替换 `model` 为上游精确 ID，
//! 并把响应里的身份改写回 alias。少改一处就少一处损失。

use super::{AdaptationLoss, PreparedRequest, RouteLimits};
use crate::domain::error::CoreError;

/// 上游 Responses 端点。`base` 是供应商配置里的 API 地址（形如 `https://host/v1`）。
pub fn endpoint(base: &str) -> String {
    format!("{}/responses", base.trim_end_matches('/'))
}

/// 准备透传请求：只替换 `model`，其余字段原样保留。
pub fn prepare(
    base: &str,
    upstream_id: &str,
    request: &serde_json::Value,
    limits: &RouteLimits,
) -> Result<PreparedRequest, CoreError> {
    let mut body = request.clone();
    let mut losses = Vec::new();
    let object = body
        .as_object_mut()
        .ok_or_else(|| CoreError::validation("请求体必须是 JSON 对象"))?;
    object.insert(
        "model".to_owned(),
        serde_json::Value::String(upstream_id.to_owned()),
    );

    // 输出上限同样要按模型策略收口，透传不等于放任。
    let requested = object
        .get("max_output_tokens")
        .and_then(|value| value.as_u64());
    let (effective, capped) = limits.clamp_output_limit(requested);
    if let Some(limit) = effective {
        object.insert("max_output_tokens".to_owned(), serde_json::json!(limit));
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

    // Responses 原生支持 reasoning.effort，但“原生支持”不等于“声明过就能发”。
    //
    // 声明集合为空 = 这个模型**什么都没声明**，此时整个 `reasoning` 都不能转发：宿主在没有
    // 档位可选时会送一个 `none`（本机实测），转发出去就是替模型虚报能力——真实上游直接拒绝：
    // moonshot 的 Responses 接口回 400 `reasoning.effort value "none" is not supported`。
    // 从前这里只在「声明过、但档位不在集合里」时摘除，于是空集合这条最该摘的路径反而漏了。
    if limits.reasoning_efforts.is_empty() {
        if object.remove("reasoning").is_some() {
            losses.push(AdaptationLoss::new(
                "reasoning.effort",
                "loss.reasoningEffortNotDeclared",
                "该模型未声明任何思考档位，宿主请求的 reasoning 字段未转发".to_owned(),
            ));
        }
    } else if let Some(effort) = object
        .get("reasoning")
        .and_then(|value| value.get("effort"))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
    {
        if !limits.allows_effort(&effort) {
            object.remove("reasoning");
            losses.push(AdaptationLoss::new(
                "reasoning.effort",
                "loss.reasoningEffortOutOfRange",
                format!(
                    "思考档位 {} 不在该模型声明的集合 {:?} 内，已摘掉该字段",
                    effort, limits.reasoning_efforts
                ),
            ));
        }
    }

    let bytes = serde_json::to_vec(&body).map_err(|_| CoreError::internal("请求体序列化失败"))?;
    Ok(PreparedRequest {
        url: endpoint(base),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: bytes,
        losses,
    })
}

/// 把响应或其事件里的 `model` 改写回 alias。
///
/// 客户端只应看到自己请求的身份：上游的真实模型 ID 属于网关内部路由信息，
/// 泄漏出去会让续接与诊断对不上。
pub fn rewrite_model(payload: &mut serde_json::Value, alias: &str) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    if object.contains_key("model") {
        object.insert(
            "model".to_owned(),
            serde_json::Value::String(alias.to_owned()),
        );
    }
    if let Some(response) = object.get_mut("response") {
        rewrite_model(response, alias);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_trims_trailing_slash() {
        assert_eq!(endpoint("https://host/v1"), "https://host/v1/responses");
        assert_eq!(endpoint("https://host/v1/"), "https://host/v1/responses");
    }

    #[test]
    fn prepare_replaces_only_the_model_field() {
        let request = json!({
            "model": "gs/p_a/m_1",
            "stream": true,
            "input": [{"type": "message", "role": "user"}],
            "reasoning": {"effort": "low"},
        });
        let prepared = prepare(
            "https://host/v1",
            "vendor/Model-X",
            &request,
            // 声明了 low：这条测的是「除 model 之外原样透传」，所以档位要在声明集合里
            // （未声明的档位会被摘掉，那是另一条测试的事）。
            &RouteLimits {
                output_limit: None,
                reasoning_efforts: vec!["low".to_owned()],
            },
        )
        .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(body["model"], "vendor/Model-X");
        assert_eq!(body["stream"], true);
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["reasoning"]["effort"], "low");
        assert!(prepared.losses.is_empty(), "透传不应产生损失记录");
    }

    #[test]
    fn prepare_rejects_non_object_bodies() {
        assert!(prepare(
            "https://host/v1",
            "m",
            &json!([1, 2]),
            &RouteLimits::default()
        )
        .is_err());
    }

    #[test]
    fn rewrite_hides_the_upstream_identity_in_both_shapes() {
        // 非流式响应：model 在顶层。
        let mut bare = json!({"id": "resp_1", "model": "vendor/Model-X", "output": []});
        rewrite_model(&mut bare, "gs/p_a/m_1");
        assert_eq!(bare["model"], "gs/p_a/m_1");

        // 流式事件：model 在嵌套的 response 里。
        let mut event =
            json!({"type": "response.completed", "response": {"model": "vendor/Model-X"}});
        rewrite_model(&mut event, "gs/p_a/m_1");
        assert_eq!(event["response"]["model"], "gs/p_a/m_1");

        // 没有 model 字段的载荷保持不变。
        let mut other = json!({"type": "response.output_text.delta", "delta": "hi"});
        rewrite_model(&mut other, "gs/p_a/m_1");
        assert_eq!(other["delta"], "hi");
    }
}

#[cfg(test)]
mod reasoning_tests {
    use super::super::RouteLimits;
    use super::*;
    use serde_json::json;

    fn limits(efforts: &[&str]) -> RouteLimits {
        RouteLimits {
            output_limit: None,
            reasoning_efforts: efforts.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    fn request(effort: &str) -> serde_json::Value {
        json!({"model": "gs/x", "input": [], "reasoning": {"effort": effort}})
    }

    #[test]
    fn a_declared_effort_is_forwarded_as_is() {
        let prepared = prepare(
            "https://host/v1",
            "vendor/M",
            &request("high"),
            &limits(&["low", "high"]),
        )
        .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();
        assert_eq!(body["reasoning"]["effort"], "high");
        assert!(prepared.losses.is_empty());
    }

    #[test]
    fn an_undeclared_effort_is_removed_rather_than_forwarded() {
        let prepared = prepare(
            "https://host/v1",
            "vendor/M",
            &request("max"),
            &limits(&["low", "high"]),
        )
        .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();

        assert!(body.get("reasoning").is_none(), "未声明的档位不得转发");
        assert_eq!(prepared.losses.len(), 1);
        assert_eq!(
            prepared.losses[0].message_key,
            "loss.reasoningEffortOutOfRange"
        );
    }

    #[test]
    fn a_model_that_declares_nothing_gets_no_reasoning_field_at_all() {
        // 曾经这里选择「透传，由用户自行承担」。那是错的：宿主在没有档位可选时**自己**会送
        // 一个 `none`，而真实上游会直接拒绝它（moonshot：`reasoning.effort value "none" is
        // not supported`），用户看到的是一句与「我没配过思考档位」毫无关系的 400。
        // 没声明 = 不发，并如实记为损失。
        for effort in ["none", "high"] {
            let prepared = prepare(
                "https://host/v1",
                "vendor/M",
                &request(effort),
                &RouteLimits::default(),
            )
            .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();
            assert!(
                body.get("reasoning").is_none(),
                "未声明档位的模型不得收到 reasoning 字段（effort={effort}）"
            );
            assert_eq!(
                prepared.losses[0].message_key, "loss.reasoningEffortNotDeclared",
                "摘掉要说出来，不能悄悄丢"
            );
        }
    }

    #[test]
    fn reasoning_without_an_effort_is_also_dropped_when_nothing_is_declared() {
        // `reasoning.summary` 之类同样属于「未声明就不转发」：我们并不知道这个上游认不认它。
        let prepared = prepare(
            "https://host/v1",
            "vendor/M",
            &json!({"model": "gs/x", "input": [], "reasoning": {"summary": "auto"}}),
            &RouteLimits::default(),
        )
        .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();
        assert!(body.get("reasoning").is_none());
        assert_eq!(
            prepared.losses[0].message_key,
            "loss.reasoningEffortNotDeclared"
        );
    }

    #[test]
    fn a_model_with_no_reasoning_in_the_request_is_left_alone() {
        // 宿主根本没发 reasoning：不该凭空产生一条「损失」。
        let prepared = prepare(
            "https://host/v1",
            "vendor/M",
            &json!({"model": "gs/x", "input": []}),
            &RouteLimits::default(),
        )
        .unwrap();
        assert!(prepared.losses.is_empty());
    }

    #[test]
    fn the_declared_output_limit_is_applied_to_passthrough_too() {
        let mut request = request("low");
        request["max_output_tokens"] = json!(64_000);
        let prepared = prepare(
            "https://host/v1",
            "vendor/M",
            &request,
            &RouteLimits {
                output_limit: Some(4_096),
                reasoning_efforts: vec!["low".to_owned()],
            },
        )
        .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(
            body["max_output_tokens"], 4096,
            "透传也必须执行模型声明的上限"
        );
        assert!(prepared
            .losses
            .iter()
            .any(|loss| loss.message_key == "loss.outputLimitCapped"));
    }
}
