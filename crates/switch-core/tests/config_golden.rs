//! TOML golden corpus 测试：注释、CRLF、内联表、quoted key、Unicode、损坏结构与秘密脱敏。
//!
//! fixture 位于仓库根 `tests/fixtures/config/`，与设计文档的目录规划一致。

use std::path::{Path, PathBuf};
use switch_core::codex::config::{
    self, apply_managed, diff_managed, execute_restore, plan_restore, ConfigSnapshot,
    FieldOwnership, ManagedConfig, ManagedProvider, ProviderAuth, RestoreOutcome,
};
use switch_core::domain::error::ErrorCode;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config")
        .join(name)
}

fn load(name: &str) -> ConfigSnapshot {
    ConfigSnapshot::read(fixture(name)).expect("fixture 应能解析")
}

fn gateway_provider() -> ManagedProvider {
    ManagedProvider {
        base_url: "http://127.0.0.1:18765/i/local-main/c/rev_0007/v1".to_owned(),
        wire_api: "responses".to_owned(),
        auth: ProviderAuth::Command {
            command: "/Applications/GPTSwitch.app/Contents/MacOS/gptswitch-auth".to_owned(),
            timeout_ms: 5000,
            refresh_interval_ms: 300_000,
        },
    }
}

fn managed() -> ManagedConfig {
    ManagedConfig {
        model: Some("gs/p_a/m_1".to_owned()),
        model_provider: Some("gptswitch".to_owned()),
        model_catalog_json: Some("/Users/example/Library/Application Support/GPTSwitch/catalogs/rev_0007/models.json".to_owned()),
        provider: Some(gateway_provider()),
        model_context_window: None,
        model_reasoning_effort: None,
    }
}

#[test]
fn preserves_comments_unknown_fields_and_sections() {
    let snapshot = load("commented.toml");
    let (text, _) = apply_managed(&snapshot, &managed(), &[]).unwrap();

    assert!(text.contains("# 用户自己的注释，必须保留"));
    assert!(text.contains("# 下面这段不能动"));
    assert!(text.contains("[mcp_servers.docs]"));
    assert!(text.contains("docs-server"));
    assert!(text.contains("[projects.\"/Users/example/Code/Ünïcode\"]"));
    assert!(text.contains("trust_level = \"trusted\""));
    // 其他工具的 provider 保留且不被改写。
    assert!(text.contains("other-tool"));
    assert!(text.contains("Other Manager"));
    // 受管字段已更新。
    assert!(text.contains("model = \"gs/p_a/m_1\""));
    assert!(text.contains("model_provider = \"gptswitch\""));
    assert!(text.contains("[model_providers.gptswitch]"));

    // 重新解析后仍是合法 TOML，且无关字段语义不变。
    let reparsed = ConfigSnapshot::parse(fixture("commented.toml"), &text).unwrap();
    assert_eq!(
        reparsed
            .document()
            .get("mcp_servers")
            .and_then(|i| i.get("docs"))
            .and_then(|i| i.get("command"))
            .and_then(|i| i.as_str()),
        Some("npx")
    );
}

#[test]
fn preserves_crlf_line_endings() {
    let snapshot = load("crlf.toml");
    let (text, _) = apply_managed(&snapshot, &managed(), &[]).unwrap();
    assert!(text.contains("\r\n"), "CRLF 应被保留");
    assert!(!text.replace("\r\n", "").contains('\n'), "不应混入裸 LF");
}

#[test]
fn preserves_inline_tables() {
    let snapshot = load("inline-table.toml");
    let (text, _) = apply_managed(&snapshot, &managed(), &[]).unwrap();
    assert!(text.contains("network_access = false"));
    assert!(text.contains("writable_roots"));
}

#[test]
fn preserves_quoted_keys() {
    let snapshot = load("quoted-keys.toml");
    let (text, _) = apply_managed(&snapshot, &managed(), &[]).unwrap();
    // quoted key 语义仍可读回；写入后不产生重复键。
    let reparsed = ConfigSnapshot::parse(fixture("quoted-keys.toml"), &text).unwrap();
    assert_eq!(reparsed.managed_value("model").as_deref(), Some("gs/p_a/m_1"));
}

