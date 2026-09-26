//! Codex 实例检测。
//!
//! 规则来自 [配置生命周期](../../../../docs/architecture/02-configuration-lifecycle.md)：
//! 必须用应用身份与二进制探测，不能只找 `Codex.app`；终端里的 `$CODEX_HOME` 不能
//! 直接当成 GUI 实例的值；多个实例必须让用户选择，不能自动挑“正在运行的那个”。

use crate::domain::error::CoreError;
use crate::domain::ids::InstanceId;
use crate::domain::version::{CompatibilityStatus, VersionFingerprint};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// 已知的 Desktop bundle 标识。本机实测为 ChatGPT.app + `com.openai.codex`。
pub const KNOWN_BUNDLE_IDS: [&str; 2] = ["com.openai.codex", "com.openai.chatgpt"];
/// macOS 常见安装位置候选。
pub const MACOS_APP_CANDIDATES: [&str; 3] = [
    "/Applications/ChatGPT.app",
    "/Applications/Codex.app",
    "/Applications/OpenAI Codex.app",
];
/// 新版布局的 CLI 描述文件（相对 bundle）。宿主升级换了 CLI 位置时，先信它。
pub const BUNDLE_CLI_PACKAGE_RELATIVE: &str = "Contents/Resources/codex-cli/codex-package.json";

