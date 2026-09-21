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

/// 只读的仓库读取接口。
pub trait RepoFetcher: Send + Sync {
    /// 把分支名解析成提交 SHA。
    fn resolve_commit(&self, repo: &str, git_ref: Option<&str>) -> Result<String, CoreError>;
    /// 列出仓库里所有 `SKILL.md` 的路径。
    fn list_skill_paths(&self, repo: &str, commit: &str) -> Result<Vec<String>, CoreError>;
    /// 读一个文件。
    fn read_file(&self, repo: &str, commit: &str, path: &str) -> Result<Vec<u8>, CoreError>;
    /// 列出某个目录下的文件（递归）。
    ///
    /// 默认返回空：不支持这个能力的抓取器只会装载 `SKILL.md` 本身，
    /// 不会假装技能里还有别的文件。
    fn list_files_under(
        &self,
        _repo: &str,
        _commit: &str,
        _dir: &str,
    ) -> Result<Vec<String>, CoreError> {
        Ok(Vec::new())
    }
}

/// 把 `SKILL.md` 路径列表变成技能清单：每个技能带上它同目录下的所有文件。
pub fn assemble(
    fetcher: &dyn RepoFetcher,
    repo: &str,
    commit: &str,
    skill_paths: &[String],
    now: i64,
) -> Result<RepoCatalog, CoreError> {
    let mut skills = Vec::new();
    let mut total_bytes = 0usize;
    let mut truncated = false;

    let mut sorted: Vec<&String> = skill_paths.iter().collect();
    sorted.sort();
    for path in sorted.into_iter().take(MAX_SKILLS_PER_REPO) {
        let Some(dir_name) = skill_directory(path) else {
            continue;
        };
        let source_path = match path.rsplit_once('/') {
            Some((dir, _)) => dir.to_owned(),
            // 仓库根目录下的 SKILL.md 没有目录层，装在以技能名命名的目录里。
            None => dir_name.clone(),
        };

        let markdown = fetcher.read_file(repo, commit, path)?;
        if markdown.len() > MAX_FILE_BYTES {
            truncated = true;
            continue;
        }
        let text = String::from_utf8_lossy(&markdown).into_owned();
        let document = skill::parse(&text, &dir_name);

        let mut files = vec![RepoFile {
            path: "SKILL.md".to_owned(),
            bytes: markdown.len() as u64,
            text,
        }];
        total_bytes += markdown.len();

        // 同目录下的其它文件一并带上（技能常配 references/、examples/ 之类）。
        for sibling in fetcher.list_files_under(repo, commit, &source_path)? {
            if files.len() >= MAX_FILES_PER_SKILL {
                truncated = true;
                break;
            }
            if sibling.ends_with("/SKILL.md") || sibling == "SKILL.md" {
                continue;
            }
            if total_bytes >= MAX_TOTAL_BYTES {
                truncated = true;
                break;
            }
            let bytes = fetcher.read_file(repo, commit, &sibling)?;
            if bytes.len() > MAX_FILE_BYTES {
                truncated = true;
                continue;
            }
            total_bytes += bytes.len();
            let relative = sibling
                .strip_prefix(&format!("{source_path}/"))
                .unwrap_or(sibling.as_str())
                .to_owned();
            files.push(RepoFile {
                path: relative,
                bytes: bytes.len() as u64,
                text: String::from_utf8_lossy(&bytes).into_owned(),
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

    fn list_skill_paths(&self, repo: &str, commit: &str) -> Result<Vec<String>, CoreError> {
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
            .filter_map(|node| node.get("path").and_then(|value| value.as_str()))
            .filter(|path| {
                path.rsplit_once('/')
                    .map(|(_, file)| file.eq_ignore_ascii_case("SKILL.md"))
                    .unwrap_or(false)
            })
            .map(str::to_owned)
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

    fn list_files_under(
        &self,
        repo: &str,
        commit: &str,
        dir: &str,
    ) -> Result<Vec<String>, CoreError> {
        self.list_under(repo, commit, dir)
    }
}

/// 列出技能目录下的文件。GitHub 的 tree 接口默认是递归的，
/// 这里单独再查一次同目录，避免把返回体撑大。
impl GithubFetcher {
    fn list_under(&self, repo: &str, commit: &str, dir: &str) -> Result<Vec<String>, CoreError> {
        if dir.is_empty() {
            return Ok(Vec::new());
        }
        let payload = self.json(&format!(
            "{}/repos/{repo}/git/trees/{commit}?recursive=1",
            self.api_base
        ))?;
        let Some(tree) = payload.get("tree").and_then(|value| value.as_array()) else {
            return Ok(Vec::new());
        };
        Ok(tree
            .iter()
            .filter(|node| node.get("type").and_then(|value| value.as_str()) == Some("blob"))
            .filter_map(|node| node.get("path").and_then(|value| value.as_str()))
            .filter(|path| path.starts_with(&format!("{dir}/")))
            .map(str::to_owned)
            .collect())
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

        fn list_skill_paths(&self, _repo: &str, _commit: &str) -> Result<Vec<String>, CoreError> {
            Ok(self
                .files
                .keys()
                .filter(|path| path.ends_with("SKILL.md"))
                .cloned()
                .collect())
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

    #[test]
    fn assemble_collects_sibling_files_into_the_same_skill() {
        let fetcher = FakeFetcher::new(&[
            (
                "skills/demo/SKILL.md",
                "---\nname: demo\ndescription: 演示\n---\n正文",
            ),
            ("skills/demo/references/notes.md", "笔记"),
            ("skills/other/SKILL.md", "---\nname: other\n---\n"),
        ]);
        // 用真实抓取器的同级文件能力需要网络；这里只验证 SKILL.md 的装载。
        let catalog = assemble(
            &fetcher,
            "owner/repo",
            &fetcher.commit,
            &fetcher.list_skill_paths("owner/repo", "deadbeef").unwrap(),
            42,
        )
        .unwrap();
        assert_eq!(catalog.skills.len(), 2);
        assert_eq!(catalog.commit, "deadbeef");
        let demo = catalog
            .skills
            .iter()
            .find(|skill| skill.dir_name == "demo")
            .unwrap();
        assert_eq!(demo.document.id, "demo");
        assert_eq!(demo.files.len(), 1, "假抓取器不提供同级文件");
        assert_eq!(demo.files[0].path, "SKILL.md");
        assert_eq!(demo.source_path, "skills/demo");
    }

    #[test]
    fn skills_without_a_name_use_the_directory_name() {
        let fetcher = FakeFetcher::new(&[("x/noname/SKILL.md", "正文，没有 front-matter")]);
        let catalog = assemble(
            &fetcher,
            "o/r",
            "deadbeef",
            &fetcher.list_skill_paths("o/r", "deadbeef").unwrap(),
            0,
        )
        .unwrap();
        assert_eq!(catalog.skills[0].document.id, "noname");
        assert!(!catalog.skills[0].document.front_matter_parsed);
    }
}
