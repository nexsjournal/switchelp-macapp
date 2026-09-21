//! 技能目录来源：从公开公开仓库里读出技能。
//!
//! 目录来自公开源，所以这一层要额外小心两件事：
//! - **只读**。我们只发 GET，只读文件内容，不执行仓库里的任何东西，也不碰 git 协议。
//! - **有上限**。一个仓库能塞多少文件是它说了算，不能让它把内存和界面拖垮。
//!
//! 网络访问抽成 `RepoFetcher`，测试用假实现，不去打真实接口。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::domain::error::{CoreError, ErrorCode};

use super::skill::{self, SkillDocument, MAX_FILES_PER_SKILL, MAX_FILE_BYTES};

/// 一次目录抓取的技能数上限。
pub const MAX_SKILLS_PER_REPO: usize = 60;
/// 单次抓取的字节总量上限。
pub const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
/// 并发读文件的线程数上限。太低没效果，太高对源站不礼貌。
const READ_CONCURRENCY: usize = 6;

/// 从哪抓的。写进归属清单，卸载与更新时才知道自己是从哪一版装的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSourceRef {
    pub skill_id: String,
    pub repo: String,
    pub commit: String,
    pub path: String,
}

/// 仓库里读到的一个技能。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSkill {
    /// 目录名（技能在仓库里的最后一层目录名）。
    pub dir_name: String,
    /// 在仓库里的路径。
    pub source_path: String,
    pub document: SkillDocument,
    /// 相对技能目录的文件清单，含 `SKILL.md`。
    pub files: Vec<RepoFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoFile {
    pub path: String,
    pub bytes: u64,
    pub text: String,
}

/// 一次仓库目录读取的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCatalog {
    pub repo: String,
    /// 解析出的提交 SHA。目录与安装都钉在这一版上，不用浮动的分支名。
    pub commit: String,
    pub skills: Vec<RepoSkill>,
    pub fetched_at: i64,
    /// 读全了多少个技能目录里的文件。超过上限时小于 `skills.len()`。
    pub truncated: bool,
}

/// 解析 `owner/repo`、`owner/repo@ref` 或 `owner/repo#ref` 形式的来源。
pub fn parse_repo_spec(raw: &str) -> Result<(String, Option<String>), CoreError> {
    let trimmed = raw
        .trim()
        .trim_end_matches(".git")
        .trim_end_matches('/')
        .trim_start_matches("https://github.com/")
        .trim_start_matches("http://github.com/")
        .trim_start_matches("github.com/");
    let (repo, git_ref) = trimmed
        .split_once('@')
        .or_else(|| trimmed.split_once('#'))
        .map(|(repo, git_ref)| (repo, Some(git_ref.trim().to_owned())))
        .unwrap_or((trimmed, None));
    let parts: Vec<&str> = repo.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() != 2 {
        return Err(
            CoreError::new(ErrorCode::ValidationFailed, "error.pluginRepoInvalid")
                .with_detail(format!("来源要写成 owner/repo，收到的是：{raw}")),
        );
    }
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    };
    if !valid(parts[0]) || !valid(parts[1]) {
        return Err(
            CoreError::new(ErrorCode::ValidationFailed, "error.pluginRepoInvalid")
                .with_detail(format!("仓库名里有不支持的字符：{raw}")),
        );
    }
    // `.` 与 `..` 能通过字符白名单，但它们是要逃出路径的写法，必须显式拒绝。
    if parts.iter().any(|part| *part == "." || *part == "..") {
        return Err(
            CoreError::new(ErrorCode::ValidationFailed, "error.pluginRepoInvalid")
                .with_detail(format!("仓库名不能是 . 或 ..：{raw}")),
        );
    }
    let git_ref = git_ref.filter(|value| !value.is_empty());
    if let Some(value) = git_ref.as_deref() {
        if !valid(value) || value.starts_with('.') {
            return Err(
                CoreError::new(ErrorCode::ValidationFailed, "error.pluginRepoInvalid")
                    .with_detail(format!("分支或提交名里有不支持的字符：{value}")),
            );
        }
    }
    Ok((format!("{}/{}", parts[0], parts[1]), git_ref))
}

/// 提交里的一个文件：路径 + 字节数。
///
/// 目录阶段只要这两样：技能在哪、装上去会写多少。**正文一律不在这里读**——
/// 见 [`assemble`] 的说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoBlob {
    pub path: String,
    pub size: u64,
}

