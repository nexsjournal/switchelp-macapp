//! 工具管理：本机有哪些 agent 工作台与命令行工具、装在哪、能不能用。
//!
//! 本模块只做**观察**。清单来自 `catalog/tools.json`，探测走 `probe`，
//! 结论由 `detect` 给出；本层负责缓存与对外的服务接口，不自己判断工具状态。

pub mod catalog;
pub mod detect;
pub mod probe;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::error::{CoreError, ErrorCode},
    platform::Platform,
    storage::HubStore,
};

pub use catalog::{expand_path, ToolCatalog, ToolCategory};
pub use detect::{InstalledTool, PathSource, ToolState, ToolStatus};

/// 可以安装技能的目标工具。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillTarget {
    pub tool_id: String,
    pub display_name: String,
    pub root: String,
}

/// 工具清单与探测的服务。
pub struct ToolHubService {
    catalog: ToolCatalog,
    platform: Platform,
    home: PathBuf,
    store: Arc<dyn HubStore>,
    /// 全量扫描互斥：同时点两次「重新检测」不该并发跑两轮子进程。
    scan: Mutex<()>,
}

impl ToolHubService {
    pub fn new(
        catalog: ToolCatalog,
        platform: Platform,
        home: PathBuf,
        store: Arc<dyn HubStore>,
    ) -> Self {
        Self {
            catalog,
            platform,
            home,
            store,
            scan: Mutex::new(()),
        }
    }

    pub fn catalog(&self) -> &ToolCatalog {
        &self.catalog
    }

    pub fn home(&self) -> &std::path::Path {
        &self.home
    }

    /// 列全部工具状态。
    ///
    /// `refresh = false` 时优先用上次的结论（未过期的话），因此普通页面加载不会
    /// 每次都拉起十几个子进程。过期的、以及「未验证」的结论一律重探。
    pub fn list(&self, locale: &str, refresh: bool, now: i64) -> Result<Vec<ToolState>, CoreError> {
        let _guard = self
            .scan
            .lock()
            .map_err(|_| CoreError::internal("工具扫描锁不可用"))?;

        let cached: Vec<ToolState> = if refresh {
            Vec::new()
        } else {
            self.store.cached_tool_states()?
        };

        let mut states = Vec::with_capacity(self.catalog.tools().len());
        for tool in self.catalog.tools() {
            let reusable = cached
                .iter()
                .find(|state| state.id == tool.id)
                .filter(|state| detect::cache_hit(state, now))
                .cloned();
            let state = match reusable {
                Some(state) => state,
                None => {
                    let fresh = detect::detect_tool(tool, self.platform, &self.home, locale, now);
                    self.store.save_tool_state(&fresh)?;
                    fresh
                }
            };
            states.push(state);
        }
        Ok(states)
    }

    /// 强制重探一个工具。
    pub fn probe_one(&self, tool_id: &str, locale: &str, now: i64) -> Result<ToolState, CoreError> {
        let tool = self
            .catalog
            .get(tool_id)
            .ok_or_else(|| unknown_tool(tool_id))?;
        let state = detect::detect_tool(tool, self.platform, &self.home, locale, now);
        self.store.save_tool_state(&state)?;
        Ok(state)
    }

    /// 展示名。用于插件中心直说「装到了哪个工具」。
    pub fn display_name(&self, tool_id: &str) -> Option<String> {
        self.catalog
            .get(tool_id)
            .map(|tool| tool.display_name("en"))
    }

    /// 支持安装技能的目标。
    pub fn skill_targets(&self) -> Vec<SkillTarget> {
        self.catalog
            .skill_targets()
            .into_iter()
            .filter_map(|tool| {
                let root = tool
                    .skills_root
                    .as_deref()
                    .map(|raw| expand_path(raw, &self.home))?;
                Some(SkillTarget {
                    tool_id: tool.id.clone(),
                    display_name: tool.display_name("en"),
                    root: root.display().to_string(),
                })
            })
            .collect()
    }

    /// 某个工具的技能根目录。**即使目录当前不存在也返回路径**——
    /// 调用方要能区分「不支持装技能」和「支持但目录还没建」这两种情况。
    pub fn skill_root(&self, tool_id: &str) -> Option<PathBuf> {
        let tool = self.catalog.get(tool_id)?;
        let raw = tool.skills_root.as_deref()?;
        Some(expand_path(raw, &self.home))
    }

