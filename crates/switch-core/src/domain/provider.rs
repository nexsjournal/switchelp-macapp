use super::error::CoreError;
use super::ids::{CredentialId, ProviderId};
use super::url::BaseUrl;
use serde::{Deserialize, Serialize};

/// Codex 侧 wire_api 目前只有 responses；Chat Completions 由网关转译。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Responses,
    ChatCompletions,
}

impl Protocol {
    pub fn wire_api(self) -> &'static str {
        // 宿主一律使用 responses，Chat 在网关内转译，不写进 Codex 配置。
        "responses"
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Protocol::Responses => "protocol.responses",
            Protocol::ChatCompletions => "protocol.chatCompletions",
        }
    }

    /// 适配器实现是否已通过工具调用门禁。未通过时界面标“实验”。
    pub fn is_verified_adapter(self) -> bool {
        matches!(self, Protocol::Responses)
    }
}

/// 认证方式。本地无认证端点仍需经本机网关认证。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    ApiKey,
    None,
}

impl AuthKind {
    pub fn label_key(self) -> &'static str {
        match self {
            AuthKind::ApiKey => "auth.apiKey",
            AuthKind::None => "auth.localNoAuth",
        }
    }

    /// 无认证只对 loopback 有意义；远程端点必须提供 Key。
    pub fn allows_endpoint(self, url: &BaseUrl) -> bool {
        match self {
            AuthKind::ApiKey => true,
            AuthKind::None => url.is_loopback(),
        }
    }
}

/// 供应商预设只填公开 Endpoint 与协议建议，不预填真实 Key。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: Protocol,
    pub notes_key: Option<String>,
}

/// 供应商实体。`display name` 允许重复，列表必须同时展示 endpoint 以区分。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: ProviderId,
    pub name: String,
    pub endpoint: String,
    pub protocol: Protocol,
    pub auth_kind: AuthKind,
    pub preset_id: Option<String>,
    pub active_credential_id: Option<CredentialId>,
    pub enabled: bool,
    pub notes: Option<String>,
    /// optimistic version：并发编辑冲突时返回 CONFlict，不静默覆盖。
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

impl Provider {
    pub const NAME_MAX: usize = 64;
    pub const NOTES_MAX: usize = 500;

    /// 创建草稿。id 与时间戳由调用方注入，便于确定性测试。
    pub fn draft(
        id: ProviderId,
        name: impl Into<String>,
        endpoint: &str,
        protocol: Protocol,
        auth_kind: AuthKind,
        now: impl Into<String>,
    ) -> Result<Self, CoreError> {
        let now = now.into();
        let mut provider = Self {
            id,
            name: name.into(),
            endpoint: endpoint.to_owned(),
            protocol,
            auth_kind,
            preset_id: None,
            active_credential_id: None,
            enabled: true,
            notes: None,
            version: 1,
            created_at: now.clone(),
            updated_at: now,
        };
        provider.validate()?;
        Ok(provider)
    }

    /// 校验名称、URL、认证方式组合与备注长度，并归一化 endpoint。
    pub fn validate(&mut self) -> Result<(), CoreError> {
        let name = self.name.trim().to_owned();
        if name.is_empty() {
            return Err(CoreError::validation("供应商名称不能为空"));
        }
        if name.chars().count() > Self::NAME_MAX {
            return Err(CoreError::validation(format!(
                "供应商名称不能超过 {} 字符",
                Self::NAME_MAX
            )));
        }
        if let Some(notes) = &self.notes {
            if notes.chars().count() > Self::NOTES_MAX {
                return Err(CoreError::validation(format!(
                    "备注不能超过 {} 字符",
                    Self::NOTES_MAX
                )));
            }
        }
        let url = BaseUrl::parse(&self.endpoint)?;
        if !self.auth_kind.allows_endpoint(&url) {
            return Err(CoreError::validation(
                "远程供应商必须使用 API Key；无认证仅适用于本机 loopback",
            ));
        }
        self.name = name;
        self.endpoint = url.normalized().to_owned();
        Ok(())
    }

    pub fn parsed_endpoint(&self) -> Result<BaseUrl, CoreError> {
        BaseUrl::parse(&self.endpoint)
    }

    /// 供应商身份是否因 URL 变化而失效，导致旧探测结果 stale。
    pub fn invalidates_probe(&self, other: &Provider) -> bool {
        self.endpoint != other.endpoint || self.protocol != other.protocol
    }

    /// 乐观并发检查：草稿携带的版本必须等于当前版本。
    pub fn check_version(&self, expected: u64) -> Result<(), CoreError> {
        if self.version != expected {
            return Err(CoreError::conflict("error.providerVersionConflict"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(endpoint: &str, auth: AuthKind) -> Provider {
        Provider::draft(
            ProviderId::new("p_1"),
            "供应商 A",
            endpoint,
            Protocol::Responses,
            auth,
            "2026-09-18T00:00:00Z",
        )
        .unwrap()
    }

    #[test]
    fn normalizes_endpoint_and_trims_name() {
        let mut provider = provider("https://api.example.com/v1/", AuthKind::ApiKey);
        provider.name = "  供应商 A  ".to_owned();
        provider.validate().unwrap();
        assert_eq!(provider.endpoint, "https://api.example.com/v1");
        assert_eq!(provider.name, "供应商 A");
    }

    #[test]
    fn rejects_empty_or_overlong_name() {
        let mut draft = provider("https://api.example.com", AuthKind::ApiKey);
        draft.name = "   ".to_owned();
        assert!(draft.validate().is_err());

        draft.name = "a".repeat(65);
        assert!(draft.validate().is_err());
    }

    #[test]
    fn rejects_no_auth_on_remote_endpoint() {
        assert!(Provider::draft(
            ProviderId::new("p"),
            "本地",
            "https://api.example.com",
            Protocol::Responses,
            AuthKind::None,
            "now",
        )
        .is_err());
        assert!(Provider::draft(
            ProviderId::new("p"),
            "本地",
            "http://127.0.0.1:18765/v1",
            Protocol::Responses,
            AuthKind::None,
            "now",
        )
        .is_ok());
    }

    #[test]
    fn rejects_overlong_notes() {
        let mut draft = provider("https://api.example.com", AuthKind::ApiKey);
        draft.notes = Some("n".repeat(Provider::NOTES_MAX + 1));
        assert!(draft.validate().is_err());
    }

    #[test]
    fn url_change_invalidates_previous_probe() {
        let original = provider("https://api.example.com/v1", AuthKind::ApiKey);
        let changed = provider("https://api.example.com/v2", AuthKind::ApiKey);
        assert!(original.invalidates_probe(&changed));
        assert!(!original.invalidates_probe(&original.clone()));
    }

    #[test]
    fn optimistic_version_conflict_is_reported() {
        let provider = provider("https://api.example.com", AuthKind::ApiKey);
        assert!(provider.check_version(1).is_ok());
        let error = provider.check_version(2).unwrap_err();
        assert_eq!(error.code, super::super::error::ErrorCode::Conflict);
    }

    #[test]
    fn host_wire_api_is_always_responses() {
        assert_eq!(Protocol::ChatCompletions.wire_api(), "responses");
        assert!(!Protocol::ChatCompletions.is_verified_adapter());
        assert!(Protocol::Responses.is_verified_adapter());
    }
}
