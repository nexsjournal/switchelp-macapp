use serde::{Deserialize, Serialize};

/// 能力支持状态：未知不能当作支持，也不能当作不支持。
///
/// 默认值是未知而不是不支持：老数据里没有这一项时，界面要能把它显示成「没声明过」，
/// 而不是替用户下一个「不支持」的结论。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

impl Support {
    /// CSS/文案层需要的稳定 key。
    pub fn key(self) -> &'static str {
        match self {
            Support::Supported => "supported",
            Support::Unsupported => "unsupported",
            Support::Unknown => "unknown",
        }
    }

    /// 交集原则：任一层明确不支持即不支持；任一层未知则未知。
    pub fn intersect(values: &[Support]) -> Support {
        if values.contains(&Support::Unsupported) {
            return Support::Unsupported;
        }
        if values.is_empty() || values.contains(&Support::Unknown) {
            return Support::Unknown;
        }
        Support::Supported
    }
}

/// 证据来源，用于界面上的“服务商返回 / 官方文档 / 用户填写 / 测试确认”。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Provider,
    OfficialDocs,
    Registry,
    User,
    Probe,
}

impl EvidenceSource {
    pub fn label_key(self) -> &'static str {
        match self {
            EvidenceSource::Provider => "source.provider",
            EvidenceSource::OfficialDocs => "source.officialDocs",
            EvidenceSource::Registry => "source.registry",
            EvidenceSource::User => "source.user",
            EvidenceSource::Probe => "source.probe",
        }
    }
}

/// 验证状态。`verified` 只对具体断言成立，一次小样本成功不能升级整项能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verification {
    Declared,
    Verified,
    Failed,
    Stale,
}

/// 业务分类的输入模态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Text,
    Image,
    Audio,
    Video,
    Pdf,
    Document,
}

impl InputKind {
    pub const ALL: [InputKind; 6] = [
        InputKind::Text,
        InputKind::Image,
        InputKind::Audio,
        InputKind::Video,
        InputKind::Pdf,
        InputKind::Document,
    ];

    /// 该输入类别是否允许写入 Codex 目录的 `input_modalities`。
    ///
    /// 本机协议只有 text / image / audio 枚举，没有 pdf / video，
    /// 因此 pdf、video 即使声明支持也不能投影为原生能力。
    pub fn is_host_projectable(self) -> bool {
        matches!(self, InputKind::Text | InputKind::Image | InputKind::Audio)
    }

    /// 对应 Codex 目录里的 `input_modalities` 字面量。
    pub fn modality_literal(self) -> Option<&'static str> {
        match self {
            InputKind::Text => Some("text"),
            InputKind::Image => Some("image"),
            InputKind::Audio => Some("audio"),
            InputKind::Video | InputKind::Pdf | InputKind::Document => None,
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            InputKind::Text => "capability.text",
            InputKind::Image => "capability.image",
            InputKind::Audio => "capability.audio",
            InputKind::Video => "capability.video",
            InputKind::Pdf => "capability.pdf",
            InputKind::Document => "capability.document",
        }
    }
}

/// 附件进入模型的实际路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputPath {
    Native,
    Converted,
    ToolRead,
    Blocked,
}

/// 带来源与验证状态的通用能力值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityValue<T> {
    pub value: Option<T>,
    pub source: EvidenceSource,
    pub source_ref: Option<String>,
    pub observed_at: String,
    pub verification: Verification,
}

impl<T> CapabilityValue<T> {
    pub fn unknown(observed_at: impl Into<String>) -> Self {
        Self {
            value: None,
            source: EvidenceSource::User,
            source_ref: None,
            observed_at: observed_at.into(),
            verification: Verification::Declared,
        }
    }

    pub fn declared(value: T, source: EvidenceSource, observed_at: impl Into<String>) -> Self {
        Self {
            value: Some(value),
            source,
            source_ref: None,
            observed_at: observed_at.into(),
            verification: Verification::Declared,
        }
    }

    /// 用户覆盖是否替换了来源值。
    pub fn is_user_override(&self) -> bool {
        self.source == EvidenceSource::User && self.value.is_some()
    }
}

/// 单项输入能力的完整记录：上游 / 网关 / 宿主三层加有效路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputCapability {
    pub kind: InputKind,
    pub upstream: Support,
    pub gateway: Support,
    pub host: Support,
    pub effective_path: InputPath,
    pub mime_types: Vec<String>,
    pub max_bytes: Option<u64>,
    pub conversion_id: Option<String>,
    pub verification: Verification,
    /// 不可用时的原因 key，界面必须展示而不仅是禁用。
    pub blocked_reason_key: Option<String>,
}

