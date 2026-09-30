//! 整仓下载：一次 tarball 换掉三次 REST 调用。
//!
//! 为什么要换：GitHub 公开 REST 接口匿名只有 60 次/小时，而浏览一个仓库要按顺序问三次
//! ——仓库信息（默认分支）→ 解析 commit → git tree。用户从列表进详情、装完退回列表、
//! 切走再切回来，每次都重新浏览同一个来源，十几趟就能把一小时的额度磨光，界面于是长期
//! 停在「访问频率已用尽」。codeload 的整仓下载是**另一条计费路径**：匿名可下、响应里没有
//! 限额头（实测 REST 配额 `remaining: 0` 时连续下载仍然全部 200）。
//!
//! 一次下载顺带解决三件事：
//! - `ref` 传 `HEAD` 时由服务端自己解析默认分支，不必先问一次「默认分支是谁」；
//! - `legacy.tar.gz` 的最外层目录名带短 sha（`anthropics-skills-8a1541c/`），提交号从
//!   目录名里读出来，不必再打一次 commit 接口（同源的普通 `tar.gz` 给的是 `skills-HEAD/`，
//!   没有 sha，所以只用 legacy 那种）。归档第一行的 PAX 全局头里还有**完整** sha，
//!   与目录名互为校验后取长的那个，见 [`pick_commit`]；
//! - 文件正文已经在压缩包里，同一个提交的 `read_file` 不再联网——从前预览与安装每读一个
//!   同目录文件就是一次 `raw.githubusercontent.com` 请求。
//!
//! 代价是整仓进内存，所以体积上限必须写死（见下面四个常量），而且要在**下载之前**就挡住。

use std::{
    collections::HashMap,
    io::Read,
    path::{Component, Path},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::domain::error::{CoreError, ErrorCode};

use super::source::{RepoBlob, RepoFetcher};

/// codeload 是 GitHub 的归档主机：只发字节，不参与 REST 的限额计费。
const CODELOAD_BASE: &str = "https://codeload.github.com";

/// `HEAD` 交给服务端解析默认分支（默认分支叫 `master` 的仓库同样有效）。
const DEFAULT_REF: &str = "HEAD";

/// 压缩包字节上限。
///
/// 上限必须挡在下载之前（先读 `Content-Length`，超了连 GET 都不发）：整仓 tarball 是要
/// 整个进内存的，等流式检查触发时，几十 MB 的带宽与内存已经花掉了。25 MB 是量出来的余量：
/// 预置源里最大的 `anthropics/skills` 4.0 MB、`obra/superpowers` 641 KB，日常用到的
/// `git/git` 12.8 MB，正常仓库翻一倍也够。
const MAX_ARCHIVE_BYTES: u64 = 25 * 1024 * 1024;

/// 解压后字节上限。
///
/// 压缩比没有下限：几十 KB 的 tar.gz 能解出几十 GB 的零（压缩炸弹），所以挡完压缩包还要
/// 挡解压结果。数字要给正常仓库留足余量——实测 `git/git` 的压缩比是 4.2 倍（12.5 MB →
/// 52.8 MB），25 MB 的包按 5 倍算就是 125 MB，所以放到 256 MB：这条上限管的是**不封顶的
/// 爆炸**，不是给大仓库设门槛。真正在内存里常驻的由 [`MAX_RETAINED_BYTES`] 管。
const MAX_DECOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;

/// 单文件正文的保留上限。**取值与 `skill::MAX_FILE_BYTES` 相同**：`assemble` 用同一个
/// 门槛决定「同目录文件列不列进清单」，两边取值不同就会出现清单里有名字、正文却读不到的
/// 条目。超过上限的文件丢弃正文，但名字与真实大小留在 `list_blobs` 里，`assemble` 据此
/// 把 `RepoCatalog.truncated` 标出来。
const MAX_FILE_BYTES: u64 = super::skill::MAX_FILE_BYTES as u64;

/// 一棵树里保留的正文总量上限（不含解压时的临时缓冲）。
///
/// 它比解压上限小得多，因为这才是**常驻内存**的那一份：到达上限之后的文件只留名字与大小。
/// 正常来源够不着（`anthropics/skills` 全部正文 11.7 MB）。
const MAX_RETAINED_BYTES: u64 = 24 * 1024 * 1024;

/// 进程内最多留几棵树，满了丢最旧的。
///
/// 每棵树的正文最多 [`MAX_RETAINED_BYTES`]，所以最坏是 4 × 24 MB 常驻；
/// 预置 4 个来源正好各留一棵，用户手填的来源会把最旧的顶掉。
const MAX_CACHED_TREES: usize = 4;

/// 可变 ref（`HEAD`、分支、标签）解析出来的树能活多久（秒）。
///
/// 提交的内容不会变，但 `HEAD` 会前进：`check_updates` 正是靠重新解析 `HEAD` 看到新提交的，
/// 不设寿命就会永远拿同一个提交跟安装记录比，永远报「已是最新」。取值与
/// `plugins::CATALOG_TTL_SECONDS` 一致——两条缓存按同一个节奏走，不会一条新一条旧。
const TREE_TTL_SECONDS: i64 = 300;

/// 短 sha 的长度区间。codeload 给的是 7 位，上限放到 40 位以容忍上游改成完整 sha。
const SHORT_SHA_LEN: std::ops::RangeInclusive<usize> = 7..=40;

/// 一次抓取的体积上限。测试与镜像可以整体换掉，不必真造一个 25 MB 的仓库。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// 压缩包字节上限。
    pub archive_bytes: u64,
    /// 解压后字节上限。
    pub decompressed_bytes: u64,
    /// 单个文件保留正文的上限。
    pub file_bytes: u64,
    /// 一棵树保留正文的总量上限。
    pub retained_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            archive_bytes: MAX_ARCHIVE_BYTES,
            decompressed_bytes: MAX_DECOMPRESSED_BYTES,
            file_bytes: MAX_FILE_BYTES,
            retained_bytes: MAX_RETAINED_BYTES,
        }
    }
}

/// 真实抓取器：codeload 的整仓 tar.gz，一次下载读完整棵树。
///
/// 与 [`super::source::GithubFetcher`] 实现同一个接口，装配处可以互换；
/// 两个抓取器的网络细节（代理、UA、超时）故意保持一致，免得同一条链路上两套行为。
pub struct TarballFetcher {
    agent: ureq::Agent,
    /// 归档主机。测试与镜像指到别处。
    base: String,
    /// 随包标识。源站至少能看到是谁在请求。
    user_agent: String,
    limits: Limits,
    ttl_seconds: i64,
    /// 进程内的树缓存。**只在这一层缓存**：上层 `PluginService` 缓存的是目录（带 TTL），
    /// 而这一层要挡住的是「同一个提交被重复下载」——预览与安装会在目录之后把文件正文
    /// 再读一遍，那些读操作不该各打一次网络。
    trees: Mutex<Vec<Cached>>,
}

