//! 内容中心：本机自己抓公开源，存一份最新快照。
//!
//! 与参考产品最大的差别在这里，值得写清楚：**没有云端聚合**，所以核心直接抓源。
//! 因此本模块把「礼貌」当成一等需求：
//!
//! - 条件请求（`ETag` / `If-Modified-Since`），源没更新时只更新「上次成功时间」；
//! - 串行抓取（不并发轰炸同一个源站）；
//! - 失败退避 5 分钟 → 15 分钟 → 1 小时；
//! - 单次刷新的总预算（见 `REFRESH_BUDGET`），到点就停并如实报告哪些源没轮到；
//! - 抓取只在应用运行时发生，界面在订阅源页直说这一点。

pub mod feed;

use std::{collections::HashSet, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    domain::error::{CoreError, ErrorCode},
    storage::HubStore,
};

pub use feed::{ParsedFeed, ParsedItem};

/// 每天两次自动抓取，本地时间：06:00 与 18:00。
///
/// 固定时刻而不是固定间隔：用户要的是「早上 6 点、晚上 6 点各一次」，而且应用一天里
/// 会被开开关关，固定间隔会把这套节奏漂到任意时刻去。
pub const DAILY_FETCH_HOURS: [u8; 2] = [6, 18];
/// 同一计划时刻内，各源按自己 id 错开 0–5 分钟：同时开五六个请求对源站不礼貌，也容易被限流。
pub const SLOT_SPREAD_SECONDS: i64 = 300;
/// 每个源保留的最新条目数。资讯页一屏至少放 10 条（用户要求），这里留一倍余量。
pub const PER_SOURCE_KEEP_ITEMS: usize = 20;
/// 单次刷新的总时间预算。到点停止启动新的抓取，并把没轮到的源报出来。
pub const REFRESH_BUDGET: Duration = Duration::from_secs(60);
/// 条目保留天数与总数上限。
///
/// 成功抓取时是**整源覆盖**（见 `sync_source_items`）：本地只留「这次抓到的」，
/// 所以保留期只是兜底——源长期失败时，旧条目不会被永久留在本地（用户要求「尽量不要存数据」）。
pub const RETENTION_DAYS: i64 = 3;
pub const RETAINED_ITEMS: usize = PER_SOURCE_KEEP_ITEMS * 20;

/// GitHub 令牌在系统凭据库里的条目引用。令牌只进凭据库，不进设置表。
pub const GITHUB_TOKEN_REF: &str = "gptswitch/content/github-token/v1";