/// 只读的仓库读取接口。
pub trait RepoFetcher: Send + Sync {
    /// 把分支名解析成提交 SHA。
    fn resolve_commit(&self, repo: &str, git_ref: Option<&str>) -> Result<String, CoreError>;
    /// 列出提交里的**全部文件**（路径 + 字节数）。
    ///
    /// 一次请求拿全：技能清单与「技能目录里还有哪些文件」都从这一份里筛。
    /// 从前这两件事各有一个方法，目录阶段于是按技能数重复下载同一棵 tree
    /// （实测 anthropics/skills：20 个技能 = 21 次 tree 请求，每次 160 KB），
    /// 既慢又白烧 GitHub 的接口限额（未认证只有 60 次/小时）。
    fn list_blobs(&self, repo: &str, commit: &str) -> Result<Vec<RepoBlob>, CoreError>;
    /// 读一个文件。
    fn read_file(&self, repo: &str, commit: &str, path: &str) -> Result<Vec<u8>, CoreError>;
}

/// 一份文件清单里的技能路径（`SKILL.md`），按路径排序。
pub fn skill_paths(blobs: &[RepoBlob]) -> Vec<String> {
    let mut paths: Vec<String> = blobs
        .iter()
        .map(|blob| blob.path.as_str())
        .filter(|path| {
            path.rsplit_once('/')
                .map(|(_, file)| file.eq_ignore_ascii_case("SKILL.md"))
                .unwrap_or(false)
        })
        .map(str::to_owned)
        .collect();
    paths.sort();
    paths
}

/// 某个目录下的文件（含更深一层）。
fn blobs_under<'a>(blobs: &'a [RepoBlob], dir: &str) -> Vec<&'a RepoBlob> {
    if dir.is_empty() {
        return Vec::new();
    }
    let prefix = format!("{dir}/");
    blobs
        .iter()
        .filter(|blob| blob.path.starts_with(&prefix))
        .collect()
}

/// 读一批文件。**并发**：一个仓库有几十个 `SKILL.md`，串行读一轮就是几十秒
/// （实测每个 raw 请求约 1.4 秒），页面看起来就是卡住了。抓取器是 `Sync` 的，
/// 所以用受限的线程池分批读，把「读几十个文件」压到个位数秒。
fn read_many(
    fetcher: &dyn RepoFetcher,
    repo: &str,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CoreError> {
    let workers = paths.len().min(READ_CONCURRENCY).max(1);
    if workers == 1 {
        return paths
            .iter()
            .map(|path| fetcher.read_file(repo, commit, path))
            .collect();
    }
    let mut slots: Vec<Option<Result<Vec<u8>, CoreError>>> = Vec::new();
    slots.resize_with(paths.len(), || None);
    let slots = std::sync::Mutex::new(slots);
    std::thread::scope(|scope| {
        for worker in 0..workers {
            let slots = &slots;
            scope.spawn(move || {
                let mut index = worker;
                while index < paths.len() {
                    let read = fetcher.read_file(repo, commit, &paths[index]);
                    slots.lock().expect("读结果锁")[index] = Some(read);
                    index += workers;
                }
            });
        }
    });
    slots
        .into_inner()
        .expect("读结果锁")
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| Err(CoreError::internal("读取任务没有返回结果"))))
        .collect()
}