/// bundle 内 CLI 候选相对路径，按优先级排列。
///
/// 2026-09-26 的 ChatGPT 26.924 把 CLI 从 `Contents/Resources/codex`（单个可执行文件）
/// 挪进了 `codex-cli/`：真二进制在 `CodexCLI.app` 里，`bin/codex` 只是个转发脚本，
/// 版本落在 `codex-package.json`。这里必须留成**列表**：宿主每次升级都可能再挪一次位置，
/// 写死一个路径就会「更新完就认不出 Codex」（旧路径在新版里已经不存在了）。
pub const BUNDLE_CLI_RELATIVES: [&str; 3] = [
    "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
    "Contents/Resources/codex-cli/bin/codex",
    "Contents/Resources/codex",
];
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
            let cli_path = Self::resolve_bundle_cli(probe, &candidate);
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

        // 去重：同一个实例（身份相同）只保留一个。身份已经含配置根与安装位置，
        // 不再拿 CLI 路径当去重键——那是 bundle 内部路径，会随升级变。
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        instances.retain(|instance| seen.insert(instance.id.as_str().to_owned()));

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
            .and_then(|cli| read_cli_version(probe, app_path.as_deref(), cli));
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

        // 实例标识由配置根与安装位置派生，稳定且不含用户秘密。
        let identity = instance_identity(config_root, app_path.as_deref(), cli_path.as_deref());

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

    /// 在 bundle 里找 codex CLI。
    ///
    /// 先信 `codex-package.json` 里声明的 `entrypoint`：那是宿主自己写下的入口，
    /// 下次它再挪位置，只要描述文件跟着走，这里不用改代码。描述文件不在（或入口不存在、
    /// 或入口不老实——绝对路径与 `..` 一律不认，这个值最后会被 bridge 执行）时，
    /// 才退到固定候选列表。
    fn resolve_bundle_cli(probe: &dyn PathProbe, app: &Path) -> Option<PathBuf> {
        let package = app.join(BUNDLE_CLI_PACKAGE_RELATIVE);
        if let Some(entry) = probe
            .read_to_string(&package)
            .and_then(|text| package_field(&text, "entrypoint"))
            .filter(|entry| is_safe_relative_entry(entry))
        {
            if let Some(dir) = package.parent() {
                let cli = dir.join(&entry);
                if probe.exists(&cli) {
                    return Some(cli);
                }
            }
        }
        BUNDLE_CLI_RELATIVES
            .iter()
            .map(|relative| app.join(relative))
            .find(|cli| probe.exists(cli))
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

/// 实例身份串：只用**稳定**的事实（配置根 + 安装位置），绝不包含 bundle 内部的文件路径。
///
/// 为什么不能用 CLI 路径（旧口径是 `配置根|CLI 路径`）：那个路径在 bundle 里面，宿主每次
/// 升级都可能挪位置——2026-09-26 的 ChatGPT 26.924 就把 `Contents/Resources/codex` 挪进了
/// `codex-cli/`。身份一飘，按实例记录的写入历史（operations 的 `instanceId`）就全部对不上，
/// 「还原原生配置」会直接报「本工具尚未写入过该实例的配置」，用户明明刚用过。
///
/// 两条路径都要先做词法归一化：界面上有一个手填应用路径的输入框，`…/ChatGPT.app` 与
/// `…/ChatGPT.app/` 是同一个安装，不归一就会算出两个身份，于是「用这种写法应用、
/// 用那种写法还原」又掉回同一个坑。
///
/// 没有 bundle 的裸 CLI 没有更稳的锚点，仍用 CLI 路径——那条路上路径本来就是用户自己选的。
fn instance_identity(
    config_root: &Path,
    app_path: Option<&Path>,
    cli_path: Option<&Path>,
) -> String {
    let anchor = app_path
        .or(cli_path)
        .map(|path| normalize_path(path).display().to_string())
        .unwrap_or_default();
    format!("{}|{}", normalize_path(config_root).display(), anchor)
}

/// 词法归一化路径：吃掉 `.` 与重复/结尾的分隔符，尽量回退 `..`。
///
/// 只做词法处理，**不碰文件系统**：探测阶段引入 IO 会破坏其余部分的纯函数性质
/// （检测器要在内存假文件系统上跑出确定性结果）。也因此不解析符号链接。
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // 只有前面确实是一个普通片段时才回退；根目录之上、开头的 `..` 都如实保留。
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.is_absolute() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// `codex-package.json` 里的 `entrypoint` 只接受老实的相对文件路径。
///
/// 这个值最终会被 bridge 拿去执行，所以绝对路径（`Path::join` 会整体替换掉描述文件所在
/// 目录）与 `..`（能爬出 bundle）都不认，`.` 这类指不到文件的写法也不认——
/// 宁可退回候选列表，也不执行一个权威来源之外的路径。
fn is_safe_relative_entry(entry: &str) -> bool {
    let mut segments = 0;
    for component in Path::new(entry).components() {
        match component {
            Component::Normal(_) => segments += 1,
            Component::CurDir => {}
            // 绝对路径、根、`..` 一律拒绝。
            _ => return false,
        }
    }
    segments > 0
}

/// 旧口径的实例 ID（配置根 + bundle 内 CLI 路径），用来兼容升级前留下的写入记录。
///
/// 只在**查历史**时用；新记录一律写新口径。候选路径逐个算一遍，因为不知道用户当时
/// 用的是哪一个布局；`配置根|` 那个空后缀是「当时没认出 CLI」的历史形态，一并覆盖。
pub fn legacy_instance_ids(config_root: &Path, app_path: Option<&Path>) -> Vec<InstanceId> {
    let Some(app) = app_path else {
        return Vec::new();
    };
    let mut ids: Vec<InstanceId> = BUNDLE_CLI_RELATIVES
        .iter()
        .map(|relative| legacy_instance_id(config_root, app, relative))
        .collect();
    ids.push(InstanceId::new(stable_instance_id(&format!(
        "{}|",
        config_root.display()
    ))));
    ids
}

/// 旧口径下的单个实例 ID：`sha256(配置根 + "|" + bundle 内 CLI 路径)`。
pub fn legacy_instance_id(config_root: &Path, app_path: &Path, cli_relative: &str) -> InstanceId {
    InstanceId::new(stable_instance_id(&format!(
        "{}|{}",
        config_root.display(),
        app_path.join(cli_relative).display()
    )))
}

/// 从 bundle 内的 CLI 读版本：新布局读 `codex-package.json` 的 `version`，
/// 旧布局读 CLI 旁边的版本标记文件。
///
/// 描述文件里的版本只在选中的 CLI **确实位于** `codex-cli/` 之下时才用：否则（例如描述文件
/// 在、但入口与两个新候选都不在、最后回落到了旧路径）会把新布局的版本号安到旧布局的
/// CLI 上，喂给 `VersionFingerprint` 的是一个错的版本。
fn read_cli_version(probe: &dyn PathProbe, app_path: Option<&Path>, cli: &Path) -> Option<String> {
    let package = app_path.map(|app| app.join(BUNDLE_CLI_PACKAGE_RELATIVE));
    let cli_is_from_package = package
        .as_deref()
        .and_then(Path::parent)
        .is_some_and(|dir| cli.starts_with(dir));
    if cli_is_from_package {
        if let Some(version) = package
            .as_deref()
            .and_then(|path| probe.read_to_string(path))
            .and_then(|text| package_field(&text, "version"))
        {
            return Some(version);
        }
    }
    InstanceDetector::read_version_marker(probe, cli)
}

/// 从 `codex-package.json` 里取一个字符串字段。看不懂的输入返回 `None`，
/// 由调用方退回候选路径/旧来源——绝不猜。
fn package_field(text: &str, field: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let raw = value.get(field)?.as_str()?.trim();
    let trimmed = raw.trim_start_matches("./");
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
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
        // 查表前先做词法归一化：真实文件系统不区分 `/a/b`、`/a/b/` 与 `/a/./b`，
        // 假文件系统若按原始字符串精确比较，就会让「同一路径的不同写法」这类断言
        // 因为候选被整个跳过而恒真。
        fn exists(&self, path: &Path) -> bool {
            let key = normalize_path(path).to_string_lossy().to_string();
            self.files.contains_key(&key) || self.dirs.iter().any(|d| d == &key)
        }

        fn read_to_string(&self, path: &Path) -> Option<String> {
            self.files
                .get(&normalize_path(path).to_string_lossy().to_string())
                .cloned()
        }

        fn list_dir(&self, path: &Path) -> Vec<PathBuf> {
            let prefix = format!("{}/", normalize_path(path).to_string_lossy());
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

    /// 新版布局的描述文件（照抄真机 ChatGPT 26.924 的 `codex-cli/codex-package.json`）。
    const CODEX_PACKAGE: &str = r#"{
  "layoutVersion": 1,
  "version": "0.158.0-alpha.2.1",
  "target": "aarch64-apple-darwin",
  "variant": "codex",
  "entrypoint": "bin/codex",
  "resourcesDir": "codex-resources",
  "pathDir": "codex-path"
}"#;

    const NEW_CLI_DIR: &str = "/Applications/ChatGPT.app/Contents/Resources/codex-cli";

    /// 新版布局：CLI 在 `codex-cli/` 下，`bin/codex` 是入口脚本，版本写在描述文件里。
    fn fake_env() -> (FakeFs, DetectInput) {
        let fs = FakeFs::default()
            .dir("/Applications/ChatGPT.app")
            .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
            .file(&format!("{NEW_CLI_DIR}/codex-package.json"), CODEX_PACKAGE)
            .file(&format!("{NEW_CLI_DIR}/bin/codex"), "#!/bin/sh\n")
            .file(
                &format!("{NEW_CLI_DIR}/CodexCLI.app/Contents/MacOS/codex"),
                "binary",
            )
            .dir("/Users/example/.codex")
            .file(
                "/Users/example/.codex/config.toml",
                "model = \"gpt-5-codex\"\n",
            );
        let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
        (fs, input)
    }

    /// 旧布局：CLI 就是 `Contents/Resources/codex` 一个文件，版本在旁边的 `codex.version`。
    fn legacy_layout_env() -> (FakeFs, DetectInput) {
        let fs = FakeFs::default()
            .dir("/Applications/ChatGPT.app")
            .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
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
            Some(&format!("{NEW_CLI_DIR}/bin/codex")[..]),
            "应认描述文件声明的入口，而不是猜固定路径"
        );
        assert_eq!(instance.desktop_version.as_deref(), Some("26.908.70816"));
        assert_eq!(instance.cli_version.as_deref(), Some("0.158.0-alpha.2.1"));
        assert_eq!(instance.config_root, "/Users/example/.codex");
        assert!(instance.config_exists);
        assert!(instance.is_usable());
        assert_eq!(instance.blocked_reason_key, None);
    }

    #[test]
    fn legacy_layout_is_still_detected() {
        let (fs, input) = legacy_layout_env();
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].cli_path.as_deref(),
            Some("/Applications/ChatGPT.app/Contents/Resources/codex"),
            "旧布局（单个 codex 文件）必须继续认得出"
        );
        assert_eq!(found[0].cli_version.as_deref(), Some("0.154.0-alpha.6.2"));
        assert!(found[0].is_usable());
    }

    /// 这条是「更新完 ChatGPT 就认不出 Codex」的回归测试：宿主升级只挪了 bundle 内部的
    /// CLI 位置，同一个安装必须仍然算同一个实例，否则按实例记录的写入历史全部作废，
    /// 「还原原生配置」会报「本工具尚未写入过该实例」。
    #[test]
    fn instance_id_survives_cli_relocation_inside_the_bundle() {
        let (old_fs, input) = legacy_layout_env();
        let (new_fs, _) = fake_env();
        let old = InstanceDetector::detect(&input, &old_fs, None, 0).unwrap();
        let new = InstanceDetector::detect(&input, &new_fs, None, 0).unwrap();
        assert_ne!(
            old[0].cli_path, new[0].cli_path,
            "前提：这次升级确实换了 bundle 内的 CLI 位置"
        );
        assert_eq!(old[0].app_path, new[0].app_path);
        assert_eq!(
            old[0].id, new[0].id,
            "换了 CLI 位置但安装位置与配置根没变，实例身份必须不变"
        );
    }

    #[test]
    fn legacy_instance_ids_reproduce_the_old_formula() {
        // 旧口径：sha256(配置根 + "|" + bundle 内 CLI 路径)。历史记录就挂在这些 ID 上。
        let ids = legacy_instance_ids(
            Path::new("/Users/example/.codex"),
            Some(Path::new("/Applications/ChatGPT.app")),
        );
        let old_formula = stable_instance_id(&format!(
            "{}|{}",
            "/Users/example/.codex", "/Applications/ChatGPT.app/Contents/Resources/codex"
        ));
        assert!(
            ids.iter().any(|id| id.as_str() == old_formula),
            "必须覆盖旧布局那条路径，否则老用户的还原记录还是找不到"
        );
        // 「当时没认出 CLI」的历史形态也要能查到。
        let no_cli = stable_instance_id("/Users/example/.codex|");
        assert!(ids.iter().any(|id| id.as_str() == no_cli));
        // 裸 CLI 没有 bundle，没有旧 ID 可兼容。
        assert!(legacy_instance_ids(Path::new("/Users/example/.codex"), None).is_empty());
    }

    #[test]
    fn cli_falls_back_to_candidate_paths_when_the_package_file_is_unusable() {
        for package in ["", "{ not json", "{\"entrypoint\": \"bin/does-not-exist\"}"] {
            let fs = FakeFs::default()
                .dir("/Applications/ChatGPT.app")
                .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
                .file(&format!("{NEW_CLI_DIR}/codex-package.json"), package)
                .file(
                    &format!("{NEW_CLI_DIR}/CodexCLI.app/Contents/MacOS/codex"),
                    "binary",
                )
                .dir("/Users/example/.codex");
            let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
            let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
            assert_eq!(
                found[0].cli_path.as_deref(),
                Some(&format!("{NEW_CLI_DIR}/CodexCLI.app/Contents/MacOS/codex")[..]),
                "描述文件不可用时退到候选路径，且优先真二进制：{package:?}"
            );
            assert_eq!(
                found[0].cli_version, None,
                "版本来源不可读时返回 None，不猜一个版本号"
            );
        }
    }

    /// 描述文件里的入口是要被执行的路径，绝对路径与 `..` 一律不认。
    #[test]
    fn an_untrustworthy_entrypoint_is_ignored() {
        for entry in [
            "/tmp/elsewhere/codex",
            "../../../../tmp/elsewhere/codex",
            "bin/../../codex",
            ".",
            "./",
        ] {
            let package = format!("{{\"entrypoint\": \"{entry}\", \"version\": \"9.9.9\"}}");
            let fs = FakeFs::default()
                .dir("/Applications/ChatGPT.app")
                .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
                .file(&format!("{NEW_CLI_DIR}/codex-package.json"), &package)
                .file("/tmp/elsewhere/codex", "binary")
                .file(
                    &format!("{NEW_CLI_DIR}/CodexCLI.app/Contents/MacOS/codex"),
                    "binary",
                )
                .dir("/Users/example/.codex");
            let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
            let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
            assert_eq!(
                found[0].cli_path.as_deref(),
                Some(&format!("{NEW_CLI_DIR}/CodexCLI.app/Contents/MacOS/codex")[..]),
                "不该执行描述文件指到 bundle 之外的入口：{entry}"
            );
        }
    }

    /// 同一个安装的两种写法必须算同一个实例：手填路径的输入框允许用户写尾斜杠。
    #[test]
    fn instance_id_ignores_spelling_of_the_same_path() {
        // 先直接钉住身份函数本身（不经过检测流程，避免被「候选不存在被跳过」掩盖）。
        assert_eq!(
            instance_identity(
                Path::new("/Users/example/.codex/"),
                Some(Path::new("/Applications/ChatGPT.app/")),
                None,
            ),
            instance_identity(
                Path::new("/Users/example/.codex"),
                Some(Path::new("/Applications/ChatGPT.app")),
                None,
            ),
            "归一化没生效：尾斜杠会算出第二个身份"
        );

        let variants = [
            "/Applications/ChatGPT.app",
            "/Applications/ChatGPT.app/",
            "/Applications/./ChatGPT.app",
            "/Applications/Codex/../ChatGPT.app",
        ];
        let ids: Vec<String> = variants
            .iter()
            .map(|app| {
                let fs = FakeFs::default()
                    .dir("/Applications/ChatGPT.app")
                    .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
                    .file(&format!("{NEW_CLI_DIR}/bin/codex"), "b")
                    .dir("/Users/example/.codex");
                let mut input = DetectInput::for_macos(PathBuf::from("/Users/example"));
                input.app_path = Some(PathBuf::from(app));
                let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
                assert_eq!(found.len(), 1, "{app} 只该检出一个实例");
                // cli_path 保留用户写法的原样（它要在磁盘上用），要断言的是「确实解析到了 CLI」，
                // 而不是被当成不存在的路径整个跳过——后者会让下面的一致性断言恒真。
                assert!(
                    found[0].cli_path.is_some(),
                    "{app} 必须真的走到 CLI 解析，而不是被当成不存在的路径跳过"
                );
                found[0].id.as_str().to_owned()
            })
            .collect();
        assert!(
            ids.windows(2).all(|pair| pair[0] == pair[1]),
            "同一个安装的不同写法算出了不同身份：{variants:?} -> {ids:?}"
        );
    }

    /// 选中旧布局的 CLI 时，不能把新布局描述文件里的版本号安到它头上。
    #[test]
    fn package_version_is_not_used_for_a_cli_outside_the_package_dir() {
        let fs = FakeFs::default()
            .dir("/Applications/ChatGPT.app")
            .file("/Applications/ChatGPT.app/Contents/Info.plist", PLIST)
            .file(
                &format!("{NEW_CLI_DIR}/codex-package.json"),
                "{\"entrypoint\": \"bin/does-not-exist\", \"version\": \"0.158.0-alpha.2.1\"}",
            )
            .file(
                "/Applications/ChatGPT.app/Contents/Resources/codex",
                "binary",
            )
            .file(
                "/Applications/ChatGPT.app/Contents/Resources/codex.version",
                "0.154.0-alpha.6.2\n",
            )
            .dir("/Users/example/.codex");
        let input = DetectInput::for_macos(PathBuf::from("/Users/example"));
        let found = InstanceDetector::detect(&input, &fs, None, 0).unwrap();
        assert_eq!(
            found[0].cli_path.as_deref(),
            Some("/Applications/ChatGPT.app/Contents/Resources/codex")
        );
        assert_eq!(
            found[0].cli_version.as_deref(),
            Some("0.154.0-alpha.6.2"),
            "版本必须来自实际选中的那份 CLI，而不是描述文件"
        );
    }

    #[test]
    fn normalize_path_is_lexical_and_keeps_meaning() {
        assert_eq!(normalize_path(Path::new("/a/b/")), PathBuf::from("/a/b"));
        assert_eq!(normalize_path(Path::new("/a//b")), PathBuf::from("/a/b"));
        assert_eq!(normalize_path(Path::new("/a/./b")), PathBuf::from("/a/b"));
        assert_eq!(
            normalize_path(Path::new("/a/c/../b")),
            PathBuf::from("/a/b")
        );
        // 根之上、以及相对路径开头的 `..` 如实保留，不能悄悄吃掉。
        assert_eq!(normalize_path(Path::new("/../a")), PathBuf::from("/a"));
        assert_eq!(normalize_path(Path::new("../a")), PathBuf::from("../a"));
        assert_eq!(normalize_path(Path::new("/")), PathBuf::from("/"));
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
            cli_version: Some("0.158.0-alpha.2.1".into()),
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
            .file(&format!("{NEW_CLI_DIR}/bin/codex"), "b")
            .dir("/Applications/Codex.app")
            .file("/Applications/Codex.app/Contents/Info.plist", PLIST)
            .file(
                "/Applications/Codex.app/Contents/Resources/codex-cli/bin/codex",
                "b",
            )
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
            .file(&format!("{NEW_CLI_DIR}/bin/codex"), "b")
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