/// 随包标识。源站至少要知道是谁在请求。
pub const DEFAULT_USER_AGENT: &str = concat!(
    "Switchelp/",
    env!("CARGO_PKG_VERSION"),
    " (RSS reader; +https://github.com/nexsjournal/switchelp-macapp)"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedKind {
    /// RSS / Atom 文档。
    Rss,
    /// GitHub 仓库搜索接口。
    GithubSearch,
}

/// 预置源。全部于 2026-09-21 实测可访问。
pub const DEFAULT_SOURCES: [(&str, FeedKind, &str, &str, &str); 8] = [
    (
        "sspai",
        FeedKind::Rss,
        "https://sspai.com/feed",
        "少数派",
        "zh",
    ),
    (
        "ruanyifeng",
        FeedKind::Rss,
        "https://www.ruanyifeng.com/blog/atom.xml",
        "阮一峰的网络日志",
        "zh",
    ),
    (
        "linuxdo",
        FeedKind::Rss,
        "https://linux.do/latest.rss",
        "linux.do",
        "zh",
    ),
    (
        "baoyu",
        FeedKind::Rss,
        "https://s.baoyu.io/feed.xml",
        "宝玉的分享",
        "zh",
    ),
    (
        "hn",
        FeedKind::Rss,
        "https://hnrss.org/newest?points=100",
        "Hacker News（100 分以上）",
        "en",
    ),
    (
        "github-today",
        FeedKind::GithubSearch,
        "today",
        "GitHub 今日热门",
        "",
    ),
    (
        "github-week",
        FeedKind::GithubSearch,
        "week",
        "GitHub 本周热门",
        "",
    ),
    (
        "github-month",
        FeedKind::GithubSearch,
        "month",
        "GitHub 本月热门",
        "",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSource {
    pub id: String,
    pub kind: FeedKind,
    /// RSS 是网址；GitHub 搜索是窗口标识（`today` / `week` / `month`）。
    pub url: String,
    pub label: String,
    pub lang: String,
    pub enabled: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub last_ok_at: Option<i64>,
    pub last_error: Option<String>,
    pub fail_streak: u32,
    pub next_fetch_at: i64,
    /// 是否预置。预置源可停用也可删，但界面会标注来源，便于用户知道自己在看什么。
    pub builtin: bool,
}

/// 保存订阅源时的输入。**只带用户能改的字段**：ETag、连续失败次数、下次抓取时间
/// 都是服务端事实，不能让界面回传，否则一次保存就会把抓取状态清掉。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSourceDraft {
    /// 新建时留空，由核心生成。
    pub id: Option<String>,
    pub kind: FeedKind,
    pub url: String,
    pub label: String,
    pub lang: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub url: String,
    pub source_id: String,
    /// 源的中文名，读列表时一并带出，界面不必再查一次。
    pub source_label: String,
    pub title: String,
    pub summary: String,
    pub published_at: i64,
    pub first_seen_at: i64,
    pub lang: String,
    /// GitHub 搜索条目专有：仓库的星标数与语言。
    pub stars: Option<u64>,
    pub repo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedFailure {
    pub source_id: String,
    pub label: String,
    pub message: String,
    /// 连续失败次数，界面据此说明「已经连续失败 N 次」。
    pub fail_streak: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    pub attempted: Vec<String>,
    pub succeeded: Vec<String>,
    pub not_modified: Vec<String>,
    pub failed: Vec<FeedFailure>,
    /// 因时间预算用尽而没有开始的源。
    pub skipped: Vec<String>,
    pub new_items: usize,
    pub next_fetch_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentStatus {
    pub last_ok_at: Option<i64>,
    pub next_fetch_at: i64,
    /// 每天自动抓取的本地小时数（升序），例如 `[6, 18]`。
    pub schedule_hours: Vec<u8>,
    pub failing: Vec<FeedFailure>,
    pub total_items: usize,
}

/// 一次 HTTP 抓取的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedResponse {
    pub status: u16,
    pub body: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl FeedResponse {
    pub fn not_modified(&self) -> bool {
        self.status == 304
    }
}

/// 抓取接口。测试用假实现，不联网。
pub trait FeedFetcher: Send + Sync {
    fn get(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        user_agent: &str,
        bearer: Option<&str>,
    ) -> Result<FeedResponse, CoreError>;
}

/// 真实抓取器。
pub struct HttpFeedFetcher {
    agent: ureq::Agent,
}

impl Default for HttpFeedFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpFeedFetcher {
    pub fn new() -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_recv_response(Some(Duration::from_secs(15)))
                .proxy(ureq::Proxy::try_from_env())
                .build(),
        );
        Self { agent }
    }
}

impl FeedFetcher for HttpFeedFetcher {
    fn get(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        user_agent: &str,
        bearer: Option<&str>,
    ) -> Result<FeedResponse, CoreError> {
        let mut request = self.agent.get(url).header("user-agent", user_agent).header(
            "accept",
            "application/rss+xml, application/atom+xml, application/json;q=0.9, */*;q=0.5",
        );
        if let Some(etag) = etag {
            request = request.header("if-none-match", etag);
        }
        if let Some(last_modified) = last_modified {
            request = request.header("if-modified-since", last_modified);
        }
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }

        let mut response = request.call().map_err(|error| {
            CoreError::new(ErrorCode::Internal, "error.feedUnreachable")
                .with_detail(format!("访问 {url} 失败：{error}"))
        })?;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let etag = header("etag");
        let last_modified = header("last-modified");
        let mut body = String::new();
        if status != 304 {
            body = response
                .body_mut()
                .with_config()
                .limit(4 * 1024 * 1024)
                .read_to_string()
                .map_err(|error| {
                    CoreError::internal(format!("读取 {url} 的响应体失败：{error}"))
                })?;
        }
        Ok(FeedResponse {
            status,
            body,
            etag,
            last_modified,
        })
    }
}

/// 抓取与缓存的服务。
pub struct ContentService {
    store: Arc<dyn HubStore>,
    fetcher: Arc<dyn FeedFetcher>,
    user_agent: String,
    /// GitHub 搜索用的可选令牌。存系统凭据库，这里只拿到明文一次。
    github_token: Option<String>,
}

impl ContentService {
    pub fn new(
        store: Arc<dyn HubStore>,
        fetcher: Arc<dyn FeedFetcher>,
        github_token: Option<String>,
    ) -> Self {
        Self {
            store,
            fetcher,
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            github_token,
        }
    }

    /// 首次运行时把预置源写进库。已有的源不动——用户删掉的不会被重新塞回来。
    pub fn ensure_defaults(&self, now: i64) -> Result<(), CoreError> {
        let existing: HashSet<String> = self
            .store
            .list_feed_sources()?
            .into_iter()
            .map(|source| source.id)
            .collect();
        for (id, kind, url, label, lang) in DEFAULT_SOURCES {
            if existing.contains(id) {
                continue;
            }
            self.store.save_feed_source(&FeedSource {
                id: id.to_owned(),
                kind,
                url: url.to_owned(),
                label: label.to_owned(),
                lang: lang.to_owned(),
                enabled: true,
                etag: None,
                last_modified: None,
                last_ok_at: None,
                last_error: None,
                fail_streak: 0,
                next_fetch_at: 0,
                builtin: true,
            })?;
        }
        let _ = now;
        Ok(())
    }

    pub fn sources(&self) -> Result<Vec<FeedSource>, CoreError> {
        self.store.list_feed_sources()
    }

    pub fn save_source(&self, draft: FeedSourceDraft, now: i64) -> Result<FeedSource, CoreError> {
        if draft.label.trim().is_empty() {
            return Err(CoreError::validation("订阅源需要一个名字"));
        }
        if draft.url.trim().is_empty() {
            return Err(CoreError::validation("订阅源需要一个地址"));
        }
        match draft.kind {
            FeedKind::Rss => {
                if !draft.url.starts_with("http://") && !draft.url.starts_with("https://") {
                    return Err(CoreError::validation(
                        "RSS 源的地址要以 http:// 或 https:// 开头",
                    ));
                }
            }
            FeedKind::GithubSearch => {
                if !matches!(draft.url.as_str(), "today" | "week" | "month") {
                    return Err(CoreError::validation(
                        "GitHub 搜索源只能填 today / week / month",
                    ));
                }
            }
        }

        let existing = match draft.id.as_deref() {
            Some(id) => self
                .store
                .list_feed_sources()?
                .into_iter()
                .find(|source| source.id == id),
            None => None,
        };
        let source = match existing {
            // 改已有的源：保留抓取状态与预置标记，只更新用户能改的字段。
            Some(mut source) => {
                source.kind = draft.kind;
                source.url = draft.url.clone();
                source.label = draft.label.clone();
                source.lang = draft.lang.clone();
                source.enabled = draft.enabled;
                // 地址变了就把条件请求的凭据丢掉：拿旧 ETag 去问新地址是错的。
                if source.etag.is_some() || source.last_modified.is_some() {
                    source.etag = None;
                    source.last_modified = None;
                }
                source
            }
            None => FeedSource {
                id: draft
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("feed-{}", uuid::Uuid::new_v4())),
                kind: draft.kind,
                url: draft.url.clone(),
                label: draft.label.clone(),
                lang: draft.lang.clone(),
                enabled: draft.enabled,
                etag: None,
                last_modified: None,
                last_ok_at: None,
                last_error: None,
                fail_streak: 0,
                // 新建的源立刻可抓，不要求用户等一个周期。
                next_fetch_at: now,
                builtin: false,
            },
        };
        self.store.save_feed_source(&source)?;
        Ok(source)
    }

    pub fn delete_source(&self, source_id: &str) -> Result<(), CoreError> {
        self.store.delete_feed_source(source_id)
    }

    pub fn items(
        &self,
        source_id: Option<&str>,
        lang: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<FeedItem>, CoreError> {
        let mut items = self.store.list_feed_items(source_id, lang, limit, offset)?;
        let labels: std::collections::BTreeMap<String, String> = self
            .store
            .list_feed_sources()?
            .into_iter()
            .map(|source| (source.id, source.label))
            .collect();
        for item in items.iter_mut() {
            item.source_label = labels.get(&item.source_id).cloned().unwrap_or_default();
        }
        Ok(items)
    }

    pub fn status(&self, now: i64) -> Result<ContentStatus, CoreError> {
        let sources = self.store.list_feed_sources()?;
        let last_ok_at = sources.iter().filter_map(|source| source.last_ok_at).max();
        let next_fetch_at = sources
            .iter()
            .filter(|source| source.enabled)
            .filter_map(|source| {
                // 从未抓过的源应立刻可抓，不是「等一个周期」。
                (source.next_fetch_at > now).then_some(source.next_fetch_at)
            })
            .min()
            .unwrap_or(now);
        let failing = sources
            .iter()
            .filter(|source| source.fail_streak > 0)
            .map(|source| FeedFailure {
                source_id: source.id.clone(),
                label: source.label.clone(),
                message: source
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "未知失败".to_owned()),
                fail_streak: source.fail_streak,
            })
            .collect();
        Ok(ContentStatus {
            last_ok_at,
            next_fetch_at,
            // 计划时刻由核心持有并报给界面：界面照它显示「每天 06:00、18:00 自动更新」，
            // 不自己再写一份小时数，改规则时不会两边不一致。
            schedule_hours: DAILY_FETCH_HOURS.to_vec(),
            failing,
            total_items: self.store.count_feed_items()?,
        })
    }

    /// 刷新。`only` 指定单个源（手动刷新那一个）；否则刷新所有**已到期**的启用源。
    ///
    /// `force` 为真时忽略到期时间（用于「立即刷新」按钮）。
    pub fn refresh(
        &self,
        only: Option<&str>,
        force: bool,
        now: i64,
    ) -> Result<RefreshReport, CoreError> {
        let sources = self.store.list_feed_sources()?;
        let due: Vec<FeedSource> = sources
            .into_iter()
            .filter(|source| source.enabled || only == Some(source.id.as_str()))
            .filter(|source| {
                only == Some(source.id.as_str()) || force || source.next_fetch_at <= now
            })
            .collect();

        let mut report = RefreshReport {
            attempted: Vec::new(),
            succeeded: Vec::new(),
            not_modified: Vec::new(),
            failed: Vec::new(),
            skipped: Vec::new(),
            new_items: 0,
            next_fetch_at: next_daily_fetch(now, local_offset_seconds()),
        };
        let started = std::time::Instant::now();

        for mut source in due {
            if started.elapsed() >= REFRESH_BUDGET {
                report.skipped.push(source.id.clone());
                continue;
            }
            report.attempted.push(source.id.clone());
            match self.fetch_one(&source, now) {
                Ok(Fetched::Items(items, etag, last_modified)) => {
                    // 整源覆盖：这次抓到的就是本地留下的全部，旧条目里不在这次结果中的会被删掉。
                    // 「新增 N 条」仍然是「以前没见过」的条数，语义不变。
                    let inserted = self.store.sync_source_items(
                        &source.id,
                        &source.lang,
                        &items,
                        now,
                        PER_SOURCE_KEEP_ITEMS,
                    )?;
                    report.new_items += inserted;
                    source.etag = etag;
                    source.last_modified = last_modified;
                    source.last_ok_at = Some(now);
                    source.last_error = None;
                    source.fail_streak = 0;
                    source.next_fetch_at = next_fetch_for(&source.id, now);
                    report.succeeded.push(source.id.clone());
                }
                Ok(Fetched::NotModified {
                    etag,
                    last_modified,
                }) => {
                    source.etag = etag.or(source.etag);
                    source.last_modified = last_modified.or(source.last_modified);
                    source.last_ok_at = Some(now);
                    source.last_error = None;
                    source.fail_streak = 0;
                    source.next_fetch_at = next_fetch_for(&source.id, now);
                    report.not_modified.push(source.id.clone());
                }
                Err(error) => {
                    let message = error
                        .safe_details
                        .first()
                        .cloned()
                        .unwrap_or_else(|| error.message_key.clone());
                    source.fail_streak = source.fail_streak.saturating_add(1);
                    let backoff = backoff_seconds(source.fail_streak);
                    source.last_error = Some(message.clone());
                    source.next_fetch_at = now + backoff;
                    report.failed.push(FeedFailure {
                        source_id: source.id.clone(),
                        label: source.label.clone(),
                        message,
                        fail_streak: source.fail_streak,
                    });
                }
            }
            self.store.save_feed_source(&source)?;
        }

        report.next_fetch_at = self.status(now)?.next_fetch_at;
        self.store
            .prune_feed_items(now, RETENTION_DAYS, RETAINED_ITEMS)?;
        Ok(report)
    }

    fn fetch_one(&self, source: &FeedSource, now: i64) -> Result<Fetched, CoreError> {
        match source.kind {
            FeedKind::Rss => {
                let response = self.fetcher.get(
                    &source.url,
                    source.etag.as_deref(),
                    source.last_modified.as_deref(),
                    &self.user_agent,
                    None,
                )?;
                if response.not_modified() {
                    return Ok(Fetched::NotModified {
                        etag: response.etag,
                        last_modified: response.last_modified,
                    });
                }
                if response.status >= 400 {
                    return Err(CoreError::new(ErrorCode::Internal, "error.feedFailed")
                        .with_detail(format!("{} 返回 {}", source.url, response.status)));
                }
                let parsed = feed::parse(&response.body, now)?;
                Ok(Fetched::Items(
                    parsed.items,
                    response.etag,
                    response.last_modified,
                ))
            }
            FeedKind::GithubSearch => {
                let url = github_search_url(&source.url, now)?;
                let response = self.fetcher.get(
                    &url,
                    None,
                    None,
                    &self.user_agent,
                    self.github_token.as_deref(),
                )?;
                if response.status == 403 || response.status == 429 {
                    return Err(CoreError::new(ErrorCode::Internal, "error.feedRateLimited")
                        .with_detail(
                            "GitHub 的匿名访问频率已用尽；在设置里填一个令牌可以放宽限制"
                                .to_owned(),
                        ));
                }
                if response.status >= 400 {
                    return Err(CoreError::new(ErrorCode::Internal, "error.feedFailed")
                        .with_detail(format!("GitHub 搜索返回 {}", response.status)));
                }
                Ok(Fetched::Items(
                    parse_github_search(&response.body, &source.id, now)?,
                    None,
                    None,
                ))
            }
        }
    }
}

