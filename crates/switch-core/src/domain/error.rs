use serde::{Deserialize, Serialize};

/// 错误分类码，与 `docs/architecture/04-data-and-contracts.md` 的错误契约一一对应。
///
/// Rust 只产出 code 与安全细节；最终中文文案由前端 locale key 决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ConfigChanged,
    ConfigParseFailed,
    KeystoreLocked,
    CredentialMissing,
    ModelPermissionDenied,
    CapabilityUnsupported,
    CatalogSchemaMismatch,
    DesktopReloadRequired,
    RouteMismatch,
    ContinuationBound,
    PortInUse,
    /// 本机网关认证失败（缺失/非法令牌，或来源不被允许）。
    Unauthorized,
    /// 请求体超过运输保护上限。
    RequestTooLarge,
    ValidationFailed,
    NotFound,
    Conflict,
    Internal,
}

impl ErrorCode {
    /// 该错误是否值得让用户直接重试同一动作。
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::ConfigChanged
                | ErrorCode::KeystoreLocked
                | ErrorCode::PortInUse
                | ErrorCode::Internal
        )
    }
}

/// 结构化恢复入口，供 UI 生成唯一推荐动作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAction {
    pub action: String,
    pub message_key: String,
}

impl RecoveryAction {
    pub fn new(action: &str, message_key: &str) -> Self {
        Self {
            action: action.to_owned(),
            message_key: message_key.to_owned(),
        }
    }
}

/// 统一错误结构：`code, messageKey, safeDetails, retryable, recoveryActions, operationId?`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreError {
    pub code: ErrorCode,
    pub message_key: String,
    pub safe_details: Vec<String>,
    pub retryable: bool,
    pub recovery_actions: Vec<RecoveryAction>,
    pub operation_id: Option<String>,
}

impl CoreError {
    pub fn new(code: ErrorCode, message_key: &str) -> Self {
        Self {
            code,
            message_key: message_key.to_owned(),
            safe_details: Vec::new(),
            retryable: code.retryable(),
            recovery_actions: Vec::new(),
            operation_id: None,
        }
    }

    /// 校验失败。`detail` 必须已经过脱敏，只放字段名与规则，不放用户密钥。
    pub fn validation(detail: impl AsRef<str>) -> Self {
        Self::new(ErrorCode::ValidationFailed, "error.validation")
            .with_detail(detail.as_ref().to_owned())
    }

    pub fn not_found(what: impl AsRef<str>) -> Self {
        Self::new(ErrorCode::NotFound, "error.notFound")
            .with_detail(format!("未找到 {}", what.as_ref()))
    }

    pub fn conflict(message_key: &str) -> Self {
        Self::new(ErrorCode::Conflict, message_key)
    }

    pub fn internal(detail: impl AsRef<str>) -> Self {
        Self::new(ErrorCode::Internal, "error.internal").with_detail(detail.as_ref().to_owned())
    }

    pub fn with_detail(mut self, detail: String) -> Self {
        self.safe_details.push(detail);
        self
    }

    pub fn with_recovery(mut self, action: &str, message_key: &str) -> Self {
        self.recovery_actions.push(RecoveryAction::new(action, message_key));
        self
    }

    pub fn with_operation(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message_key)?;
        if !self.safe_details.is_empty() {
            write!(f, " ({})", self.safe_details.join("; "))?;
        }
        Ok(())
    }
}

impl std::error::Error for CoreError {}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        CoreError::internal(format!("io: {}", error.kind()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_follows_code() {
        assert!(CoreError::new(ErrorCode::ConfigChanged, "k").retryable);
        assert!(!CoreError::new(ErrorCode::ValidationFailed, "k").retryable);
    }

    #[test]
    fn serializes_code_as_screaming_snake_case() {
        let json = serde_json::to_value(CoreError::validation("名称不能为空")).unwrap();
        assert_eq!(json["code"], "VALIDATION_FAILED");
        assert_eq!(json["messageKey"], "error.validation");
        assert_eq!(json["safeDetails"][0], "名称不能为空");
        assert_eq!(json["retryable"], false);
    }

    #[test]
    fn recovery_actions_are_structured() {
        let error = CoreError::conflict("error.configChanged")
            .with_recovery("recompare", "action.recompare")
            .with_operation("op_1");
        assert_eq!(error.recovery_actions.len(), 1);
        assert_eq!(error.operation_id.as_deref(), Some("op_1"));
    }
}