/// 缓存里的一棵树。`key` 是 `owner/repo@ref`；`repo` 单独留着，因为按提交查找时要跨 key
/// 匹配同一个仓库。
struct Cached {
    key: String,
    repo: String,
    fetched_at: i64,
    tree: Arc<Tree>,
}

/// 一次解压出来的树。
struct Tree {
    /// 提交号（见 [`pick_commit`]）。安装记录与更新检查都钉在它上面。
    commit: String,
    /// 全部普通文件（路径已剥掉最外层目录），按路径排序。
    blobs: Vec<RepoBlob>,
    /// 保留了正文的文件。超过体积上限的文件在 `blobs` 里有名字、这里没有。
    files: HashMap<String, Vec<u8>>,
}

impl TarballFetcher {
    /// 应用里的装配方式。
    pub fn new(user_agent: String) -> Self {
        Self::with_base(user_agent, CODELOAD_BASE)
    }

    /// 测试与镜像用：把归档主机指到别处。
    pub fn with_base(user_agent: String, base: impl Into<String>) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                // 正文比 REST 的响应大一个量级（实测 4–13 MB），30 秒只够拿到响应头；
                // 正文单独给一个宽限，否则一次慢速下载会被当成断网。
                .timeout_recv_body(Some(Duration::from_secs(180)))
                // GitHub 的归档主机在部分地区直连不通，用户开着代理时它本该走代理
                // ——与 `GithubFetcher` 同一条规则：见 `platform::proxy::outbound_proxy`。
                .proxy(crate::platform::proxy::outbound_proxy(
                    crate::platform::Platform::current(),
                ))
                .build(),
        );
        Self {
            agent,
            base: base.into().trim_end_matches('/').to_owned(),
            user_agent,
            limits: Limits::default(),
            ttl_seconds: TREE_TTL_SECONDS,
            trees: Mutex::new(Vec::new()),
        }
    }

    /// 换一套体积上限。测试用：真造一个 25 MB 的仓库太慢，把数字压小能测到同一条分支。
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// 换树缓存里可变 ref 的寿命。`0` 表示每次都重新解析，测试用。
    pub fn with_ttl_seconds(mut self, seconds: i64) -> Self {
        self.ttl_seconds = seconds;
        self
    }

    /// 拼下载地址。
    ///
    /// `ref` 只允许字面上的仓库名/提交号字符（`parse_repo_spec` 已经把关），这里再挡一次：
    /// 这个字符串要拼进 URL 路径，多一个 `/` 就能把请求换到别的仓库或别的端点上。
    fn archive_url(&self, repo: &str, reference: &str) -> Result<String, CoreError> {
        if !is_url_safe(reference) {
            return Err(
                CoreError::new(ErrorCode::ValidationFailed, "error.pluginRepoInvalid")
                    .with_detail(format!("分支或提交名里有不支持的字符：{reference}")),
            );
        }
        Ok(format!("{}/{repo}/legacy.tar.gz/{reference}", self.base))
    }

    /// 只读一次响应头，看归档报多大。
    ///
    /// 拿不到（分块响应、镜像不支持 HEAD）就返回 `None`：预检是省流量的机会，
    /// 不是必过的一关，流式上限还在后面兜着。
    fn declared_length(&self, url: &str) -> Result<Option<u64>, CoreError> {
        let response = self
            .agent
            .head(url)
            .header("user-agent", &self.user_agent)
            .call()
            .map_err(|error| unreachable(url, &error))?;
        let status = response.status().as_u16();
        // 405/501：这个主机不认 HEAD。不该因此说仓库不可用，交给 GET 去拿。
        if status == 405 || status == 501 {
            return Ok(None);
        }
        check_status(url, status)?;
        Ok(content_length(response.headers()))
    }

    /// 下载并解出一棵树。
    fn download(&self, repo: &str, reference: &str) -> Result<Arc<Tree>, CoreError> {
        let url = self.archive_url(repo, reference)?;
        let declared = self.declared_length(&url)?;
        if let Some(declared) = declared.filter(|declared| *declared > self.limits.archive_bytes) {
            return Err(archive_too_large(format!(
                "{url} 报出 {}，超过整仓下载上限 {}，没有下载",
                human_bytes(declared),
                human_bytes(self.limits.archive_bytes)
            )));
        }

        let mut response = self
            .agent
            .get(&url)
            .header("user-agent", &self.user_agent)
            .call()
            .map_err(|error| unreachable(&url, &error))?;
        let status = response.status().as_u16();
        check_status(&url, status)?;
        // 预检没读到、头里却有的情况（例如 HEAD 被 405 挡掉）：能挡一次是一次。
        if let Some(declared) = declared.or_else(|| content_length(response.headers())) {
            if declared > self.limits.archive_bytes {
                return Err(archive_too_large(format!(
                    "{url} 的正文有 {}，超过整仓下载上限 {}，已中断",
                    human_bytes(declared),
                    human_bytes(self.limits.archive_bytes)
                )));
            }
        }

        let bytes = match response
            .body_mut()
            .with_config()
            // 上限 +1：刚好等于上限的包要能下完，超出的那个字节才是证据。
            .limit(self.limits.archive_bytes + 1)
            .read_to_vec()
        {
            Ok(bytes) => bytes,
            // 分块响应没有 `Content-Length`，预检帮不上忙，只能在这里截住。
            Err(ureq::Error::BodyExceedsLimit(_)) => {
                return Err(archive_too_large(format!(
                    "{url} 的正文超过整仓下载上限 {}，已中断",
                    human_bytes(self.limits.archive_bytes)
                )))
            }
            Err(error) => {
                return Err(
                    CoreError::new(ErrorCode::Internal, "error.pluginFetchUnreachable")
                        .with_detail(format!("读取 {url} 的正文失败：{error}")),
                )
            }
        };
        self.parse(&url, &bytes).map(Arc::new)
    }

    /// 把 tar.gz 解成一棵树。**不写盘**：解压全程在内存里，条目只当字节流看。
    ///
    /// 解压是**边读边数**的（见 [`Budgeted`]）：解压上限一到就报错，而不是先把解压结果
    /// 整块拿在手里——文本仓库实测压缩比 3.0–4.2 倍（`anthropics/skills` 3.9 MB → 11.8 MB、
    /// `git/git` 12.5 MB → 52.8 MB），按「先解压再解析」的写法，25 MB 的包就要 100 MB 常驻，
    /// 而其实只有保留的正文（≤ [`MAX_RETAINED_BYTES`]）需要留住。
    fn parse(&self, url: &str, archive: &[u8]) -> Result<Tree, CoreError> {
        let overflow = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let bounded = Budgeted {
            inner: flate2::read::GzDecoder::new(archive),
            seen: 0,
            limit: self.limits.decompressed_bytes,
            overflow: overflow.clone(),
        };
        // 归档要先绑在一个变量上：`entries()` 借的是它，不能用临时值。
        let mut archive = tar::Archive::new(bounded);
        let entries = archive.entries().map_err(|error| {
            archive_error(url, &error, &overflow, self.limits.decompressed_bytes)
        })?;
        let mut root: Option<String> = None;
        let mut short_commit = String::new();
        let mut full_commit: Option<String> = None;
        let mut blobs: Vec<RepoBlob> = Vec::new();
        let mut files: HashMap<String, Vec<u8>> = HashMap::new();
        let mut retained = 0u64;

        for entry in entries {
            let mut entry = entry.map_err(|error| {
                archive_error(url, &error, &overflow, self.limits.decompressed_bytes)
            })?;
            let entry_type = entry.header().entry_type();
            // PAX 全局头是元数据，不进树。codeload 的归档第一条就是它（typeflag `g`），
            // 名字写作 `pax_global_header`——照「第一条的名字就是仓库根目录」去认，
            // 会拿它当最外层目录名，然后一个文件都留不下。它带着完整提交号，见下。
            if entry_type.is_pax_global_extensions() {
                full_commit = pax_comment(&mut entry);
                continue;
            }
            let Ok(path) = entry.path() else {
                // 名字不是合法路径（罕见）。丢掉这一条，不因为一个坏条目废掉整棵树。
                continue;
            };
            let Some((head, rest)) = split_entry_path(&path) else {
                // 绝对路径、`..`、非 UTF-8：这一条本身就不可信，丢掉。
                continue;
            };
            match root.as_deref() {
                None => {
                    // 最外层目录名只从**文件或目录**条目上认：其它类型（GNU 长名、稀疏块）
                    // 也带名字，都不是仓库里的东西。
                    if !entry_type.is_file() && !entry_type.is_dir() {
                        continue;
                    }
                    // 提交号从最外层目录名里读。读不出来就直接失败：把 `HEAD` 这种 ref 名
                    // 当提交号写进归属清单，会让「检查更新」拿两个不可比的东西比。
                    let Some(sha) = short_sha(&head) else {
                        return Err(CoreError::new(
                            ErrorCode::Internal,
                            "error.pluginArchiveUnsupported",
                        )
                        .with_detail(format!(
                            "{url} 的最外层目录名是 {head}，里面没有提交号（要的是 legacy.tar.gz）"
                        )));
                    };
                    short_commit = sha;
                    root = Some(head);
                }
                // 归档里混进了别的根目录：不猜它属于谁，丢掉。
                Some(existing) if existing != head => continue,
                Some(_) => {}
            }
            // 顶层目录自己的条目（`owner-repo-sha/`）：不是文件，也没有相对路径。
            if rest.is_empty() {
                continue;
            }
            // **只留普通文件**：符号链接、硬链接、目录、设备节点都不进树。
            // 这是「仓库里的东西只当数据看」的底线，写成文件时也不会有链接被跟着走。
            if !entry_type.is_file() {
                continue;
            }

            let size = entry.header().size().unwrap_or(0);
            blobs.push(RepoBlob {
                path: rest.clone(),
                size,
            });
            // 超限的文件丢弃正文，但名字与真实大小已经在清单里——`assemble` 拿这个大小
            // 判断「这个技能装不完整」，于是 `RepoCatalog.truncated` 是真的。
            if size > self.limits.file_bytes
                || retained.saturating_add(size) > self.limits.retained_bytes
            {
                continue;
            }
            let mut content = Vec::with_capacity(size as usize);
            entry.read_to_end(&mut content).map_err(|error| {
                archive_error(url, &error, &overflow, self.limits.decompressed_bytes)
            })?;
            retained += content.len() as u64;
            files.insert(rest, content);
        }

        if root.is_none() {
            return Err(
                CoreError::new(ErrorCode::Internal, "error.pluginArchiveUnsupported")
                    .with_detail(format!("{url} 里没有任何文件")),
            );
        }
        blobs.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(Tree {
            commit: pick_commit(&short_commit, full_commit),
            blobs,
            files,
        })
    }

    /// 按 ref 取树。`HEAD`/分支/标签都可能指向新的提交，所以要受 [`TREE_TTL_SECONDS`] 约束。
    fn tree_for_ref(&self, repo: &str, git_ref: Option<&str>) -> Result<Arc<Tree>, CoreError> {
        let reference = git_ref.unwrap_or(DEFAULT_REF);
        let key = format!("{repo}@{reference}");
        if let Some(tree) = self.cached_by_key(&key) {
            return Ok(tree);
        }
        let tree = self.download(repo, reference)?;
        self.remember(key, repo, tree.clone());
        Ok(tree)
    }

    /// 按提交取树。提交的内容不会变，所以**不看寿命**：同一个提交的 `read_file` 永远是
    /// 内存里的这一份，不再联网（预览与安装读同目录文件走的正是这条路）。
    fn tree_for_commit(&self, repo: &str, commit: &str) -> Result<Arc<Tree>, CoreError> {
        if let Some(tree) = self.cached_by_commit(repo, commit) {
            return Ok(tree);
        }
        // 缓存里没有：拿提交号当 ref 再下一次。下过一次就按提交号入缓存，此后不再失效。
        let tree = self.download(repo, commit)?;
        if !commit_matches(commit, &tree.commit) {
            return Err(
                CoreError::new(ErrorCode::Internal, "error.pluginFetchFailed").with_detail(
                    format!(
                        "本地缓存里没有提交 {commit}，而上游现在给的是 {}；请重新浏览这个来源",
                        tree.commit
                    ),
                ),
            );
        }
        self.remember(format!("{repo}@{commit}"), repo, tree.clone());
        Ok(tree)
    }

    fn cached_by_key(&self, key: &str) -> Option<Arc<Tree>> {
        let trees = self.trees.lock().ok()?;
        let entry = trees.iter().find(|entry| entry.key == key)?;
        (crate::time_now().saturating_sub(entry.fetched_at) < self.ttl_seconds)
            .then(|| entry.tree.clone())
    }

    fn cached_by_commit(&self, repo: &str, commit: &str) -> Option<Arc<Tree>> {
        let trees = self.trees.lock().ok()?;
        trees
            .iter()
            .find(|entry| entry.repo == repo && commit_matches(commit, &entry.tree.commit))
            .map(|entry| entry.tree.clone())
    }

    /// 记下一棵树。缓存满了丢最旧的；同一个 ref 指向了新的提交时，旧树**改按提交号留**
    /// ——提交的内容不会变，留着这份才接得住「浏览完过一会儿才安装」。
    fn remember(&self, key: String, repo: &str, tree: Arc<Tree>) {
        // 锁中毒只是退化成不缓存，不该把一次成功的下载变成失败。
        let Ok(mut trees) = self.trees.lock() else {
            return;
        };
        let now = crate::time_now();
        if let Some(existing) = trees.iter_mut().find(|entry| entry.key == key) {
            if existing.tree.commit == tree.commit {
                existing.fetched_at = now;
                return;
            }
            existing.key = format!("{repo}@{}", existing.tree.commit);
        }
        if trees.len() >= MAX_CACHED_TREES {
            let oldest = trees
                .iter()
                .min_by_key(|entry| entry.fetched_at)
                .map(|entry| entry.key.clone());
            if let Some(oldest) = oldest {
                trees.retain(|entry| entry.key != oldest);
            }
        }
        trees.push(Cached {
            key,
            repo: repo.to_owned(),
            fetched_at: now,
            tree,
        });
    }
}

