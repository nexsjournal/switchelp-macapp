//! 目录编译器：把已选择用于 Codex 的模型编译为版本化宿主目录 JSON。
//!
//! 字段结构对齐本机 Codex app-server 的 `ModelInfo`（见 `.local/schema` 与
//! `scripts/g0/catalog.mjs`）：`slug` / `display_name` / `supported_reasoning_levels`
//! / `context_window` / `input_modalities` 等。
//!
//! 编译器不探测上游、不保存凭据，也不猜测能力：未知项保持未知。

use crate::domain::capability::Support;
use crate::domain::error::{CoreError, ErrorCode};
use crate::domain::model::{HostState, Model};
use crate::domain::tokens::TokenCount;
use serde::{Deserialize, Serialize};

/// 目录 schema 版本。宿主 schema 变化时递增，并要求宿主重载。
pub const CATALOG_SCHEMA_VERSION: u32 = 1;

/// 目录投影的可见性。首发只写 `list`。
pub const DEFAULT_VISIBILITY: &str = "list";
/// 目录投影的 shell 类型，与本机 schema 观测一致。
pub const DEFAULT_SHELL_TYPE: &str = "unified_exec";
/// 有效上下文百分比：本机 schema 使用 95，意思是宿主只用 95% 的窗口。
///
/// 必须是整数：真实 Codex 0.155.0-alpha.2.6 用整型解析该字段，
/// 写成 `95.0` 会让**整个目录反序列化失败并被静默丢弃**（不回退报错，
/// 只回落到内置模型列表）。因此这里不用 `f64`，避免再引入小数序列化。
pub const DEFAULT_EFFECTIVE_CONTEXT_PERCENT: u32 = 95;
/// 未声明上下文时的保守兜底值，仅用于“用户策略值”模板，标记为策略而非真实上限。
pub const CONSERVATIVE_CONTEXT_FALLBACK: u64 = 32_768;

/// 单个推理档位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CatalogReasoningLevel {
    pub effort: String,
    pub description: String,
}

/// 截断策略。本机 schema 为 `{ mode: "tokens", limit: n }`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TruncationPolicy {
    pub mode: String,
    pub limit: u64,
}

/// 目录中的一个模型条目，字段顺序对齐本机 app-server 序列化结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CatalogModelEntry {
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub default_reasoning_level: Option<String>,
    pub supported_reasoning_levels: Vec<CatalogReasoningLevel>,
    pub shell_type: String,
    pub visibility: String,
    pub supported_in_api: bool,
    pub priority: i64,
    pub availability_nux: Option<String>,
    pub upgrade: Option<String>,
    pub base_instructions: String,
    pub support_verbosity: bool,
    pub default_verbosity: Option<String>,
    pub apply_patch_tool_type: Option<String>,
    pub truncation_policy: TruncationPolicy,
    pub context_window: u64,
    pub max_context_window: u64,
    /// 宿主可用上下文百分比。整数：写成小数会让真实 Codex 丢弃整个目录。
    pub effective_context_window_percent: u32,
    pub experimental_supported_tools: Vec<String>,
    pub input_modalities: Vec<String>,
    pub supports_reasoning_summary_parameter: bool,
    pub supports_search_tool: bool,
}

/// 完整目录文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Catalog {
    pub models: Vec<CatalogModelEntry>,
}

/// 编译警告：不阻止生成目录，但必须在 UI 上以“待确认”呈现。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileWarning {
    pub model_id: String,
    pub message_key: String,
    pub detail: String,
}

/// 编译结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledCatalog {
    pub catalog: Catalog,
    pub warnings: Vec<CompileWarning>,
    pub schema_version: u32,
}