enum Fetched {
    Items(Vec<ParsedItem>, Option<String>, Option<String>),
    NotModified {
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

/// GitHub 搜索接口的地址：按创建时间过滤 + 星标排序。
fn github_search_url(window: &str, now: i64) -> Result<String, CoreError> {
    let days = match window {
        "today" => 1,
        "week" => 7,
        "month" => 30,
        other => {
            return Err(
                CoreError::new(ErrorCode::ValidationFailed, "error.feedBadWindow")
                    .with_detail(format!("不认识的时间窗口：{other}")),
            )
        }
    };
    let since = time::OffsetDateTime::from_unix_timestamp(now - days * 86_400)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .date();
    let query = format!(
        "created:>{}",
        since
            .format(&time::macros::format_description!("[year]-[month]-[day]"))
            .unwrap_or_default()
    );
    Ok(format!(
        "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page=25",
        percent_encoding::utf8_percent_encode(&query, percent_encoding::NON_ALPHANUMERIC)
    ))
}

/// 解析 GitHub 搜索结果。字段缺失的条目按「没有这条信息」处理，不编造。
pub fn parse_github_search(
    body: &str,
    source_id: &str,
    now: i64,
) -> Result<Vec<ParsedItem>, CoreError> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|error| {
        CoreError::new(ErrorCode::Internal, "error.feedUnparsable")
            .with_detail(format!("GitHub 搜索的响应不是合法 JSON：{error}"))
    })?;
    let Some(items) = value.get("items").and_then(|items| items.as_array()) else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|item| {
            let repo = item.get("full_name")?.as_str()?.to_owned();
            let url = item
                .get("html_url")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("https://github.com/{repo}"));
            let description = item
                .get("description")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_owned();
            let stars = item
                .get("stargazers_count")
                .and_then(|value| value.as_u64());
            let language = item
                .get("language")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let summary = if language.is_empty() {
                description.clone()
            } else {
                format!("{language} · {description}")
            };
            Some(ParsedItem {
                title: repo.clone(),
                url,
                published_at: Some(now),
                summary,
                id: format!("{source_id}:{repo}"),
                stars,
                repo: Some(repo),
            })
        })
        .collect())
}