impl InputCapability {
    /// 从三层支持状态推导出最终可用能力。
    pub fn new(
        kind: InputKind,
        upstream: Support,
        gateway: Support,
        host: Support,
        verification: Verification,
    ) -> Self {
        let effective = Support::intersect(&[upstream, gateway, host]);
        let projectable = kind.is_host_projectable();
        let (effective_path, blocked_reason_key) = match (effective, projectable) {
            (Support::Unsupported, _) => (
                InputPath::Blocked,
                Some("capability.reason.upstreamUnsupported".to_owned()),
            ),
            (_, false) => (
                InputPath::Blocked,
                Some("capability.reason.hostCannotSend".to_owned()),
            ),
            (Support::Unknown, _) => (
                InputPath::Blocked,
                Some("capability.reason.unverified".to_owned()),
            ),
            (Support::Supported, true) => (InputPath::Native, None),
        };
        Self {
            kind,
            upstream,
            gateway,
            host,
            effective_path,
            mime_types: Vec::new(),
            max_bytes: None,
            conversion_id: None,
            verification,
            blocked_reason_key,
        }
    }

    /// 是否可以在界面里提供“加入 Codex 原生能力”的开关。
    pub fn can_enable_native(&self) -> bool {
        self.effective_path == InputPath::Native && self.kind.is_host_projectable()
    }

    pub fn effective_support(&self) -> Support {
        Support::intersect(&[self.upstream, self.gateway, self.host])
    }
}

/// 工具调用能力（function tools 与并行调用）单独记录，不与模态混在一起。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapability {
    pub function_tools: Support,
    pub parallel_tools: Support,
    pub custom_tools: Support,
    /// 上游自己执行的服务端内置工具（`web_search`、`file_search`、`code_interpreter` 等）。
    ///
    /// 与 function / custom 工具不是一回事：那两类由宿主（Codex）自己调用并回传结果，
    /// 网关只负责转发；内置工具是**上游的**功能，上游没实现就整条请求被拒。
    /// 实测：第三方网关收到 `{"type":"web_search"}` 直接回 400
    /// `responses_feature_not_supported: tool type 'web_search' is not supported`，
    /// 而这条 400 与用户「我根本没开过联网搜索」的认知完全对不上。
    ///
    /// 所以它不是「有没有这个能力」而是「要不要转发」：未声明（含未知）时不转发，
    /// 并在诊断里记为损失。老数据缺这一项时按未知处理。
    #[serde(default)]
    pub builtin_tools: Support,
    pub verification: Verification,
}

impl Default for ToolCapability {
    fn default() -> Self {
        Self {
            function_tools: Support::Unknown,
            parallel_tools: Support::Unknown,
            custom_tools: Support::Unknown,
            builtin_tools: Support::Unknown,
            verification: Verification::Declared,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_prefers_explicit_unsupported() {
        assert_eq!(
            Support::intersect(&[Support::Supported, Support::Unsupported, Support::Unknown]),
            Support::Unsupported
        );
        assert_eq!(
            Support::intersect(&[Support::Supported, Support::Unknown]),
            Support::Unknown
        );
        assert_eq!(
            Support::intersect(&[Support::Supported, Support::Supported]),
            Support::Supported
        );
        assert_eq!(Support::intersect(&[]), Support::Unknown);
    }

    #[test]
    fn video_and_pdf_are_never_host_projectable() {
        assert!(!InputKind::Video.is_host_projectable());
        assert!(!InputKind::Pdf.is_host_projectable());
        assert_eq!(InputKind::Video.modality_literal(), None);
        assert_eq!(InputKind::Image.modality_literal(), Some("image"));
    }

    #[test]
    fn declared_video_stays_blocked_with_reason() {
        let capability = InputCapability::new(
            InputKind::Video,
            Support::Supported,
            Support::Supported,
            Support::Unknown,
            Verification::Declared,
        );
        assert_eq!(capability.effective_path, InputPath::Blocked);
        assert!(!capability.can_enable_native());
        assert!(capability.blocked_reason_key.is_some());
    }

    #[test]
    fn unknown_image_is_not_reported_as_native() {
        let capability = InputCapability::new(
            InputKind::Image,
            Support::Unknown,
            Support::Supported,
            Support::Supported,
            Verification::Declared,
        );
        assert_eq!(capability.effective_support(), Support::Unknown);
        assert_eq!(capability.effective_path, InputPath::Blocked);
        assert!(!capability.can_enable_native());
    }

    #[test]
    fn fully_supported_text_passes_through_natively() {
        let capability = InputCapability::new(
            InputKind::Text,
            Support::Supported,
            Support::Supported,
            Support::Supported,
            Verification::Verified,
        );
        assert_eq!(capability.effective_path, InputPath::Native);
        assert!(capability.can_enable_native());
        assert_eq!(capability.blocked_reason_key, None);
    }

    #[test]
    fn user_override_is_flagged() {
        let value =
            CapabilityValue::declared(128_000_u64, EvidenceSource::User, "2026-09-18T00:00:00Z");
        assert!(value.is_user_override());
        let unknown: CapabilityValue<u64> = CapabilityValue::unknown("2026-09-18T00:00:00Z");
        assert!(!unknown.is_user_override());
    }
}