impl CompiledCatalog {
    pub fn to_json(&self) -> Result<String, CoreError> {
        serde_json::to_string_pretty(&self.catalog)
            .map_err(|error| CoreError::internal(format!("catalog serialize: {error}")))
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, CoreError> {
        serde_json::to_vec_pretty(&self.catalog)
            .map_err(|error| CoreError::internal(format!("catalog serialize: {error}")))
    }
}

/// 编译选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOptions {
    /// 应用阶段：要求上下文已声明；草稿预览允许未知。
    pub require_context: bool,
    /// 未声明上下文时使用的保守值；`None` 表示不加兜底。
    pub conservative_context_fallback: Option<u64>,
    /// 基础指令，必须由调用方显式提供。
    pub base_instructions: String,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            require_context: false,
            conservative_context_fallback: Some(CONSERVATIVE_CONTEXT_FALLBACK),
            base_instructions: "You are a coding assistant. Follow the user instructions."
                .to_owned(),
        }
    }
}

/// 目录编译器。
pub struct CatalogCompiler;

impl CatalogCompiler {
    /// 编译已纳入 Codex 的模型。
    ///
    /// - 只包含 `in_catalog == true` 且 `lifecycle != Disabled` 的模型。
    /// - alias 必须唯一，否则拒绝编译（不能模糊按模型名路由）。
    /// - 未声明的上下文走保守模板并产生 warning，而不是伪造真实上限。
    /// - pdf / video 永不写入 `input_modalities`。
    pub fn compile(
        models: &[Model],
        options: &CompileOptions,
    ) -> Result<CompiledCatalog, CoreError> {
        let mut alias_seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut warnings: Vec<CompileWarning> = Vec::new();
        let mut entries: Vec<CatalogModelEntry> = Vec::new();

        let mut selected: Vec<&Model> = models
            .iter()
            .filter(|m| {
                m.in_catalog && m.lifecycle != crate::domain::model::ModelLifecycle::Disabled
            })
            .collect();
        selected.sort_by(|a, b| a.catalog_alias.as_str().cmp(b.catalog_alias.as_str()));

        for model in selected {
            if !alias_seen.insert(model.catalog_alias.as_str()) {
                return Err(
                    CoreError::new(ErrorCode::ValidationFailed, "error.duplicateAlias")
                        .with_detail(format!("alias 重复：{}", model.catalog_alias)),
                );
            }
            entries.push(Self::entry(model, options, &mut warnings)?);
        }

        // 目录投影完整性：应用阶段不允许存在未知上下文。
        if options.require_context {
            let unresolved: Vec<String> = warnings
                .iter()
                .filter(|w| w.message_key == "warning.contextUnknown")
                .map(|w| w.model_id.clone())
                .collect();
            if !unresolved.is_empty() {
                return Err(
                    CoreError::new(ErrorCode::ValidationFailed, "error.contextRequired")
                        .with_detail(format!("以下模型缺少上下文声明：{}", unresolved.join("、"))),
                );
            }
        }

        Ok(CompiledCatalog {
            catalog: Catalog { models: entries },
            warnings,
            schema_version: CATALOG_SCHEMA_VERSION,
        })
    }

