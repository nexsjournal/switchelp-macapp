use super::capability::{InputCapability, InputKind, Support, ToolCapability, Verification};
use super::error::CoreError;
use super::ids::{CatalogAlias, ModelId, ProviderId};
use super::provider::Protocol;
use super::reasoning::ReasoningPolicy;
use super::tokens::{BudgetCheck, TokenCount};
use serde::{Deserialize, Serialize};

/// 模型业务状态（本工具侧）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLifecycle {
    Draft,
    Saved,
    Disabled,
}

/// 宿主状态（Codex 侧），与业务状态分列显示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostState {
    /// 未选择用于 Codex。
    NotInCatalog,
    /// 已保存但尚未应用。
    PendingApply,
    /// 已应用，等待宿主重载。
    AwaitingReload,
    /// 宿主已确认加载。
    Loaded,
    /// 已提交但无法确认宿主已加载。
    LoadUnconfirmed,
}

impl HostState {
    pub fn label_key(self) -> &'static str {
        match self {
            HostState::NotInCatalog => "host.notInCatalog",
            HostState::PendingApply => "host.pendingApply",
            HostState::AwaitingReload => "host.awaitingReload",
            HostState::Loaded => "host.loaded",
            HostState::LoadUnconfirmed => "host.loadUnconfirmed",
        }
    }

    /// 测试通过不会把宿主状态提升为已加载。
    pub fn is_confirmed_loaded(self) -> bool {
        matches!(self, HostState::Loaded)
    }
}

/// 模型能力与请求策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPolicy {
    pub context_limit: Option<TokenCount>,
    pub output_limit: Option<TokenCount>,
    pub compact_limit: Option<TokenCount>,
    pub reasoning: ReasoningPolicy,
    pub inputs: Vec<InputCapability>,
    pub tools: ToolCapability,
}

impl Default for ModelPolicy {
    fn default() -> Self {
        Self {
            context_limit: None,
            output_limit: None,
            compact_limit: None,
            reasoning: ReasoningPolicy::default(),
            inputs: default_inputs(),
            tools: ToolCapability::default(),
        }
    }
}

/// 默认能力记录：文本已声明，其余保持未知。
fn default_inputs() -> Vec<InputCapability> {
    InputKind::ALL
        .iter()
        .map(|kind| match kind {
            InputKind::Text => InputCapability::new(
                *kind,
                Support::Supported,
                Support::Supported,
                Support::Supported,
                Verification::Declared,
            ),
            _ => InputCapability::new(
                *kind,
                Support::Unknown,
                Support::Supported,
                Support::Unknown,
                Verification::Declared,
            ),
        })
        .collect()
}

impl ModelPolicy {
    pub fn input(&self, kind: InputKind) -> Option<&InputCapability> {
        self.inputs.iter().find(|entry| entry.kind == kind)
    }

    pub fn budget_check(&self, safety_margin: u64) -> BudgetCheck {
        super::tokens::check_budget(self.context_limit, self.output_limit, safety_margin)
    }

    /// 能力与预算的一致性检查。`require_context` 用于应用阶段：草稿允许未知。
    pub fn validate(&self, require_context: bool) -> Result<(), CoreError> {
        let mut issues: Vec<String> = Vec::new();

        if require_context && self.context_limit.is_none() {
            issues.push("应用前必须填写上下文窗口".to_owned());
        }
        if let (Some(context), Some(output)) = (self.context_limit, self.output_limit) {
            if output.value() >= context.value() {
                issues.push("最大输出必须小于上下文窗口".to_owned());
            }
        }
        if let Some(compact) = self.compact_limit {
            if let Some(context) = self.context_limit {
                if compact.value() > context.value() {
                    issues.push("压缩阈值不能超过上下文窗口".to_owned());
                }
            }
        }
        if let Err(error) = self.reasoning.validate() {
            issues.extend(error.safe_details);
        }
        if let Err(error) = self.reasoning.check_budget_conflict(self.output_limit) {
            issues.extend(error.safe_details);
        }

        // 未投影的输入类别不能声明为原生可用。
        for entry in &self.inputs {
            if entry.effective_path == super::capability::InputPath::Native
                && !entry.kind.is_host_projectable()
            {
                issues.push(format!(
                    "{} 不能标记为 Codex 原生能力",
                    entry.kind.label_key()
                ));
            }
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(CoreError::validation(issues.join("；")))
        }
    }