#[test]
fn preserves_unicode_paths_and_values() {
    let snapshot = load("unicode.toml");
    let (text, _) = apply_managed(&snapshot, &managed(), &[]).unwrap();
    assert!(text.contains("/Users/example/项目 中文"));
    assert!(text.contains("文档 服务"));
    let reparsed = ConfigSnapshot::parse(fixture("unicode.toml"), &text).unwrap();
    assert_eq!(
        reparsed.managed_value("model_catalog_json").as_deref(),
        Some("/Users/example/Library/Application Support/GPTSwitch/catalogs/rev_0007/models.json")
    );
}

#[test]
fn missing_keys_are_recorded_as_absent_baseline() {
    let snapshot = load("missing-keys.toml");
    assert_eq!(snapshot.managed_value("model_provider"), None);
    let (_, ownership) = apply_managed(&snapshot, &managed(), &[]).unwrap();

    let provider_record = ownership
        .iter()
        .find(|o| o.key_path == "model_provider")
        .expect("应记录 model_provider 所有权");
    assert!(!provider_record.baseline_presence);
    assert_eq!(provider_record.baseline_value, None);
    assert_eq!(provider_record.last_written_value.as_deref(), Some("gptswitch"));

    // 应用后还原：原本不存在的键应被删除，而不是写成空值。
    let applied = ConfigSnapshot::parse(fixture("missing-keys.toml"), &apply(&snapshot, &ownership)).unwrap();
    let outcomes = plan_restore(&applied, &ownership);
    let model_provider = outcomes
        .iter()
        .find(|o| o.key_path() == "model_provider")
        .unwrap();
    assert!(matches!(model_provider, RestoreOutcome::Delete { .. }));

    let (restored, _) = execute_restore(&applied, &ownership).unwrap();
    assert!(!restored.contains("model_provider"));
    assert!(!restored.contains("gptswitch"));
    // 无关内容保留。
    assert!(restored.contains("[mcp_servers.docs]"));
}

#[test]
fn restore_detects_external_modification_and_keeps_current_value() {
    let snapshot = load("commented.toml");
    let (applied_text, ownership) = apply_managed(&snapshot, &managed(), &[]).unwrap();

    // 模拟外部工具在本工具写入后又改了默认模型。
    let externally_edited = applied_text.replace("model = \"gs/p_a/m_1\"", "model = \"external-model\"");
    let current = ConfigSnapshot::parse(fixture("commented.toml"), &externally_edited).unwrap();

    let outcomes = plan_restore(&current, &ownership);
    let model = outcomes.iter().find(|o| o.key_path() == "model").unwrap();
    assert!(model.is_conflict(), "外部改动必须进入冲突而不是被覆盖");

    let (restored, _) = execute_restore(&current, &ownership).unwrap();
    assert!(restored.contains("model = \"external-model\""), "冲突字段保留当前值");
    // 未被外部改动的字段仍可安全恢复。
    assert!(!restored.contains("[model_providers.gptswitch]"));
}

#[test]
fn same_revision_applied_twice_is_idempotent() {
    let snapshot = load("commented.toml");
    let (first, ownership) = apply_managed(&snapshot, &managed(), &[]).unwrap();
    let applied = ConfigSnapshot::parse(fixture("commented.toml"), &first).unwrap();
    let (second, _) = apply_managed(&applied, &managed(), &ownership).unwrap();
    assert_eq!(first, second, "重复应用同一 revision 不应产生新的写入");

    let changes = diff_managed(&applied, &managed());
    assert!(
        changes.is_empty(),
        "已应用后差异应为空，实际残留 {:?}",
        changes.iter().map(|c| &c.key_path).collect::<Vec<_>>()
    );
}

#[test]
fn diff_reports_field_level_changes_with_reasons() {
    let snapshot = load("missing-keys.toml");
    let changes = diff_managed(&snapshot, &managed());
    let keys: Vec<&str> = changes.iter().map(|c| c.key_path.as_str()).collect();
    assert!(keys.contains(&"model"));
    assert!(keys.contains(&"model_provider"));
    assert!(keys.contains(&"model_providers.gptswitch"));
    assert!(changes.iter().all(|c| !c.reason_key.is_empty()));
    assert!(changes.iter().all(|c| c.before != c.after));
}

#[test]
fn redacted_preview_masks_secret_bearing_fields() {
    let snapshot = load("secret-bearing.toml");
    let preview = snapshot.redacted_preview();
    assert!(!preview.contains("sk-legacy-canary-0123456789"), "env_key 必须脱敏");
    assert!(!preview.contains("sk-mcp-canary-abcdefghij"), "args 中的 canary 不能被导出");
    assert!(preview.contains("••••••••"));
    // 非秘密字段仍然可读，便于用户核对。
    assert!(preview.contains("https://legacy.example.com/v1"));
}

