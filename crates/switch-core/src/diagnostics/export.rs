//! 诊断包：先预览，再落盘。
//!
//! 预览必须让用户在保存前看到三件事：包含什么、排除了什么、有多大。
//! 导出内容与预览用的是同一个构建函数，避免“预览一套、实际另一套”。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{DiagnosticEvent, DiagnosticLog};
use crate::domain::error::CoreError;

/// 诊断包格式标识。升级时递增，读取方据此判断兼容性。
pub const EXPORT_SCHEMA: &str = "gptswitch.diagnostics/1";

/// 预览里的一项：要么会被写入，要么被明确排除。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub name: String,
    pub included: bool,
    pub note: String,
}

/// 诊断包预览。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreview {
    pub items: Vec<PreviewItem>,
    pub total_bytes: usize,
    pub events: usize,
}

/// 组装诊断包字节。`scopes` 为空表示包含全部类别。
pub fn build_export(
    log: &DiagnosticLog,
    scopes: &[String],
    app_version: &str,
) -> Result<Vec<u8>, CoreError> {
    let events: Vec<DiagnosticEvent> = log
        .list(None)
        .into_iter()
        .filter(|event| event.in_scope(scopes))
        .collect();
    let document = serde_json::json!({
        "schema": EXPORT_SCHEMA,
        "generated_at": super::now_rfc3339(),
        "app_version": app_version,
        "scopes": scopes,
        "retention": {
            "days": super::RETENTION_DAYS,
            "max_bytes": super::MAX_RETAINED_BYTES,
        },
        "redaction": {
            "mode": "allowlist",
            "metadata_keys": super::ALLOWED_METADATA_KEYS,
            "excluded": [
                "prompt 与 completion 正文",
                "工具调用参数与结果",
                "文件内容",
                "Authorization 与 cookie",
                "完整 URL query",
                "上游 API Key 与网关令牌",
            ],
        },
        "dropped_events": log.dropped(),
        "events": events,
    });
    serde_json::to_vec_pretty(&document).map_err(|_| CoreError::internal("诊断包序列化失败"))
}

/// 预览：列出包含项、被排除项与准确体积。
///
/// 体积由真实构建结果测得，不用估算——用户看到多少，保存下来就是多少。
pub fn preview_export(
    log: &DiagnosticLog,
    scopes: &[String],
    app_version: &str,
) -> Result<ExportPreview, CoreError> {
    let bytes = build_export(log, scopes, app_version)?;
    let events = log
        .list(None)
        .into_iter()
        .filter(|event| event.in_scope(scopes))
        .count();
    let scope_note = if scopes.is_empty() {
        "全部类别".to_owned()
    } else {
        format!("仅 {} 类别", scopes.join("、"))
    };
    let mut items = vec![
        PreviewItem {
            name: "diagnostics.json".to_owned(),
            included: true,
            note: format!(
                "{} 条事件，{}（共 {} 字节）",
                events,
                scope_note,
                bytes.len()
            ),
        },
        PreviewItem {
            name: "上游 API Key 与网关令牌".to_owned(),
            included: false,
            note: "永不写入：只记录凭据引用与版本号".to_owned(),
        },
        PreviewItem {
            name: "请求与响应正文".to_owned(),
            included: false,
            note: "不收集 prompt、completion 与工具参数".to_owned(),
        },
        PreviewItem {
            name: "Codex 配置内容".to_owned(),
            included: false,
            note: "只记录受管键路径与摘要，不含值".to_owned(),
        },
        PreviewItem {
            name: "原始配置备份".to_owned(),
            included: false,
            note: "可能含第三方工具写入的密钥，单独权限保护，不混入诊断包".to_owned(),
        },
    ];
    let excluded: usize = log
        .list(None)
        .into_iter()
        .filter(|event| !event.in_scope(scopes))
        .count();
    if excluded > 0 {
        items.push(PreviewItem {
            name: "范围外事件".to_owned(),
            included: false,
            note: format!("{excluded} 条事件不在本次范围内，未包含"),
        });
    }
    if log.dropped() > 0 {
        items.push(PreviewItem {
            name: "已丢弃事件".to_owned(),
            included: false,
            note: format!(
                "{} 条事件超出保留上限已被丢弃，诊断包只能反映仍保留的部分",
                log.dropped()
            ),
        });
    }
    Ok(ExportPreview {
        items,
        total_bytes: bytes.len(),
        events,
    })
}