/// 把一份文件清单变成技能目录。
///
/// **目录阶段只读 `SKILL.md` 的正文，同目录的其它文件只列清单（路径 + 字节数）。**
/// 这不是省事，是这一页能不能用的分界线：技能的同目录里放着 `references/`、脚本、
/// 甚至字体与 XSD（实测默认源 anthropics/skills：414 个文件、10.4 MB），
/// 为了显示一份清单把它们逐个下下来，就是用户看到的「一直显示正在读取仓库」——
/// 几百个串行 HTTPS 请求、好几分钟，还烧掉 GitHub 未认证限额。正文在
/// [`hydrate`] 里按**选中的技能**补，装什么读什么。
pub fn assemble(
    fetcher: &dyn RepoFetcher,
    repo: &str,
    commit: &str,
    blobs: &[RepoBlob],
    now: i64,
) -> Result<RepoCatalog, CoreError> {
    let mut skills = Vec::new();
    let mut total_bytes = 0usize;
    let mut truncated = false;

    let listed = skill_paths(blobs);
    let wanted: Vec<String> = listed.iter().take(MAX_SKILLS_PER_REPO).cloned().collect();
    if listed.len() > wanted.len() {
        truncated = true;
    }
    // 一次并发读回所有 SKILL.md：它们的正文是目录列表与详情页要显示的东西。
    let markdowns = read_many(fetcher, repo, commit, &wanted)?;

    for (path, markdown) in wanted.iter().zip(markdowns) {
        let Some(dir_name) = skill_directory(path) else {
            continue;
        };
        let source_path = match path.rsplit_once('/') {
            Some((dir, _)) => dir.to_owned(),
            // 仓库根目录下的 SKILL.md 没有目录层，装在以技能名命名的目录里。
            None => dir_name.clone(),
        };

        if markdown.len() > MAX_FILE_BYTES {
            truncated = true;
            continue;
        }
        let text = String::from_utf8_lossy(&markdown).into_owned();
        let document = skill::parse(&text, &dir_name);
        total_bytes += markdown.len();

        // 同目录下的其它文件：只记路径与大小（技能常配 references/、examples/ 之类）。
        let prefix = format!("{source_path}/");
        let mut files = vec![RepoFile {
            path: "SKILL.md".to_owned(),
            bytes: markdown.len() as u64,
            text,
        }];
        for sibling in blobs_under(blobs, &source_path) {
            if sibling.path.ends_with("/SKILL.md") || sibling.path == "SKILL.md" {
                continue;
            }
            if files.len() >= MAX_FILES_PER_SKILL || total_bytes >= MAX_TOTAL_BYTES {
                truncated = true;
                break;
            }
            // 超过单文件上限的同目录文件不列进清单：安装走的是 `RepoFile.text`（字符串），
            // 二进制附件在那一层已经被改坏，宁可不装也不要装个坏的。
            // 待办：把 `RepoFile` 换成能带原始字节（或 base64）的形状，再放开这一条。
            if sibling.size > MAX_FILE_BYTES as u64 {
                truncated = true;
                continue;
            }
            total_bytes += sibling.size as usize;
            files.push(RepoFile {
                path: sibling
                    .path
                    .strip_prefix(&prefix)
                    .unwrap_or(sibling.path.as_str())
                    .to_owned(),
                bytes: sibling.size,
                text: String::new(),
            });
        }

        skills.push(RepoSkill {
            dir_name,
            source_path,
            document,
            files,
        });
    }

    Ok(RepoCatalog {
        repo: repo.to_owned(),
        commit: commit.to_owned(),
        skills,
        fetched_at: now,
        truncated,
    })
}

/// 把**选中技能**的同目录文件正文读回来（目录阶段只列了清单）。
///
/// 装什么读什么：预览与安装只对用户勾选的技能调用它，所以一个仓库里有多少个大文件
/// 都不再影响浏览那一页的速度。失败原样上报——半套文件装上去比失败更糟。
pub fn hydrate(
    fetcher: &dyn RepoFetcher,
    repo: &str,
    commit: &str,
    skill: &mut RepoSkill,
) -> Result<(), CoreError> {
    let wanted: Vec<String> = skill
        .files
        .iter()
        .filter(|file| file.path != "SKILL.md")
        .map(|file| {
            if skill.source_path.is_empty() {
                file.path.clone()
            } else {
                format!("{}/{}", skill.source_path, file.path)
            }
        })
        .collect();
    let bodies = read_many(fetcher, repo, commit, &wanted)?;
    let mut bodies = bodies.into_iter();
    for file in skill.files.iter_mut().filter(|file| file.path != "SKILL.md") {
        let Some(bytes) = bodies.next() else { break };
        file.text = String::from_utf8_lossy(&bytes).into_owned();
        file.bytes = bytes.len() as u64;
    }
    Ok(())
}

/// `skills/demo/SKILL.md` → `demo`。
fn skill_directory(path: &str) -> Option<String> {
    let (dir, file) = path.rsplit_once('/')?;
    if !file.eq_ignore_ascii_case("SKILL.md") {
        return None;
    }
    let name = dir.rsplit('/').next()?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_owned())
}