impl RepoFetcher for TarballFetcher {
    fn resolve_commit(&self, repo: &str, git_ref: Option<&str>) -> Result<String, CoreError> {
        Ok(self.tree_for_ref(repo, git_ref)?.commit.clone())
    }

    fn list_blobs(&self, repo: &str, commit: &str) -> Result<Vec<RepoBlob>, CoreError> {
        Ok(self.tree_for_commit(repo, commit)?.blobs.clone())
    }

    fn read_file(&self, repo: &str, commit: &str, path: &str) -> Result<Vec<u8>, CoreError> {
        let tree = self.tree_for_commit(repo, commit)?;
        if let Some(content) = tree.files.get(path) {
            return Ok(content.clone());
        }
        if tree.blobs.iter().any(|blob| blob.path == path) {
            // 名字在清单里、正文没留：只能是体积上限挡下的。如实报错，不返回空内容
            // ——半份文件装上去比失败更糟。
            return Err(
                CoreError::new(ErrorCode::Internal, "error.pluginFetchFailed").with_detail(
                    format!(
                        "{path} 的体积超过上限（单文件 {}、每棵树 {}），这次没有把正文取回来",
                        human_bytes(self.limits.file_bytes),
                        human_bytes(self.limits.retained_bytes)
                    ),
                ),
            );
        }
        Err(
            CoreError::new(ErrorCode::NotFound, "error.pluginRepoNotFound")
                .with_detail(format!("{repo} 的提交 {commit} 里没有 {path}")),
        )
    }
}

