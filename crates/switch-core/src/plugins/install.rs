//! 技能安装的文件系统操作：写入、归属清单、卸载。
//!
//! 三条决定安全性的规则：
//!
//! 1. **归属清单是唯一依据。** 卸载只删清单里记录过的文件，且删之前逐个核对指纹。
//!    用户改过的文件保留并如实报告——「装上去的东西可回滚」不能变成「你的改动被顺手删了」。
//! 2. **不越界。** 所有写路径先拼进目标技能的到根目录下，再逐段检查没有 `..` 与绝对路径，
//!    最后确认规范化后的父目录仍在技能根目录之内。符号链接不跟随。
//! 3. **先写临时文件再改名。** 中途失败留下的是临时文件而不是半个 `SKILL.md`。

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::domain::error::{CoreError, ErrorCode};

use super::skill::fingerprint;

/// 归属标记文件名。它与被装的技能放在同一个目录里。
pub const MANAGED_MARKER: &str = ".switchelp-managed";

/// 归属清单的当前结构版本。
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// 禁用技能时给目录加的后缀。
///
/// 各个工具对「禁用某个技能」没有统一约定（有的看配置项，有的根本不管），
/// 把目录改名是唯一在各工具上都生效的做法。界面必须说明这是本工具的做法。
pub const DISABLED_SUFFIX: &str = ".disabled";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFingerprint {
    /// 相对技能目录的路径。
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

/// 我们写在目标技能目录里的归属清单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedManifest {
    pub schema_version: u32,
    pub skill_id: String,
    pub dir_name: String,
    /// 装到哪个工具（工具 id）。同一个技能可以同时装在多个工具里。
    pub target_tool: String,
    pub source_repo: String,
    pub source_commit: String,
    pub source_path: String,
    pub installed_at: i64,
    pub files: Vec<FileFingerprint>,
}

/// 一个目标目录的安装动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlannedAction {
    /// 目录不存在，直接新建。
    Create,
    /// 目录里已有我们自己的归属清单，按新内容更新。
    Update,
    /// 目录已存在但**不是我们装的**。默认不动它，要用户明确选择。
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// 一个目标工具的安装计划。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetPlan {
    pub tool_id: String,
    pub display_name: String,
    /// 技能根目录（已展开 `~`）。
    pub root: String,
    /// 将写入的目录（完整路径）。
    pub dir: String,
    /// 目录名。冲突时可能带后缀，因此与技能在仓库里的目录名不一定相同。
    pub dir_name: String,
    pub action: PlannedAction,
    pub files: Vec<PlannedFile>,
    /// 冲突时给用户看的说明：目录里现在有什么。
    pub conflict_detail: Option<String>,
    /// 更新时列出对方目录里不属于本技能的文件（我们不会动它们）。
    pub foreign_files: Vec<String>,
}

/// 检查一段相对路径是否可以安全地拼进根目录。
///
/// 拒绝绝对路径、`..`、以及 Windows 的盘符前缀。不跟随符号链接：
/// 目标目录若是个指向别处的软链，规范化之后会落在根目录之外，同样被拦下。
fn safe_relative(relative: &str) -> Result<PathBuf, CoreError> {
    let path = Path::new(relative);
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(
                    CoreError::new(ErrorCode::ValidationFailed, "error.skillPathUnsafe")
                        .with_detail(format!("技能里含有不安全的相对路径：{relative}")),
                );
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(
            CoreError::new(ErrorCode::ValidationFailed, "error.skillPathUnsafe")
                .with_detail("技能里出现空路径".to_owned()),
        );
    }
    Ok(clean)
}

/// 把技能根目录规范化。根目录不在时直接报错——不替用户创建他不知道的目录。
fn canonical_root(root: &Path) -> Result<PathBuf, CoreError> {
    root.canonicalize().map_err(|error| {
        CoreError::new(ErrorCode::NotFound, "error.skillRootMissing").with_detail(format!(
            "技能根目录 {} 不存在或不可访问：{error}",
            root.display()
        ))
    })
}

