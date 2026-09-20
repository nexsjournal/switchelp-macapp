//! Codex 实例检测。
//!
//! 规则来自 [配置生命周期](../../../../docs/architecture/02-configuration-lifecycle.md)：
//! 必须用应用身份与二进制探测，不能只找 `Codex.app`；终端里的 `$CODEX_HOME` 不能
//! 直接当成 GUI 实例的值；多个实例必须让用户选择，不能自动挑“正在运行的那个”。

use crate::domain::error::CoreError;
use crate::domain::ids::InstanceId;
use crate::domain::version::{CompatibilityStatus, VersionFingerprint};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 已知的 Desktop bundle 标识。本机实测为 ChatGPT.app + `com.openai.codex`。
pub const KNOWN_BUNDLE_IDS: [&str; 2] = ["com.openai.codex", "com.openai.chatgpt"];
/// macOS 常见安装位置候选。
pub const MACOS_APP_CANDIDATES: [&str; 3] = [
    "/Applications/ChatGPT.app",
    "/Applications/Codex.app",
    "/Applications/OpenAI Codex.app",
];
/// bundle 内 CLI 相对路径。
pub const BUNDLE_CLI_RELATIVE: &str = "Contents/Resources/codex";
/// 配置根目录名。
pub const CONFIG_DIR_NAME: &str = ".codex";
/// 配置文件名校验用名。
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// 启动方式：决定认证投影是否可用（受管启动才能安全注入本机令牌）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupMode {
    /// 由本工具启动，可控制环境与参数。
    Managed,
    /// 用户自己启动；env 注入不可靠，只能依赖 command auth。
    Unmanaged,
    /// 尚未启动。
    NotRunning,
}

impl StartupMode {
    /// 非受管启动时不能默默把上游 Key 写进 TOML，也不能假定能注入 env。
    pub fn allows_env_injection(self) -> bool {
        matches!(self, StartupMode::Managed)
    }
}

/// 检测到的 Codex 实例。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexInstance {
    pub id: InstanceId,
    pub app_path: Option<String>,
    pub cli_path: Option<String>,
    pub desktop_version: Option<String>,
    pub cli_version: Option<String>,
    pub config_root: String,
    pub config_file: String,
    /// config.toml 是否存在。
    pub config_exists: bool,
    pub startup_mode: StartupMode,
    pub compatibility: CompatibilityStatus,
    pub fingerprint: VersionFingerprint,
    /// 检测到的冲突工具标识（仅可识别标记，不读取其他应用的密钥库）。
    pub conflicting_managers: Vec<String>,
    /// 不可用原因 key；可用时为空。
    pub blocked_reason_key: Option<String>,
}

impl CodexInstance {
    pub fn has_cli(&self) -> bool {
        self.cli_path.is_some()
    }

    /// 是否具备继续接管的先决条件（有 CLI 且有可写配置根）。
    pub fn is_usable(&self) -> bool {
        self.has_cli() && self.blocked_reason_key.is_none()
    }
}

/// 文件系统探测端口：便于在测试中注入确定性结果。
pub trait PathProbe {
    fn exists(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> Option<String>;
    fn list_dir(&self, path: &Path) -> Vec<PathBuf>;
}

/// 真实文件系统。
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFs;

impl PathProbe for RealFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn list_dir(&self, path: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(path)
            .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default()
    }
}

/// 检测输入：所有外部状态由调用方注入，保证可测且不读取用户秘密。
#[derive(Debug, Clone, Default)]
pub struct DetectInput {
    /// 显式指定的应用路径。
    pub app_path: Option<PathBuf>,
    /// 显式指定的 CLI 路径。
    pub cli_path: Option<PathBuf>,
    /// 显式指定的配置根。
    pub config_root: Option<PathBuf>,
    /// 用户主目录。
    pub home: Option<PathBuf>,
    /// 进程环境中的 CODEX_HOME（只作为线索，不直接采用）。
    pub env_codex_home: Option<PathBuf>,
    /// 平台的候选应用路径。
    pub app_candidates: Vec<PathBuf>,
}