    fn entry(
        model: &Model,
        options: &CompileOptions,
        warnings: &mut Vec<CompileWarning>,
    ) -> Result<CatalogModelEntry, CoreError> {
        model.validate_draft()?;

        let declared_context: Option<TokenCount> = model.policy.context_limit;
        let context_window = match declared_context {
            Some(context) => context.value(),
            None => match options.conservative_context_fallback {
                Some(fallback) => {
                    warnings.push(CompileWarning {
                        model_id: model.id.as_str().to_owned(),
                        message_key: "warning.contextUnknown".to_owned(),
                        detail: format!(
                            "{} 未声明上下文，暂用保守策略值 {}，不是模型真实上限",
                            model.display_name, fallback
                        ),
                    });
                    fallback
                }
                None => {
                    return Err(CoreError::new(
                        ErrorCode::ValidationFailed,
                        "error.contextRequired",
                    )
                    .with_detail(format!("{} 缺少上下文声明", model.display_name)))
                }
            },
        };

        let levels: Vec<CatalogReasoningLevel> = model
            .policy
            .reasoning
            .catalog_levels()
            .into_iter()
            .map(|effort| CatalogReasoningLevel {
                effort: effort.to_owned(),
                description: effort.to_owned(),
            })
            .collect();

        let default_level = model
            .policy
            .reasoning
            .default_value
            .clone()
            .filter(|value| levels.iter().any(|l| &l.effort == value));

        if model.policy.reasoning.default_value.is_some() && default_level.is_none() {
            warnings.push(CompileWarning {
                model_id: model.id.as_str().to_owned(),
                message_key: "warning.reasoningDefaultNotProjected".to_owned(),
                detail: format!(
                    "{} 的默认思考档位无法投影到宿主目录，将在网关侧固定生效",
                    model.display_name
                ),
            });
        }

        if model.policy.reasoning.support == Support::Supported
            && !model.policy.reasoning.host_selectable()
        {
            warnings.push(CompileWarning {
                model_id: model.id.as_str().to_owned(),
                message_key: "warning.reasoningNotSelectableInHost".to_owned(),
                detail: format!(
                    "{} 的推理控制在 Codex 中不可切换，仅使用网关固定策略",
                    model.display_name
                ),
            });
        }

        if model.policy.context_limit.is_none() && options.require_context {
            // 已在 compile 汇总阶段拒绝，这里保持条目生成的一致性。
        }

        let modalities: Vec<String> = model
            .policy
            .catalog_modalities()
            .into_iter()
            .map(|m| m.to_owned())
            .collect();

        let output_reserve = model.policy.output_limit.map(|o| o.value()).unwrap_or(0);
        let truncation_limit = model
            .policy
            .compact_limit
            .map(|c| c.value())
            .filter(|c| *c > 0)
            .unwrap_or_else(|| {
                // 压缩阈值缺失时用预算建议值，仍保证是正数。
                let suggestion = model
                    .policy
                    .budget_check(0)
                    .compact_suggestion
                    .unwrap_or(context_window);
                suggestion.max(1).min(context_window)
            });

        Ok(CatalogModelEntry {
            slug: model.catalog_alias.as_str().to_owned(),
            display_name: model.display_name.clone(),
            description: format!("由 Switchelp 管理；上游模型 ID：{}", model.upstream_id),
            default_reasoning_level: default_level,
            supported_reasoning_levels: levels,
            shell_type: DEFAULT_SHELL_TYPE.to_owned(),
            visibility: DEFAULT_VISIBILITY.to_owned(),
            supported_in_api: true,
            priority: 0,
            availability_nux: None,
            upgrade: None,
            base_instructions: options.base_instructions.clone(),
            support_verbosity: false,
            default_verbosity: None,
            apply_patch_tool_type: None,
            truncation_policy: TruncationPolicy {
                mode: "tokens".to_owned(),
                limit: truncation_limit,
            },
            context_window,
            max_context_window: context_window,
            effective_context_window_percent: DEFAULT_EFFECTIVE_CONTEXT_PERCENT,
            experimental_supported_tools: Vec::new(),
            input_modalities: modalities,
            supports_reasoning_summary_parameter: false,
            // 恒为 false，与模型的「上游内置工具」声明**不是**同一件事：实测宿主并不按它
            // 决定发不发 `web_search`（本机目录里写着 false，请求里照样带着它），所以
            // 这里不拿它去表达那个声明——写了也管不住，反而像是「已经关掉了联网搜索」。
            // 真正的开关在网关侧（见 protocols::responses 的内置工具摘除）。
            supports_search_tool: false,
            // output_reserve 只用于校验，不进目录；目录没有通用最大输出字段。
            // 保留变量使用避免未使用告警。
        })
        .map(|mut entry| {
            let _ = output_reserve;
            entry.priority = 0;
            entry
        })
    }