/// 校验目标目录确实落在技能根目录之下。
fn ensure_within(root: &Path, dir: &Path) -> Result<PathBuf, CoreError> {
    let canonical_root = canonical_root(root)?;
    // 目标目录可能还不存在，只能逐级找最近的已存在祖先来规范化。
    let mut probe = dir.to_path_buf();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if probe.exists() {
            break;
        }
        let Some(name) = probe.file_name().map(|name| name.to_os_string()) else {
            return Err(
                CoreError::new(ErrorCode::ValidationFailed, "error.skillPathUnsafe")
                    .with_detail(format!("无法解析技能目录 {}", dir.display())),
            );
        };
        suffix.push(name);
        if !probe.pop() {
            return Err(
                CoreError::new(ErrorCode::ValidationFailed, "error.skillPathUnsafe")
                    .with_detail(format!("技能目录 {} 没有可用的上级目录", dir.display())),
            );
        }
    }
    let mut resolved = probe.canonicalize().map_err(|error| {
        CoreError::internal(format!("规范化 {} 失败：{error}", probe.display()))
    })?;
    for name in suffix.iter().rev() {
        resolved.push(name);
    }
    if !resolved.starts_with(&canonical_root) {
        return Err(
            CoreError::new(ErrorCode::ValidationFailed, "error.skillPathUnsafe").with_detail(
                format!(
                    "技能目录 {} 落在技能根目录 {} 之外，已拒绝",
                    resolved.display(),
                    canonical_root.display()
                ),
            ),
        );
    }
    Ok(resolved)
}

/// 读取目标目录里的归属清单。不是我们装的那份返回 `None`。
pub fn read_manifest(dir: &Path) -> Option<ManagedManifest> {
    let raw = fs::read_to_string(dir.join(MANAGED_MARKER)).ok()?;
    let manifest: ManagedManifest = serde_json::from_str(&raw).ok()?;
    (manifest.schema_version == MANIFEST_SCHEMA_VERSION).then_some(manifest)
}

