//! 工具检测：把清单变成界面能显示的结论。
//!
//! 这一层最重要的性质是**不猜**。每个状态都必须能追到一条观察：
//! 路径存在、探针退出码、探针输出里的特征串、或静态判据文件是否在。
//! 观察不到就给「未知」并说明缺哪一步，绝不把「没查」显示成「没问题」。

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::platform::Platform;

use super::{
    catalog::{expand_path, AgentUsage, ToolCategory, ToolDescriptor},
    probe::{self, ProbeOutcome},
};

/// 版本探针的超时。`--version` 正常在毫秒级返回，10 秒已经是故障级的宽松值。
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
/// 登录探针的超时。`gh auth status` 可能要走一次网络，给得比版本探针宽。
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
/// 界面「探针原文」保留的行数。
const PROBE_TAIL_LINES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatus {
    /// 二进制在、版本探针通过、且清单声明的静态判据全部满足。
    Ready,
    /// 二进制在、探针通过，但登录探针明确说没登录。
    NeedsLogin,
    /// 二进制在、探针通过；该工具没有可判定的登录概念，或还没做过登录判定。
    Installed,
    /// 找到了文件但探针没通过（非零退出、超时或无法执行）。
    /// 这是一个**需要人看一眼**的状态，不是「已就绪」。
    Unverified,
    NotInstalled,
    /// 清单里没有当前平台的装法，我们不知道该怎么找。
    UnsupportedPlatform,
}

impl ToolStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolStatus::Ready => "ready",
            ToolStatus::NeedsLogin => "needsLogin",
            ToolStatus::Installed => "installed",
            ToolStatus::Unverified => "unverified",
            ToolStatus::NotInstalled => "notInstalled",
            ToolStatus::UnsupportedPlatform => "unsupportedPlatform",
        }
    }
}