#[test]
fn rejects_upstream_key_as_env_key_projection() {
    let snapshot = load("missing-keys.toml");
    let mut config = managed();
    config.provider = Some(ManagedProvider {
        base_url: "http://127.0.0.1:18765/v1".to_owned(),
        wire_api: "responses".to_owned(),
        auth: ProviderAuth::EnvKey {
            env_key: "OPENAI_API_KEY=sk-upstream-canary".to_owned(),
        },
    });
    let error = apply_managed(&snapshot, &config, &[]).unwrap_err();
    assert_eq!(error.code, ErrorCode::ValidationFailed);
}

#[test]
fn rejects_non_responses_wire_api() {
    let snapshot = load("commented.toml");
    let mut config = managed();
    config.provider = Some(ManagedProvider {
        wire_api: "chat".to_owned(),
        ..gateway_provider()
    });
    let error = apply_managed(&snapshot, &config, &[]).unwrap_err();
    assert_eq!(error.code, ErrorCode::ValidationFailed);
}

#[test]
fn broken_document_fails_without_panicking() {
    let error = ConfigSnapshot::read(fixture("broken.toml")).unwrap_err();
    assert_eq!(error.code, ErrorCode::ConfigParseFailed);
    assert!(!error.recovery_actions.is_empty());
    // 错误细节只含位置信息，不含用户文件内容。
    assert!(error
        .safe_details
        .iter()
        .all(|d| !d.contains("gpt-5-codex") && !d.contains("mcp_servers")));
}

#[test]
fn duplicate_keys_are_rejected_without_panicking() {
    // 重复键属于损坏结构：必须结构化报错并保留可定位信息，而不是崩溃或静默取值。
    let error = ConfigSnapshot::read(fixture("duplicate-key.toml")).unwrap_err();
    assert_eq!(error.code, ErrorCode::ConfigParseFailed);
    assert!(!error.recovery_actions.is_empty());
    // 错误细节只含位置偏移，不回显用户文件内容。
    assert!(error.safe_details.iter().all(|d| !d.contains("model")));
}

#[test]
fn foreign_manager_detection_does_not_touch_other_providers() {
    let snapshot = load("commented.toml");
    assert_eq!(snapshot.foreign_managers(), vec!["other-tool".to_owned()]);

    let (text, _) = apply_managed(&snapshot, &managed(), &[]).unwrap();
    let applied = ConfigSnapshot::parse(fixture("commented.toml"), &text).unwrap();
    // 本工具 provider 加入后不再被算作外部管理器。
    assert_eq!(applied.foreign_managers(), vec!["other-tool".to_owned()]);
}

#[test]
fn content_hash_changes_and_is_stable() {
    let a = config::hash("model = \"a\"\n");
    let b = config::hash("model = \"a\"\n");
    let c = config::hash("model = \"b\"\n");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn atomic_write_leaves_no_partial_file() {
    let dir = std::env::temp_dir().join(format!("gptswitch-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    config::write_atomic(&path, "model = \"a\"\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "model = \"a\"\n");
    config::write_atomic(&path, "model = \"b\"\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "model = \"b\"\n");
    // 临时文件不残留。
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
        .collect();
    assert!(leftovers.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ownership_records_do_not_store_secrets_verbatim() {
    let snapshot = load("secret-bearing.toml");
    // 只管理非秘密字段；秘密 env_key 不属于本工具的受管键。
    let config = ManagedConfig {
        model: None,
        model_provider: None,
        model_catalog_json: None,
        provider: None,
        model_context_window: None,
        model_reasoning_effort: None,
    };
    let (text, ownership) = apply_managed(&snapshot, &config, &[]).unwrap();
    assert_eq!(text, snapshot.to_text());
    assert!(ownership.is_empty());

    let explicit = vec![FieldOwnership::new("model", snapshot.managed_value("model"))];
    let (_, ownership) = apply_managed(&snapshot, &config, &explicit).unwrap();
    assert!(ownership
        .iter()
        .all(|o| o.baseline_value.as_deref() != Some("sk-legacy-canary-0123456789")));
}

fn apply(snapshot: &ConfigSnapshot, ownership: &[FieldOwnership]) -> String {
    let (text, _) = apply_managed(snapshot, &managed(), ownership).unwrap();
    text
}