/// 写入诊断包，返回写入路径与字节数。
pub fn write_export(
    log: &DiagnosticLog,
    scopes: &[String],
    app_version: &str,
    path: &Path,
) -> Result<(PathBuf, usize), CoreError> {
    let bytes = build_export(log, scopes, app_version)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| CoreError::internal("无法创建诊断包目录"))?;
    }
    std::fs::write(path, &bytes).map_err(|_| CoreError::internal("无法写入诊断包"))?;
    Ok((path.to_path_buf(), bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticEvent, LogLevel};

    fn populated() -> DiagnosticLog {
        let log = DiagnosticLog::default();
        log.record(DiagnosticEvent::new(
            "2026-09-18T00:00:00Z",
            LogLevel::Info,
            "gateway",
            "gs/p_a/m_1",
            "result.ok",
        ));
        log.record(DiagnosticEvent::new(
            "2026-09-18T00:00:01Z",
            LogLevel::Error,
            "apply",
            "inst_1",
            "error.configChanged",
        ));
        log
    }

    #[test]
    fn preview_states_what_is_included_and_what_is_never_collected() {
        let preview = preview_export(&populated(), &[], "0.1.0").unwrap();

        let included: Vec<&PreviewItem> =
            preview.items.iter().filter(|item| item.included).collect();
        assert_eq!(included.len(), 1);
        assert_eq!(included[0].name, "diagnostics.json");
        assert!(included[0].note.contains("2 条事件"));

        let notes: Vec<&str> = preview
            .items
            .iter()
            .map(|item| item.note.as_str())
            .collect();
        assert!(notes.iter().any(|note| note.contains("永不写入")));
        assert!(notes.iter().any(|note| note.contains("prompt")));
        assert_eq!(preview.events, 2);
    }

    #[test]
    fn preview_size_matches_the_actual_export() {
        let log = populated();
        let preview = preview_export(&log, &[], "0.1.0").unwrap();
        let bytes = build_export(&log, &[], "0.1.0").unwrap();

        assert_eq!(
            preview.total_bytes,
            bytes.len(),
            "预览体积必须等于真实导出体积"
        );
    }

    #[test]
    fn scope_filters_events_and_is_visible_in_the_preview() {
        let preview = preview_export(&populated(), &["gateway".to_owned()], "0.1.0").unwrap();
        assert_eq!(preview.events, 1);
        assert!(preview
            .items
            .iter()
            .any(|item| item.name == "范围外事件" && !item.included));

        let bytes = build_export(&populated(), &["gateway".to_owned()], "0.1.0").unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("result.ok"));
        assert!(!text.contains("error.configChanged"), "范围外事件不得出现");
    }

    #[test]
    fn export_declares_its_redaction_rules() {
        let text = String::from_utf8(build_export(&populated(), &[], "0.1.0").unwrap()).unwrap();
        let document: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(document["schema"], EXPORT_SCHEMA);
        assert_eq!(document["redaction"]["mode"], "allowlist");
        assert!(document["redaction"]["metadata_keys"].is_array());
        assert!(document["redaction"]["excluded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry.as_str().unwrap().contains("API Key")));
    }

    #[test]
    fn export_never_contains_a_gateway_token_or_upstream_key() {
        let log = DiagnosticLog::default();
        log.record(
            DiagnosticEvent::new(
                "2026-09-18T00:00:00Z",
                LogLevel::Error,
                "gateway",
                "alias",
                "error.upstreamFailed",
            )
            .with_metadata("error_code", format!("bad key {}", "b".repeat(64))),
        );
        let text = String::from_utf8(build_export(&log, &[], "0.1.0").unwrap()).unwrap();
        assert!(!text.contains(&"b".repeat(64)));
        assert!(text.contains("••••"));
    }

    /// Canary：一条**无前缀**的供应商密钥穿过整条导出链路，不能出现在诊断包里。
    ///
    /// 这条走的是完整路径（allowlist 键 → 脱敏 → 序列化 → 打包），所以它拦的是
    /// 「规则漏了一种形态」这类回归，而不只是单个函数的行为。
    #[test]
    fn export_never_contains_an_unprefixed_provider_key() {
        const CANARY: &str = "Qw3Er5Ty7Ui9Op1As3Df5Gh7Jk9Lz2Xc";
        let log = DiagnosticLog::default();
        log.record(
            DiagnosticEvent::new(
                "2026-09-18T00:00:00Z",
                LogLevel::Error,
                "gateway",
                "alias",
                "error.upstreamFailed",
            )
            .with_metadata("error_code", format!("upstream echoed {CANARY}"))
            .with_metadata("model_id", format!("model-{CANARY}")),
        );

        let text = String::from_utf8(build_export(&log, &[], "0.1.0").unwrap()).unwrap();
        assert!(!text.contains(CANARY), "无前缀密钥不得出现在诊断包里");
        // 脱敏要留下痕迹，而不是整条抹掉：诊断仍然要能看出「这里有过一个值」。
        assert!(text.contains("••••"));
        assert!(text.contains("upstream echoed"));
    }

    #[test]
    fn write_export_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("diagnostics.json");
        let (written, size) = write_export(&populated(), &[], "0.1.0", &path).unwrap();

        assert_eq!(written, path);
        assert!(size > 0);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(EXPORT_SCHEMA));
    }
}