/// 已安装实例的事实。字段全部来自观察，`version` 缺失就是缺失。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledTool {
    /// 实际命中的可执行文件路径。
    pub path: String,
    /// 路径是 PATH 查到的还是清单候选路径命中的。界面据此解释「为什么找到的是这个」。
    pub path_source: PathSource,
    pub version: Option<String>,
    pub config_path: Option<String>,
    pub config_exists: bool,
    pub skills_path: Option<String>,
    /// 该技能目录下含 `SKILL.md` 的子目录数。目录不存在时为 0。
    pub skills_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathSource {
    /// 在 PATH 里按命令名找到。
    Path,
    /// 命中清单里的候选路径。
    Candidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolState {
    pub id: String,
    pub display_name: String,
    pub category: ToolCategory,
    pub description: String,
    pub status: ToolStatus,
    pub installed: Option<InstalledTool>,
    pub website: Option<String>,
    pub docs: Option<String>,
    /// 这个工具是否需要模型配置。界面据此决定要不要提「模型」这件事。
    pub model_config: bool,
    /// 是否支持安装技能（清单里有 `skillsRoot`）。
    pub skill_target: bool,
    pub agent_usage: AgentUsage,
    /// 版本探针输出的尾部原文。
    pub version_probe_tail: String,
    /// 登录探针输出尾部。没做过登录判定时为 `None`——**不是空字符串**，
    /// 两者的含义完全不同：前者是「没查」，后者是「查了但没输出」。
    pub auth_probe_tail: Option<String>,
    /// 需要人看的补充事实（例如「两个静态判据都不存在」）。
    pub notes: Vec<String>,
    pub probed_at: i64,
    pub cache_seconds: u64,
}

/// 一个工具的所有可执行候选路径：PATH 优先，其次清单。
fn candidate_paths(
    tool: &ToolDescriptor,
    platform: Platform,
    home: &Path,
) -> Vec<(PathBuf, PathSource)> {
    if let Some(command) = tool.command.as_deref() {
        if let Some(found) = probe::which(command) {
            return vec![(found, PathSource::Path)];
        }
    }
    tool.paths
        .for_platform(platform)
        .iter()
        .map(|raw| (expand_path(raw, home), PathSource::Candidate))
        .collect()
}

fn count_skills(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .count()
}

/// 判定登录状态。返回 `(状态, 探针原文)`。
///
/// 探针通过 → 已就绪。探针失败且输出命中声明的特征 → 需要登录。
/// **既没通过、也没命中特征时返回 `Installed`**：我们无法据此断定是没登录，
/// 硬说成「未授权」会把网络故障、权限错误一起算到登录头上。
fn check_auth(tool: &ToolDescriptor, program: &Path) -> Option<(ToolStatus, String)> {
    let auth = tool.auth_check.as_ref()?;
    let outcome = probe::run(program, &auth.args, AUTH_TIMEOUT);
    let tail = outcome.tail(PROBE_TAIL_LINES);
    if outcome.matches_exit(auth.ok_exit) {
        return Some((ToolStatus::Ready, tail));
    }
    let combined = outcome.combined().to_lowercase();
    let needs_login = auth
        .needs_login_patterns
        .iter()
        .any(|pattern| combined.contains(&pattern.to_lowercase()));
    Some((
        if needs_login {
            ToolStatus::NeedsLogin
        } else {
            ToolStatus::Installed
        },
        tail,
    ))
}

/// 检测单个工具。
pub fn detect_tool(
    tool: &ToolDescriptor,
    platform: Platform,
    home: &Path,
    locale: &str,
    now: i64,
) -> ToolState {
    let mut notes: Vec<String> = Vec::new();
    let mut state = ToolState {
        id: tool.id.clone(),
        display_name: tool.display_name(locale),
        category: tool.category,
        description: tool.description.clone(),
        status: ToolStatus::NotInstalled,
        installed: None,
        website: tool.website.clone(),
        docs: tool.docs.clone(),
        model_config: tool.model_config,
        skill_target: tool.skills_root.is_some(),
        agent_usage: tool.agent_usage.clone(),
        version_probe_tail: String::new(),
        auth_probe_tail: None,
        notes: Vec::new(),
        probed_at: now,
        cache_seconds: tool.readiness.cache_seconds,
    };

    let candidates = candidate_paths(tool, platform, home);
    if candidates.is_empty() {
        state.status = ToolStatus::UnsupportedPlatform;
        notes.push(format!(
            "清单里没有 {} 平台的候选路径，也没有可查找的命令名",
            platform.as_str()
        ));
        state.notes = notes;
        return state;
    }

    let Some((path, path_source)) = candidates.into_iter().find(|(path, _)| path.is_file()) else {
        // 只有「该平台有候选路径」或「有命令名」两种情况下，找不到才算真的没装。
        state.status = ToolStatus::NotInstalled;
        state.notes = notes;
        return state;
    };

    let outcome = probe::run(&path, &tool.readiness.version_args, VERSION_TIMEOUT);
    state.version_probe_tail = probe_tail(&outcome);
    let version = probe::first_line(&outcome);

    let static_any: Vec<PathBuf> = tool
        .readiness
        .static_any
        .iter()
        .map(|raw| expand_path(raw, home))
        .collect();
    let static_satisfied = static_any.iter().any(|path| path.exists());
    let config_path = tool
        .config_file
        .as_deref()
        .map(|raw| expand_path(raw, home));
    let config_exists = config_path.as_deref().is_some_and(Path::exists);
    let skills_path = tool
        .skills_root
        .as_deref()
        .map(|raw| expand_path(raw, home));

    let mut status = if outcome.matches_exit(tool.readiness.ok_exit) {
        if !static_any.is_empty() && !static_satisfied {
            notes.push(format!(
                "版本探针通过，但清单声明的本地文件都不存在：{}",
                static_any
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            ));
            ToolStatus::Installed
        } else {
            ToolStatus::Ready
        }
    } else {
        if outcome.timed_out {
            notes.push("版本探针超时，已终止该进程".to_owned());
        } else if outcome.exit_code.is_none() {
            notes.push("版本探针无法执行，可能是文件权限或平台不匹配".to_owned());
        } else {
            notes.push(format!(
                "版本探针退出码为 {:?}，与清单期望的 {} 不一致",
                outcome.exit_code, tool.readiness.ok_exit
            ));
        }
        ToolStatus::Unverified
    };

    // 登录判定只在探针已经能跑起来时才有意义：连版本都问不出来时，
    // 再跑一个更复杂的命令只会给出更难解释的结论。
    if matches!(status, ToolStatus::Ready | ToolStatus::Installed) {
        if let Some((auth_status, auth_tail)) = check_auth(tool, &path) {
            status = auth_status;
            state.auth_probe_tail = Some(auth_tail);
        }
    }
    if tool.require_config_file && !config_exists {
        notes.push("清单要求必须有配置文件才算可用，但该文件不存在".to_owned());
        if status == ToolStatus::Ready {
            status = ToolStatus::Installed;
        }
    }

    state.status = status;
    state.installed = Some(InstalledTool {
        path: path.display().to_string(),
        path_source,
        version,
        config_path: config_path.map(|path| path.display().to_string()),
        config_exists,
        skills_path: skills_path
            .as_deref()
            .map(|path| path.display().to_string()),
        skills_count: skills_path.as_deref().map(count_skills).unwrap_or(0),
    });
    state.notes = notes;
    state
}

fn probe_tail(outcome: &ProbeOutcome) -> String {
    outcome.tail(PROBE_TAIL_LINES)
}

/// 检测清单里的全部工具，按清单顺序返回。顺序即界面顺序，不在这里重排。
pub fn detect_all(
    catalog: &ToolCatalogView<'_>,
    platform: Platform,
    home: &Path,
    locale: &str,
    now: i64,
) -> Vec<ToolState> {
    catalog
        .tools
        .iter()
        .map(|tool| detect_tool(tool, platform, home, locale, now))
        .collect()
}

/// 检测所需的清单视图。单独抽出来是为了让测试不必构造完整清单。
pub struct ToolCatalogView<'a> {
    pub tools: &'a [ToolDescriptor],
}