    /// 目录发布后的宿主状态：提交后最多到“等待重载”，不能推断为已加载。
    pub fn host_state_after_publish(previous: HostState) -> HostState {
        match previous {
            // 已加载或首次纳入目录都要回到“等待重载”；提交成功不等于宿主已重新加载。
            HostState::Loaded | HostState::NotInCatalog | HostState::PendingApply => {
                HostState::AwaitingReload
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::capability::{InputCapability, InputKind, InputPath, Verification};
    use crate::domain::ids::{CatalogAlias, ModelId, ProviderId};
    use crate::domain::model::ModelLifecycle;
    use crate::domain::reasoning::ReasoningPolicy;

    fn model(id: &str, alias: &str, upstream: &str, in_catalog: bool) -> Model {
        let mut model = Model::draft(
            ModelId::new(id),
            ProviderId::new("p_a"),
            upstream,
            format!("显示名 {id}"),
            CatalogAlias::parse(alias).unwrap(),
            "2026-09-18T00:00:00Z",
        )
        .unwrap();
        model.in_catalog = in_catalog;
        model.lifecycle = ModelLifecycle::Saved;
        model
    }

    #[test]
    fn only_catalog_selected_models_are_compiled() {
        let a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        let b = model("m_2", "gs/p_a/m_2", "vendor/b", false);
        let compiled = CatalogCompiler::compile(&[a, b], &CompileOptions::default()).unwrap();
        assert_eq!(compiled.catalog.models.len(), 1);
        assert_eq!(compiled.catalog.models[0].slug, "gs/p_a/m_1");
    }

    #[test]
    fn disabled_models_are_excluded() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.lifecycle = ModelLifecycle::Disabled;
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        assert!(compiled.catalog.models.is_empty());
    }

    #[test]
    fn duplicate_alias_is_rejected() {
        let a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        let b = model("m_2", "gs/p_a/m_1", "vendor/b", true);
        let error = CatalogCompiler::compile(&[a, b], &CompileOptions::default()).unwrap_err();
        assert_eq!(error.code, ErrorCode::ValidationFailed);
    }

    #[test]
    fn unknown_context_uses_conservative_value_with_warning() {
        let a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        assert_eq!(
            compiled.catalog.models[0].context_window,
            CONSERVATIVE_CONTEXT_FALLBACK
        );
        assert_eq!(compiled.warnings.len(), 1);
        assert_eq!(compiled.warnings[0].message_key, "warning.contextUnknown");
    }

    #[test]
    fn apply_stage_requires_declared_context() {
        let a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        let options = CompileOptions {
            require_context: true,
            ..CompileOptions::default()
        };
        let error = CatalogCompiler::compile(&[a], &options).unwrap_err();
        assert_eq!(error.code, ErrorCode::ValidationFailed);
    }

    #[test]
    fn declared_context_is_projected_verbatim() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.policy.context_limit = TokenCount::parse("128k").unwrap();
        a.policy.output_limit = TokenCount::parse("8192").unwrap();
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        let entry = &compiled.catalog.models[0];
        assert_eq!(entry.context_window, 128_000);
        assert_eq!(entry.max_context_window, 128_000);
        assert_eq!(entry.truncation_policy.mode, "tokens");
        assert!(entry.truncation_policy.limit > 0);
        assert!(compiled.warnings.is_empty());
    }

    #[test]
    fn video_and_pdf_never_reach_input_modalities() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.policy.context_limit = TokenCount::parse("128k").unwrap();
        for entry in a.policy.inputs.iter_mut() {
            if entry.kind == InputKind::Video || entry.kind == InputKind::Pdf {
                entry.upstream = Support::Supported;
                entry.gateway = Support::Supported;
                entry.host = Support::Supported;
                entry.effective_path = InputPath::Native;
            }
        }
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        let modalities = &compiled.catalog.models[0].input_modalities;
        assert!(modalities.contains(&"text".to_owned()));
        assert!(!modalities.contains(&"video".to_owned()));
        assert!(!modalities.contains(&"pdf".to_owned()));
    }