/// 目录里除归属清单外的现存文件（相对路径，排序）。
pub fn existing_files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    collect_files(dir, dir, &mut found);
    found.sort();
    found
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == MANAGED_MARKER {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// 为一个目标目录生成安装计划。**不写任何文件。**
pub fn plan_target(
    root: &Path,
    dir_name: &str,
    target_tool: &str,
    display_name: &str,
    files: &[(String, Vec<u8>)],
) -> Result<TargetPlan, CoreError> {
    for (relative, _) in files {
        safe_relative(relative)?;
    }
    let dir = root.join(dir_name);
    let planned: Vec<PlannedFile> = files
        .iter()
        .map(|(path, bytes)| PlannedFile {
            path: path.clone(),
            bytes: bytes.len() as u64,
            sha256: fingerprint(bytes),
        })
        .collect();

    let mut plan = TargetPlan {
        tool_id: target_tool.to_owned(),
        display_name: display_name.to_owned(),
        root: root.display().to_string(),
        dir: dir.display().to_string(),
        dir_name: dir_name.to_owned(),
        action: PlannedAction::Create,
        files: planned,
        conflict_detail: None,
        foreign_files: Vec::new(),
    };

    if !dir.exists() {
        return Ok(plan);
    }
    match read_manifest(&dir) {
        Some(manifest) => {
            plan.action = PlannedAction::Update;
            let ours: Vec<&str> = manifest
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect();
            plan.foreign_files = existing_files(&dir)
                .into_iter()
                .filter(|path| !ours.contains(&path.as_str()))
                .collect();
        }
        None => {
            plan.action = PlannedAction::Conflict;
            let existing = existing_files(&dir);
            plan.conflict_detail = Some(if existing.is_empty() {
                format!("{} 已存在，但不是本工具安装的", dir.display())
            } else {
                format!(
                    "{} 已存在，且含有 {} 个不是本工具写入的文件",
                    dir.display(),
                    existing.len()
                )
            });
        }
    }
    Ok(plan)
}

/// 执行写入。返回本次实际落盘的文件指纹。
///
/// `dir_name` 由调用方决定（冲突时可能带后缀），这里不再判重。
pub fn write_skill(
    root: &Path,
    dir_name: &str,
    target_tool: &str,
    files: &[(String, Vec<u8>)],
    source: &super::SkillSourceRef,
    now: i64,
) -> Result<ManagedManifest, CoreError> {
    let dir = ensure_within(root, &root.join(dir_name))?;
    fs::create_dir_all(&dir).map_err(|error| {
        CoreError::internal(format!("创建技能目录 {} 失败：{error}", dir.display()))
    })?;

    let mut fingerprints = Vec::with_capacity(files.len());
    for (relative, bytes) in files {
        let relative_path = safe_relative(relative)?;
        let destination = dir.join(&relative_path);
        // 逐级确认父目录仍在技能根目录内。
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                CoreError::internal(format!("创建 {} 失败：{error}", parent.display()))
            })?;
            ensure_within(root, parent)?;
        }
        write_atomically(&destination, bytes)?;
        fingerprints.push(FileFingerprint {
            path: relative_path.to_string_lossy().replace('\\', "/"),
            sha256: fingerprint(bytes),
            bytes: bytes.len() as u64,
        });
    }

    let manifest = ManagedManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        skill_id: source.skill_id.clone(),
        dir_name: dir_name.to_owned(),
        target_tool: target_tool.to_owned(),
        source_repo: source.repo.clone(),
        source_commit: source.commit.clone(),
        source_path: source.path.clone(),
        installed_at: now,
        files: fingerprints,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| CoreError::internal(format!("归属清单序列化失败：{error}")))?;
    write_atomically(&dir.join(MANAGED_MARKER), &manifest_bytes)?;
    Ok(manifest)
}

/// 先写同目录下的临时文件，再改名覆盖。中途失败不会留下半个文件。
fn write_atomically(destination: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let parent = destination
        .parent()
        .ok_or_else(|| CoreError::internal("目标文件没有父目录"))?;
    let temporary = parent.join(format!(
        ".switchelp-tmp-{}",
        destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_owned())
    ));
    fs::write(&temporary, bytes).map_err(|error| {
        CoreError::internal(format!("写入 {} 失败：{error}", temporary.display()))
    })?;
    fs::rename(&temporary, destination).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        CoreError::internal(format!("替换 {} 失败：{error}", destination.display()))
    })
}

/// 卸载结果。`kept` 与 `skipped` 是**必须报给用户**的事实，不是日志。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallOutcome {
    pub dir: String,
    pub removed_files: Vec<String>,
    /// 指纹与清单不一致（用户改过）而未删除的文件。
    pub kept_modified: Vec<String>,
    /// 清单里有但磁盘上已经没有的文件。不是错误，但要说出来。
    pub missing_files: Vec<String>,
    /// 清单之外、我们没有动过的文件。
    pub foreign_files: Vec<String>,
    pub removed_dir: bool,
}

