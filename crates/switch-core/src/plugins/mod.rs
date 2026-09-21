//! 插件中心：把公开仓库里的技能装进本机 agent 工作台。
//!
//! 这个模块不碰云端目录、不做账号体系；目录来源是**用户自己填的公开仓库**，
//! 配上三个经核实的预置来源。安装动作只有一件事——往目标工具的技能目录写文件，
//! 并留下可回滚的归属清单（见 `install.rs`）。

pub mod install;
pub mod skill;
pub mod source;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::error::{CoreError, ErrorCode},
    storage::{HubStore, Repository},
    toolhub::ToolHubService,
};

pub use install::{
    FileFingerprint, ManagedManifest, PlannedAction, PlannedFile, TargetPlan, UninstallOutcome,
};
pub use skill::SkillDocument;
pub use source::{GithubFetcher, RepoCatalog, RepoFetcher, RepoSkill, SkillSourceRef};

/// 预置来源。三个都经实测存在且含 `SKILL.md`（2026-09-21 核对）。
pub const DEFAULT_SOURCES: [(&str, &str, &str); 3] = [
    (
        "anthropics/skills",
        "Anthropic 官方技能集合",
        "官方公开的 Agent Skills，包含文档、设计与协作相关的技能。",
    ),
    (
        "obra/superpowers",
        "Superpowers",
        "社区维护的技能框架，覆盖头脑风暴、排查与并行协作等做法。",
    ),
    (
        "wshobson/agents",
        "Agents 插件合集",
        "面向编码 agent 的插件与技能合集，数量多但取向偏工程。",
    ),
];

/// 用户自己添加的来源存在设置表里的键。
const SOURCES_KEY: &str = "plugins.sources";

/// 一个可浏览的来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub repo: String,
    pub label: String,
    pub description: String,
    /// 是否预置。预置来源不可删除，避免用户把自己唯一的入口删掉。
    pub builtin: bool,
}

/// 已安装技能的一条记录。**按（技能，工具）拆行**：同一个技能装到两个工具里
/// 是两条记录，各自可以单独禁用与卸载。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    pub skill_id: String,
    pub dir_name: String,
    pub target_tool: String,
    /// 目标工具的展示名，用于界面直说「装到了哪里」。
    pub target_display_name: String,
    pub source_repo: String,
    pub source_commit: String,
    pub source_path: String,
    pub installed_path: String,
    pub enabled: bool,
    pub installed_at: i64,
    pub files: Vec<FileFingerprint>,
}