impl DetectInput {
    /// 按平台给出默认候选，不读取环境。
    pub fn for_macos(home: PathBuf) -> Self {
        Self {
            home: Some(home),
            app_candidates: MACOS_APP_CANDIDATES.iter().map(PathBuf::from).collect(),
            ..Self::default()
        }
    }
}

/// 实例检测器。
pub struct InstanceDetector;

impl InstanceDetector {
    /// 检测实例。返回列表可能为空（表示未安装），由 UI 显示安装指引。
    pub fn detect(
        input: &DetectInput,
        probe: &dyn PathProbe,
        known: Option<&VersionFingerprint>,
        now_unix: i64,
    ) -> Result<Vec<CodexInstance>, CoreError> {
        let _ = now_unix;
        let home = input
            .home
            .clone()
            .ok_or_else(|| CoreError::validation("缺少用户主目录，无法定位默认配置"))?;

        // 配置根优先级：显式指定 > 默认 ~/.codex。环境变量仅作提示保留在 detections 说明里。
        let config_root = input
            .config_root
            .clone()
            .unwrap_or_else(|| home.join(CONFIG_DIR_NAME));

        let mut instances: Vec<CodexInstance> = Vec::new();
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(app) = &input.app_path {
            candidates.push(app.clone());
        }
        candidates.extend(input.app_candidates.iter().cloned());

        for candidate in candidates {
            if !probe.exists(&candidate) {
                continue;
            }
            let cli = candidate.join(BUNDLE_CLI_RELATIVE);
            let cli_path = if probe.exists(&cli) { Some(cli) } else { None };
            instances.push(Self::build(
                &config_root,
                Some(candidate),
                cli_path,
                input,
                probe,
                known,
            ));
        }

        // 只有一个裸 CLI（无 app bundle）也算一个可管理实例。
        if instances.is_empty() {
            if let Some(cli) = &input.cli_path {
                if probe.exists(cli) {
                    instances.push(Self::build(
                        &config_root,
                        None,
                        Some(cli.clone()),
                        input,
                        probe,
                        known,
                    ));
                }
            }
        }

        // 去重：同一配置根 + 同一 CLI 只保留一个。
        let mut seen: std::collections::HashSet<(String, Option<String>)> =
            std::collections::HashSet::new();
        instances.retain(|instance| {
            seen.insert((instance.config_root.clone(), instance.cli_path.clone()))
        });

        Ok(instances)
    }

    fn build(
        config_root: &Path,
        app_path: Option<PathBuf>,
        cli_path: Option<PathBuf>,
        input: &DetectInput,
        probe: &dyn PathProbe,
        known: Option<&VersionFingerprint>,
    ) -> CodexInstance {
        let config_file = config_root.join(CONFIG_FILE_NAME);
        let config_exists = probe.exists(&config_file);
        let cli_version = cli_path
            .as_ref()
            .and_then(|cli| Self::read_version_marker(probe, cli));
        let desktop_version = app_path
            .as_ref()
            .and_then(|app| Self::read_plist_version(probe, app));

        let fingerprint = VersionFingerprint {
            desktop_version: desktop_version.clone(),
            cli_version: cli_version.clone(),
            schema_hash: None,
        };
        let compatibility = match known {
            Some(known) => fingerprint.status_against(known),
            None => CompatibilityStatus::Unverified,
        };

        let conflicting_managers = if config_exists {
            probe
                .read_to_string(&config_file)
                .map(|text| detect_foreign_managers(&text))
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let blocked_reason_key = if cli_path.is_none() {
            Some("instance.reason.noCli".to_owned())
        } else if !config_exists && !probe.exists(config_root) {
            Some("instance.reason.configRootMissing".to_owned())
        } else {
            None
        };

        // 实例标识由配置根与 CLI 路径派生，稳定且不含用户秘密。
        let identity = format!(
            "{}|{}",
            config_root.display(),
            cli_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        );

        CodexInstance {
            id: InstanceId::new(stable_instance_id(&identity)),
            app_path: app_path.map(|p| p.display().to_string()),
            cli_path: cli_path.map(|p| p.display().to_string()),
            desktop_version,
            cli_version,
            config_root: config_root.display().to_string(),
            config_file: config_file.display().to_string(),
            config_exists,
            startup_mode: if input.env_codex_home.is_some() {
                StartupMode::Unmanaged
            } else {
                StartupMode::NotRunning
            },
            compatibility,
            fingerprint,
            conflicting_managers,
            blocked_reason_key,
        }
    }

    /// 读取 bundle 内 CLI 的版本标记文件（`codex.version`），缺失时返回 None。
    fn read_version_marker(probe: &dyn PathProbe, cli: &Path) -> Option<String> {
        let parent = cli.parent()?;
        for name in ["codex.version", "VERSION", "version.txt"] {
            if let Some(text) = probe.read_to_string(&parent.join(name)) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_owned());
                }
            }
        }
        None
    }

    /// 从 Info.plist 文本中读取 `CFBundleShortVersionString`。
    fn read_plist_version(probe: &dyn PathProbe, app: &Path) -> Option<String> {
        let plist = probe.read_to_string(&app.join("Contents/Info.plist"))?;
        extract_plist_string(&plist, "CFBundleShortVersionString")
    }

    /// 检查 bundle 标识是否属于已知 Codex 应用身份。
    pub fn matches_bundle_id(plist_text: &str) -> bool {
        KNOWN_BUNDLE_IDS
            .iter()
            .any(|id| plist_text.contains(&format!("<string>{id}</string>")))
    }
}