    /// 某个工具在清单里的描述（用于界面解释「这个工具是干嘛的」）。
    pub fn describe(&self, tool_id: &str) -> Result<ToolState, CoreError> {
        let tool = self
            .catalog
            .get(tool_id)
            .ok_or_else(|| unknown_tool(tool_id))?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        Ok(detect::detect_tool(
            tool,
            self.platform,
            &self.home,
            "en",
            now,
        ))
    }

    /// 供界面显示清单条数这类事实。
    pub fn catalog_size(&self) -> usize {
        self.catalog.tools().len()
    }
}

/// 让人一眼看出「这条清单是给哪个平台的」。
pub fn platform_label(platform: Platform) -> &'static str {
    match platform {
        Platform::Macos => "macos",
        Platform::Windows => "windows",
        Platform::Linux => "linux",
    }
}

/// 清单条目缺失时的错误，集中一处以免各处措辞不一。
pub(crate) fn unknown_tool(tool_id: &str) -> CoreError {
    CoreError::new(ErrorCode::NotFound, "error.toolUnknown")
        .with_detail(format!("工具清单里没有 {tool_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InMemoryHubStore;

    fn service(home: &std::path::Path) -> (ToolHubService, Arc<InMemoryHubStore>) {
        let store = Arc::new(InMemoryHubStore::new());
        let catalog = ToolCatalog::embedded().unwrap();
        let service =
            ToolHubService::new(catalog, Platform::Macos, home.to_path_buf(), store.clone());
        (service, store)
    }

    #[test]
    fn list_returns_one_state_per_catalog_entry() {
        let home = tempfile::tempdir().unwrap();
        let (service, _store) = service(home.path());
        let states = service.list("zh-Hans", true, 1000).unwrap();
        assert_eq!(states.len(), service.catalog_size());
        assert!(states.iter().all(|state| !state.display_name.is_empty()));
    }

    #[test]
    fn cached_states_are_reused_within_the_cache_window() {
        let home = tempfile::tempdir().unwrap();
        let (service, store) = service(home.path());
        let first = service.list("zh-Hans", true, 1000).unwrap();
        assert!(!store.cached_tool_states().unwrap().is_empty());

        // 同一时刻再列一次：probed_at 应当完全一致，说明没有重探。
        let second = service.list("zh-Hans", false, 1000).unwrap();
        assert_eq!(
            first
                .iter()
                .map(|state| state.probed_at)
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|state| state.probed_at)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn refresh_forces_a_new_probe() {
        let home = tempfile::tempdir().unwrap();
        let (service, _store) = service(home.path());
        service.list("zh-Hans", true, 1000).unwrap();
        let refreshed = service.list("zh-Hans", true, 2000).unwrap();
        assert!(refreshed.iter().all(|state| state.probed_at == 2000));
    }

    #[test]
    fn probe_one_updates_only_that_tool() {
        let home = tempfile::tempdir().unwrap();
        let (service, _store) = service(home.path());
        service.list("zh-Hans", true, 1000).unwrap();
        let state = service.probe_one("codex", "zh-Hans", 3000).unwrap();
        assert_eq!(state.id, "codex");
        assert_eq!(state.probed_at, 3000);
    }

    #[test]
    fn probe_one_rejects_an_unknown_tool() {
        let home = tempfile::tempdir().unwrap();
        let (service, _store) = service(home.path());
        assert!(service.probe_one("no-such-tool", "en", 1).is_err());
    }

    #[test]
    fn skill_targets_expose_roots_for_the_two_supported_tools() {
        let home = tempfile::tempdir().unwrap();
        let (service, _store) = service(home.path());
        let targets = service.skill_targets();
        assert!(targets.iter().any(|target| target.tool_id == "codex"));
        assert!(targets.iter().any(|target| target.tool_id == "claude-code"));
        assert!(targets
            .iter()
            .all(|target| target.root.contains(".skills") || target.root.contains("skills")));
        assert_eq!(
            service.skill_root("codex").unwrap(),
            home.path().join(".codex/skills")
        );
        assert!(service.skill_root("ffmpeg").is_none());
    }
}
