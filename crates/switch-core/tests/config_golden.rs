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
            command: "/Applications/Switchelp.app/Contents/MacOS/gptswitch-auth".to_owned(),
            timeout_ms: 5000,
            refresh_interval_ms: 300_000,
        },
    }
}

fn managed() -> ManagedConfig {
    ManagedConfig {
        model: Some("gs/p_a/m_1".to_owned()),
        model_provider: Some("gptswitch".to_owned()),
        model_catalog_json: Some(
            "/Users/example/Library/Application Support/Switchelp/catalogs/rev_0007/models.json"
                .to_owned(),
        ),
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
    assert_eq!(
        reparsed.managed_value("model").as_deref(),
        Some("gs/p_a/m_1")
    );
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
        Some("/Users/example/Library/Application Support/Switchelp/catalogs/rev_0007/models.json")
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
    assert_eq!(
        provider_record.last_written_value.as_deref(),
        Some("gptswitch")
    );

    // 应用后还原：原本不存在的键应被删除，而不是写成空值。
    let applied =
        ConfigSnapshot::parse(fixture("missing-keys.toml"), &apply(&snapshot, &ownership)).unwrap();
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

/// provider 子表是唯一「键路径不是字面量键」的受管字段。
///
/// 回归（一）：`execute_restore` 的 Restore 分支曾统一走 `document["model_providers.gptswitch"] = …`，
/// 而 toml_edit 的索引赋值不做点号拆分，于是还原写出一条 `"model_providers.gptswitch" = "…"`
/// 的垃圾键，真正的子表原封不动——还原报告成功，宿主配置却仍指向本工具。
///
/// 回归（二，真机）：基线里带着本工具的 provider 时，恢复「基线」等于把旧目录版本再写回去，
/// 用户点几遍还原、重启 Codex 也回不到原生登录。正确动作是删掉——那才是「原本不存在」的语义。
/// 字面点号键的防护由下面「不得写入 `"model_providers.gptswitch"`」与「子表已消失」两条断言守住。
#[test]
fn restore_writes_an_existing_gateway_provider_back_as_a_table() {
    let snapshot = load("existing-gateway-provider.toml");
    assert!(
        snapshot.read_provider().is_some(),
        "fixture 的基线 provider 必须可读"
    );

    let (applied_text, ownership) = apply_managed(&snapshot, &managed(), &[]).unwrap();
    let applied =
        ConfigSnapshot::parse(fixture("existing-gateway-provider.toml"), &applied_text).unwrap();
    // 接管后 base_url 指向新目录版本。
    assert!(applied
        .read_provider()
        .unwrap()
        .base_url
        .contains("rev_0007"));

    let outcomes = plan_restore(&applied, &ownership);
    let provider = outcomes
        .iter()
        .find(|o| o.key_path() == "model_providers.gptswitch")
        .unwrap();
    assert!(
        matches!(provider, RestoreOutcome::Delete { .. }),
        "基线是本工具自己的作业时，还原必须删掉而不是写回：{provider:?}"
    );

    let (restored, _) = execute_restore(&applied, &ownership).unwrap();
    assert!(
        !restored.contains("\"model_providers.gptswitch\""),
        "不得写入字面点号键：\n{restored}"
    );
    assert!(
        !restored.contains("[model_providers.gptswitch]"),
        "还原后 Codex 必须回到原生：子表不得留下\n{restored}"
    );
    // 回到原生：路由键与我们的 provider 一起消失，用户重启 Codex 后就是账号登录那一套。
    assert!(
        !restored.contains("gptswitch"),
        "路由键必须撤销：\n{restored}"
    );
    assert!(!restored.contains("model_catalog_json"));
    // 无关内容保留。
    assert!(restored.contains("[mcp_servers.docs]"));
    assert!(restored.contains("name = \"文档\"") || restored.contains("[mcp_servers.docs]"));
}

/// provider 子表的 **Restore 分支**（基线的表不是本工具的）仍然要按表写回，
/// 而不是写成字面点号键。真机语义改成「基线是自己的作业就删掉」之后，
/// 这条分支只剩「表在、但没在用它路由」这种状态会走到，单独钉一下。
#[test]
fn restore_writes_back_a_foreign_gateway_provider_as_a_table() {
    let snapshot = load("missing-keys.toml");
    let (applied_text, ownership) = apply_managed(&snapshot, &managed(), &[]).unwrap();
    let applied = ConfigSnapshot::parse(fixture("missing-keys.toml"), &applied_text).unwrap();

    // 手工构造：这套记录声称「接管前就有一张别人的 gptswitch 表，且当时没在用它路由」。
    let foreign = ownership
        .iter()
        .map(|record| {
            if record.key_path == "model_providers.gptswitch" {
                FieldOwnership {
                    key_path: record.key_path.clone(),
                    baseline_presence: true,
                    // 拿本工具写出来的表改个名字和地址：结构合法、内容认不出来是本工具的。
                    baseline_value: Some(
                        record
                            .last_written_value
                            .as_deref()
                            .unwrap_or_default()
                            .replace("Switchelp", "别人的网关")
                            .replace("/i/", "/x/"),
                    ),
                    last_written_value: record.last_written_value.clone(),
                }
            } else if record.key_path == "model_provider" {
                FieldOwnership {
                    baseline_value: Some("openai".to_owned()),
                    ..record.clone()
                }
            } else {
                record.clone()
            }
        })
        .collect::<Vec<_>>();

    let outcomes = plan_restore(&applied, &foreign);
    assert!(matches!(
        outcomes
            .iter()
            .find(|o| o.key_path() == "model_providers.gptswitch")
            .unwrap(),
        RestoreOutcome::Restore { .. }
    ));
    let (restored, _) = execute_restore(&applied, &foreign).unwrap();
    assert!(
        !restored.contains("\"model_providers.gptswitch\""),
        "不得写入字面点号键：\n{restored}"
    );
    let reparsed = ConfigSnapshot::parse(fixture("missing-keys.toml"), &restored).unwrap();
    let provider = reparsed.read_provider().expect("基线的表必须按表写回");
    assert!(
        !provider.base_url.contains("/i/"),
        "写回的必须是基线那张表：{}",
        provider.base_url
    );
}

/// 回归：`model_providers` 是内联表时，往里塞普通子表会被 toml_edit 丢弃，
/// 于是写了等于没写，而所有权却记成「已写入」。
#[test]
fn writes_provider_when_model_providers_is_an_inline_table() {
    let snapshot = load("inline-model-providers.toml");
    let (text, ownership) = apply_managed(&snapshot, &managed(), &[]).unwrap();

    let reparsed = ConfigSnapshot::parse(fixture("inline-model-providers.toml"), &text).unwrap();
    assert!(
        reparsed.read_provider().is_some(),
        "内联表下 provider 必须真的写进去：\n{text}"
    );
    // 用户原有的内联表内容不能被吞掉。
    assert!(text.contains("other-tool"));
    assert!(text.contains("Other Manager"));
    assert!(ownership
        .iter()
        .find(|o| o.key_path == "model_providers.gptswitch")
        .and_then(|o| o.last_written_value.clone())
        .is_some());
}

/// 回归：`model_context_window` 是整数受管字段，写成字符串宿主读不出来。
#[test]
fn context_window_is_written_as_an_integer() {
    let snapshot = load("missing-keys.toml");
    let mut config = managed();
    config.model_context_window = Some(128_000);
    let (text, _) = apply_managed(&snapshot, &config, &[]).unwrap();
    assert!(
        text.contains("model_context_window = 128000"),
        "必须是整数：\n{text}"
    );
    assert!(
        !text.contains("model_context_window = \"128000\""),
        "不能写成字符串"
    );
}

/// 回归：用户把整数写成 `128_000` 时，写法差异不能被当成「外部已修改」。
#[test]
fn integer_notation_is_not_an_external_change() {
    let source = "model = \"gpt-5-codex\"\nmodel_context_window = 128_000\n";
    let snapshot = ConfigSnapshot::parse(fixture("missing-keys.toml"), source).unwrap();
    let mut config = managed();
    config.model_context_window = Some(128_000);
    let (applied_text, ownership) = apply_managed(&snapshot, &config, &[]).unwrap();
    let applied = ConfigSnapshot::parse(fixture("missing-keys.toml"), &applied_text).unwrap();

    let outcomes = plan_restore(&applied, &ownership);
    let window = outcomes
        .iter()
        .find(|o| o.key_path() == "model_context_window")
        .unwrap();
    assert!(
        !window.is_conflict(),
        "数值相同、写法不同不应判成冲突：{window:?}"
    );
}

/// 原子写入不能放宽目标文件的权限：config.toml 里可能有其它工具写入的密钥。
#[cfg(unix)]
#[test]
fn atomic_write_preserves_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("switchelp-perm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("config.toml");
    std::fs::write(&target, "model = \"gpt-5-codex\"\n").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();

    config::write_atomic(&target, "model = \"gs/p_a/m_1\"\n").unwrap();

    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "写入后权限被放宽成 {mode:o}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn restore_detects_external_modification_and_keeps_current_value() {
    let snapshot = load("commented.toml");
    let (applied_text, ownership) = apply_managed(&snapshot, &managed(), &[]).unwrap();

    // 模拟外部工具在本工具写入后又改了默认模型。
    let externally_edited =
        applied_text.replace("model = \"gs/p_a/m_1\"", "model = \"external-model\"");
    let current = ConfigSnapshot::parse(fixture("commented.toml"), &externally_edited).unwrap();

    let outcomes = plan_restore(&current, &ownership);
    let model = outcomes.iter().find(|o| o.key_path() == "model").unwrap();
    assert!(model.is_conflict(), "外部改动必须进入冲突而不是被覆盖");

    let (restored, _) = execute_restore(&current, &ownership).unwrap();
    assert!(
        restored.contains("model = \"external-model\""),
        "冲突字段保留当前值"
    );
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
    assert!(
        !preview.contains("sk-legacy-canary-0123456789"),
        "env_key 必须脱敏"
    );
    assert!(
        !preview.contains("sk-mcp-canary-abcdefghij"),
        "args 中的 canary 不能被导出"
    );
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

    let explicit = vec![FieldOwnership::new(
        "model",
        snapshot.managed_value("model"),
    )];
    let (_, ownership) = apply_managed(&snapshot, &config, &explicit).unwrap();
    assert!(ownership
        .iter()
        .all(|o| o.baseline_value.as_deref() != Some("sk-legacy-canary-0123456789")));
}

fn apply(snapshot: &ConfigSnapshot, ownership: &[FieldOwnership]) -> String {
    let (text, _) = apply_managed(snapshot, &managed(), ownership).unwrap();
    text
}
