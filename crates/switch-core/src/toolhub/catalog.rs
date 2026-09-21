//! 随包工具清单。
//!
//! 清单是**数据**不是代码：新增一个工具只应该改 `catalog/tools.json`，不改 Rust。
//! 因此这里的职责只有三件——解析、校验、按平台取候选路径。
//!
//! 校验失败的清单直接报错而不是跳过条目：一个字段写错的清单会让界面显示
//! 「未安装」，那是最难排查的一类假信息。宁可整个清单加载失败、在界面上说清楚。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::error::{CoreError, ErrorCode};
use crate::platform::Platform;

/// 随包分发的清单正文。改 `catalog/tools.json` 就会进这个常量。
const CATALOG_JSON: &str = include_str!("../../catalog/tools.json");

/// 本应用能读的清单版本。清单结构变化时递增，并在 `validate` 里拒绝未来版本。
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCategory {
    /// 编码 agent CLI。
    CliCode,
    /// 通用命令行工具。
    Utility,
    /// 语言或包运行时。
    Runtime,
}

impl ToolCategory {
    /// 与前端 `ToolCategory` 的取值一致，界面的分组标签按它查文案。
    pub fn as_str(self) -> &'static str {
        match self {
            ToolCategory::CliCode => "cliCode",
            ToolCategory::Utility => "utility",
            ToolCategory::Runtime => "runtime",
        }
    }
}

/// 各平台的可执行文件候选路径。允许含 `~` 与 `%VAR%`，取值时才展开。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformPaths {
    #[serde(default)]
    pub macos: Vec<String>,
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub linux: Vec<String>,
}

impl PlatformPaths {
    pub fn for_platform(&self, platform: Platform) -> &[String] {
        match platform {
            Platform::Macos => &self.macos,
            Platform::Windows => &self.windows,
            Platform::Linux => &self.linux,
        }
    }

    /// 清单里有没有这个平台的条目。空 = 我们不认识这个平台上的装法。
    pub fn supports(&self, platform: Platform) -> bool {
        !self.for_platform(platform).is_empty()
    }
}

/// 就绪判定：先看静态文件，再跑版本探针。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Readiness {
    /// 版本探针的参数。程序本身取 `command`，这里只给参数，避免清单里出现整条命令行。
    pub version_args: Vec<String>,
    /// 探针成功时的退出码。绝大多数是 0，但显式写出来比隐含约定好。
    pub ok_exit: i32,
    /// 探测结果可复用的秒数。界面必须显示探测时间，所以这个值不是可选的。
    pub cache_seconds: u64,
    /// 任一存在即认为有本地状态文件。空表示没有这类判据。
    #[serde(default)]
    pub static_any: Vec<String>,
}

/// 登录状态判定。缺省表示这个工具没有可判定的登录概念，界面显示「不适用」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthCheck {
    pub args: Vec<String>,
    pub ok_exit: i32,
    /// 探针失败且输出命中这些片段时，判为「已安装·未授权」。
    /// 命中不了就退回「已安装」，**不猜**。
    #[serde(default)]
    pub needs_login_patterns: Vec<String>,
}

/// 给 agent 的用法提示。这些是展示文本，不是本工具要执行的命令。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentUsage {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub non_interactive: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolDescriptor {
    pub id: String,
    /// 展示名。缺当前语言时回落到 `id`。
    pub names: BTreeMap<String, String>,
    pub category: ToolCategory,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
    pub description: String,
    /// 可执行文件名（不是路径）。探测时按 PATH 再找一次，清单里的绝对路径只是兜底。
    pub command: Option<String>,
    #[serde(default)]
    pub config_dir: Option<String>,
    #[serde(default)]
    pub config_file: Option<String>,
    /// 是否必须有配置文件才算可用。默认 false：装了但没配过也是「已安装」。
    #[serde(default)]
    pub require_config_file: bool,
    /// 这个工具有没有模型概念。false 的工具不该出现模型相关入口。
    #[serde(default)]
    pub model_config: bool,
    /// 技能装到哪儿。没有这一项的工具不支持技能安装。
    #[serde(default)]
    pub skills_root: Option<String>,
    pub readiness: Readiness,
    #[serde(default)]
    pub auth_check: Option<AuthCheck>,
    #[serde(default)]
    pub agent_usage: AgentUsage,
    pub paths: PlatformPaths,
}