/// 真实抓取器：GitHub 的公开 REST 接口。
///
/// 只用三个只读端点：仓库信息（拿默认分支）、commit 解析、git tree（拿文件列表）。
/// 文件正文走 `raw.githubusercontent.com`，它不占用 API 的限额。
pub struct GithubFetcher {
    agent: ureq::Agent,
    /// 可选令牌。填了能把 API 限额从每小时 60 次提到 5000 次。
    token: Option<String>,
    api_base: String,
    raw_base: String,
    /// 随包标识。源站至少能看到是谁在请求。
    user_agent: String,
}

impl GithubFetcher {
    pub fn new(token: Option<String>, user_agent: String) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                .proxy(ureq::Proxy::try_from_env())
                .build(),
        );
        Self {
            agent,
            token,
            api_base: "https://api.github.com".to_owned(),
            raw_base: "https://raw.githubusercontent.com".to_owned(),
            user_agent,
        }
    }

    /// 测试与镜像用：指向别的端点。
    pub fn with_bases(
        token: Option<String>,
        user_agent: String,
        api_base: impl Into<String>,
        raw_base: impl Into<String>,
    ) -> Self {
        let mut fetcher = Self::new(token, user_agent);
        fetcher.api_base = api_base.into();
        fetcher.raw_base = raw_base.into();
        fetcher
    }

    fn request(&self, url: &str, accept: &str) -> Result<Vec<u8>, CoreError> {
        let mut request = self
            .agent
            .get(url)
            .header("accept", accept)
            .header("user-agent", &self.user_agent);
        if let Some(token) = self.token.as_deref() {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let mut response = request.call().map_err(|error| {
            CoreError::new(ErrorCode::Internal, "error.pluginFetchUnreachable")
                .with_detail(format!("访问 {url} 失败：{error}"))
        })?;
        let status = response.status().as_u16();
        if status == 404 {
            return Err(
                CoreError::new(ErrorCode::NotFound, "error.pluginRepoNotFound")
                    .with_detail(format!("{url} 返回 404")),
            );
        }
        if status == 403 || status == 429 {
            return Err(
                CoreError::new(ErrorCode::Internal, "error.pluginRateLimited").with_detail(
                    "公开接口的访问频率已用尽，稍后再试或在设置里填一个 GitHub 令牌".to_owned(),
                ),
            );
        }
        if !(200..300).contains(&status) {
            return Err(
                CoreError::new(ErrorCode::Internal, "error.pluginFetchFailed")
                    .with_detail(format!("{url} 返回 {status}")),
            );
        }
        response
            .body_mut()
            .with_config()
            .limit(MAX_FILE_BYTES as u64)
            .read_to_vec()
            .map_err(|error| CoreError::internal(format!("读取 {url} 的响应失败：{error}")))
    }

    fn json(&self, url: &str) -> Result<serde_json::Value, CoreError> {
        let bytes = self.request(url, "application/vnd.github+json")?;
        serde_json::from_slice(&bytes)
            .map_err(|error| CoreError::internal(format!("{url} 的响应不是合法 JSON：{error}")))
    }
}