/// 卸载一个技能目录。
///
/// 只删归属清单里记录过、且指纹一致的文件。目录里若有清单之外的内容，
/// 目录本身保留——**绝不递归删一个可能装着用户东西的目录**。
pub fn uninstall_skill(root: &Path, dir_name: &str) -> Result<UninstallOutcome, CoreError> {
    let dir = ensure_within(root, &root.join(dir_name))?;
    let manifest = read_manifest(&dir).ok_or_else(|| {
        CoreError::new(ErrorCode::NotFound, "error.skillManifestMissing").with_detail(format!(
            "{} 里没有本工具的归属清单，拒绝删除",
            dir.display()
        ))
    })?;

    let mut outcome = UninstallOutcome {
        dir: dir.display().to_string(),
        removed_files: Vec::new(),
        kept_modified: Vec::new(),
        missing_files: Vec::new(),
        foreign_files: Vec::new(),
        removed_dir: false,
    };

    for file in &manifest.files {
        let relative = safe_relative(&file.path)?;
        let path = dir.join(&relative);
        if !path.exists() {
            outcome.missing_files.push(file.path.clone());
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| {
            CoreError::internal(format!("读取 {} 失败：{error}", path.display()))
        })?;
        if fingerprint(&bytes) == file.sha256 {
            fs::remove_file(&path).map_err(|error| {
                CoreError::internal(format!("删除 {} 失败：{error}", path.display()))
            })?;
            outcome.removed_files.push(file.path.clone());
        } else {
            outcome.kept_modified.push(file.path.clone());
        }
    }

    // 技能可能带子目录（references/ 之类），删完文件后要把空目录一并收掉，
    // 否则目录永远「不为空」，卸载看起来像没成功。
    prune_empty_subdirs(&dir);
    outcome.foreign_files = existing_files(&dir);
    let _ = fs::remove_file(dir.join(MANAGED_MARKER));

    // 只在目录已经空掉时才删它。
    if outcome.foreign_files.is_empty()
        && dir
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
    {
        fs::remove_dir(&dir).map_err(|error| {
            CoreError::internal(format!("删除 {} 失败：{error}", dir.display()))
        })?;
        outcome.removed_dir = true;
    }
    prune_empty_parents(&dir, root);
    Ok(outcome)
}

/// 递归删掉目录下的空子目录（自底向上）。
fn prune_empty_subdirs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        prune_empty_subdirs(&path);
        let empty = path
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if empty {
            let _ = fs::remove_dir(&path);
        }
    }
}

/// 清掉卸载后留下的空目录（例如技能带子目录时的中间层）。
fn prune_empty_parents(dir: &Path, root: &Path) {
    let mut current = dir.parent().map(Path::to_path_buf);
    while let Some(path) = current {
        if !path.starts_with(root) || path == root {
            break;
        }
        let empty = path
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !empty {
            break;
        }
        if fs::remove_dir(&path).is_err() {
            break;
        }
        current = path.parent().map(Path::to_path_buf);
    }
}

/// 启用 / 禁用：目录改名。返回改名后的目录名。
pub fn set_enabled(root: &Path, dir_name: &str, enabled: bool) -> Result<String, CoreError> {
    let source = ensure_within(root, &root.join(dir_name))?;
    if !source.exists() {
        return Err(CoreError::new(ErrorCode::NotFound, "error.skillDirMissing")
            .with_detail(format!("{} 不存在", source.display())));
    }
    let target = if enabled {
        let base = dir_name.strip_suffix(DISABLED_SUFFIX).unwrap_or(dir_name);
        root.join(base)
    } else {
        root.join(format!("{dir_name}{DISABLED_SUFFIX}"))
    };
    let target = ensure_within(root, &target)?;
    if target.exists() {
        return Err(
            CoreError::conflict("error.skillDirOccupied").with_detail(format!(
                "{} 已存在，无法改名；请先处理该目录",
                target.display()
            )),
        );
    }
    fs::rename(&source, &target).map_err(|error| {
        CoreError::internal(format!(
            "把 {} 改名为 {} 失败：{error}",
            source.display(),
            target.display()
        ))
    })?;
    let manifest_path = target.join(MANAGED_MARKER);
    if let Some(mut manifest) = read_manifest(&target) {
        manifest.dir_name = target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Ok(bytes) = serde_json::to_vec_pretty(&manifest) {
            let _ = fs::write(&manifest_path, bytes);
        }
    }
    Ok(target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default())
}