/// 从 plist 中提取某个 key 后紧跟的 `<string>` 值。
fn extract_plist_string(plist: &str, key: &str) -> Option<String> {
    let key_pos = plist.find(&format!("<key>{key}</key>"))?;
    let rest = &plist[key_pos..];
    let open = rest.find("<string>")? + "<string>".len();
    let close = rest[open..].find("</string>")? + open;
    let value = rest[open..close].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

/// 稳定实例 ID：使用不可逆摘要，避免把用户路径直接当 ID 暴露。
fn stable_instance_id(identity: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(identity.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("inst_{}", &digest[..16])
}

/// 仅在配置文本中查找可识别的第三方管理标记，不解析其他应用的凭据。
pub fn detect_foreign_managers(config_text: &str) -> Vec<String> {
    // `opencodex` 是 CodexSplit 改名前的名字，它的托管块在真实配置里就写作
    // `# >>> opencodex managed >>>`。少了这一条，另一个工具把本工具写进去的
    // `model_provider` / `model_catalog_json` 圈进它自己的托管块时，界面什么都不提示。
    const MARKERS: [(&str, &str); 5] = [
        ("codexsplit", "CodexSplit"),
        ("opencodex", "CodexSplit"),
        ("cc-switch", "CC Switch"),
        ("ccswitch", "CC Switch"),
        ("xingsuan", "星算助手"),
    ];
    let lowered = config_text.to_ascii_lowercase();
    let mut found: Vec<String> = Vec::new();
    for (needle, label) in MARKERS {
        if lowered.contains(needle) && !found.iter().any(|f| f == label) {
            found.push(label.to_owned());
        }
    }
    found.sort_unstable();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 内存文件系统：测试不触碰真实磁盘。
    #[derive(Debug, Default)]
    struct FakeFs {
        files: HashMap<String, String>,
        dirs: Vec<String>,
    }

    impl FakeFs {
        fn file(mut self, path: &str, content: &str) -> Self {
            self.files.insert(path.to_owned(), content.to_owned());
            self
        }

        fn dir(mut self, path: &str) -> Self {
            self.dirs.push(path.to_owned());
            self
        }
    }

    impl PathProbe for FakeFs {
        fn exists(&self, path: &Path) -> bool {
            let key = path.to_string_lossy().to_string();
            self.files.contains_key(&key) || self.dirs.iter().any(|d| d == &key)
        }

        fn read_to_string(&self, path: &Path) -> Option<String> {
            self.files.get(&path.to_string_lossy().to_string()).cloned()
        }

        fn list_dir(&self, path: &Path) -> Vec<PathBuf> {
            let prefix = format!("{}/", path.to_string_lossy());
            self.files
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .map(PathBuf::from)
                .collect()
        }
    }

    const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.openai.codex</string>
  <key>CFBundleShortVersionString</key><string>26.908.70816</string>
</dict></plist>"#;

    fn fake_env() -> (FakeFs, DetectInput) {
        let fs = FakeFs::default()
            .dir("/Applications/ChatGPT.app")
            .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
            .dir("/Applications/ChatGPT.app/Contents/Resources")
            .file(
                "/Applications/ChatGPT.app/Contents/Resources/codex",
                "binary",
            )
            .file(
                "/Applications/ChatGPT.app/Contents/Resources/codex.version",
                "0.154.0-alpha.6.2\n",
            )
            .dir("/Users/example/.codex")
            .file(
                "/Users/example/.codex/config.toml",
                "model = \"gpt-5-codex\"\n",
            );
        let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
        (fs, input)
    }

    #[test]
    fn detects_macos_bundle_with_identity_not_name() {
        let (fs, input) = fake_env();
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(found.len(), 1);
        let instance = &found[0];
        assert_eq!(
            instance.app_path.as_deref(),
            Some("/Applications/ChatGPT.app")
        );
        assert_eq!(
            instance.cli_path.as_deref(),
            Some("/Applications/ChatGPT.app/Contents/Resources/codex")
        );
        assert_eq!(instance.desktop_version.as_deref(), Some("26.908.70816"));
        assert_eq!(instance.cli_version.as_deref(), Some("0.154.0-alpha.6.2"));
        assert_eq!(instance.config_root, "/Users/example/.codex");
        assert!(instance.config_exists);
        assert!(instance.is_usable());
        assert_eq!(instance.blocked_reason_key, None);
    }

    #[test]
    fn known_bundle_ids_are_recognized_from_plist() {
        assert!(InstanceDetector::matches_bundle_id(PLIST));
        assert!(InstanceDetector::matches_bundle_id(
            "<string>com.openai.chatgpt</string>"
        ));
        assert!(!InstanceDetector::matches_bundle_id(
            "<string>com.example.codex</string>"
        ));
    }

    #[test]
    fn missing_installation_returns_empty_list() {
        let fs = FakeFs::default().dir("/Users/example/.codex");
        let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert!(found.is_empty(), "未安装时返回空列表，由 UI 显示安装指引");
    }

    #[test]
    fn env_codex_home_is_only_a_hint_not_the_gui_value() {
        let (fs, mut input) = fake_env();
        input.env_codex_home = Some(PathBuf::from("/tmp/isolated-codex-home"));
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        // 仍然使用默认 ~/.codex，而不是终端里的环境变量。
        assert_eq!(found[0].config_root, "/Users/example/.codex");
        assert_eq!(found[0].startup_mode, StartupMode::Unmanaged);
    }

    #[test]
    fn explicit_config_root_wins() {
        let (fs, mut input) = fake_env();
        input.config_root = Some(PathBuf::from("/tmp/test-codex-home"));
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(found[0].config_root, "/tmp/test-codex-home");
        assert!(!found[0].config_exists);
        assert_eq!(
            found[0].blocked_reason_key.as_deref(),
            Some("instance.reason.configRootMissing")
        );
    }

    #[test]
    fn instance_id_is_stable_and_path_free() {
        let (fs, input) = fake_env();
        let first = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        let second = InstanceDetector::detect(&input, &fs, None, 1).unwrap();
        assert_eq!(first[0].id, second[0].id);
        assert!(first[0].id.as_str().starts_with("inst_"));
        assert!(!first[0].id.as_str().contains("Users"));
    }

    #[test]
    fn unknown_version_stays_unverified_and_known_version_is_stable() {
        let (fs, input) = fake_env();
        let unknown = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(unknown[0].compatibility, CompatibilityStatus::Unverified);

        let known = VersionFingerprint {
            desktop_version: Some("26.908.70816".into()),
            cli_version: Some("0.154.0-alpha.6.2".into()),
            schema_hash: None,
        };
        let matched = InstanceDetector::detect(&input, &fs, Some(&known), 0).unwrap();
        assert_eq!(matched[0].compatibility, CompatibilityStatus::Stable);
    }

    #[test]
    fn bundle_without_cli_is_reported_with_reason() {
        let fs = FakeFs::default()
            .dir("/Applications/Codex.app")
            .file("/Applications/Codex.app/Contents/Info.plist", PLIST)
            .dir("/Users/example/.codex")
            .file("/Users/example/.codex/config.toml", "");
        let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(found.len(), 1);
        assert!(!found[0].is_usable());
        assert_eq!(
            found[0].blocked_reason_key.as_deref(),
            Some("instance.reason.noCli")
        );
    }

    #[test]
    fn multiple_candidates_produce_distinct_instances() {
        let fs = FakeFs::default()
            .dir("/Applications/ChatGPT.app")
            .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
            .file("/Applications/ChatGPT.app/Contents/Resources/codex", "b")
            .dir("/Applications/Codex.app")
            .file("/Applications/Codex.app/Contents/Info.plist", PLIST)
            .file("/Applications/Codex.app/Contents/Resources/codex", "b")
            .dir("/Users/example/.codex");
        let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(found.len(), 2, "多实例必须让用户选择，不能自动挑一个");
        assert_ne!(found[0].id, found[1].id);
    }

    #[test]
    fn bare_cli_without_bundle_is_still_manageable() {
        let fs = FakeFs::default()
            .file("/opt/homebrew/bin/codex", "binary")
            .dir("/Users/example/.codex")
            .file("/Users/example/.codex/config.toml", "");
        let input = DetectInput {
            home: Some(PathBuf::from("/Users/example")),
            cli_path: Some(PathBuf::from("/opt/homebrew/bin/codex")),
            ..Default::default()
        };
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].app_path, None);
        assert!(found[0].is_usable());
    }

    #[test]
    fn detects_foreign_managers_from_config_text_only() {
        let text = "[model_providers.codexsplit]\nname = \"CodexSplit\"\n";
        assert_eq!(detect_foreign_managers(text), vec!["CodexSplit".to_owned()]);

        let text = "[mcp_servers.x]\ncommand = \"cc-switch\"\n";
        assert_eq!(detect_foreign_managers(text), vec!["CC Switch".to_owned()]);

        // 改名前的写法：托管块标记里只有 `opencodex`，不含 `codexsplit`。
        // 这条曾经漏掉，于是配置被另一个工具圈走时界面一片安静。
        let text = "# >>> opencodex managed >>>\nmodel_provider = \"gptswitch\"\n# <<< opencodex managed >>>\n";
        assert_eq!(detect_foreign_managers(text), vec!["CodexSplit".to_owned()]);

        // 两种写法同时出现只报一次。
        let text =
            "# >>> opencodex managed >>>\n[model_providers.codexsplit]\nname = \"CodexSplit\"\n";
        assert_eq!(detect_foreign_managers(text), vec!["CodexSplit".to_owned()]);

        assert!(detect_foreign_managers("[mcp_servers.docs]\ncommand = \"npx\"\n").is_empty());
    }

    #[test]
    fn conflicting_managers_are_reported_on_the_instance() {
        let fs = FakeFs::default()
            .dir("/Applications/ChatGPT.app")
            .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
            .file("/Applications/ChatGPT.app/Contents/Resources/codex", "b")
            .dir("/Users/example/.codex")
            .file(
                "/Users/example/.codex/config.toml",
                "[model_providers.codexsplit]\nname = \"CodexSplit\"\n",
            );
        let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(found[0].conflicting_managers, vec!["CodexSplit".to_owned()]);
    }

    #[test]
    fn missing_home_is_a_validation_error_not_a_panic() {
        let fs = FakeFs::default();
        let input = DetectInput::default();
        let error = InstanceDetector::detect(&input, &fs, None, 0).unwrap_err();
        assert_eq!(
            error.code,
            crate::domain::error::ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn env_injection_requires_managed_startup() {
        assert!(StartupMode::Managed.allows_env_injection());
        assert!(!StartupMode::Unmanaged.allows_env_injection());
        assert!(!StartupMode::NotRunning.allows_env_injection());
    }
}