/// `owner-repo-sha/` 拆成「最外层目录名 + 剩下的相对路径」。
///
/// 返回 `None` 表示这一条不该进树：绝对路径、`..`、空名、非 UTF-8。
/// 解压是纯内存操作，但这份清单会被安装层拿去落盘，所以穿越写法必须在**进树之前**丢掉。
fn split_entry_path(path: &Path) -> Option<(String, String)> {
    let mut head: Option<&str> = None;
    let mut rest: Vec<&str> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str()?;
                if head.is_none() {
                    head = Some(part);
                } else {
                    rest.push(part);
                }
            }
            // `.` 只是打包器写的噪声，留着不影响路径含义。
            Component::CurDir => {}
            // `RootDir` / `Prefix`（Windows 盘符）/ `ParentDir` 都是要逃出仓库的写法。
            _ => return None,
        }
    }
    Some((head?.to_owned(), rest.join("/")))
}

/// 从最外层目录名里读短 sha：`anthropics-skills-8a1541c` → `8a1541c`。
///
/// 只认「最后一个 `-` 之后全是十六进制、且长度够」这一种形状。`HEAD`、`main` 这类 ref 名
/// 里的字母有落在十六进制字符里的（`HEAD` 全是 H/E/A/D），但长度不够 7 位，所以挡得住。
fn short_sha(dir_name: &str) -> Option<String> {
    let (_, tail) = dir_name.rsplit_once('-')?;
    (SHORT_SHA_LEN.contains(&tail.len()) && tail.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| tail.to_ascii_lowercase())
}

/// 从 PAX 全局头里读 git 写下的完整提交号（codeload 的归档里是 `52 comment=<40 位 sha>`）。
///
/// 全局头是按「`<记录长度> <键>=<值>`」逐行写的，这里只关心 `comment`；值必须是纯十六进制，
/// 免得把一个自由文本的注释当成提交号。
fn pax_comment<R: Read>(entry: &mut R) -> Option<String> {
    // 全局头只有几十字节；给个上限，避免被伪造的超长头吃内存。
    let mut bytes = Vec::new();
    entry.take(4096).read_to_end(&mut bytes).ok()?;
    for line in String::from_utf8_lossy(&bytes).lines() {
        let Some((_length, record)) = line.split_once(' ') else {
            continue;
        };
        let Some(value) = record.strip_prefix("comment=") else {
            continue;
        };
        let value = value.trim();
        if SHORT_SHA_LEN.contains(&value.len())
            && value.chars().all(|character| character.is_ascii_hexdigit())
        {
            return Some(value.to_ascii_lowercase());
        }
    }
    None
}

/// 选哪个提交号写进归属清单。
///
/// 目录名里的短 sha 是**必选项**（它证明这就是 legacy 归档，也是「这个包属于哪个仓库」的唯一
/// 自证）。PAX 全局头里的完整 sha 只在**以短 sha 开头**时才采纳——两个来源互相校验。
///
/// 采纳完整 sha 是为了跟历史版本装出来的记录同形：REST 抓取器（配了令牌的用户还在用）与
/// 0.3.8 之前装出来的记录都是 40 位，而 `check_updates` 直接按字符串比提交号，7 位与 40 位
/// 会被当成两个提交，于是每次升级后所有已装技能都会报「有更新」。
fn pick_commit(short: &str, full: Option<String>) -> String {
    full.filter(|full| full.starts_with(short))
        .unwrap_or_else(|| short.to_owned())
}