impl RepoFetcher for GithubFetcher {
    fn resolve_commit(&self, repo: &str, git_ref: Option<&str>) -> Result<String, CoreError> {
        let reference = match git_ref {
            Some(git_ref) => git_ref.to_owned(),
            None => {
                let info = self.json(&format!("{}/repos/{repo}", self.api_base))?;
                info.get("default_branch")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| CoreError::internal(format!("{repo} 没有返回默认分支")))?
                    .to_owned()
            }
        };
        let payload = self.json(&format!(
            "{}/repos/{repo}/commits/{}",
            self.api_base,
            super::percent_encode(&reference)
        ))?;
        payload
            .get("sha")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or_else(|| CoreError::internal(format!("{repo} 没有返回提交 SHA")))
    }

    fn list_blobs(&self, repo: &str, commit: &str) -> Result<Vec<RepoBlob>, CoreError> {
        let payload = self.json(&format!(
            "{}/repos/{repo}/git/trees/{commit}?recursive=1",
            self.api_base
        ))?;
        let tree = payload
            .get("tree")
            .and_then(|value| value.as_array())
            .ok_or_else(|| CoreError::internal(format!("{repo} 的 tree 响应缺少 tree 字段")))?;
        Ok(tree
            .iter()
            .filter(|node| node.get("type").and_then(|value| value.as_str()) == Some("blob"))
            .filter_map(|node| {
                let path = node.get("path").and_then(|value| value.as_str())?;
                // tree 里的每个 blob 都带 size；缺了就按 0 记，装的时候以实际写入为准。
                let size = node.get("size").and_then(|value| value.as_u64()).unwrap_or(0);
                Some(RepoBlob {
                    path: path.to_owned(),
                    size,
                })
            })
            .collect())
    }

    fn read_file(&self, repo: &str, commit: &str, path: &str) -> Result<Vec<u8>, CoreError> {
        self.request(
            &format!(
                "{}/{repo}/{commit}/{}",
                self.raw_base,
                path.split('/')
                    .map(super::percent_encode)
                    .collect::<Vec<_>>()
                    .join("/")
            ),
            "text/plain",
        )
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::collections::HashMap;

    /// 内存抓取器：按「路径 → 内容」提供仓库内容。
    pub struct FakeFetcher {
        pub commit: String,
        pub files: HashMap<String, Vec<u8>>,
    }

    impl FakeFetcher {
        pub fn new(files: &[(&str, &str)]) -> Self {
            Self {
                commit: "deadbeef".to_owned(),
                files: files
                    .iter()
                    .map(|(path, text)| ((*path).to_owned(), text.as_bytes().to_vec()))
                    .collect(),
            }
        }
    }

    impl RepoFetcher for FakeFetcher {
        fn resolve_commit(&self, _repo: &str, _git_ref: Option<&str>) -> Result<String, CoreError> {
            Ok(self.commit.clone())
        }

        fn list_blobs(&self, _repo: &str, _commit: &str) -> Result<Vec<RepoBlob>, CoreError> {
            let mut blobs: Vec<RepoBlob> = self
                .files
                .iter()
                .map(|(path, bytes)| RepoBlob {
                    path: path.clone(),
                    size: bytes.len() as u64,
                })
                .collect();
            blobs.sort_by(|left, right| left.path.cmp(&right.path));
            Ok(blobs)
        }

        fn read_file(&self, _repo: &str, _commit: &str, path: &str) -> Result<Vec<u8>, CoreError> {
            self.files.get(path).cloned().ok_or_else(|| {
                CoreError::new(ErrorCode::NotFound, "error.pluginRepoNotFound")
                    .with_detail(format!("假抓取器里没有 {path}"))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeFetcher;
    use super::*;

    #[test]
    fn parses_plain_and_ref_pinned_specs() {
        assert_eq!(
            parse_repo_spec("owner/repo").unwrap(),
            ("owner/repo".to_owned(), None)
        );
        assert_eq!(
            parse_repo_spec(" owner/repo@v1.2 ").unwrap(),
            ("owner/repo".to_owned(), Some("v1.2".to_owned()))
        );
        assert_eq!(
            parse_repo_spec("owner/repo#main").unwrap(),
            ("owner/repo".to_owned(), Some("main".to_owned()))
        );
        // 直接粘贴浏览器地址也认，并且归一化成 owner/repo。
        assert_eq!(
            parse_repo_spec("https://github.com/owner/repo.git").unwrap(),
            ("owner/repo".to_owned(), None)
        );
        assert_eq!(
            parse_repo_spec("https://github.com/owner/repo/tree/main")
                .unwrap_err()
                .code,
            crate::domain::error::ErrorCode::ValidationFailed,
            "带子路径的地址不猜，直接说写法不对"
        );
    }

    #[test]
    fn malformed_specs_are_rejected_with_a_readable_reason() {
        for bad in [
            "",
            "just-a-name",
            "owner/repo/extra",
            "owner/re po",
            "own er/repo",
            "owner/..",
        ] {
            let error = parse_repo_spec(bad).unwrap_err();
            assert!(
                error.safe_details[0].contains("owner/repo") || !error.safe_details.is_empty(),
                "{bad} 的报错应当可读：{:?}",
                error.safe_details
            );
        }
    }

    #[test]
    fn resolves_skill_directory_from_a_path() {
        assert_eq!(
            skill_directory("skills/demo/SKILL.md").as_deref(),
            Some("demo")
        );
        assert_eq!(skill_directory("SKILL.md"), None, "顶层文件没有目录名");
        assert_eq!(skill_directory("a/b/readme.md"), None);
    }

    /// 目录阶段只列同目录文件的清单（路径 + 大小），**不读正文**。
    ///
    /// 这是「一直显示正在读取仓库」的回归测试：从前每个技能都会把同目录的文件逐个下下来
    /// （默认源实测 394 个文件、10.4 MB、几百个串行请求），而界面只需要一份清单。
    #[test]
    fn assemble_lists_sibling_files_without_downloading_them() {
        let fetcher = FakeFetcher::new(&[
            (
                "skills/demo/SKILL.md",
                "---\nname: demo\ndescription: 演示\n---\n正文",
            ),
            ("skills/demo/references/notes.md", "笔记"),
            ("skills/other/SKILL.md", "---\nname: other\n---\n"),
        ]);
        let blobs = fetcher.list_blobs("owner/repo", &fetcher.commit).unwrap();
        let catalog = assemble(&fetcher, "owner/repo", &fetcher.commit, &blobs, 42).unwrap();
        assert_eq!(catalog.skills.len(), 2);
        assert_eq!(catalog.commit, "deadbeef");
        let demo = catalog
            .skills
            .iter()
            .find(|skill| skill.dir_name == "demo")
            .unwrap();
        assert_eq!(demo.document.id, "demo");
        assert_eq!(demo.source_path, "skills/demo");
        assert_eq!(
            demo.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(),
            vec!["SKILL.md", "references/notes.md"],
        );
        let sibling = &demo.files[1];
        assert_eq!(sibling.bytes, "笔记".len() as u64, "大小来自 tree，不必读正文");
        assert!(sibling.text.is_empty(), "目录阶段不该下载同目录文件的正文");

        // 选中这个技能时才把正文读回来。
        let mut wanted = catalog
            .skills
            .iter()
            .find(|skill| skill.dir_name == "demo")
            .unwrap()
            .clone();
        hydrate(&fetcher, "owner/repo", &fetcher.commit, &mut wanted).unwrap();
        assert_eq!(wanted.files[1].text, "笔记");
    }

    /// 一次目录抓取只请求一次 tree。
    ///
    /// 从前是按技能数重复请求（默认源 20 个技能 = 21 次 160 KB 的 tree），
    /// 既慢又烧 GitHub 未认证的 60 次/小时限额。
    #[test]
    fn browsing_asks_for_the_tree_once() {
        struct Counting<'a> {
            inner: &'a FakeFetcher,
            trees: std::sync::atomic::AtomicUsize,
        }
        impl RepoFetcher for Counting<'_> {
            fn resolve_commit(
                &self,
                repo: &str,
                git_ref: Option<&str>,
            ) -> Result<String, CoreError> {
                self.inner.resolve_commit(repo, git_ref)
            }
            fn list_blobs(&self, repo: &str, commit: &str) -> Result<Vec<RepoBlob>, CoreError> {
                self.trees.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.inner.list_blobs(repo, commit)
            }
            fn read_file(
                &self,
                repo: &str,
                commit: &str,
                path: &str,
            ) -> Result<Vec<u8>, CoreError> {
                self.inner.read_file(repo, commit, path)
            }
        }

        let inner = FakeFetcher::new(&[
            ("skills/a/SKILL.md", "---\nname: a\n---\n"),
            ("skills/a/refs/one.md", "一"),
            ("skills/b/SKILL.md", "---\nname: b\n---\n"),
            ("skills/b/refs/two.md", "二"),
        ]);
        let counting = Counting {
            inner: &inner,
            trees: std::sync::atomic::AtomicUsize::new(0),
        };
        let blobs = counting.list_blobs("owner/repo", &inner.commit).unwrap();
        let catalog = assemble(&counting, "owner/repo", &inner.commit, &blobs, 0).unwrap();
        assert_eq!(catalog.skills.len(), 2);
        assert_eq!(
            counting.trees.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "tree 只该请求一次"
        );
    }

    #[test]
    fn skills_without_a_name_use_the_directory_name() {
        let fetcher = FakeFetcher::new(&[("x/noname/SKILL.md", "正文，没有 front-matter")]);
        let blobs = fetcher.list_blobs("o/r", &fetcher.commit).unwrap();
        let catalog = assemble(&fetcher, "o/r", "deadbeef", &blobs, 0).unwrap();
        assert_eq!(catalog.skills[0].document.id, "noname");
        assert!(!catalog.skills[0].document.front_matter_parsed);
    }
}