/// 本机时区相对 UTC 的偏移（秒）。拿不到时按 UTC 排（`local-offset` 在某些平台上会失败）。
///
/// 界面只显示「下次自动更新 <相对时间>」，不解释时区；所以这里退化成 UTC 不会产生错误文案，
/// 最多是时刻偏了几小时——比整个抓取停摆好。
fn local_offset_seconds() -> i32 {
    time::UtcOffset::current_local_offset()
        .map(|offset| offset.whole_seconds())
        .unwrap_or(0)
}

/// 下一个计划时刻：本地时间的 06:00 或 18:00（`DAILY_FETCH_HOURS`）。
///
/// 纯函数：偏移由调用方给，所以断言可以用固定时刻写，不依赖跑测试的机器在哪个时区。
pub fn next_daily_fetch(now: i64, offset_seconds: i32) -> i64 {
    let offset = i64::from(offset_seconds);
    let local = now + offset;
    let day_start = local - local.rem_euclid(86_400);
    for hour in DAILY_FETCH_HOURS {
        let candidate = day_start + i64::from(hour) * 3600;
        if candidate > local {
            return candidate - offset;
        }
    }
    day_start + 86_400 + i64::from(DAILY_FETCH_HOURS[0]) * 3600 - offset
}

/// 同一个计划时刻内，各源按 id 错开一小段（0–`SLOT_SPREAD_SECONDS`）。
///
/// 用 id 做哈希而不是随机数：同一个源每次都落在同一分钟，行为可复现、可断言。
pub fn slot_jitter(source_id: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in source_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % SLOT_SPREAD_SECONDS as u64) as i64
}