/// 冲突时用户的处置方式。**没有「覆盖」**：目标目录不是我们装的时，
/// 覆盖就意味着删除别人的文件，不提供这个选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictChoice {
    /// 跳过这个目标，其它目标照装。
    Skip,
    /// 装到带后缀的新目录里（`<dir>-2`），两边都留着。
    KeepBoth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub repo: String,
    pub git_ref: Option<String>,
    /// 要安装的技能目录名（仓库里的那层目录）。
    pub skill_dirs: Vec<String>,
    /// 装到哪些工具（工具 id）。
    pub targets: Vec<String>,
    /// 冲突处置，键是 `工具id::目录名`。缺省按 `Skip` 处理。
    #[serde(default)]
    pub conflict_choices: BTreeMap<String, ConflictChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPlan {
    pub skill_id: String,
    pub dir_name: String,
    pub source_path: String,
    pub description: Option<String>,
    pub requires_bins: Vec<String>,
    pub targets: Vec<TargetPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPreview {
    pub repo: String,
    pub commit: String,
    pub skills: Vec<SkillPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedTarget {
    pub tool_id: String,
    pub dir_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedTarget {
    pub tool_id: String,
    pub dir_name: String,
    pub message: String,
}

/// 安装结果。**部分完成是常态**，所以三个列表都返回给界面，
/// 不把「3 个成功 2 个失败」压成一句「安装失败」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub repo: String,
    pub commit: String,
    pub installed: Vec<SkillRecord>,
    pub skipped: Vec<SkippedTarget>,
    pub failed: Vec<FailedTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub skill_id: String,
    pub target_tool: String,
    pub current_commit: String,
    pub latest_commit: String,
}

/// 安装编排：取目录、为每个目标生成计划、按用户选择落盘。
pub struct PluginService {
    store: Arc<dyn HubStore>,
    repository: Arc<dyn Repository>,
    tools: Arc<ToolHubService>,
    fetcher: Arc<dyn RepoFetcher>,
}

impl PluginService {
    pub fn new(
        store: Arc<dyn HubStore>,
        repository: Arc<dyn Repository>,
        tools: Arc<ToolHubService>,
        fetcher: Arc<dyn RepoFetcher>,
    ) -> Self {
        Self {
            store,
            repository,
            tools,
            fetcher,
        }
    }

    pub fn sources(&self) -> Result<Vec<SourceSummary>, CoreError> {
        let mut sources: Vec<SourceSummary> = DEFAULT_SOURCES
            .iter()
            .map(|(repo, label, description)| SourceSummary {
                repo: (*repo).to_owned(),
                label: (*label).to_owned(),
                description: (*description).to_owned(),
                builtin: true,
            })
            .collect();
        for repo in self.user_sources()? {
            if sources.iter().any(|source| source.repo == repo) {
                continue;
            }
            sources.push(SourceSummary {
                label: repo.clone(),
                description: String::new(),
                builtin: false,
                repo,
            });
        }
        Ok(sources)
    }

    fn user_sources(&self) -> Result<Vec<String>, CoreError> {
        let raw = self.repository.setting(SOURCES_KEY)?;
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default())
    }

    /// 添加一个来源。只校验写法与是否已存在，不在这里联网验证——
    /// 验证发生在第一次浏览时，失败原因会直接显示在那一页。
    pub fn add_source(&self, repo_spec: &str) -> Result<Vec<SourceSummary>, CoreError> {
        let (repo, _) = source::parse_repo_spec(repo_spec)?;
        let mut current = self.user_sources()?;
        if !current.iter().any(|item| item == &repo) {
            current.push(repo);
            self.repository.set_setting(
                SOURCES_KEY,
                &serde_json::to_string(&current).unwrap_or_default(),
            )?;
        }
        self.sources()
    }

    pub fn remove_source(&self, repo: &str) -> Result<Vec<SourceSummary>, CoreError> {
        let mut current = self.user_sources()?;
        current.retain(|item| item != repo);
        self.repository.set_setting(
            SOURCES_KEY,
            &serde_json::to_string(&current).unwrap_or_default(),
        )?;
        self.sources()
    }

    /// 读一个来源的技能目录。目录与安装都钉在解析出的提交上。
    ///
    /// **只读 `SKILL.md` 的正文**：同目录的其它文件只列清单（路径 + 大小），正文留给
    /// [`Self::preview`] / [`Self::install`] 按选中的技能补。这样一个仓库里有多少大文件
    /// 都不影响浏览这一页——从前每个技能都会重下一遍整棵 git tree 并把同目录文件全读下来
    /// （实测默认源 anthropics/skills：21 次 tree 请求 + 394 个文件 10.4 MB），
    /// 页面停在「正在读取仓库」好几分钟。
    pub fn browse(&self, repo_spec: &str, now: i64) -> Result<RepoCatalog, CoreError> {
        let (repo, git_ref) = source::parse_repo_spec(repo_spec)?;
        let commit = self.fetcher.resolve_commit(&repo, git_ref.as_deref())?;
        let blobs = self.fetcher.list_blobs(&repo, &commit)?;
        if source::skill_paths(&blobs).is_empty() {
            return Err(
                CoreError::new(ErrorCode::NotFound, "error.pluginRepoHasNoSkills").with_detail(
                    format!(
                        "{repo} 在提交 {} 里没有任何 SKILL.md",
                        &commit[..commit.len().min(8)]
                    ),
                ),
            );
        }
        source::assemble(self.fetcher.as_ref(), &repo, &commit, &blobs, now)
    }

    /// 生成安装计划。**不写文件**，界面据此展示将写入什么、哪里会冲突。
    pub fn preview(&self, request: &InstallRequest) -> Result<InstallPreview, CoreError> {
        let mut catalog = self.browse(&spec_of(request), 0)?;
        // 先取出仓库与提交：下面要拿 `skills` 的可变借用，同时还得知道往哪儿读文件。
        let repo = catalog.repo.clone();
        let commit = catalog.commit.clone();
        let selected: Vec<&mut RepoSkill> = catalog
            .skills
            .iter_mut()
            .filter(|skill| {
                request.skill_dirs.is_empty() || request.skill_dirs.contains(&skill.dir_name)
            })
            .collect();
        if selected.is_empty() {
            return Err(
                CoreError::new(ErrorCode::ValidationFailed, "error.pluginSelectionEmpty")
                    .with_detail("没有选中任何技能".to_owned()),
            );
        }

        let targets = self.resolve_targets(&request.targets)?;
        let mut skills = Vec::with_capacity(selected.len());
        for skill in selected {
            // 目录阶段只列了清单，正文在这里按**选中的技能**补——装什么读什么。
            source::hydrate(self.fetcher.as_ref(), &repo, &commit, skill)?;
            let mut planned = Vec::new();
            for (tool_id, display_name, root) in &targets {
                let files = to_file_pairs(skill);
                // 冲突时「保留两者」要装进带后缀的目录，计划里就得先把名字算出来，
                // 否则界面看到的目录名和实际写入的目录名会是两个。
                let mut dir_name = skill.dir_name.clone();
                if request
                    .conflict_choices
                    .get(&format!("{tool_id}::{}", skill.dir_name))
                    == Some(&ConflictChoice::KeepBoth)
                {
                    dir_name = next_available_dir(root, &skill.dir_name);
                }
                planned.push(install::plan_target(
                    root,
                    &dir_name,
                    tool_id,
                    display_name,
                    &files,
                )?);
            }
            skills.push(SkillPlan {
                skill_id: skill.document.id.clone(),
                dir_name: skill.dir_name.clone(),
                source_path: skill.source_path.clone(),
                description: skill.document.description.clone(),
                requires_bins: skill.document.requires_bins.clone(),
                targets: planned,
            });
        }
        Ok(InstallPreview {
            repo: catalog.repo,
            commit: catalog.commit,
            skills,
        })
    }

    /// 执行安装。逐目标独立，失败的不会连累已成功的。
    pub fn install(&self, request: &InstallRequest, now: i64) -> Result<InstallReport, CoreError> {
        let preview = self.preview(request)?;
        let mut catalog = self.browse(&spec_of(request), now)?;
        let repo = catalog.repo.clone();
        let commit = catalog.commit.clone();
        // 写盘用的是文件正文，所以这里也要把选中技能的正文读回来（目录阶段只列了清单）。
        for plan in &preview.skills {
            if let Some(skill) = catalog
                .skills
                .iter_mut()
                .find(|skill| skill.dir_name == plan.dir_name)
            {
                source::hydrate(self.fetcher.as_ref(), &repo, &commit, skill)?;
            }
        }
        let mut report = InstallReport {
            repo: preview.repo.clone(),
            commit: preview.commit.clone(),
            installed: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
        };

        for plan in &preview.skills {
            let Some(skill) = catalog
                .skills
                .iter()
                .find(|skill| skill.dir_name == plan.dir_name)
            else {
                continue;
            };
            let files = to_file_pairs(skill);
            for target in &plan.targets {
                let root = std::path::PathBuf::from(&target.root);
                let choice = request
                    .conflict_choices
                    .get(&format!("{}::{}", target.tool_id, plan.dir_name));
                let dir_name = match (target.action, choice) {
                    (PlannedAction::Conflict, None)
                    | (PlannedAction::Conflict, Some(ConflictChoice::Skip)) => {
                        report.skipped.push(SkippedTarget {
                            tool_id: target.tool_id.clone(),
                            dir_name: plan.dir_name.clone(),
                            reason: target
                                .conflict_detail
                                .clone()
                                .unwrap_or_else(|| "目标目录已存在且不是本工具安装的".to_owned()),
                        });
                        continue;
                    }
                    (PlannedAction::Conflict, Some(ConflictChoice::KeepBoth)) => {
                        next_available_dir(&root, &plan.dir_name)
                    }
                    _ => plan.dir_name.clone(),
                };

                let source_ref = SkillSourceRef {
                    skill_id: skill.document.id.clone(),
                    repo: catalog.repo.clone(),
                    commit: catalog.commit.clone(),
                    path: skill.source_path.clone(),
                };
                // 计划里已经算好了实际目录名（可能带后缀），这里不再重算一遍——
                // 两处各算一次迟早会出现「计划显示 alpha-2、实际装进 alpha」。
                let dir_name = if target.dir_name.is_empty() {
                    dir_name
                } else {
                    target.dir_name.clone()
                };
                match install::write_skill(
                    &root,
                    &dir_name,
                    &target.tool_id,
                    &files,
                    &source_ref,
                    now,
                ) {
                    Ok(manifest) => {
                        let record = SkillRecord {
                            skill_id: manifest.skill_id.clone(),
                            dir_name: manifest.dir_name.clone(),
                            target_tool: manifest.target_tool.clone(),
                            target_display_name: target.display_name.clone(),
                            source_repo: manifest.source_repo.clone(),
                            source_commit: manifest.source_commit.clone(),
                            source_path: manifest.source_path.clone(),
                            installed_path: root.join(&dir_name).display().to_string(),
                            enabled: true,
                            installed_at: now,
                            files: manifest.files.clone(),
                        };
                        self.store.save_skill(&record)?;
                        report.installed.push(record);
                    }
                    Err(error) => report.failed.push(FailedTarget {
                        tool_id: target.tool_id.clone(),
                        dir_name: plan.dir_name.clone(),
                        message: error
                            .safe_details
                            .first()
                            .cloned()
                            .unwrap_or_else(|| error.message_key.clone()),
                    }),
                }
            }
        }
        Ok(report)
    }

    pub fn installed(&self) -> Result<Vec<SkillRecord>, CoreError> {
        let mut records = self.store.list_skills()?;
        // 磁盘上的目录名可能与记录不同（启用/禁用会改名），以记录为准排序。
        records.sort_by(|left, right| {
            left.skill_id
                .cmp(&right.skill_id)
                .then_with(|| left.target_tool.cmp(&right.target_tool))
        });
        Ok(records)
    }

    /// 启用 / 禁用：目录改名，随后更新记录。
    pub fn set_enabled(
        &self,
        skill_id: &str,
        target_tool: &str,
        enabled: bool,
    ) -> Result<SkillRecord, CoreError> {
        let mut record = self
            .store
            .list_skills()?
            .into_iter()
            .find(|record| record.skill_id == skill_id && record.target_tool == target_tool)
            .ok_or_else(|| CoreError::not_found("已安装技能"))?;
        let target = self
            .tools
            .skill_root(target_tool)
            .ok_or_else(|| CoreError::not_found("目标工具的技能目录"))?;
        let renamed = install::set_enabled(&target, &record.dir_name, enabled)?;
        record.dir_name = renamed;
        record.enabled = enabled;
        record.installed_path = target.join(&record.dir_name).display().to_string();
        self.store.save_skill(&record)?;
        Ok(record)
    }

    /// 卸载。返回每个目标的处置结果——改过的文件留在原处会被如实列出来。
    pub fn uninstall(
        &self,
        skill_id: &str,
        targets: &[String],
    ) -> Result<Vec<install::UninstallOutcome>, CoreError> {
        let records: Vec<SkillRecord> = self
            .store
            .list_skills()?
            .into_iter()
            .filter(|record| record.skill_id == skill_id)
            .filter(|record| targets.is_empty() || targets.contains(&record.target_tool))
            .collect();
        if records.is_empty() {
            return Err(CoreError::not_found("已安装技能"));
        }
        let mut outcomes = Vec::new();
        for record in records {
            let root = self
                .tools
                .skill_root(&record.target_tool)
                .ok_or_else(|| CoreError::not_found("目标工具的技能目录"))?;
            let outcome = install::uninstall_skill(&root, &record.dir_name)?;
            self.store
                .delete_skill(&record.skill_id, &record.target_tool)?;
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    /// 检查更新：按仓库分组，每个仓库只解析一次最新提交。
    pub fn check_updates(&self) -> Result<Vec<UpdateInfo>, CoreError> {
        let records = self.store.list_skills()?;
        let repos: BTreeSet<String> = records.iter().map(|r| r.source_repo.clone()).collect();
        let mut latest: BTreeMap<String, Option<String>> = BTreeMap::new();
        for repo in repos {
            let resolved = self.fetcher.resolve_commit(&repo, None).ok();
            latest.insert(repo, resolved);
        }
        let mut updates = Vec::new();
        for record in records {
            let Some(Some(latest_commit)) = latest.get(&record.source_repo) else {
                // 仓库解析失败时不报「有更新」，也不报「已是最新」——什么都不说。
                continue;
            };
            if latest_commit != &record.source_commit {
                updates.push(UpdateInfo {
                    skill_id: record.skill_id,
                    target_tool: record.target_tool,
                    current_commit: record.source_commit,
                    latest_commit: latest_commit.clone(),
                });
            }
        }
        Ok(updates)
    }

    /// 把工具 id 解析成（id、展示名、技能根目录）。目标必须是清单里声明了技能目录的工具。
    fn resolve_targets(
        &self,
        targets: &[String],
    ) -> Result<Vec<(String, String, std::path::PathBuf)>, CoreError> {
        let requested: Vec<String> = if targets.is_empty() {
            self.tools
                .skill_targets()
                .into_iter()
                .map(|target| target.tool_id)
                .collect()
        } else {
            targets.to_vec()
        };
        let mut resolved = Vec::new();
        for tool_id in requested {
            let display_name = self.tools.display_name(&tool_id).ok_or_else(|| {
                CoreError::new(ErrorCode::NotFound, "error.pluginTargetUnknown")
                    .with_detail(format!("清单里没有工具 {tool_id}"))
            })?;
            let root = self.tools.skill_root(&tool_id).ok_or_else(|| {
                CoreError::new(
                    ErrorCode::CapabilityUnsupported,
                    "error.pluginTargetUnsupported",
                )
                .with_detail(format!("{tool_id} 不支持安装技能"))
            })?;
            resolved.push((tool_id, display_name, root));
        }
        if resolved.is_empty() {
            return Err(
                CoreError::new(ErrorCode::CapabilityUnsupported, "error.pluginNoTargets")
                    .with_detail(
                        "没有可用的目标工具：清单里没有任何声明了技能目录的工具".to_owned(),
                    ),
            );
        }
        Ok(resolved)
    }
}

/// 取一个当前不存在的目录名。
fn next_available_dir(root: &std::path::Path, dir_name: &str) -> String {
    let mut taken: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.filter_map(Result::ok) {
            taken.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    install::suffixed_dir_name(dir_name, &taken)
}

/// 安装请求对应的来源写法：带 ref 时拼成 `owner/repo@ref`。
fn spec_of(request: &InstallRequest) -> String {
    match &request.git_ref {
        Some(git_ref) => format!("{}@{git_ref}", request.repo),
        None => request.repo.clone(),
    }
}

fn to_file_pairs(skill: &RepoSkill) -> Vec<(String, Vec<u8>)> {
    skill
        .files
        .iter()
        .map(|file| (file.path.clone(), file.text.as_bytes().to_vec()))
        .collect()
}

/// 路径里的每一段单独做百分号编码。用于把分支名、文件名安全地拼进 URL。
pub(crate) fn percent_encode(segment: &str) -> String {
    percent_encoding::utf8_percent_encode(segment, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        storage::InMemoryHubStore, toolhub::catalog::ToolCatalog, toolhub::ToolHubService,
    };

    fn service_with(
        files: &[(&str, &str)],
        home: &std::path::Path,
    ) -> (PluginService, tempfile::TempDir) {
        let store = Arc::new(InMemoryHubStore::new());
        let repository = Arc::new(crate::storage::InMemoryRepository::new());
        let catalog = ToolCatalog::embedded().unwrap();
        let tools = Arc::new(ToolHubService::new(
            catalog,
            crate::platform::Platform::Macos,
            home.to_path_buf(),
            store.clone(),
        ));
        let fetcher = Arc::new(source::fake::FakeFetcher::new(files));
        (
            PluginService::new(store, repository, tools, fetcher),
            tempfile::tempdir().unwrap(),
        )
    }

    fn repo_files() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "skills/alpha/SKILL.md",
                "---\nname: alpha\ndescription: 第一个技能\n---\n正文",
            ),
            ("skills/beta/SKILL.md", "---\nname: beta\n---\n正文"),
        ]
    }

    fn install_request() -> InstallRequest {
        InstallRequest {
            repo: "owner/repo".to_owned(),
            git_ref: None,
            skill_dirs: vec!["alpha".to_owned()],
            targets: vec!["codex".to_owned()],
            conflict_choices: BTreeMap::new(),
        }
    }

    #[test]
    fn default_sources_are_listed_and_marked_builtin() {
        let (service, _temp) = service_with(&repo_files(), std::path::Path::new("/tmp"));
        let sources = service.sources().unwrap();
        assert_eq!(sources.len(), DEFAULT_SOURCES.len());
        assert!(sources.iter().all(|source| source.builtin));
        assert!(sources
            .iter()
            .any(|source| source.repo == "anthropics/skills"));
    }

    #[test]
    fn user_sources_are_added_normalised_and_removable() {
        let (service, _temp) = service_with(&repo_files(), std::path::Path::new("/tmp"));
        let sources = service.add_source("  Some/Repo@main ").unwrap();
        let added = sources.iter().find(|s| s.repo == "Some/Repo").unwrap();
        assert!(!added.builtin);
        let sources = service.remove_source("Some/Repo").unwrap();
        assert!(!sources.iter().any(|s| s.repo == "Some/Repo"));
    }

    #[test]
    fn malformed_source_is_rejected() {
        let (service, _temp) = service_with(&repo_files(), std::path::Path::new("/tmp"));
        assert!(service.add_source("not-a-repo").is_err());
    }

    #[test]
    fn browse_lists_every_skill_in_the_repository() {
        let (service, _temp) = service_with(&repo_files(), std::path::Path::new("/tmp"));
        let catalog = service.browse("owner/repo", 7).unwrap();
        assert_eq!(catalog.skills.len(), 2);
        assert_eq!(catalog.commit, "deadbeef");
        assert_eq!(catalog.fetched_at, 7);
    }

    #[test]
    fn preview_never_writes_files() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex/skills")).unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());
        let preview = service.preview(&install_request()).unwrap();
        assert_eq!(preview.skills.len(), 1);
        assert_eq!(preview.skills[0].targets[0].action, PlannedAction::Create);
        assert_eq!(service.installed().unwrap().len(), 0);
        assert!(
            !home.path().join(".codex/skills/alpha").exists(),
            "preview 不能落盘"
        );
    }

    #[test]
    fn install_writes_the_skill_and_records_it() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex/skills")).unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());

        let report = service.install(&install_request(), 100).unwrap();
        assert_eq!(report.installed.len(), 1);
        assert!(report.failed.is_empty());
        let record = &report.installed[0];
        assert_eq!(record.skill_id, "alpha");
        assert_eq!(record.target_tool, "codex");
        assert_eq!(record.source_commit, "deadbeef");
        assert!(home.path().join(".codex/skills/alpha/SKILL.md").is_file());

        let installed = service.installed().unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].target_display_name, "Codex CLI");
    }

    #[test]
    fn install_can_go_to_two_tools_at_once() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex/skills")).unwrap();
        std::fs::create_dir_all(home.path().join(".claude/skills")).unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());

        let mut request = install_request();
        request.targets = vec!["codex".to_owned(), "claude-code".to_owned()];
        let report = service.install(&request, 100).unwrap();
        assert_eq!(report.installed.len(), 2);
        assert!(home.path().join(".codex/skills/alpha/SKILL.md").is_file());
        assert!(home.path().join(".claude/skills/alpha/SKILL.md").is_file());
        assert_eq!(service.installed().unwrap().len(), 2);
    }

    #[test]
    fn conflict_is_skipped_by_default_and_reported() {
        let home = tempfile::tempdir().unwrap();
        let foreign = home.path().join(".codex/skills/alpha");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("SKILL.md"), "别人装的").unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());

        let report = service.install(&install_request(), 100).unwrap();
        assert!(report.installed.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].reason.contains("不是本工具"));
        assert_eq!(
            std::fs::read_to_string(foreign.join("SKILL.md")).unwrap(),
            "别人装的",
            "冲突时绝不能覆盖别人的文件"
        );
    }

    #[test]
    fn keep_both_installs_into_a_suffixed_directory() {
        let home = tempfile::tempdir().unwrap();
        let foreign = home.path().join(".codex/skills/alpha");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("SKILL.md"), "别人装的").unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());

        let mut request = install_request();
        request
            .conflict_choices
            .insert("codex::alpha".to_owned(), ConflictChoice::KeepBoth);
        let report = service.install(&request, 100).unwrap();
        assert_eq!(report.installed.len(), 1);
        assert_eq!(report.installed[0].dir_name, "alpha-2");
        assert!(home.path().join(".codex/skills/alpha-2/SKILL.md").is_file());
        assert_eq!(
            std::fs::read_to_string(foreign.join("SKILL.md")).unwrap(),
            "别人装的"
        );
    }

    #[test]
    fn updating_our_own_install_does_not_need_a_conflict_choice() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex/skills")).unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());
        service.install(&install_request(), 100).unwrap();

        let preview = service.preview(&install_request()).unwrap();
        assert_eq!(preview.skills[0].targets[0].action, PlannedAction::Update);

        let report = service.install(&install_request(), 200).unwrap();
        assert_eq!(report.installed.len(), 1, "重装自己的技能不算冲突");
        assert_eq!(report.installed[0].installed_at, 200);
    }

    #[test]
    fn uninstall_removes_the_record_and_the_files() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex/skills")).unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());
        service.install(&install_request(), 100).unwrap();

        let outcomes = service.uninstall("alpha", &["codex".to_owned()]).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].removed_dir);
        assert!(service.installed().unwrap().is_empty());
        assert!(!home.path().join(".codex/skills/alpha").exists());
    }

    #[test]
    fn enable_toggle_updates_both_disk_and_record() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex/skills")).unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());
        service.install(&install_request(), 100).unwrap();

        let record = service.set_enabled("alpha", "codex", false).unwrap();
        assert!(!record.enabled);
        assert_eq!(record.dir_name, "alpha.disabled");
        assert!(home.path().join(".codex/skills/alpha.disabled").is_dir());

        let record = service.set_enabled("alpha", "codex", true).unwrap();
        assert!(record.enabled);
        assert_eq!(record.dir_name, "alpha");
    }

    #[test]
    fn unknown_target_is_refused_with_a_readable_reason() {
        let home = tempfile::tempdir().unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());
        let mut request = install_request();
        request.targets = vec!["ffmpeg".to_owned()];
        let error = service.preview(&request).unwrap_err();
        assert!(
            error.safe_details[0].contains("不支持"),
            "{:?}",
            error.safe_details
        );
    }

    #[test]
    fn empty_selection_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());
        let mut request = install_request();
        request.skill_dirs = vec!["does-not-exist".to_owned()];
        assert!(service.preview(&request).is_err());
    }

    #[test]
    fn updates_are_detected_only_when_the_repository_moved_on() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex/skills")).unwrap();
        let (service, _temp) = service_with(&repo_files(), home.path());
        service.install(&install_request(), 100).unwrap();

        // 假抓取器的提交固定，所以此刻不该报出更新。
        assert!(service.check_updates().unwrap().is_empty());
    }
}