/// 提交号可能一边是短 sha（7 位）一边是完整 sha（40 位）：互为前缀就算同一个提交。
/// 短的一侧要够长——否则一个 1 个字符的输入会跟任何提交号「匹配」上。
fn commit_matches(requested: &str, stored: &str) -> bool {
    let shorter = if requested.len() <= stored.len() {
        requested
    } else {
        stored
    };
    shorter.len() >= *SHORT_SHA_LEN.start()
        && (requested.starts_with(shorter) && stored.starts_with(shorter))
}

/// URL 路径段的白名单：只认仓库名与提交号里允许出现的字符。
fn is_url_safe(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

fn content_length(headers: &ureq::http::HeaderMap) -> Option<u64> {
    headers
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

/// 状态码 → 错误。404 单独说：仓库不存在，或者它是私有的（匿名读不到）。
///
/// **这里不会产生 `error.pluginRateLimited`**：归档主机不参与 REST 的限额计费，
/// 出现 403/429 更像被中间设备（公司网关、代理）拦下，如实报状态码比猜「限额用尽」有用。
fn check_status(url: &str, status: u16) -> Result<(), CoreError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    if status == 404 {
        return Err(
            CoreError::new(ErrorCode::NotFound, "error.pluginRepoNotFound")
                .with_detail(format!("{url} 返回 404（仓库不存在，或它是私有的）")),
        );
    }
    Err(
        CoreError::new(ErrorCode::Internal, "error.pluginFetchFailed")
            .with_detail(format!("{url} 返回 {status}")),
    )
}

fn unreachable(url: &str, error: &dyn std::fmt::Display) -> CoreError {
    CoreError::new(ErrorCode::Internal, "error.pluginFetchUnreachable")
        .with_detail(format!("访问 {url} 失败：{error}"))
}

/// 压缩包读不出来：不是 gzip、不是 tar、或者最外层目录名里没有提交号。
fn archive_unreadable(url: &str, error: &dyn std::fmt::Display) -> CoreError {
    CoreError::new(ErrorCode::Internal, "error.pluginArchiveUnsupported")
        .with_detail(format!("{url} 的归档读不出来：{error}"))
}

/// 解压/解析时的错误分类。
///
/// `overflow` 是 [`Budgeted`] 打的标记：解压读到上限时它返回的也是 io 错误，与「归档本身
/// 坏了」在类型上分不开，只能靠这个标记区分——否则压缩炸弹会被报成「格式不认识」，
/// 用户按那句话做不了任何事。
fn archive_error(
    url: &str,
    error: &dyn std::fmt::Display,
    overflow: &std::sync::atomic::AtomicBool,
    limit: u64,
) -> CoreError {
    if overflow.load(std::sync::atomic::Ordering::SeqCst) {
        return archive_too_large(format!(
            "{url} 解压后超过 {}，已中断（压缩炸弹挡在这里）",
            human_bytes(limit)
        ));
    }
    archive_unreadable(url, error)
}

fn archive_too_large(detail: String) -> CoreError {
    CoreError::new(ErrorCode::Internal, "error.pluginArchiveTooLarge").with_detail(detail)
}

/// 边解压边数上限的读取器。
///
/// 到上限就返回 io 错误（而不是把流悄悄截断成「就这么多」）：截断会让解析器以为归档到此
/// 为止，把一棵**残缺的树**当成完整结果交上去——宁可失败也不给半个仓库。
struct Budgeted<R> {
    inner: R,
    seen: u64,
    limit: u64,
    /// 超限时置位。错误要穿过 tar 的解析层才能回到我们手里，类型上还原不出原因，
    /// 所以在源头上打个标记。
    overflow: Arc<std::sync::atomic::AtomicBool>,
}

impl<R: Read> Read for Budgeted<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.seen >= self.limit {
            self.overflow
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "解压总量超过上限",
            ));
        }
        // 最后一次读只放到上限为止：多读的那一字节就是超限的证据。
        let room = (self.limit - self.seen).min(buffer.len() as u64) as usize;
        let read = self.inner.read(&mut buffer[..room])?;
        self.seen += read as u64;
        Ok(read)
    }
}