impl ToolDescriptor {
    /// 按语言取展示名；没有对应语言时回落到 `id`。
    pub fn display_name(&self, locale: &str) -> String {
        self.names
            .get(locale)
            .or_else(|| self.names.get("en"))
            .or_else(|| self.names.values().next())
            .cloned()
            .unwrap_or_else(|| self.id.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogFile {
    schema_version: u32,
    tools: Vec<ToolDescriptor>,
}

/// 加载后的清单。只读，可安全共享。
#[derive(Debug, Clone)]
pub struct ToolCatalog {
    tools: Vec<ToolDescriptor>,
}

impl ToolCatalog {
    /// 解析随包清单。解析或校验失败返回错误，不返回「空清单」。
    pub fn embedded() -> Result<Self, CoreError> {
        Self::parse(CATALOG_JSON)
    }

    pub fn parse(json: &str) -> Result<Self, CoreError> {
        let file: CatalogFile = serde_json::from_str(json).map_err(|error| {
            CoreError::new(
                ErrorCode::CatalogSchemaMismatch,
                "error.toolCatalogUnreadable",
            )
            .with_detail(format!("工具清单不是合法 JSON：{error}"))
        })?;
        if file.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(CoreError::new(
                ErrorCode::CatalogSchemaMismatch,
                "error.toolCatalogVersion",
            )
            .with_detail(format!(
                "工具清单版本为 {}，本版本只支持 {}",
                file.schema_version, SUPPORTED_SCHEMA_VERSION
            )));
        }
        let catalog = Self { tools: file.tools };
        catalog.validate()?;
        Ok(catalog)
    }

    fn validate(&self) -> Result<(), CoreError> {
        if self.tools.is_empty() {
            return Err(CoreError::new(
                ErrorCode::CatalogSchemaMismatch,
                "error.toolCatalogEmpty",
            ));
        }
        let mut seen: Vec<&str> = Vec::with_capacity(self.tools.len());
        for tool in &self.tools {
            let fail = |detail: String| {
                Err(CoreError::new(
                    ErrorCode::CatalogSchemaMismatch,
                    "error.toolCatalogEntryInvalid",
                )
                .with_detail(detail))
            };
            if tool.id.trim().is_empty() {
                return fail("工具 id 不能为空".to_owned());
            }
            if seen.contains(&tool.id.as_str()) {
                return fail(format!("工具 id 重复：{}", tool.id));
            }
            seen.push(&tool.id);
            if tool.command.is_none()
                && !tool.paths.supports(Platform::Macos)
                && !tool.paths.supports(Platform::Windows)
            {
                return fail(format!(
                    "工具 {} 既没有命令名也没有任何平台的候选路径，无法检测",
                    tool.id
                ));
            }
            if tool.readiness.version_args.is_empty() {
                return fail(format!(
                    "工具 {} 没有版本探针参数；没有探针就无法给出「已就绪」结论",
                    tool.id
                ));
            }
            if tool.readiness.cache_seconds == 0 {
                return fail(format!("工具 {} 的探测缓存时间为 0", tool.id));
            }
            if tool
                .readiness
                .static_any
                .iter()
                .any(|item| item.trim().is_empty())
            {
                return fail(format!("工具 {} 的静态判据里有空路径", tool.id));
            }
            if let Some(auth) = &tool.auth_check {
                if auth.args.is_empty() {
                    return fail(format!("工具 {} 声明了登录判定但没有参数", tool.id));
                }
                if auth
                    .needs_login_patterns
                    .iter()
                    .any(|pattern| pattern.trim().is_empty())
                {
                    return fail(format!("工具 {} 的登录特征里有空片段", tool.id));
                }
            }
        }
        Ok(())
    }

    pub fn tools(&self) -> &[ToolDescriptor] {
        &self.tools
    }

    pub fn get(&self, tool_id: &str) -> Option<&ToolDescriptor> {
        self.tools.iter().find(|tool| tool.id == tool_id)
    }

    /// 支持技能安装的工具。只有清单里写了 `skillsRoot` 的才算，
    /// 这样「能不能装技能」永远由数据决定，而不是散在界面里的硬编码白名单。
    pub fn skill_targets(&self) -> Vec<&ToolDescriptor> {
        self.tools
            .iter()
            .filter(|tool| tool.skills_root.is_some())
            .collect()
    }
}

/// 展开路径里的 `~` 与 `%VAR%`。
///
/// 展开不了的路径原样返回：调用方按「不存在」处理，而不是猜一个位置。
pub fn expand_path(raw: &str, home: &std::path::Path) -> std::path::PathBuf {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return home.join(rest);
    }
    if trimmed == "~" {
        return home.to_path_buf();
    }
    if let Some(start) = trimmed.find('%') {
        if let Some(end) = trimmed[start + 1..].find('%') {
            let name = &trimmed[start + 1..start + 1 + end];
            if let Ok(value) = std::env::var(name) {
                let expanded = format!(
                    "{}{}{}",
                    &trimmed[..start],
                    value,
                    &trimmed[start + end + 2..]
                );
                return expand_path(&expanded, home);
            }
        }
    }
    std::path::PathBuf::from(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_parses_and_validates() {
        let catalog = ToolCatalog::embedded().expect("随包清单必须能通过校验");
        assert!(
            catalog.tools().len() >= 10,
            "清单里至少要有一批可检测的工具"
        );
        assert!(catalog.get("codex").is_some());
    }

    #[test]
    fn every_entry_is_detectable_on_at_least_one_platform() {
        let catalog = ToolCatalog::embedded().unwrap();
        for tool in catalog.tools() {
            let any = tool.paths.supports(Platform::Macos)
                || tool.paths.supports(Platform::Windows)
                || tool.paths.supports(Platform::Linux);
            assert!(any || tool.command.is_some(), "{} 无法检测", tool.id);
        }
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let json = r#"{"schemaVersion":1,"tools":[
            {"id":"x","names":{},"category":"utility","description":"","command":"x",
             "readiness":{"versionArgs":["--version"],"okExit":0,"cacheSeconds":60},
             "paths":{"macos":["/x"]}},
            {"id":"x","names":{},"category":"utility","description":"","command":"x",
             "readiness":{"versionArgs":["--version"],"okExit":0,"cacheSeconds":60},
             "paths":{"macos":["/x"]}}
        ]}"#;
        let error = ToolCatalog::parse(json).unwrap_err();
        assert!(
            error.safe_details[0].contains("重复"),
            "{:?}",
            error.safe_details
        );
    }