    /// 目录投影：`input_modalities` 只包含可投影且通过交集判定的类别。
    pub fn catalog_modalities(&self) -> Vec<&'static str> {
        let mut modalities: Vec<&'static str> = self
            .inputs
            .iter()
            .filter(|entry| entry.can_enable_native())
            .filter_map(|entry| entry.kind.modality_literal())
            .collect();
        modalities.sort_unstable();
        modalities.dedup();
        modalities
    }
}

/// 用户覆盖的分层记录：发现值与用户值分开存储，刷新不覆盖用户值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayeredField<T> {
    /// `/models` 或目录返回的发现值；没有发现结果时为空。
    pub discovered: Option<T>,
    /// 用户手工填写或确认的值。
    pub user_value: Option<T>,
    /// 用户是否显式覆盖了发现值。
    pub overridden: bool,
}

impl<T: Clone + PartialEq> LayeredField<T> {
    pub fn empty() -> Self {
        Self {
            discovered: None,
            user_value: None,
            overridden: false,
        }
    }

    pub fn from_discovered(value: T) -> Self {
        Self {
            discovered: Some(value),
            user_value: None,
            overridden: false,
        }
    }

    pub fn with_user(value: T) -> Self {
        Self {
            discovered: None,
            user_value: Some(value.clone()),
            overridden: true,
        }
    }

    /// 生效值：用户覆盖优先，否则用发现值。
    pub fn effective(&self) -> Option<&T> {
        if self.overridden {
            self.user_value.as_ref()
        } else {
            self.user_value.as_ref().or(self.discovered.as_ref())
        }
    }

    /// 刷新发现结果：只有在用户未覆盖时才更新发现层。
    pub fn refresh(&mut self, discovered: Option<T>) {
        self.discovered = discovered;
        if self.overridden {
            // 保留用户值，仅记录新的发现值用于差异展示。
            return;
        }
        self.user_value = None;
    }

    /// 用户覆盖：保留发现的原始来源，供“恢复来源值”使用。
    pub fn override_with(&mut self, value: T) {
        self.user_value = Some(value);
        self.overridden = true;
    }

    pub fn clear_override(&mut self) {
        self.user_value = None;
        self.overridden = false;
    }

    /// 是否与发现值存在可见冲突（供差异展示）。
    pub fn conflicts_with_discovered(&self) -> bool {
        match (&self.discovered, &self.user_value) {
            (Some(discovered), Some(user)) => self.overridden && discovered != user,
            _ => false,
        }
    }
}

/// 模型实体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: ModelId,
    pub provider_id: ProviderId,
    /// 上游精确 ID：保留大小写、斜杠与 Unicode。
    pub upstream_id: String,
    /// 全局唯一、稳定的目录 alias。
    pub catalog_alias: CatalogAlias,
    pub display_name: String,
    pub protocol_override: Option<Protocol>,
    pub lifecycle: ModelLifecycle,
    pub host_state: HostState,
    pub in_catalog: bool,
    pub policy: ModelPolicy,
    /// 发现层与用户覆盖分层的展示名。
    pub display_name_layer: LayeredField<String>,
    pub capability_revision: u32,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

impl Model {
    pub const DISPLAY_NAME_MAX: usize = 96;
    pub const UPSTREAM_ID_MAX: usize = 256;