/// 给人看的体积：详情里写「26214400 字节」不如写「25.0 MB」。
fn human_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{Ipv4Addr, TcpListener};

    /// 头里的 `Content-Length` 怎么给。
    #[derive(Clone, Copy)]
    enum Declared {
        /// 照正文长度写（正常服务器）。
        Actual,
        /// 报一个指定的长度（用来模拟「头里报得很大」）。
        Value(u64),
        /// 不写这个头（分块 / close 界定，只能靠流式上限挡）。
        Absent,
    }

    /// 一份预设响应。
    struct Reply {
        status: u16,
        declared: Declared,
        body: Vec<u8>,
    }

    impl Reply {
        fn archive(body: Vec<u8>) -> Self {
            Self {
                status: 200,
                declared: Declared::Actual,
                body,
            }
        }

        fn declared(status: u16, length: u64) -> Self {
            Self {
                status,
                declared: Declared::Value(length),
                body: Vec::new(),
            }
        }

        fn without_length(body: Vec<u8>) -> Self {
            Self {
                status: 200,
                declared: Declared::Absent,
                body,
            }
        }
    }

    /// 可编排的上游：按 `(方法, 路径)` 现算响应，并记下每一次请求。
    ///
    /// 用真的 socket 而不是假 agent：这里要验证的正是**请求的条数与顺序**——「预检超限就不
    /// 下载」「第二次读文件不再联网」只有在真实请求上才看得出来（写法照
    /// `diagnostics::probe` 的用例）。
    struct Mock {
        endpoint: String,
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Mock {
        fn start(handler: impl Fn(&str, &str) -> Reply + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let sink = seen.clone();
            // 每条连接一个线程，所以处理函数要能跨线程共享（也得能同时被用）。
            let handler = std::sync::Arc::new(handler);
            std::thread::spawn(move || {
                for incoming in listener.incoming() {
                    let Ok(stream) = incoming else { break };
                    let sink = sink.clone();
                    let handler = handler.clone();
                    std::thread::spawn(move || {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut request_line = String::new();
                        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
                            return;
                        }
                        // 头要读干净：不读掉，客户端会卡在「请求还没发完」。
                        loop {
                            let mut line = String::new();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                                break;
                            }
                        }
                        let mut parts = request_line.split_whitespace();
                        let method = parts.next().unwrap_or_default().to_owned();
                        let path = parts.next().unwrap_or_default().to_owned();
                        sink.lock().unwrap().push(format!("{method} {path}"));
                        let reply = handler(&method, &path);
                        let length = match reply.declared {
                            Declared::Actual => {
                                format!("content-length: {}\r\n", reply.body.len())
                            }
                            Declared::Value(length) => format!("content-length: {length}\r\n"),
                            Declared::Absent => String::new(),
                        };
                        let mut stream = stream;
                        let _ = stream.write_all(
                            format!(
                                "HTTP/1.1 {} X\r\n{length}connection: close\r\n\r\n",
                                reply.status
                            )
                            .as_bytes(),
                        );
                        // HEAD 不许带正文，带了客户端也不会读。
                        if method != "HEAD" {
                            let _ = stream.write_all(&reply.body);
                        }
                        let _ = stream.flush();
                    });
                }
            });
            Self {
                endpoint: format!("http://127.0.0.1:{port}"),
                seen,
            }
        }

        fn requests(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }

        fn request_count(&self) -> usize {
            self.requests().len()
        }
    }

    /// 指向 Mock 的抓取器：网络细节与应用里一致，只是主机与上限可换。
    fn fetcher(mock: &Mock, limits: Limits) -> TarballFetcher {
        TarballFetcher::with_base("test-agent".to_owned(), mock.endpoint.clone())
            .with_limits(limits)
    }

    /// 现场造一个 tar.gz。`tar` 与 `flate2` 已经是直接依赖，测试里直接拿它们当打包器，
    /// 不引入新的测试依赖。
    fn tarball(root: &str, files: &[(&str, &str)]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        // 真实归档的第一条是顶层目录自己的条目（`owner-repo-sha/`）：解析器不该把它当文件。
        let mut directory = tar::Header::new_gnu();
        directory.set_entry_type(tar::EntryType::Directory);
        directory.set_mode(0o755);
        directory.set_size(0);
        builder
            .append_data(&mut directory, format!("{root}/"), &[][..])
            .unwrap();
        for (path, text) in files {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(text.len() as u64);
            builder
                .append_data(&mut header, format!("{root}/{path}"), text.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// 直接往原始头里写条目名，再交给 [`tar::Builder::append`]。
    ///
    /// `append_data` 自己会拒掉带 `..` 的名字（tar 把「归档里的路径不能有 ..」当错误），
    /// 而我们要模拟的正是**上游能给出来的恶意归档**，所以绕过打包器的校验。
    fn append_raw(
        builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
        raw_name: &str,
        text: &str,
        entry_type: tar::EntryType,
    ) {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(text.len() as u64);
        header.set_entry_type(entry_type);
        {
            let name = &mut header.as_gnu_mut().expect("gnu 头").name;
            name.fill(0);
            name[..raw_name.len()].copy_from_slice(raw_name.as_bytes());
        }
        header.set_cksum();
        builder.append(&header, text.as_bytes()).unwrap();
    }

    /// PAX 的一条记录：`<总字节数> <键>=<值>\n`，总字节数把长度字段自己也算进去。
    fn pax_record(key: &str, value: &str) -> String {
        let body = format!("{key}={value}\n");
        let mut length = body.len() + 2;
        loop {
            let candidate = format!("{length} {body}");
            if candidate.len() == length {
                return candidate;
            }
            length = candidate.len();
        }
    }

    /// 造一个**带 PAX 全局头**的 tar.gz：codeload 的真实归档就是这个形状
    /// （第一条 `pax_global_header`，typeflag `g`，正文里写着完整提交号）。
    fn tarball_with_pax(root: &str, comment: &str, files: &[(&str, &str)]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        append_raw(
            &mut builder,
            "pax_global_header",
            &pax_record("comment", comment),
            tar::EntryType::XGlobalHeader,
        );
        let mut directory = tar::Header::new_gnu();
        directory.set_entry_type(tar::EntryType::Directory);
        directory.set_mode(0o755);
        directory.set_size(0);
        builder
            .append_data(&mut directory, format!("{root}/"), &[][..])
            .unwrap();
        for (path, text) in files {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(text.len() as u64);
            builder
                .append_data(&mut header, format!("{root}/{path}"), text.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// codeload 的归档以 PAX 全局头开头：它是元数据，既不能进树，也不能被当成最外层
    /// 目录名（它长得像 `pax_global_header`，照「第一条的名字就是仓库名」认就全错了）。
    /// 它同时带着完整提交号，与目录名的短 sha 互为校验。
    #[test]
    fn a_leading_pax_header_is_metadata_and_carries_the_full_commit() {
        let archive = tarball_with_pax(
            "owner-repo-abc1234",
            "abc1234def567890123456789012345678901234",
            &[("SKILL.md", "---\nname: demo\n---\n")],
        );
        let mock = Mock::start(move |_method, _path| Reply::archive(archive.clone()));
        let fetcher = fetcher(&mock, Limits::default());

        let commit = fetcher.resolve_commit("owner/repo", None).unwrap();
        assert_eq!(
            commit, "abc1234def567890123456789012345678901234",
            "全局头里的完整 sha 与目录名的短 sha 一致，取长的那个"
        );
        let blobs = fetcher.list_blobs("owner/repo", &commit).unwrap();
        assert_eq!(
            blobs
                .iter()
                .map(|blob| blob.path.as_str())
                .collect::<Vec<_>>(),
            vec!["SKILL.md"],
            "全局头不能进清单"
        );
    }

    /// 全局头里的提交号与目录名对不上时只信目录名：那是这个包唯一自证的东西。
    #[test]
    fn a_pax_comment_that_disagrees_with_the_directory_is_ignored() {
        let archive = tarball_with_pax(
            "owner-repo-abc1234",
            &"feedface".repeat(5),
            &[("SKILL.md", "---\nname: demo\n---\n")],
        );
        let mock = Mock::start(move |_method, _path| Reply::archive(archive.clone()));
        let fetcher = fetcher(&mock, Limits::default());
        assert_eq!(
            fetcher.resolve_commit("owner/repo", None).unwrap(),
            "abc1234"
        );
    }

    /// 一次下载回答后面全部问题：提交号、清单、正文，第二次读文件不再联网。
    #[test]
    fn one_download_answers_the_commit_the_listing_and_every_read() {
        let archive = tarball(
            "owner-repo-abc1234",
            &[
                ("SKILL.md", "---\nname: root\n---\n"),
                ("skills/demo/SKILL.md", "---\nname: demo\n---\n正文"),
                ("skills/demo/references/notes.md", "笔记"),
            ],
        );
        let mock = Mock::start(move |_method, _path| Reply::archive(archive.clone()));
        let fetcher = fetcher(&mock, Limits::default());

        let commit = fetcher.resolve_commit("owner/repo", None).unwrap();
        assert_eq!(commit, "abc1234", "提交号来自最外层目录名的尾部");
        assert_eq!(
            mock.requests(),
            vec![
                "HEAD /owner/repo/legacy.tar.gz/HEAD".to_owned(),
                "GET /owner/repo/legacy.tar.gz/HEAD".to_owned(),
            ],
            "预检一次 HEAD + 下载一次 GET，端点必须是 legacy.tar.gz"
        );

        let blobs = fetcher.list_blobs("owner/repo", &commit).unwrap();
        assert_eq!(
            blobs
                .iter()
                .map(|blob| blob.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "SKILL.md",
                "skills/demo/SKILL.md",
                "skills/demo/references/notes.md"
            ],
            "顶层目录自己的条目不该进清单"
        );
        assert_eq!(blobs[2].size, "笔记".len() as u64);
        assert_eq!(
            fetcher
                .read_file("owner/repo", &commit, "skills/demo/references/notes.md")
                .unwrap(),
            "笔记".as_bytes()
        );

        // 同一个提交的第二次读取：正文已经在树上，一次网络都不该有
        //（从前预览与安装都要再打一次 raw.githubusercontent.com）。
        let before = mock.request_count();
        assert_eq!(
            fetcher
                .read_file("owner/repo", &commit, "skills/demo/references/notes.md")
                .unwrap(),
            "笔记".as_bytes()
        );
        assert_eq!(
            mock.request_count(),
            before,
            "同一个提交的 read_file 不再联网"
        );
    }

    /// 顶层目录名里没有提交号（同源普通 `tar.gz` 给的就是 `skills-HEAD/`）就该失败，
    /// **不能把 `HEAD` 当提交号**：安装记录与更新检查都拿它比对。
    #[test]
    fn an_archive_without_a_commit_in_the_directory_name_is_refused() {
        let archive = tarball("skills-HEAD", &[("SKILL.md", "---\nname: head\n---\n")]);
        let mock = Mock::start(move |_method, _path| Reply::archive(archive.clone()));
        let fetcher = fetcher(&mock, Limits::default());

        let error = fetcher.resolve_commit("owner/repo", None).unwrap_err();
        assert_eq!(
            error.message_key, "error.pluginArchiveUnsupported",
            "前端按这个 key 区分「归档不认识」与网络错误"
        );
        assert!(
            error.safe_details[0].contains("skills-HEAD"),
            "要说清是哪个目录名读不出来：{:?}",
            error.safe_details
        );
    }

    /// 预检就超限：连 GET 都不该发。
    #[test]
    fn an_archive_over_the_size_cap_is_never_downloaded() {
        let limits = Limits {
            archive_bytes: 1024,
            ..Limits::default()
        };
        let mock = Mock::start(|method, _path| {
            if method == "HEAD" {
                Reply::declared(200, 4096)
            } else {
                Reply::archive(vec![7; 2048])
            }
        });
        let fetcher = fetcher(&mock, limits);

        let error = fetcher.resolve_commit("owner/repo", None).unwrap_err();
        assert_eq!(error.message_key, "error.pluginArchiveTooLarge");
        assert_eq!(
            mock.requests(),
            vec!["HEAD /owner/repo/legacy.tar.gz/HEAD".to_owned()],
            "预检已经超限，GET 一次都不该发"
        );
    }

    /// 没有 `Content-Length` 时靠流式上限截住（分块响应下的同一条保护）。
    #[test]
    fn a_chunked_archive_is_cut_off_while_downloading() {
        let limits = Limits {
            archive_bytes: 64,
            ..Limits::default()
        };
        // 预检与响应头都不报长度：只剩「边读边数」这一条路。
        let mock = Mock::start(|_method, _path| Reply::without_length(vec![7; 4096]));
        let fetcher = fetcher(&mock, limits);
        let error = fetcher.resolve_commit("owner/repo", None).unwrap_err();
        assert_eq!(error.message_key, "error.pluginArchiveTooLarge");
        assert_eq!(
            mock.request_count(),
            2,
            "GET 发了，只是在读正文时被截住（不是靠头里的长度）"
        );
    }

    /// 压缩炸弹：解压总量超限就报错，不把它当成「空仓库」，也不把内存交出去。
    #[test]
    fn a_zip_bomb_is_refused() {
        let big = "0".repeat(64 * 1024);
        let archive = tarball("owner-repo-abc1234", &[("skills/demo/big.bin", &big)]);
        let limits = Limits {
            decompressed_bytes: 4096,
            ..Limits::default()
        };
        let mock = Mock::start(move |_method, _path| Reply::archive(archive.clone()));
        let fetcher = fetcher(&mock, limits);

        let error = fetcher.resolve_commit("owner/repo", None).unwrap_err();
        assert_eq!(error.message_key, "error.pluginArchiveTooLarge");
        assert!(
            error.safe_details[0].contains("解压"),
            "要说清是被解压上限挡下的：{:?}",
            error.safe_details
        );
    }

    /// 解压上限：够用的上限要能过，明显不够（读到一半就不够了）必须失败。
    ///
    /// 上限卡的是「读过多少字节」，所以它不要求解压总量恰好等于上限——tar 在归档的结束
    /// 标记处就停了，根本没读到剩下的字节。
    #[test]
    fn the_decompressed_budget_stops_the_parse_when_it_runs_out() {
        let blob = "x".repeat(8192);
        let archive = tarball(
            "owner-repo-abc1234",
            &[
                ("skills/demo/SKILL.md", "---\nname: demo\n---\n"),
                ("skills/demo/blob", &blob),
            ],
        );
        // 量出这个夹具解压后多大。
        let mut decompressed = Vec::new();
        flate2::read::GzDecoder::new(archive.as_slice())
            .read_to_end(&mut decompressed)
            .unwrap();
        let exact = decompressed.len() as u64;

        let mock = Mock::start({
            let archive = archive.clone();
            move |_method, _path| Reply::archive(archive.clone())
        });
        let ok = fetcher(
            &mock,
            Limits {
                decompressed_bytes: exact,
                ..Limits::default()
            },
        );
        assert_eq!(ok.resolve_commit("owner/repo", None).unwrap(), "abc1234");

        let too_small = fetcher(
            &mock,
            Limits {
                decompressed_bytes: exact / 2,
                ..Limits::default()
            },
        );
        let error = too_small.resolve_commit("owner/repo", None).unwrap_err();
        assert_eq!(error.message_key, "error.pluginArchiveTooLarge");
    }

    /// 超过单文件上限：正文丢弃，但名字与真实大小要留在清单里，
    /// 并且如实反映到 `RepoCatalog.truncated`（`assemble` 正是按大小判断的）。
    #[test]
    fn an_oversized_file_keeps_its_place_in_the_listing() {
        let big = "x".repeat(MAX_FILE_BYTES as usize + 1);
        let archive = tarball(
            "owner-repo-abc1234",
            &[
                ("skills/demo/SKILL.md", "---\nname: demo\n---\n"),
                ("skills/demo/big.bin", &big),
            ],
        );
        let mock = Mock::start(move |_method, _path| Reply::archive(archive.clone()));
        let fetcher = fetcher(&mock, Limits::default());

        let commit = fetcher.resolve_commit("owner/repo", None).unwrap();
        let blobs = fetcher.list_blobs("owner/repo", &commit).unwrap();
        assert_eq!(blobs.len(), 2, "清单里一条都不能少");
        assert_eq!(blobs[1].path, "skills/demo/big.bin");
        assert_eq!(blobs[1].size, MAX_FILE_BYTES + 1, "大小要用真实的");

        // 正文没留：读它要如实报错，不能返回空内容。
        let error = fetcher
            .read_file("owner/repo", &commit, "skills/demo/big.bin")
            .unwrap_err();
        assert!(
            error.safe_details[0].contains("big.bin") && error.safe_details[0].contains("超过上限"),
            "{:?}",
            error.safe_details
        );

        // 目录那边也要知道这份技能不完整。
        let catalog =
            crate::plugins::source::assemble(&fetcher, "owner/repo", &commit, &blobs, 0).unwrap();
        assert!(catalog.truncated, "体积超限要如实反映到 truncated");
    }

    /// 路径穿越（`..`、绝对路径）与符号链接、目录项都不进树。
    #[test]
    fn entries_that_escape_the_repository_are_dropped() {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        let mut keep = tar::Header::new_gnu();
        keep.set_mode(0o644);
        keep.set_size(5);
        builder
            .append_data(
                &mut keep,
                "owner-repo-abc1234/skills/demo/SKILL.md",
                "---\n\n".as_bytes(),
            )
            .unwrap();
        for escape in [
            "owner-repo-abc1234/../../etc/passwd",
            "/etc/passwd",
            "owner-repo-abc1234/skills/../outside.md",
        ] {
            append_raw(&mut builder, escape, "坏", tar::EntryType::Regular);
        }
        append_raw(
            &mut builder,
            "owner-repo-abc1234/link",
            "",
            tar::EntryType::Symlink,
        );
        let archive = builder.into_inner().unwrap().finish().unwrap();

        let mock = Mock::start(move |_method, _path| Reply::archive(archive.clone()));
        let fetcher = fetcher(&mock, Limits::default());
        let commit = fetcher.resolve_commit("owner/repo", None).unwrap();
        let blobs = fetcher.list_blobs("owner/repo", &commit).unwrap();
        assert_eq!(
            blobs
                .iter()
                .map(|blob| blob.path.as_str())
                .collect::<Vec<_>>(),
            vec!["skills/demo/SKILL.md"],
            "穿越写法与符号链接都不该进清单"
        );
    }

    /// 可变 ref 到了寿命就要重新解析：`check_updates` 靠这一条看到新提交。
    #[test]
    fn a_mutable_ref_is_resolved_again_after_the_ttl() {
        let archive = tarball("owner-repo-abc1234", &[("SKILL.md", "---\nname: a\n---\n")]);
        let mock = Mock::start(move |_method, _path| Reply::archive(archive.clone()));

        // 默认寿命内：同一个 ref 只下载一次。
        let fresh = fetcher(&mock, Limits::default());
        fresh.resolve_commit("owner/repo", None).unwrap();
        let after_first = mock.request_count();
        assert_eq!(
            fresh.resolve_commit("owner/repo", None).unwrap(),
            "abc1234",
            "寿命内的第二次解析要用缓存"
        );
        assert_eq!(mock.request_count(), after_first, "寿命内不该再联网");

        // 寿命为 0：每次解析都重新下载，但**按提交号的读取仍然不联网**。
        let expired = fetcher(&mock, Limits::default()).with_ttl_seconds(0);
        expired.resolve_commit("owner/repo", None).unwrap();
        let after_expired = mock.request_count();
        assert!(after_expired > after_first, "寿命为 0 时必须重新解析");
        expired
            .read_file("owner/repo", "abc1234", "SKILL.md")
            .unwrap();
        assert_eq!(
            mock.request_count(),
            after_expired,
            "提交的内容不会变：按提交号读文件不看寿命"
        );
    }

    #[test]
    fn only_plain_relative_file_names_make_it_into_the_tree() {
        assert_eq!(
            split_entry_path(Path::new("root/a/b.md")),
            Some(("root".to_owned(), "a/b.md".to_owned()))
        );
        assert_eq!(
            split_entry_path(Path::new("root/")),
            Some(("root".to_owned(), String::new()))
        );
        assert_eq!(split_entry_path(Path::new("root/../evil")), None);
        assert_eq!(split_entry_path(Path::new("/etc/passwd")), None);
        assert_eq!(split_entry_path(Path::new("")), None);
    }

    #[test]
    fn the_commit_comes_from_the_trailing_sha_of_the_root_directory() {
        assert_eq!(
            short_sha("anthropics-skills-8a1541c").as_deref(),
            Some("8a1541c")
        );
        assert_eq!(
            short_sha("obra-superpowers-1A2B3C4").as_deref(),
            Some("1a2b3c4"),
            "大小写都认，统一成小写"
        );
        assert_eq!(short_sha("skills-HEAD"), None, "HEAD 不是提交号");
        assert_eq!(short_sha("skills-main"), None);
        assert_eq!(short_sha("repo-abc"), None, "太短的不认");
        assert_eq!(short_sha("repo"), None, "没有分隔符的不认");
    }

    #[test]
    fn a_short_and_a_full_commit_are_the_same_commit() {
        assert!(commit_matches("abc1234", "abc1234"));
        assert!(commit_matches(
            "abc1234",
            "abc1234def567890123456789012345678901234"
        ));
        assert!(commit_matches(
            "abc1234def567890123456789012345678901234",
            "abc1234"
        ));
        assert!(!commit_matches("abc1234", "def5678"));
        assert!(!commit_matches("a", "abc1234"), "太短的输入不许当通配");
    }

    /// 路径段白名单：多一个 `/` 就能把请求换到别的仓库去，必须在拼 URL 之前挡住。
    #[test]
    fn a_reference_that_could_change_the_url_is_refused() {
        let mock = Mock::start(|_method, _path| Reply::archive(Vec::new()));
        let fetcher = fetcher(&mock, Limits::default());
        let error = fetcher
            .resolve_commit("owner/repo", Some("../other"))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ValidationFailed);
        assert_eq!(mock.request_count(), 0, "非法 ref 连请求都不该发");
    }
}