    #[test]
    fn unknown_image_is_not_projected_as_native() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.policy.context_limit = TokenCount::parse("128k").unwrap();
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        assert!(!compiled.catalog.models[0]
            .input_modalities
            .contains(&"image".to_owned()));
    }

    #[test]
    fn verified_image_is_projected() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.policy.context_limit = TokenCount::parse("128k").unwrap();
        for entry in a.policy.inputs.iter_mut() {
            if entry.kind == InputKind::Image {
                *entry = InputCapability::new(
                    InputKind::Image,
                    Support::Supported,
                    Support::Supported,
                    Support::Supported,
                    Verification::Verified,
                );
            }
        }
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        assert!(compiled.catalog.models[0]
            .input_modalities
            .contains(&"image".to_owned()));
    }

    #[test]
    fn reasoning_levels_are_projected_only_when_host_selectable() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.policy.context_limit = TokenCount::parse("128k").unwrap();
        a.policy.reasoning =
            ReasoningPolicy::effort(vec!["low".into(), "high".into()], Some("low".into()));
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        let entry = &compiled.catalog.models[0];
        assert_eq!(entry.supported_reasoning_levels.len(), 2);
        assert_eq!(entry.default_reasoning_level.as_deref(), Some("low"));
    }

    #[test]
    fn toggle_only_reasoning_stays_gateway_side_with_warning() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.policy.context_limit = TokenCount::parse("128k").unwrap();
        a.policy.reasoning = ReasoningPolicy {
            support: Support::Supported,
            control: crate::domain::reasoning::ReasoningControl::Toggle,
            allowed_values: vec!["on".into(), "off".into()],
            default_value: None,
            budget_tokens: None,
            mapping_id: Some("m".into()),
        };
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        assert!(compiled.catalog.models[0]
            .supported_reasoning_levels
            .is_empty());
        assert!(compiled
            .warnings
            .iter()
            .any(|w| w.message_key == "warning.reasoningNotSelectableInHost"));
    }

    #[test]
    fn serialized_catalog_matches_host_field_names() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.policy.context_limit = TokenCount::parse("128k").unwrap();
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&compiled.to_json().unwrap()).unwrap();
        let entry = &json["models"][0];
        for key in [
            "slug",
            "display_name",
            "supported_reasoning_levels",
            "context_window",
            "max_context_window",
            "effective_context_window_percent",
            "input_modalities",
            "truncation_policy",
            "shell_type",
            "visibility",
            "supported_in_api",
        ] {
            assert!(entry.get(key).is_some(), "缺少宿主字段 {key}");
        }
        assert!(entry.get("slug").unwrap().is_string());
    }

    /// 该字段必须序列化成整数。真实 Codex 0.155.0-alpha.2.6 用整型解析它：
    /// 写成 `95.0` 时整个目录无法反序列化，且**不报错、静默回落到内置模型列表**，
    /// 表现为“自定义模型没有出现在原生菜单里”。这是已实测的宿主行为。
    #[test]
    fn effective_context_percent_must_be_serialized_as_an_integer() {
        let mut a = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        a.policy.context_limit = TokenCount::parse("128k").unwrap();
        let compiled = CatalogCompiler::compile(&[a], &CompileOptions::default()).unwrap();
        let text = compiled.to_json().unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert!(
            json["models"][0]["effective_context_window_percent"].is_u64(),
            "该字段必须是整数值，否则真实 Codex 会丢弃整个目录"
        );
        assert!(
            !text.contains("95.0"),
            "不得输出小数形式（例如 95.0）：宿主会静默丢弃整个目录"
        );
    }

    #[test]
    fn entries_are_ordered_by_alias_for_stable_output() {
        let a = model("m_2", "gs/p_a/m_2", "vendor/b", true);
        let b = model("m_1", "gs/p_a/m_1", "vendor/a", true);
        let compiled = CatalogCompiler::compile(&[a, b], &CompileOptions::default()).unwrap();
        let slugs: Vec<&str> = compiled
            .catalog
            .models
            .iter()
            .map(|m| m.slug.as_str())
            .collect();
        assert_eq!(slugs, vec!["gs/p_a/m_1", "gs/p_a/m_2"]);
    }

    #[test]
    fn publish_never_claims_loaded_state() {
        assert_eq!(
            CatalogCompiler::host_state_after_publish(HostState::Loaded),
            HostState::AwaitingReload
        );
        assert_eq!(
            CatalogCompiler::host_state_after_publish(HostState::NotInCatalog),
            HostState::AwaitingReload
        );
        // 首次应用后的 PendingApply 也必须推进到“等待重载”，不能停在“待应用”。
        assert_eq!(
            CatalogCompiler::host_state_after_publish(HostState::PendingApply),
            HostState::AwaitingReload
        );
    }
}