impl<'a> From<&'a super::catalog::ToolCatalog> for ToolCatalogView<'a> {
    fn from(catalog: &'a super::catalog::ToolCatalog) -> Self {
        Self {
            tools: catalog.tools(),
        }
    }
}

/// 同一份清单重复检测时的缓存键：工具 id → 上次结论。
pub type ProbeCache = BTreeMap<String, ToolState>;

/// 缓存是否还能用。过期、或状态本身是「未验证/未安装」时都要重探——
/// 「没装」这个结论会随着用户刚刚装好而失效，不能一直复用。
pub fn cache_hit(state: &ToolState, now: i64) -> bool {
    let fresh = now.saturating_sub(state.probed_at) < state.cache_seconds as i64;
    fresh && !matches!(state.status, ToolStatus::Unverified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolhub::catalog::ToolCatalog;
    use std::io::Write;

    /// 造一个可执行的假工具：`--version` 输出给定文本并按给定码退出。
    #[cfg(unix)]
    fn fake_tool(dir: &Path, name: &str, version_line: &str, exit_code: i32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "#!/bin/sh\ncase \"$1\" in\n  --version) echo '{version_line}'; exit {exit_code};;\n  auth) echo 'not logged into any host'; exit 1;;\n  *) echo '{version_line}'; exit {exit_code};;\nesac"
        )
        .unwrap();
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn descriptor(id: &str, binary: &Path) -> ToolDescriptor {
        ToolDescriptor {
            id: id.to_owned(),
            names: BTreeMap::new(),
            category: ToolCategory::Utility,
            website: None,
            docs: None,
            description: String::new(),
            command: None,
            config_dir: None,
            config_file: None,
            require_config_file: false,
            model_config: false,
            skills_root: None,
            readiness: super::super::catalog::Readiness {
                version_args: vec!["--version".to_owned()],
                ok_exit: 0,
                cache_seconds: 300,
                static_any: Vec::new(),
            },
            auth_check: None,
            agent_usage: AgentUsage::default(),
            paths: super::super::catalog::PlatformPaths {
                macos: vec![binary.display().to_string()],
                windows: Vec::new(),
                linux: vec![binary.display().to_string()],
            },
        }
    }

    #[test]
    fn missing_binary_is_reported_as_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let tool = descriptor("ghost", &dir.path().join("ghost"));
        let state = detect_tool(&tool, Platform::Macos, dir.path(), "zh-Hans", 100);
        assert_eq!(state.status, ToolStatus::NotInstalled);
        assert!(state.installed.is_none());
    }

    #[test]
    fn empty_platform_paths_report_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let mut tool = descriptor("weird", &dir.path().join("weird"));
        tool.paths.macos.clear();
        tool.paths.linux.clear();
        let state = detect_tool(&tool, Platform::Macos, dir.path(), "zh-Hans", 100);
        assert_eq!(state.status, ToolStatus::UnsupportedPlatform);
        assert!(!state.notes.is_empty(), "不支持必须给出原因");
    }

    #[cfg(unix)]
    #[test]
    fn working_binary_without_static_checks_is_ready() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_tool(dir.path(), "ok-tool", "ok-tool 1.2.3", 0);
        let tool = descriptor("ok", &binary);
        let state = detect_tool(&tool, Platform::Macos, dir.path(), "zh-Hans", 100);
        assert_eq!(state.status, ToolStatus::Ready);
        let installed = state.installed.unwrap();
        assert_eq!(installed.version.as_deref(), Some("ok-tool 1.2.3"));
        assert_eq!(installed.path_source, PathSource::Candidate);
    }

    #[cfg(unix)]
    #[test]
    fn failing_probe_is_unverified_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_tool(dir.path(), "broken-tool", "boom", 2);
        let tool = descriptor("broken", &binary);
        let state = detect_tool(&tool, Platform::Macos, dir.path(), "zh-Hans", 100);
        assert_eq!(state.status, ToolStatus::Unverified);
        assert!(
            state.notes.iter().any(|note| note.contains("退出码")),
            "必须说明为什么不是已就绪：{:?}",
            state.notes
        );
    }

    #[cfg(unix)]
    #[test]
    fn declared_static_check_that_is_missing_downgrades_to_installed() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_tool(dir.path(), "tool", "tool 1.0", 0);
        let mut tool = descriptor("tool", &binary);
        tool.readiness.static_any = vec![dir.path().join("nope.json").display().to_string()];
        let state = detect_tool(&tool, Platform::Macos, dir.path(), "zh-Hans", 100);
        assert_eq!(state.status, ToolStatus::Installed);
        assert!(state.notes.iter().any(|note| note.contains("都不存在")));
    }

    #[cfg(unix)]
    #[test]
    fn auth_check_decides_between_ready_and_needs_login() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_tool(dir.path(), "authy", "authy 1.0", 0);
        let mut tool = descriptor("authy", &binary);
        tool.auth_check = Some(super::super::catalog::AuthCheck {
            args: vec!["auth".to_owned()],
            ok_exit: 0,
            needs_login_patterns: vec!["not logged into".to_owned()],
        });
        let state = detect_tool(&tool, Platform::Macos, dir.path(), "zh-Hans", 100);
        assert_eq!(state.status, ToolStatus::NeedsLogin);
        assert!(state.auth_probe_tail.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_auth_output_does_not_get_called_a_login_problem() {
        let dir = tempfile::tempdir().unwrap();
        // 这个假工具的报告里没有任何「未登录」特征，退出码却是 1。
        use std::os::unix::fs::PermissionsExt;
        let path = dir.path().join("murky");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'murky 1.0'; exit 0;;\n  *) echo 'operation not permitted'; exit 1;;\nesac"
        )
        .unwrap();
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut tool = descriptor("murky", &path);
        tool.auth_check = Some(super::super::catalog::AuthCheck {
            args: vec!["auth".to_owned()],
            ok_exit: 0,
            needs_login_patterns: vec!["not logged into".to_owned()],
        });
        let state = detect_tool(&tool, Platform::Macos, dir.path(), "zh-Hans", 100);
        assert_eq!(
            state.status,
            ToolStatus::Installed,
            "没有命中登录特征时不能断言未授权"
        );
    }

    #[test]
    fn skills_count_only_counts_directories_with_a_skill_document() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("skills");
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::write(root.join("one/SKILL.md"), "---\nname: one\n---\n").unwrap();
        std::fs::create_dir_all(root.join("two")).unwrap();
        std::fs::write(root.join("two/readme.md"), "not a skill").unwrap();
        assert_eq!(count_skills(&root), 1);
        assert_eq!(count_skills(&dir.path().join("absent")), 0);
    }

    #[test]
    fn expired_cache_is_not_a_hit_and_unverified_never_caches() {
        let mut state = detect_tool(
            &descriptor("x", Path::new("/nope")),
            Platform::Macos,
            Path::new("/tmp"),
            "en",
            100,
        );
        state.status = ToolStatus::Ready;
        state.cache_seconds = 60;
        assert!(cache_hit(&state, 130));
        assert!(!cache_hit(&state, 200), "过期后必须重探");

        state.status = ToolStatus::Unverified;
        assert!(!cache_hit(&state, 110), "未验证的结论不能缓存");
    }

    #[test]
    fn catalog_view_wraps_the_real_catalog() {
        let catalog = ToolCatalog::embedded().unwrap();
        let view: ToolCatalogView<'_> = (&catalog).into();
        assert_eq!(view.tools.len(), catalog.tools().len());
    }
}