    /// 创建模型草稿。alias 由调用方保证唯一。
    pub fn draft(
        id: ModelId,
        provider_id: ProviderId,
        upstream_id: impl Into<String>,
        display_name: impl Into<String>,
        catalog_alias: CatalogAlias,
        now: impl Into<String>,
    ) -> Result<Self, CoreError> {
        let now = now.into();
        let upstream_id = upstream_id.into();
        validate_upstream_id(&upstream_id)?;
        let display_name = display_name.into().trim().to_owned();
        validate_display_name(&display_name)?;

        Ok(Self {
            id,
            provider_id,
            upstream_id: upstream_id.clone(),
            catalog_alias,
            display_name: display_name.clone(),
            protocol_override: None,
            lifecycle: ModelLifecycle::Draft,
            host_state: HostState::NotInCatalog,
            in_catalog: false,
            policy: ModelPolicy::default(),
            display_name_layer: LayeredField {
                discovered: None,
                user_value: Some(display_name),
                overridden: true,
            },
            capability_revision: 1,
            version: 1,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// 用发现结果构造，用户层为空。
    pub fn from_discovery(
        id: ModelId,
        provider_id: ProviderId,
        upstream_id: impl Into<String>,
        catalog_alias: CatalogAlias,
        now: impl Into<String>,
    ) -> Result<Self, CoreError> {
        let now = now.into();
        let upstream_id = upstream_id.into();
        validate_upstream_id(&upstream_id)?;
        let display_name = upstream_id.clone();

        Ok(Self {
            id,
            provider_id,
            upstream_id: upstream_id.clone(),
            catalog_alias,
            display_name: display_name.clone(),
            protocol_override: None,
            lifecycle: ModelLifecycle::Saved,
            host_state: HostState::NotInCatalog,
            in_catalog: false,
            policy: ModelPolicy::default(),
            display_name_layer: LayeredField::from_discovered(display_name),
            capability_revision: 1,
            version: 1,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// 模型展示名的生效值随分层字段变化。
    pub fn sync_display_name(&mut self) {
        if let Some(name) = self.display_name_layer.effective() {
            self.display_name = name.clone();
        }
    }

    /// 上游身份改变时，alias 必须重新生成，避免旧目录指向新模型。
    pub fn rebind_upstream(
        &mut self,
        upstream_id: impl Into<String>,
        alias: CatalogAlias,
    ) -> Result<(), CoreError> {
        let upstream_id = upstream_id.into();
        validate_upstream_id(&upstream_id)?;
        if upstream_id != self.upstream_id {
            self.upstream_id = upstream_id;
            self.catalog_alias = alias;
            self.capability_revision += 1;
            self.version += 1;
            // 上游身份变化后，宿主之前的加载结论失效。
            self.host_state = if self.in_catalog {
                HostState::PendingApply
            } else {
                HostState::NotInCatalog
            };
        }
        Ok(())
    }

    /// 精确 ID 去重键：不做小写归一化，保留原始大小写与 Unicode。
    pub fn identity_key(&self) -> (String, String, Protocol) {
        (
            self.provider_id.as_str().to_owned(),
            self.upstream_id.clone(),
            self.protocol_override.unwrap_or(Protocol::Responses),
        )
    }

    /// 保存草稿：只要求基本身份成立。
    pub fn validate_draft(&self) -> Result<(), CoreError> {
        validate_upstream_id(&self.upstream_id)?;
        validate_display_name(&self.display_name)?;
        Ok(())
    }

    /// 应用前：要求完整可执行。
    pub fn validate_for_apply(&self, require_context: bool) -> Result<(), CoreError> {
        self.validate_draft()?;
        self.policy.validate(require_context)
    }
}

/// 供应商限定名的分隔符，形如 `qiyuan/deepseek-v4.1`。
pub const PROVIDER_QUALIFIER: char = '/';

/// 前缀自身的长度上限：前缀不能长到把模型名挤没。
pub const PROVIDER_QUALIFIER_MAX: usize = 32;

/// 目录里展示的模型名：`供应商/模型名`。
///
/// Codex 的模型菜单是**一份扁平列表**：原生模型与每个供应商的模型排在同一列里。
/// 不带前缀时，两家供应商下的同名模型在菜单里长得一模一样，选错只会表现为
/// 「请求打到了别家」，而用户没有任何线索。前缀就是这条线索。
///
/// 幂等：已经带当前前缀的名字原样返回，重复发现、重复保存不会叠成 `qiyuan/qiyuan/x`。
/// 供应商名为空时不做限定——宁可少一个前缀，也不要写出 `/模型名` 这种名字。
pub fn qualified_display_name(provider_name: &str, model_name: &str) -> String {
    let model_name = model_name.trim();
    let provider = provider_qualifier(provider_name);
    if provider.is_empty() || model_name.is_empty() {
        return model_name.to_owned();
    }
    let prefix = format!("{provider}{PROVIDER_QUALIFIER}");
    if model_name.starts_with(&prefix) {
        return model_name.to_owned();
    }
    let qualified: String = format!("{prefix}{model_name}");
    if qualified.chars().count() <= Model::DISPLAY_NAME_MAX {
        return qualified;
    }
    qualified.chars().take(Model::DISPLAY_NAME_MAX).collect()
}

/// 去掉一次前缀：供应商改名时用它取出模型自己的名字。
///
/// 前缀不匹配就原样返回——宁可少剥一层，也不要剥掉用户自己写进名字里的斜杠。
pub fn strip_provider_qualifier<'a>(provider_name: &str, model_name: &'a str) -> &'a str {
    let provider = provider_qualifier(provider_name);
    if provider.is_empty() {
        return model_name;
    }
    model_name
        .strip_prefix(&format!("{provider}{PROVIDER_QUALIFIER}"))
        .unwrap_or(model_name)
}

/// 前缀里不能出现分隔符本身或控制字符：`a/b` 会让「去掉一层前缀」变成猜谜。
fn provider_qualifier(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c == PROVIDER_QUALIFIER || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .take(PROVIDER_QUALIFIER_MAX)
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_upstream_id(value: &str) -> Result<(), CoreError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CoreError::validation("上游模型 ID 不能为空"));
    }
    if trimmed.len() > Model::UPSTREAM_ID_MAX {
        return Err(CoreError::validation("上游模型 ID 过长"));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(CoreError::validation("上游模型 ID 不能包含控制字符"));
    }
    Ok(())
}

fn validate_display_name(value: &str) -> Result<(), CoreError> {
    if value.is_empty() {
        return Err(CoreError::validation("显示名称不能为空"));
    }
    if value.chars().count() > Model::DISPLAY_NAME_MAX {
        return Err(CoreError::validation(format!(
            "显示名称不能超过 {} 字符",
            Model::DISPLAY_NAME_MAX
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        Model::draft(
            ModelId::new("m_1"),
            ProviderId::new("p_1"),
            "vendor/Model-X",
            "代码模型",
            CatalogAlias::parse("gs/p_1/m_1").unwrap(),
            "2026-09-18T00:00:00Z",
        )
        .unwrap()
    }

    #[test]
    fn qualifies_display_name_with_provider() {
        assert_eq!(
            qualified_display_name("qiyuan", "deepseek-v4.1"),
            "qiyuan/deepseek-v4.1"
        );
    }

    #[test]
    fn qualifying_is_idempotent() {
        let once = qualified_display_name("qiyuan", "deepseek-v4.1");
        assert_eq!(qualified_display_name("qiyuan", &once), once);
    }

    #[test]
    fn qualifying_skips_empty_provider_or_name() {
        assert_eq!(qualified_display_name("", "deepseek-v4.1"), "deepseek-v4.1");
        assert_eq!(
            qualified_display_name("   ", "deepseek-v4.1"),
            "deepseek-v4.1"
        );
        assert_eq!(qualified_display_name("qiyuan", "  "), "");
        // 上游名字里本来就有斜杠时不能被当成已有前缀。
        assert_eq!(
            qualified_display_name("qiyuan", "vendor/model"),
            "qiyuan/vendor/model"
        );
    }

    #[test]
    fn qualifier_keeps_slashes_out_of_the_prefix() {
        // 前缀里混进分隔符后，「去掉一层前缀」就无从判断。
        assert_eq!(
            qualified_display_name("a/b", "m"),
            "a-b/m",
            "供应商名里的斜杠要换掉"
        );
        assert_eq!(strip_provider_qualifier("a/b", "a-b/m"), "m");
    }

    #[test]
    fn strips_only_the_matching_prefix() {
        assert_eq!(
            strip_provider_qualifier("qiyuan", "qiyuan/deepseek-v4.1"),
            "deepseek-v4.1"
        );
        assert_eq!(
            strip_provider_qualifier("qiyuan", "other/deepseek-v4.1"),
            "other/deepseek-v4.1"
        );
        assert_eq!(
            strip_provider_qualifier("", "deepseek-v4.1"),
            "deepseek-v4.1"
        );
    }

    #[test]
    fn qualifying_respects_display_name_limit() {
        let long = "x".repeat(Model::DISPLAY_NAME_MAX);
        let qualified = qualified_display_name("qiyuan", &long);
        assert_eq!(qualified.chars().count(), Model::DISPLAY_NAME_MAX);
        assert!(qualified.starts_with("qiyuan/"));
    }

    #[test]
    fn preserves_upstream_id_case_and_slashes() {
        let model = Model::draft(
            ModelId::new("m"),
            ProviderId::new("p"),
            "Vendor/Model-X_Ünïcode",
            "名称",
            CatalogAlias::parse("gs/p/m").unwrap(),
            "now",
        )
        .unwrap();
        assert_eq!(model.upstream_id, "Vendor/Model-X_Ünïcode");
    }

    #[test]
    fn rejects_blank_or_control_upstream_id() {
        assert!(Model::draft(
            ModelId::new("m"),
            ProviderId::new("p"),
            "   ",
            "名称",
            CatalogAlias::parse("gs/p/m").unwrap(),
            "now",
        )
        .is_err());
        assert!(Model::draft(
            ModelId::new("m"),
            ProviderId::new("p"),
            "bad\u{7}id",
            "名称",
            CatalogAlias::parse("gs/p/m").unwrap(),
            "now",
        )
        .is_err());
    }

    #[test]
    fn identity_key_keeps_case_distinct() {
        let a = Model::draft(
            ModelId::new("m1"),
            ProviderId::new("p"),
            "model/A",
            "A",
            CatalogAlias::parse("gs/p/m1").unwrap(),
            "now",
        )
        .unwrap();
        let b = Model::draft(
            ModelId::new("m2"),
            ProviderId::new("p"),
            "model/a",
            "a",
            CatalogAlias::parse("gs/p/m2").unwrap(),
            "now",
        )
        .unwrap();
        assert_ne!(a.identity_key(), b.identity_key());
    }

    #[test]
    fn draft_validation_allows_unknown_context() {
        let model = model();
        assert!(model.validate_draft().is_ok());
        assert!(model.validate_for_apply(false).is_ok());
        assert!(model.validate_for_apply(true).is_err());
    }

    #[test]
    fn apply_requires_consistent_budget() {
        let mut model = model();
        model.policy.context_limit = TokenCount::parse("128k").unwrap();
        model.policy.output_limit = TokenCount::parse("128k").unwrap();
        assert!(model.validate_for_apply(true).is_err());

        model.policy.output_limit = TokenCount::parse("8192").unwrap();
        assert!(model.validate_for_apply(true).is_ok());
    }

    #[test]
    fn catalog_modalities_exclude_video_and_pdf() {
        let mut model = model();
        model.policy.context_limit = TokenCount::parse("128k").unwrap();
        for entry in model.policy.inputs.iter_mut() {
            if entry.kind == InputKind::Video || entry.kind == InputKind::Pdf {
                entry.upstream = Support::Supported;
                entry.gateway = Support::Supported;
                entry.host = Support::Supported;
                entry.effective_path = super::super::capability::InputPath::Native;
            }
        }
        let modalities = model.policy.catalog_modalities();
        assert!(modalities.contains(&"text"));
        assert!(!modalities.contains(&"video"));
        // 未投影的原生标记必须被校验拦下。
        assert!(model.policy.validate(false).is_err());
    }

    #[test]
    fn unknown_image_stays_out_of_catalog() {
        let model = model();
        let image = model.policy.input(InputKind::Image).unwrap();
        assert_eq!(image.effective_support(), Support::Unknown);
        assert!(!model.policy.catalog_modalities().contains(&"image"));
    }

    #[test]
    fn discovery_refresh_does_not_overwrite_user_override() {
        let mut field = LayeredField::from_discovered("发现名".to_owned());
        field.override_with("用户命名".to_owned());
        field.refresh(Some("新的发现名".to_owned()));
        assert_eq!(field.effective().map(String::as_str), Some("用户命名"));
        assert!(field.conflicts_with_discovered());

        field.clear_override();
        assert_eq!(field.effective().map(String::as_str), Some("新的发现名"));
        assert!(!field.conflicts_with_discovered());
    }

    #[test]
    fn rebinding_upstream_changes_alias_and_invalidates_host_state() {
        let mut model = model();
        model.in_catalog = true;
        model.host_state = HostState::Loaded;
        model
            .rebind_upstream("vendor/other", CatalogAlias::parse("gs/p_1/m_2").unwrap())
            .unwrap();
        assert_eq!(model.catalog_alias.as_str(), "gs/p_1/m_2");
        assert_eq!(model.host_state, HostState::PendingApply);
        assert_eq!(model.capability_revision, 2);
    }
}