/// 某个源的下次抓取时刻＝下一个计划时刻 + 它自己的错峰偏移。
fn next_fetch_for(source_id: &str, now: i64) -> i64 {
    next_daily_fetch(now, local_offset_seconds()) + slot_jitter(source_id)
}

/// 失败退避：5 分钟 → 15 分钟 → 1 小时封顶。
pub fn backoff_seconds(fail_streak: u32) -> i64 {
    match fail_streak {
        0 | 1 => 5 * 60,
        2 => 15 * 60,
        _ => 60 * 60,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InMemoryHubStore;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeFetcher {
        /// 网址 → 响应。
        responses: Mutex<Vec<(String, FeedResponse)>>,
        calls: Mutex<Vec<String>>,
    }

    impl FakeFetcher {
        fn with(url: &str, body: &str) -> Self {
            let fetcher = Self::default();
            fetcher.responses.lock().unwrap().push((
                url.to_owned(),
                FeedResponse {
                    status: 200,
                    body: body.to_owned(),
                    etag: Some("\"v1\"".to_owned()),
                    last_modified: None,
                },
            ));
            fetcher
        }

        fn push(&self, url: &str, response: FeedResponse) {
            self.responses
                .lock()
                .unwrap()
                .push((url.to_owned(), response));
        }
    }

    impl FeedFetcher for FakeFetcher {
        fn get(
            &self,
            url: &str,
            _etag: Option<&str>,
            _last_modified: Option<&str>,
            _user_agent: &str,
            _bearer: Option<&str>,
        ) -> Result<FeedResponse, CoreError> {
            self.calls.lock().unwrap().push(url.to_owned());
            let responses = self.responses.lock().unwrap();
            match responses
                .iter()
                .rev()
                .find(|(key, _)| url.starts_with(key.as_str()))
            {
                Some((_, response)) => Ok(response.clone()),
                None => Err(CoreError::internal(format!(
                    "假抓取器没有为 {url} 准备响应"
                ))),
            }
        }
    }

    fn service(fetcher: Arc<dyn FeedFetcher>) -> (ContentService, Arc<InMemoryHubStore>) {
        let store = Arc::new(InMemoryHubStore::new());
        (ContentService::new(store.clone(), fetcher, None), store)
    }

    fn one_source(store: &InMemoryHubStore, kind: FeedKind, url: &str) {
        store
            .save_feed_source(&FeedSource {
                id: "test".to_owned(),
                kind,
                url: url.to_owned(),
                label: "测试源".to_owned(),
                lang: "zh".to_owned(),
                enabled: true,
                etag: None,
                last_modified: None,
                last_ok_at: None,
                last_error: None,
                fail_streak: 0,
                next_fetch_at: 0,
                builtin: false,
            })
            .unwrap();
    }

    const RSS: &str = r#"<rss><channel><title>源</title>
        <item><title>第一条</title><link>https://example.com/1</link><pubDate>Mon, 21 Sep 2026 02:00:00 +0000</pubDate></item>
        <item><title>第二条</title><link>https://example.com/2</link></item>
        </channel></rss>"#;

    #[test]
    fn defaults_are_inserted_once_and_never_reavour_a_deleted_source() {
        let (service, store) = service(Arc::new(FakeFetcher::default()));
        service.ensure_defaults(0).unwrap();
        assert_eq!(
            store.list_feed_sources().unwrap().len(),
            DEFAULT_SOURCES.len()
        );
        service.ensure_defaults(0).unwrap();
        assert_eq!(
            store.list_feed_sources().unwrap().len(),
            DEFAULT_SOURCES.len()
        );
    }

    /// 生成一份 RSS：标题与链接都由 `tag` 与索引决定，便于断言「谁还在、谁被删了」。
    fn rss_of(tag: &str, count: usize) -> String {
        let items: String = (0..count)
            .map(|index| {
                format!(
                    "<item><title>{tag}-{index}</title><link>https://a.test/{tag}/{index}</link><pubDate>Mon, 21 Sep 2026 0{index}:00:00 GMT</pubDate></item>"
                )
            })
            .collect();
        format!("<rss version=\"2.0\"><channel><title>t</title>{items}</channel></rss>")
    }

    /// 刷新是**整源覆盖**：这次没抓到的旧条目要消失，且每源只留最新的 N 条
    /// （用户要求：更新时把旧的覆盖掉、本地尽量不存数据）。
    #[test]
    fn refresh_replaces_the_previous_snapshot_and_caps_per_source() {
        let fetcher = Arc::new(FakeFetcher::with("https://a.test/feed", &rss_of("old", 25)));
        let (service, store) = service(fetcher.clone());
        one_source(&store, FeedKind::Rss, "https://a.test/feed");

        let first = service.refresh(None, true, 1_700_000_000).unwrap();
        assert_eq!(first.succeeded, vec!["test".to_owned()]);
        // 每源只留最新的 PER_SOURCE_KEEP_ITEMS 条：25 条抓进来，留下 20 条。
        assert_eq!(store.count_feed_items().unwrap(), PER_SOURCE_KEEP_ITEMS);

        // 第二轮：源上只剩 3 条，且与上一轮完全不同。
        fetcher.responses.lock().unwrap().clear();
        fetcher.push(
            "https://a.test/feed",
            FeedResponse {
                status: 200,
                body: rss_of("new", 3),
                etag: Some("\"v2\"".to_owned()),
                last_modified: None,
            },
        );
        let second = service.refresh(None, true, 1_700_086_400).unwrap();
        assert_eq!(second.new_items, 3);
        let items = service.items(None, None, 50, 0).unwrap();
        assert_eq!(items.len(), 3, "上一轮的条目应被覆盖掉，而不是累加");
        assert!(items.iter().all(|item| item.title.starts_with("new-")));
    }

    /// 源返回 200 但正文里一条都没有：保留上一份快照，不把本地清空。
    #[test]
    fn an_empty_feed_does_not_wipe_the_previous_snapshot() {
        let fetcher = Arc::new(FakeFetcher::with("https://a.test/feed", RSS));
        let (service, store) = service(fetcher.clone());
        one_source(&store, FeedKind::Rss, "https://a.test/feed");
        service.refresh(None, true, 1_700_000_000).unwrap();
        assert_eq!(store.count_feed_items().unwrap(), 2);

        fetcher.responses.lock().unwrap().clear();
        fetcher.push(
            "https://a.test/feed",
            FeedResponse {
                status: 200,
                body: "<rss version=\"2.0\"><channel><title>t</title></channel></rss>".to_owned(),
                etag: Some("\"v2\"".to_owned()),
                last_modified: None,
            },
        );
        let report = service.refresh(None, true, 1_700_086_400).unwrap();
        assert_eq!(report.new_items, 0);
        assert_eq!(store.count_feed_items().unwrap(), 2, "空正文不该清空本地");
    }

    #[test]
    fn refresh_stores_items_and_records_success() {
        let fetcher = Arc::new(FakeFetcher::with("https://a.test/feed", RSS));
        let (service, store) = service(fetcher);
        one_source(&store, FeedKind::Rss, "https://a.test/feed");

        let report = service.refresh(None, true, 1_700_000_000).unwrap();
        assert_eq!(report.succeeded, vec!["test".to_owned()]);
        assert_eq!(report.new_items, 2);
        assert!(report.failed.is_empty());

        let items = service.items(None, None, 50, 0).unwrap();
        assert_eq!(items.len(), 2);
        // 文字段必须是真实来源，不能是编的。
        assert_eq!(items[0].source_label, "测试源");
        assert!(items.iter().any(|item| item.title == "第一条"));
    }

    #[test]
    fn second_refresh_is_a_conditional_request_and_304_counts_as_healthy() {
        let fetcher = Arc::new(FakeFetcher::with("https://a.test/feed", RSS));
        let (service, store) = service(fetcher);
        one_source(&store, FeedKind::Rss, "https://a.test/feed");
        service.refresh(None, true, 100).unwrap();

        // 让源进入「已到期」状态，再让假抓取器回 304。
        let service2 = {
            let mut source = store
                .list_feed_sources()
                .unwrap()
                .into_iter()
                .next()
                .unwrap();
            source.next_fetch_at = 0;
            store.save_feed_source(&source).unwrap();
            let fetcher = Arc::new(FakeFetcher::default());
            fetcher.push(
                "https://a.test/feed",
                FeedResponse {
                    status: 304,
                    body: String::new(),
                    etag: Some("\"v1\"".to_owned()),
                    last_modified: None,
                },
            );
            ContentService::new(store.clone(), fetcher, None)
        };
        let report = service2.refresh(None, true, 200).unwrap();
        assert_eq!(report.not_modified, vec!["test".to_owned()]);
        assert!(report.failed.is_empty(), "没更新不是失败");
        assert_eq!(
            service2.items(None, None, 50, 0).unwrap().len(),
            2,
            "旧条目要留着"
        );
    }

    #[test]
    fn failure_backs_off_and_keeps_the_previous_snapshot() {
        let fetcher = Arc::new(FakeFetcher::with("https://a.test/feed", RSS));
        let (service, store) = service(fetcher);
        one_source(&store, FeedKind::Rss, "https://a.test/feed");
        service.refresh(None, true, 100).unwrap();

        // 换成会失败的抓取器，重试三次，观察退避是否递增。
        let failing: Arc<dyn FeedFetcher> = Arc::new(FakeFetcher::default());
        let service = ContentService::new(store.clone(), failing, None);
        let first = service.refresh(None, true, 200).unwrap();
        assert_eq!(first.failed.len(), 1);
        assert_eq!(first.failed[0].fail_streak, 1);
        let source = store
            .list_feed_sources()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(source.next_fetch_at, 200 + 5 * 60);
        assert!(source.last_error.is_some(), "失败原因必须留下来");

        let second = service.refresh(None, true, 300).unwrap();
        assert_eq!(second.failed[0].fail_streak, 2);
        let source = store
            .list_feed_sources()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(source.next_fetch_at, 300 + 15 * 60);

        let third = service.refresh(None, true, 400).unwrap();
        assert_eq!(third.failed[0].fail_streak, 3);
        let source = store
            .list_feed_sources()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(source.next_fetch_at, 400 + 3600, "退避封顶在 1 小时");

        assert_eq!(
            service.items(None, None, 50, 0).unwrap().len(),
            2,
            "失败时旧内容必须留着"
        );
    }

    #[test]
    fn sources_that_are_not_due_are_left_alone_unless_forced() {
        let fetcher = Arc::new(FakeFetcher::with("https://a.test/feed", RSS));
        let (service, store) = service(fetcher);
        one_source(&store, FeedKind::Rss, "https://a.test/feed");
        service.refresh(None, true, 100).unwrap();

        let quiet = service.refresh(None, false, 110).unwrap();
        assert!(quiet.attempted.is_empty(), "没到期就不该抓");
        let forced = service.refresh(None, true, 110).unwrap();
        assert_eq!(forced.attempted.len(), 1);
    }

    #[test]
    fn disabled_sources_are_skipped_unless_named_explicitly() {
        let fetcher = Arc::new(FakeFetcher::with("https://a.test/feed", RSS));
        let (service, store) = service(fetcher);
        one_source(&store, FeedKind::Rss, "https://a.test/feed");
        let mut source = store
            .list_feed_sources()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        source.enabled = false;
        store.save_feed_source(&source).unwrap();

        assert!(service
            .refresh(None, true, 100)
            .unwrap()
            .attempted
            .is_empty());
        assert_eq!(
            service
                .refresh(Some("test"), false, 100)
                .unwrap()
                .attempted
                .len(),
            1,
            "点名刷新一个停用的源应当照做"
        );
    }

    #[test]
    fn status_reports_failing_sources_with_their_reason() {
        let fetcher: Arc<dyn FeedFetcher> = Arc::new(FakeFetcher::default());
        let (service, store) = service(fetcher);
        one_source(&store, FeedKind::Rss, "https://a.test/feed");
        service.refresh(None, true, 100).unwrap();

        let status = service.status(200).unwrap();
        assert_eq!(status.failing.len(), 1);
        assert_eq!(status.failing[0].label, "测试源");
        assert!(status.last_ok_at.is_none(), "从没成功过就不能报成功过");
    }

    #[test]
    fn github_search_is_parsed_into_real_items() {
        let body = r#"{"total_count":2,"items":[
            {"full_name":"owner/repo","html_url":"https://github.com/owner/repo",
             "description":"一个项目","stargazers_count":123,"language":"Rust"},
            {"full_name":"another/one","html_url":"https://github.com/another/one",
             "description":null,"stargazers_count":9,"language":null}
        ]}"#;
        let items = parse_github_search(body, "github-week", 500).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "owner/repo");
        assert_eq!(items[0].url, "https://github.com/owner/repo");
        assert!(items[0].summary.contains("Rust"));
        assert_eq!(items[0].published_at, Some(500));
        assert!(
            items[1].summary.trim().is_empty(),
            "没有语言与描述时不编内容"
        );
    }

    #[test]
    fn github_search_windows_map_to_real_urls() {
        let url = github_search_url("week", 1_789_956_000).unwrap();
        assert!(url.starts_with("https://api.github.com/search/repositories?q=created"));
        assert!(url.contains("sort=stars"));
        assert!(github_search_url("forever", 0).is_err());
    }

    /// 落点必须永远是**本地的 06:00 或 18:00**，且严格在 `now` 之后、最多等一天。
    /// 偏移写死在用例里，所以断言与跑测试的机器在哪个时区无关；含半小时时区（+5:30）。
    #[test]
    fn next_fetch_lands_on_the_daily_slots() {
        /// 一个确定的基准日 00:00 对应的 unix 时间戳（2026-09-21T00:00:00Z）。
        const BASE: i64 = 1_789_948_800;
        for offset in [0i32, 8 * 3600, -5 * 3600, 5 * 3600 + 1800, -3 * 3600 - 1800] {
            let local_midnight = BASE - i64::from(offset) + 86_400; // 本地当天 00:00
            for probe in [
                0i64,
                1,
                3600,
                6 * 3600 - 1,
                6 * 3600,
                6 * 3600 + 1,
                12 * 3600,
                18 * 3600,
                23 * 3600 + 3599,
            ] {
                let now = local_midnight + probe;
                let next = next_daily_fetch(now, offset);
                let local = next + i64::from(offset);
                let hour = local.rem_euclid(86_400) / 3600;
                assert!(
                    hour == 6 || hour == 18,
                    "偏移 {offset}、本地第 {probe} 秒落到了 {hour} 点"
                );
                assert!(next > now, "下一次必须在未来");
                assert!(next - now <= 86_400, "最多等一天");
            }

            // 正点那一刻已经算「到点」：下一个是 18:00，不是原地返回（否则会连抓两轮）。
            let at_six = local_midnight + 6 * 3600;
            assert_eq!(next_daily_fetch(at_six, offset), local_midnight + 18 * 3600);
            // 18:00 之后跨到第二天 06:00。
            assert_eq!(
                next_daily_fetch(local_midnight + 18 * 3600 + 1, offset),
                local_midnight + 86_400 + 6 * 3600
            );
        }
    }

    /// 错峰：同一个源每次都落在同一分钟，不同源会散开，且都在 0–5 分钟内。
    #[test]
    fn slot_jitter_is_stable_and_bounded() {
        for id in ["sspai", "ruanyifeng", "hn", "github-week"] {
            let jitter = slot_jitter(id);
            assert_eq!(jitter, slot_jitter(id), "同一个源必须落在同一分钟");
            assert!(
                (0..SLOT_SPREAD_SECONDS).contains(&jitter),
                "错峰要落在 0–5 分钟内"
            );
        }
        assert_ne!(slot_jitter("sspai"), slot_jitter("hn"), "不同源应错开");
    }

    #[test]
    fn source_validation_rejects_obviously_wrong_input() {
        let (service, _store) = service(Arc::new(FakeFetcher::default()));
        let draft = |url: &str, label: &str, kind: FeedKind| FeedSourceDraft {
            id: None,
            kind,
            url: url.to_owned(),
            label: label.to_owned(),
            lang: "zh".to_owned(),
            enabled: true,
        };
        assert!(service
            .save_source(draft("ftp://example.com/feed", "x", FeedKind::Rss), 0)
            .is_err());
        assert!(
            service
                .save_source(draft("https://example.com/feed", "  ", FeedKind::Rss), 0)
                .is_err(),
            "没有名字的源不可保存"
        );
        assert!(service
            .save_source(draft("yesterday", "窗口", FeedKind::GithubSearch), 0)
            .is_err());
    }

    #[test]
    fn saving_an_existing_source_keeps_its_fetch_state() {
        let fetcher = Arc::new(FakeFetcher::with("https://a.test/feed", RSS));
        let (service, store) = service(fetcher);
        one_source(&store, FeedKind::Rss, "https://a.test/feed");
        service.refresh(None, true, 100).unwrap();
        let before = store
            .list_feed_sources()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert!(before.etag.is_some());

        let saved = service
            .save_source(
                FeedSourceDraft {
                    id: Some("test".to_owned()),
                    kind: FeedKind::Rss,
                    url: "https://a.test/feed".to_owned(),
                    label: "改过的名字".to_owned(),
                    lang: "zh".to_owned(),
                    enabled: false,
                },
                500,
            )
            .unwrap();
        assert_eq!(saved.label, "改过的名字");
        assert!(!saved.enabled);
        assert_eq!(saved.last_ok_at, Some(100), "抓取历史不能被一次保存清掉");
        assert_eq!(
            saved.next_fetch_at, before.next_fetch_at,
            "下次抓取时间属于服务端事实"
        );

        // 换地址则必须丢掉条件请求凭据。
        let moved = service
            .save_source(
                FeedSourceDraft {
                    id: Some("test".to_owned()),
                    kind: FeedKind::Rss,
                    url: "https://b.test/feed".to_owned(),
                    label: "换地址".to_owned(),
                    lang: "zh".to_owned(),
                    enabled: true,
                },
                600,
            )
            .unwrap();
        assert!(moved.etag.is_none(), "地址变了，旧的 ETag 必须丢掉");
    }

    #[test]
    fn refresh_budget_skips_the_rest_instead_of_hanging() {
        // 预算行为本身用常量断言，避免在测试里真的等 60 秒。
        assert!(REFRESH_BUDGET >= Duration::from_secs(30));
        assert!(REFRESH_BUDGET <= Duration::from_secs(120));
    }
}