/// 带后缀的目录名，用于冲突时「保留两者」。
pub fn suffixed_dir_name(dir_name: &str, existing: &[String]) -> String {
    for index in 2..100 {
        let candidate = format!("{dir_name}-{index}");
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    format!("{dir_name}-copy")
}

/// 按现有目录名集合挑一个不冲突的名字。
pub fn choose_dir_name(preferred: &str, taken: &BTreeMap<String, ()>) -> String {
    if !taken.contains_key(preferred) {
        return preferred.to_owned();
    }
    suffixed_dir_name(preferred, &taken.keys().cloned().collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::SkillSourceRef;

    fn source() -> SkillSourceRef {
        SkillSourceRef {
            skill_id: "demo".to_owned(),
            repo: "owner/repo".to_owned(),
            commit: "abc123".to_owned(),
            path: "skills/demo".to_owned(),
        }
    }

    fn files() -> Vec<(String, Vec<u8>)> {
        vec![
            (
                "SKILL.md".to_owned(),
                b"---\nname: demo\n---\nbody".to_vec(),
            ),
            ("references/notes.md".to_owned(), b"notes".to_vec()),
        ]
    }

    fn root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();
        dir
    }

    #[test]
    fn install_then_uninstall_round_trips_and_leaves_nothing() {
        let temp = root();
        let skills = temp.path().join("skills");
        let manifest =
            write_skill(&skills, "demo", "codex", &files(), &source(), 1_700_000_000).unwrap();
        assert_eq!(manifest.files.len(), 2);
        assert!(skills.join("demo/SKILL.md").is_file());
        assert!(skills.join("demo/references/notes.md").is_file());
        assert!(skills.join(format!("demo/{MANAGED_MARKER}")).is_file());

        let outcome = uninstall_skill(&skills, "demo").unwrap();
        assert_eq!(outcome.removed_files.len(), 2);
        assert!(outcome.removed_dir);
        assert!(!skills.join("demo").exists(), "卸载后目录应当不存在");
        assert_eq!(
            std::fs::read_dir(&skills).unwrap().count(),
            0,
            "技能根目录下不应留下任何东西"
        );
    }

    #[test]
    fn uninstall_keeps_files_the_user_edited() {
        let temp = root();
        let skills = temp.path().join("skills");
        write_skill(&skills, "demo", "codex", &files(), &source(), 1).unwrap();
        std::fs::write(skills.join("demo/SKILL.md"), "用户自己改过的内容").unwrap();

        let outcome = uninstall_skill(&skills, "demo").unwrap();
        assert_eq!(outcome.kept_modified, vec!["SKILL.md".to_owned()]);
        assert!(skills.join("demo/SKILL.md").is_file(), "改过的文件必须留下");
        assert!(!skills.join("demo/references/notes.md").exists());
    }

    #[test]
    fn uninstall_never_deletes_foreign_files_or_the_directory() {
        let temp = root();
        let skills = temp.path().join("skills");
        write_skill(&skills, "demo", "codex", &files(), &source(), 1).unwrap();
        std::fs::write(skills.join("demo/mine.md"), "用户放在这里的东西").unwrap();

        let outcome = uninstall_skill(&skills, "demo").unwrap();
        assert_eq!(outcome.foreign_files, vec!["mine.md".to_owned()]);
        assert!(!outcome.removed_dir);
        assert!(skills.join("demo/mine.md").is_file());
    }

    #[test]
    fn uninstall_refuses_a_directory_we_do_not_own() {
        let temp = root();
        let skills = temp.path().join("skills");
        std::fs::create_dir_all(skills.join("demo")).unwrap();
        std::fs::write(skills.join("demo/SKILL.md"), "别人装的").unwrap();
        let error = uninstall_skill(&skills, "demo").unwrap_err();
        assert!(
            error.safe_details[0].contains("拒绝删除"),
            "{:?}",
            error.safe_details
        );
        assert!(skills.join("demo/SKILL.md").is_file());
    }

    #[test]
    fn planning_detects_foreign_directory_as_conflict_and_lists_its_files() {
        let temp = root();
        let skills = temp.path().join("skills");
        std::fs::create_dir_all(skills.join("demo")).unwrap();
        std::fs::write(skills.join("demo/SKILL.md"), "别人的").unwrap();
        std::fs::write(skills.join("demo/other.md"), "别人的").unwrap();

        let plan = plan_target(&skills, "demo", "codex", "Codex CLI", &files()).unwrap();
        assert_eq!(plan.action, PlannedAction::Conflict);
        assert!(plan.conflict_detail.as_deref().map(str::to_owned).is_some());
        assert_eq!(plan.files.len(), 2, "计划里仍然要列出将写入的文件");
    }

    #[test]
    fn planning_marks_our_own_directory_as_update_and_lists_foreign_files() {
        let temp = root();
        let skills = temp.path().join("skills");
        write_skill(&skills, "demo", "codex", &files(), &source(), 1).unwrap();
        std::fs::write(skills.join("demo/notes-of-user.md"), "x").unwrap();

        let plan = plan_target(&skills, "demo", "codex", "Codex CLI", &files()).unwrap();
        assert_eq!(plan.action, PlannedAction::Update);
        assert_eq!(plan.foreign_files, vec!["notes-of-user.md".to_owned()]);
    }

    #[test]
    fn traversal_paths_are_rejected_before_anything_is_written() {
        let temp = root();
        let skills = temp.path().join("skills");
        let evil = vec![("../escaped.md".to_owned(), b"nope".to_vec())];
        let error = write_skill(&skills, "demo", "codex", &evil, &source(), 1).unwrap_err();
        assert!(
            error.safe_details[0].contains("不安全"),
            "{:?}",
            error.safe_details
        );
        assert!(!temp.path().join("escaped.md").exists());
    }

    #[test]
    fn absolute_paths_are_rejected() {
        let error = safe_relative("/etc/passwd").unwrap_err();
        assert!(error.safe_details[0].contains("不安全"));
    }

    #[test]
    fn missing_root_is_reported_instead_of_created() {
        let temp = tempfile::tempdir().unwrap();
        let absent = temp.path().join("no-such-root");
        let error = write_skill(&absent, "demo", "codex", &files(), &source(), 1).unwrap_err();
        assert!(
            error.safe_details[0].contains("不存在"),
            "{:?}",
            error.safe_details
        );
        assert!(!absent.exists(), "不能替用户创建技能根目录");
    }

    #[test]
    fn enable_and_disable_rename_the_directory() {
        let temp = root();
        let skills = temp.path().join("skills");
        write_skill(&skills, "demo", "codex", &files(), &source(), 1).unwrap();

        let disabled = set_enabled(&skills, "demo", false).unwrap();
        assert_eq!(disabled, "demo.disabled");
        assert!(skills.join("demo.disabled/SKILL.md").is_file());
        assert!(!skills.join("demo").exists());
        let manifest = read_manifest(&skills.join("demo.disabled")).unwrap();
        assert_eq!(manifest.dir_name, "demo.disabled");

        let enabled = set_enabled(&skills, "demo.disabled", true).unwrap();
        assert_eq!(enabled, "demo");
        assert!(skills.join("demo/SKILL.md").is_file());
    }

    #[test]
    fn disable_refuses_when_the_target_name_is_taken() {
        let temp = root();
        let skills = temp.path().join("skills");
        write_skill(&skills, "demo", "codex", &files(), &source(), 1).unwrap();
        std::fs::create_dir_all(skills.join("demo.disabled")).unwrap();
        let error = set_enabled(&skills, "demo", false).unwrap_err();
        assert!(
            error.safe_details[0].contains("已存在"),
            "{:?}",
            error.safe_details
        );
    }

    #[test]
    fn conflicting_names_get_a_suffix() {
        assert_eq!(suffixed_dir_name("demo", &[]), "demo-2");
        assert_eq!(suffixed_dir_name("demo", &["demo-2".to_owned()]), "demo-3");
    }
}