    #[test]
    fn missing_probe_is_rejected() {
        let json = r#"{"schemaVersion":1,"tools":[
            {"id":"x","names":{},"category":"utility","description":"","command":"x",
             "readiness":{"versionArgs":[],"okExit":0,"cacheSeconds":60},
             "paths":{"macos":["/x"]}}
        ]}"#;
        let error = ToolCatalog::parse(json).unwrap_err();
        assert!(
            error.safe_details[0].contains("探针"),
            "{:?}",
            error.safe_details
        );
    }

    #[test]
    fn future_schema_version_is_refused() {
        let json = r#"{"schemaVersion":99,"tools":[]}"#;
        let error = ToolCatalog::parse(json).unwrap_err();
        assert!(
            error.safe_details[0].contains("版本"),
            "{:?}",
            error.safe_details
        );
    }

    #[test]
    fn expands_home_and_env_vars() {
        let home = std::path::Path::new("/Users/demo");
        assert_eq!(
            expand_path("~/.codex/skills", home),
            home.join(".codex/skills")
        );
        assert_eq!(expand_path("~", home), home);
        assert_eq!(
            expand_path("/opt/homebrew/bin/rg", home),
            std::path::PathBuf::from("/opt/homebrew/bin/rg")
        );
        // 展开不了的环境变量原样保留，交给「不存在」处理。
        assert!(expand_path("%DEFINITELY_NOT_SET%/x", home)
            .to_string_lossy()
            .contains("%DEFINITELY_NOT_SET%"));
    }

    #[test]
    fn display_name_falls_back_to_id() {
        let catalog = ToolCatalog::embedded().unwrap();
        let codex = catalog.get("codex").unwrap();
        assert_eq!(codex.display_name("zh-Hans"), "Codex CLI");
        assert_eq!(codex.display_name("fr"), "Codex CLI");
    }

    #[test]
    fn skill_targets_only_include_tools_with_a_skills_root() {
        let catalog = ToolCatalog::embedded().unwrap();
        let ids: Vec<&str> = catalog
            .skill_targets()
            .iter()
            .map(|tool| tool.id.as_str())
            .collect();
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"claude-code"));
        assert!(
            !ids.contains(&"ffmpeg"),
            "ffmpeg 没有技能目录，不该出现在目标里"
        );
    }
}
