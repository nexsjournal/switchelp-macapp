//! 技能市场：ClawHub（社区）与腾讯 SkillHub 的只读抓取器。
//!
//! 两个市场都提供**匿名只读 JSON 接口**（2026-09-30 实测：无 token、无 cookie，
//! 响应里没有需要登录才能拿到的字段），所以它们和 GitHub 来源共用同一条安装链路：
//! 这里只负责把市场的技能映射成「以 slug 命名的目录」——`{slug}/SKILL.md` 以及技能自己的
//! 其余文件——`source::assemble` 认这个形状，预览 / 安装 / 已装清单因此一行都不用改。
//!
//! 只发 GET，只读内容，不执行从市场拿到的任何东西。两个市场各自的接口：
//!
//! | 用途 | ClawHub | SkillHub |
//! | --- | --- | --- |
//! | 列表 | `/api/v1/skills?limit=` | `/api/skills?pageSize=` |
//! | 搜索 | `/api/v1/search?q=&limit=` | `/api/skills?keyword=&pageSize=` |
//! | 详情 | `/api/v1/skills/{slug}?owner=` | `/api/v1/skills/{slug}` |
//! | 文件清单 | 无（平台只发 `SKILL.md`） | `/api/v1/skills/{slug}/files` |
//! | 单个文件 | 详情里的 `skill.description` 就是 `SKILL.md` 全文 | `/api/v1/skills/{slug}/file?path=` |
//!
//! **只装技能自己的文件**：平台的元数据（ClawHub 的 `_meta.json`、`skill-card.md` 之类）
//! 不进清单，也不写进用户的技能目录。
//!
//! 来源写法（界面与 `PluginService` 都用这一套；市场来源里没有 `/`，界面正是据此把
//! 它与 `owner/repo` 分开的）：
//!
//! ```text
//! clawhub                     ClawHub 的默认目录
//! clawhub?q=文档               ClawHub 搜索
//! clawhub:{归属者}/{slug}      一个 ClawHub 技能
//! skillhub / skillhub?q=文档   SkillHub 的目录 / 搜索
//! skillhub:{slug}             一个 SkillHub 技能
//! ```
//!
//! **搜索词为什么编在来源写法里**：界面装技能时只把目录里的 `repo` 原样交回来
//! （`plugins_preview` 的请求里只有 repo 与技能目录名，没有再带搜索词）。词一旦丢了，
//! 从搜索结果里点「添加技能」就会变成「拿默认目录去找这个技能」，排在 30 名之外的技能
//! 直接装不上。编进去之后，目录、预览、安装看到的始终是同一份结果。

use std::{collections::HashMap, sync::Mutex, time::Duration};

use serde_json::Value;

use crate::domain::error::{CoreError, ErrorCode};

use super::{
    skill::MAX_FILE_BYTES,
    source::{RepoBlob, RepoFetcher},
};

/// URL 里的百分号编码集合。
///
/// 只编掉「必须编」的字符（RFC 3986 的 unreserved 之外）：市场接口的路径参数里 `-`、`.` 很常见
/// （`rosemond-contract-compliance`），用 `source::percent_encode` 那种把 `-` 也编成 `%2D`
/// 的做法虽然仍然合法，却难读，也多押了一次「对方的 CDN 会不会照 RFC 解码」的赌注。
const ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// 百分号编码一段路径或查询值。
fn encode(segment: &str) -> String {
    percent_encoding::utf8_percent_encode(segment, ENCODE).to_string()
}

/* ------------------------------------------------------------------ 常量 */

/// ClawHub 的接口入口。测试用 [`RegistryFetcher::with_bases`] 换掉，所以必须可注入。
const CLAWHUB_BASE: &str = "https://clawhub.ai";
/// SkillHub 的接口入口。
const SKILLHUB_BASE: &str = "https://api.skillhub.cn";
/// ClawHub 的站点。技能页在这里，接口也在同一个域名下。
const CLAWHUB_SITE: &str = "https://clawhub.ai";
/// SkillHub 的站点。**与接口不是同一个域名**，回链要拼站点不能拼接口：
/// 接口响应里的 `homepage` 字段看着像页面，实测是 API 资源，浏览器 GET 会 405
/// （站点自己的 `rel=canonical` 指向 `https://skillhub.cn/skills/{slug}`）。
const SKILLHUB_SITE: &str = "https://skillhub.cn";

/// 两个市场在来源列表里的标识，同时也是 `RepoCatalog.repo` 与已装记录里的来源名。
pub const CLAWHUB_SOURCE: &str = "clawhub";
/// SkillHub 的标识。
pub const SKILLHUB_SOURCE: &str = "skillhub";

/// 一次列表/搜索最多取多少条，两个市场共用。
///
/// 目录阶段之后每个技能还要再打一次详情（SkillHub 还要打一次文件清单，装的时候再读一次
/// 正文），30 条已经是一次浏览 60 次上下的请求。再多就是白烧对方的限额——ClawHub 的读
/// 限额是每 IP 3000 次/分钟，但没人会在一页结果里往下翻，取多了只是让它更早被限。
const MAX_PAGE: usize = 30;

/// 列表/详情 JSON 的读取上限。
///
/// 明显大于单个技能文件的上限（[`MAX_FILE_BYTES`]）：SkillHub 一页 30 条的中文描述
/// 实测能到上百 KB，而 ClawHub 的详情响应把整篇 `SKILL.md` 包在里面。
/// 正文本身的长度由 `assemble` 按 [`MAX_FILE_BYTES`] 判（超了就跳过这个技能），
/// 不在这里判——在这里判会让整个目录页因为一个超大技能而打不开。
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;

/// 搜索词的长度上限。搜索框里不该出现 100 字以上的词；真出现了就截断，
/// 让请求还是一次正常的搜索，而不是把 URL 撑到对方看不懂。
const MAX_QUERY_CHARS: usize = 100;

/// slug / 归属者 / 文件路径里一段的长度上限。
const MAX_SEGMENT_LEN: usize = 80;

/// 列表里逐个技能的附加请求（SkillHub 的文件清单）的并发数。太低没效果，太高对源站不礼貌。
const LIST_CONCURRENCY: usize = 6;

/// 列表页的短期备忘（秒）。
///
/// `resolve_commit` 与 `list_blobs` 是同一次浏览的两步：前者要目录里最新的更新时间当
/// 版本标记，后者要全部条目。中间不该把同一页列表再打一遍——一页最多 30 条，
/// SkillHub 的响应能到上百 KB。TTL 很短，只是把这两步撮合到一起。
const LISTING_MEMO_SECONDS: i64 = 20;

/// 文件正文缓存（秒）。预览与安装是同一次浏览里的两个动作，两次都要读同一批文件；
/// 装完再点一次「添加技能」也在缓存期内——一次市场浏览最多 30 个技能，
/// 重读一轮就是几十次请求。
const FILE_CACHE_SECONDS: i64 = 300;

/// 正文缓存的条数上限。一个技能最多 40 个文件（`skill::MAX_FILES_PER_SKILL`），
/// 留够几次浏览的量即可。
const MAX_CACHED_FILES: usize = 256;

/* ------------------------------------------------------------------ 来源写法 */

/// 市场来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrySource {
    /// ClawHub（社区市场，openclaw/clawhub，MIT）。
    Clawhub,
    /// 腾讯 SkillHub。
    Skillhub,
}

impl RegistrySource {
    /// 来源写法里的标识。
    pub fn id(self) -> &'static str {
        match self {
            Self::Clawhub => CLAWHUB_SOURCE,
            Self::Skillhub => SKILLHUB_SOURCE,
        }
    }

    /// 给用户看的名字（错误说明与默认来源标签里用）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Clawhub => "ClawHub",
            Self::Skillhub => "SkillHub",
        }
    }

    /// 站点入口。回链与「在市场里查看」都从这里拼。
    fn site(self) -> &'static str {
        match self {
            Self::Clawhub => CLAWHUB_SITE,
            Self::Skillhub => SKILLHUB_SITE,
        }
    }
}

/// 这个来源写法属于哪个市场。
///
/// 判据与界面的 `isRegistry` 一致：以标识开头，后面跟 `?`（搜索）或 `:`（单个技能）；
/// `clawhubx`、`owner/clawhub` 这些都不算。返回 `None` 就是 GitHub 一侧的来源（`owner/repo`）。
pub fn registry_source(spec: &str) -> Option<RegistrySource> {
    let spec = spec.trim();
    [RegistrySource::Clawhub, RegistrySource::Skillhub]
        .into_iter()
        .find(|source| {
            spec.strip_prefix(source.id())
                .map(|rest| rest.is_empty() || rest.starts_with('?') || rest.starts_with(':'))
                .unwrap_or(false)
        })
}

/// 解析后的来源写法。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Spec {
    source: RegistrySource,
    /// 搜索词（`?q=`）。单个技能的写法上不接搜索词。
    query: Option<String>,
    /// 单个技能；`None` 表示「这个市场的目录页」。
    skill: Option<SkillRef>,
}

/// 一个技能的定位信息。`owner` 只有 ClawHub 有（同名 slug 属于多个归属者时靠它区分）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillRef {
    owner: Option<String>,
    slug: String,
}

impl Spec {
    /// 解析来源写法。**这一步必须在发任何请求之前跑完**：写法不对时不该已经打过网络。
    fn parse(raw: &str) -> Result<Self, CoreError> {
        let source = registry_source(raw).ok_or_else(|| invalid_spec(raw))?;
        let trimmed = raw.trim();
        let rest = trimmed.strip_prefix(source.id()).unwrap_or_default();
        let (head, query) = match rest.split_once('?') {
            Some((head, tail)) => {
                let query = tail
                    .strip_prefix("q=")
                    .ok_or_else(|| invalid_spec(raw))?
                    .trim();
                (
                    head,
                    Some(query.to_owned()).filter(|value| !value.is_empty()),
                )
            }
            None => (rest, None),
        };
        let skill = match head.strip_prefix(':') {
            Some(payload) => Some(parse_skill_ref(source, payload, raw)?),
            None if head.is_empty() => None,
            None => return Err(invalid_spec(raw)),
        };
        if skill.is_some() && query.is_some() {
            // 一个技能不是一个搜索，两者同时给是写错了。
            return Err(invalid_spec(raw));
        }
        Ok(Self {
            source,
            query,
            skill,
        })
    }
}

/// 解析 `:` 后面的技能写法：ClawHub 要 `{归属者}/{slug}`（归属者可省，同名 slug 有歧义时
/// 平台会回 409），SkillHub 只要 `{slug}`。
fn parse_skill_ref(
    source: RegistrySource,
    payload: &str,
    raw: &str,
) -> Result<SkillRef, CoreError> {
    let (owner, slug) = match payload.split_once('/') {
        Some((owner, slug)) => (Some(owner), slug),
        None => (None, payload),
    };
    if owner.is_some() && source == RegistrySource::Skillhub {
        // SkillHub 的技能只按 slug 定位，`skillhub:{归属者}/{slug}` 是写错了。
        return Err(invalid_spec(raw));
    }
    let slug = validate_segment(slug).map_err(|_| invalid_spec(raw))?;
    let owner = match owner {
        Some(owner) => Some(validate_segment(owner).map_err(|_| invalid_spec(raw))?),
        None => None,
    };
    Ok(SkillRef { owner, slug })
}

/// 把搜索词编进来源写法（理由见模块头）。
///
/// - `query` 是 `None`：原样返回。装技能时界面只会交回目录里的 repo，那一趟没有搜索词，
///   不能把已有的词清掉。
/// - `query` 是空串：清掉词，回到默认目录（界面清空搜索框就是这个意思）。
/// - 单个技能的写法上不挂搜索词，原样返回。
pub fn compose_spec(spec: &str, query: Option<&str>) -> String {
    let trimmed = spec.trim();
    let Ok(parsed) = Spec::parse(trimmed) else {
        return trimmed.to_owned();
    };
    let Some(query) = query else {
        return trimmed.to_owned();
    };
    if parsed.skill.is_some() {
        return trimmed.to_owned();
    }
    let query = query.trim();
    if query.is_empty() {
        return parsed.source.id().to_owned();
    }
    format!(
        "{}?q={}",
        parsed.source.id(),
        query.chars().take(MAX_QUERY_CHARS).collect::<String>()
    )
}

/// 这份目录在来源平台上的页面地址（界面上的「在市场里查看」，也是 ClawHub 要求的回链）。
///
/// - **单个技能**：技能自己的页面。ClawHub 允许第三方目录，但要求回链到技能页
///   （`https://clawhub.ai/{归属者}/skills/{slug}`）。写法里没有归属者时给不出准确的
///   地址，退一步给该 slug 的搜索页——也是一个指向平台的链接，界面的按钮不会指向别处。
/// - **目录页**：平台在该关键词下的搜索页（ClawHub 的 `?q=` 已核实会渲染出结果），
///   没有关键词时是平台的技能列表页。SkillHub 的站点搜索参数没有文档，不猜，给列表页。
/// - 不是市场来源 → `None`，界面回落到原来的行为。
pub fn homepage_of(spec: &str) -> Option<String> {
    let parsed = Spec::parse(spec).ok()?;
    let site = parsed.source.site();
    let Some(skill) = &parsed.skill else {
        return Some(match parsed.query.as_deref() {
            Some(query) if parsed.source == RegistrySource::Clawhub => {
                format!("{site}/skills?q={}", encode(query))
            }
            _ => format!("{site}/skills"),
        });
    };
    let slug = &skill.slug;
    Some(match (&skill.owner, parsed.source) {
        (Some(owner), RegistrySource::Clawhub) => format!("{site}/{owner}/skills/{slug}"),
        (None, RegistrySource::Clawhub) => format!("{site}/skills?q={slug}"),
        (_, RegistrySource::Skillhub) => format!("{site}/skills/{slug}"),
    })
}

/* ------------------------------------------------------------------ 列表里的条目 */

/// 市场列表里的一个技能：够用来拼文件清单、版本标记与回链。
///
/// 名字与描述不从这里取——它们由 `SKILL.md` 的 front-matter 决定（`skill::parse`），
/// 与仓库来源走同一条路，界面显示的两者因此不会有差别。
#[derive(Debug, Clone, PartialEq, Eq)]
struct MarketEntry {
    slug: String,
    /// ClawHub 的归属者（`ownerHandle`）；SkillHub 按 slug 定位，这里是 `None`。
    owner: Option<String>,
    /// 平台侧的版本号。
    version: Option<String>,
    /// 平台侧的更新时间（Unix 毫秒）。
    updated_at: Option<i64>,
}

/* ------------------------------------------------------------------ 抓取器 */

/// 两个市场的只读抓取器。
///
/// 一个实例同时带两边的入口，`RepoFetcher` 的实现按来源写法分派——`PluginService` 因此
/// 只要在装配时多接一个抓取器，不必为两个市场各开一条链路。
pub struct RegistryFetcher {
    agent: ureq::Agent,
    /// 随包标识。源站至少能看到是谁在请求。
    user_agent: String,
    clawhub_base: String,
    skillhub_base: String,
    /// 列表页备忘，键是来源写法（含搜索词）。
    listings: Mutex<HashMap<String, (i64, Vec<MarketEntry>)>>,
    /// 文件正文缓存，键是「来源写法 + 版本标记 + 路径」。
    files: Mutex<HashMap<String, (i64, Vec<u8>)>>,
    /// slug → 归属者（ClawHub 专用）。目录页里读某个技能的文件时，归属者只能从这里找；
    /// 它由列表与详情响应填上，填不上就不带 `?owner=` 去问平台。
    owners: Mutex<HashMap<String, String>>,
}

impl RegistryFetcher {
    pub fn new(user_agent: String) -> Self {
        Self::with_bases(
            user_agent,
            CLAWHUB_BASE.to_owned(),
            SKILLHUB_BASE.to_owned(),
        )
    }

    /// 测试与镜像用：把两个市场的入口指到别处。
    pub fn with_bases(user_agent: String, clawhub_base: String, skillhub_base: String) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                // 与 GitHub 一侧同一份出网策略（系统代理 / 环境变量，见 `platform::proxy`）。
                .proxy(crate::platform::proxy::outbound_proxy(
                    crate::platform::Platform::current(),
                ))
                .build(),
        );
        Self {
            agent,
            user_agent,
            clawhub_base,
            skillhub_base,
            listings: Mutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
            owners: Mutex::new(HashMap::new()),
        }
    }

    /* ---------------------------------------------------------------- 网络 */

    /// 只读 GET。
    ///
    /// **必须跟随重定向**：SkillHub 的单文件接口 302 到对象存储，不跟随就只能拿到一张空壳
    /// （正文里只有一行 `Found`）。ureq 默认跟随（最多 10 跳），这里不再自己处理。
    fn get(&self, url: &str, limit: usize) -> Result<Vec<u8>, CoreError> {
        let mut response = self
            .agent
            .get(url)
            .header("accept", "application/json, text/plain;q=0.9, */*;q=0.8")
            .header("user-agent", &self.user_agent)
            .call()
            .map_err(|error| {
                CoreError::new(ErrorCode::Internal, "error.pluginRegistryUnreachable")
                    .with_detail(format!("访问 {url} 失败：{error}"))
            })?;
        let status = response.status().as_u16();
        if status == 429 {
            // 两个市场都不在响应头里给限额数值（接口文档也没有），只说清是谁限的、
            // 上游要求等多久——能行动的信息就这些。
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            return Err(
                CoreError::new(ErrorCode::Internal, "error.pluginRegistryRateLimited").with_detail(
                    rate_limited_detail(self.source_of_url(url).label(), retry_after.as_deref()),
                ),
            );
        }
        if status == 404 {
            return Err(
                CoreError::new(ErrorCode::NotFound, "error.pluginRegistrySkillMissing")
                    .with_detail(format!(
                        "{url} 返回 404：这个技能在市场上不存在（可能已下架或换了名字）"
                    )),
            );
        }
        if status == 409 {
            // ClawHub 对同名 slug 的回应：列出候选，要我们指明归属者。不替用户猜。
            return Err(
                CoreError::new(ErrorCode::NotFound, "error.pluginRegistrySkillMissing")
                    .with_detail(format!(
                        "{url} 返回 409：这个 slug 属于多个归属者，请从该技能的页面重新打开一次"
                    )),
            );
        }
        if !(200..300).contains(&status) {
            return Err(
                CoreError::new(ErrorCode::Internal, "error.pluginRegistryBadResponse")
                    .with_detail(format!("{url} 返回 {status}")),
            );
        }
        response
            .body_mut()
            .with_config()
            .limit(limit as u64)
            .read_to_vec()
            .map_err(|error| {
                CoreError::new(ErrorCode::Internal, "error.pluginRegistryBadResponse")
                    .with_detail(format!("读取 {url} 的响应失败：{error}"))
            })
    }

    fn json(&self, url: &str) -> Result<Value, CoreError> {
        let bytes = self.get(url, MAX_JSON_BYTES)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| bad_response(url, &format!("响应不是合法 JSON：{error}")))
    }

    /// 出错的 URL 属于哪个市场（说明里要写清是谁限的流）。
    fn source_of_url(&self, url: &str) -> RegistrySource {
        if url.starts_with(&self.skillhub_base) {
            RegistrySource::Skillhub
        } else {
            RegistrySource::Clawhub
        }
    }

    /* ---------------------------------------------------------------- 列表 */

    /// 列出一页市场技能（最多 [`MAX_PAGE`] 条）：有搜索词走搜索接口，否则走列表接口。
    fn listing(&self, spec: &Spec, raw: &str) -> Result<Vec<MarketEntry>, CoreError> {
        if let Some(entries) = self.memo_listing(raw) {
            return Ok(entries);
        }
        let entries = match spec.source {
            RegistrySource::Clawhub => self.clawhub_listing(spec)?,
            RegistrySource::Skillhub => self.skillhub_listing(spec)?,
        };
        self.remember_listing(raw, entries.clone());
        Ok(entries)
    }

    fn memo_listing(&self, raw: &str) -> Option<Vec<MarketEntry>> {
        let listings = self.listings.lock().ok()?;
        let (fetched_at, entries) = listings.get(raw.trim())?;
        (crate::time_now().saturating_sub(*fetched_at) < LISTING_MEMO_SECONDS)
            .then(|| entries.clone())
    }

    fn remember_listing(&self, raw: &str, entries: Vec<MarketEntry>) {
        let Ok(mut listings) = self.listings.lock() else {
            return;
        };
        listings.insert(raw.trim().to_owned(), (crate::time_now(), entries));
    }

    fn clawhub_listing(&self, spec: &Spec) -> Result<Vec<MarketEntry>, CoreError> {
        let url = match spec.query.as_deref() {
            Some(query) => format!(
                "{}/api/v1/search?q={}&limit={MAX_PAGE}",
                self.clawhub_base,
                encode(query)
            ),
            None => format!("{}/api/v1/skills?limit={MAX_PAGE}", self.clawhub_base),
        };
        let payload = self.json(&url)?;
        // 搜索接口把结果放在 `results`，列表接口放在 `items`；两种条目的字段名一样。
        let items = payload
            .get("results")
            .or_else(|| payload.get("items"))
            .and_then(Value::as_array)
            .ok_or_else(|| bad_response(&url, "响应里既没有 results 也没有 items"))?;
        let mut entries: Vec<MarketEntry> = Vec::new();
        for item in items.iter().take(MAX_PAGE) {
            if !clawhub_native(item) {
                continue;
            }
            let Some(slug) = item.get("slug").and_then(Value::as_str) else {
                continue;
            };
            // 市场返回什么我们控制不了：拿不稳的条目直接不列出来，而不是让它变成
            // 一个会在写盘时才炸的目录名。
            let Ok(slug) = validate_segment(slug) else {
                continue;
            };
            let owner = item
                .get("ownerHandle")
                .and_then(Value::as_str)
                .and_then(|owner| validate_segment(owner).ok());
            // **同名 slug 只留第一条**：ClawHub 的 slug 不唯一（搜「pdf」实测有四家都叫 pdf），
            // 而我们的目录名就是 slug，两个同名技能只能进一个目录。留下的那条连同它的归属者
            // 一起记下来，后面读文件时才不会拿另一家的归属者去问同一个 slug（那会 404）。
            if entries.iter().any(|entry| entry.slug == slug) {
                continue;
            }
            if let Some(owner) = owner.as_deref() {
                self.remember_owner(&slug, owner);
            }
            entries.push(MarketEntry {
                slug,
                owner,
                version: clawhub_version_field(item),
                updated_at: item.get("updatedAt").and_then(Value::as_i64),
            });
        }
        Ok(entries)
    }

    fn skillhub_listing(&self, spec: &Spec) -> Result<Vec<MarketEntry>, CoreError> {
        let mut url = format!("{}/api/skills?pageSize={MAX_PAGE}", self.skillhub_base);
        if let Some(query) = spec.query.as_deref() {
            url.push_str(&format!("&keyword={}", encode(query)));
        }
        let payload = self.json(&url)?;
        // `code` 是平台自己的错误码：0 才是正常返回，别的值正文里带 message。
        if payload.get("code").and_then(Value::as_i64) != Some(0) {
            let message = payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("平台没有给出原因");
            return Err(bad_response(&url, &format!("平台返回 code={message}")));
        }
        let items = payload
            .get("data")
            .and_then(|data| data.get("skills"))
            .and_then(Value::as_array)
            .ok_or_else(|| bad_response(&url, "响应里没有 data.skills"))?;
        let mut entries: Vec<MarketEntry> = Vec::new();
        for item in items.iter().take(MAX_PAGE) {
            let Some(slug) = item.get("slug").and_then(Value::as_str) else {
                continue;
            };
            let Ok(slug) = validate_segment(slug) else {
                continue;
            };
            if entries.iter().any(|entry| entry.slug == slug) {
                continue;
            }
            entries.push(MarketEntry {
                slug,
                owner: None,
                version: item
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .filter(|value| !value.is_empty()),
                updated_at: item.get("updated_at").and_then(Value::as_i64),
            });
        }
        Ok(entries)
    }

    /* ---------------------------------------------------------------- 详情 */

    /// ClawHub 的技能详情。返回（响应、请求的 URL）。
    ///
    /// `?owner=` 是必须的：同名 slug 属于多个归属者时，不带 owner 会拿到 409。
    fn clawhub_detail(&self, skill: &SkillRef) -> Result<(Value, String), CoreError> {
        let mut url = format!(
            "{}/api/v1/skills/{}",
            self.clawhub_base,
            encode(&skill.slug)
        );
        if let Some(owner) = skill.owner.as_deref() {
            url.push_str(&format!("?owner={}", encode(owner)));
        }
        let payload = self.json(&url)?;
        // 平台自己告诉我们这个 slug 属于谁时记下来：目录页里读同一个技能的文件时要用。
        if let Some(owner) = payload
            .get("owner")
            .and_then(|owner| owner.get("handle"))
            .and_then(Value::as_str)
        {
            if let Ok(owner) = validate_segment(owner) {
                self.remember_owner(&skill.slug, &owner);
            }
        }
        Ok((payload, url))
    }

    fn skillhub_detail(&self, slug: &str) -> Result<Value, CoreError> {
        self.json(&format!(
            "{}/api/v1/skills/{}",
            self.skillhub_base,
            encode(slug)
        ))
    }

    /// SkillHub 的文件清单（路径 + 大小 + sha256）。
    fn skillhub_files(&self, slug: &str) -> Result<Vec<MarketFile>, CoreError> {
        let url = format!(
            "{}/api/v1/skills/{}/files",
            self.skillhub_base,
            encode(slug)
        );
        let payload = self.json(&url)?;
        let items = payload
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| bad_response(&url, "响应里没有 files"))?;
        Ok(items
            .iter()
            .filter_map(|item| {
                let path = item.get("path").and_then(Value::as_str)?;
                // 不安全或不像文件的条目直接不列：装不上总比整台机器上到处写文件好，
                // 也比因为一个怪条目让整个技能装失败好。
                if !safe_file_path(path) {
                    return None;
                }
                Some(MarketFile {
                    path: path.to_owned(),
                    size: item.get("size").and_then(Value::as_u64).unwrap_or(0),
                    sha256: item
                        .get("sha256")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                })
            })
            .collect())
    }

    /// 读一个技能文件。SkillHub 的文件接口会 302 到对象存储（`get` 跟随）。
    fn skillhub_file(&self, slug: &str, relative: &str) -> Result<Vec<u8>, CoreError> {
        let url = format!(
            "{}/api/v1/skills/{}/file?path={}",
            self.skillhub_base,
            encode(slug),
            relative
                .split('/')
                .map(encode)
                .collect::<Vec<_>>()
                .join("/")
        );
        self.get(&url, MAX_FILE_BYTES)
    }

    /// 一份技能文件清单变成 `{slug}/` 前缀的 blob。
    ///
    /// 与仓库来源同一个形状：`assemble` 会把它当成一个名为 slug 的技能目录，
    /// 大小来自 files 接口——目录里因此能如实列出「还有哪些文件、各多大」，
    /// 不必等装的时候才发现。
    ///
    /// **技能里嵌套的 `SKILL.md` 不列**（实测有：`sales-pro` 里带着
    /// `sales-scripts/SKILL.md`、`negotiation/SKILL.md` 这样的子技能）：
    /// `assemble` 把「以 `SKILL.md` 结尾的路径」当成一个技能根，嵌套的那种会变成名叫
    /// `negotiation` 的假技能，挤占 `MAX_SKILLS_PER_REPO`（60）的名额，把真技能挤出去。
    /// 而且它本来就装不上——`assemble` 列同目录文件时会跳过所有以 `SKILL.md` 结尾的文件，
    /// 所以不列出来不会少装任何东西。
    fn skillhub_blobs(slug: &str, files: &[MarketFile]) -> Vec<RepoBlob> {
        files
            .iter()
            .filter(|file| !nested_skill_md(&file.path))
            .map(|file| RepoBlob {
                path: format!("{slug}/{}", file.path),
                size: file.size,
            })
            .collect()
    }

    /* ---------------------------------------------------------------- 版本标记 */

    /// 单个 SkillHub 技能的版本标记。**不是 git 提交**：平台给出的版本号；
    /// 版本号读不到时退一步用文件清单的 sha256 组合（内容变了标记就变，比「未知」有用）。
    fn skillhub_version(&self, skill: &SkillRef) -> Result<String, CoreError> {
        let payload = self.skillhub_detail(&skill.slug)?;
        if let Some(version) = payload
            .get("latestVersion")
            .and_then(|latest| latest.get("version"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(version.trim().to_owned());
        }
        let files = self.skillhub_files(&skill.slug)?;
        Ok(files_digest(&files))
    }

    /* ---------------------------------------------------------------- 缓存 */

    fn owner_of(&self, slug: &str) -> Option<String> {
        self.owners.lock().ok()?.get(slug).cloned()
    }

    /// 记下 slug 属于谁。**先到先得**：同一个 slug 有不同的归属者时保留第一条，
    /// 这样「目录里留下的那条」与「读文件时用的归属者」始终是同一个（见 `clawhub_listing`）。
    /// 带归属者的写法（`clawhub:{归属者}/{slug}`）不走这个表——那是以写法为准的。
    fn remember_owner(&self, slug: &str, owner: &str) {
        let Ok(mut owners) = self.owners.lock() else {
            return;
        };
        owners
            .entry(slug.to_owned())
            .or_insert_with(|| owner.to_owned());
    }

    fn cached_file(&self, key: &str) -> Option<Vec<u8>> {
        let files = self.files.lock().ok()?;
        let (fetched_at, bytes) = files.get(key)?;
        (crate::time_now().saturating_sub(*fetched_at) < FILE_CACHE_SECONDS).then(|| bytes.clone())
    }

    fn remember_file(&self, key: &str, bytes: &[u8]) {
        let Ok(mut files) = self.files.lock() else {
            return;
        };
        if files.len() >= MAX_CACHED_FILES && !files.contains_key(key) {
            // 满了先丢最旧的一份：缓存只省请求，丢哪一份都不影响正确性。
            let oldest = files
                .iter()
                .min_by_key(|(_, (fetched_at, _))| *fetched_at)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                files.remove(&oldest);
            }
        }
        files.insert(key.to_owned(), (crate::time_now(), bytes.to_vec()));
    }
}

impl RepoFetcher for RegistryFetcher {
    /// 版本标记。**不是 git 提交**，也不是「某个分支的 SHA」：
    ///
    /// - 单个技能：平台给出的版本号（ClawHub 的 `latestVersion.version`，SkillHub 的
    ///   `latestVersion.version`）。同一个技能内容变了，这个标记就一定变。
    /// - 目录页：目录不是版本化的对象（它是平台按自己规则排的一页结果），只能给
    ///   「抓下来时市场的样子」——这一页里最新一次更新的日期（`20260930` 这种）。
    ///   它**不参与更新比较**：`PluginService::check_updates` 会跳过市场来源，
    ///   拿页面级的时间戳去比会让所有已装技能都报「有更新」。
    fn resolve_commit(&self, repo: &str, git_ref: Option<&str>) -> Result<String, CoreError> {
        // 市场来源没有分支可以钉：目录与技能都由平台一侧决定。带 ref 的写法是写错了。
        if git_ref.is_some() {
            return Err(invalid_spec(repo));
        }
        let spec = Spec::parse(repo)?;
        let commit = match (&spec.skill, spec.source) {
            (Some(skill), RegistrySource::Clawhub) => {
                let (payload, url) = self.clawhub_detail(skill)?;
                let commit = clawhub_version_of(&payload, &url)?;
                // 详情响应里就带着 `SKILL.md` 全文，顺手放进缓存：assemble 紧接着要读它，
                // 再打一次就是白烧一次限额（目录页里每个技能都会走这条路径）。
                let text = skill_md_of(&payload, &url)?;
                self.remember_file(
                    &cache_key(repo, &commit, &format!("{}/SKILL.md", skill.slug)),
                    text.as_bytes(),
                );
                commit
            }
            (Some(skill), RegistrySource::Skillhub) => self.skillhub_version(skill)?,
            (None, source) => listing_marker(source, &self.listing(&spec, repo)?),
        };
        Ok(commit)
    }

    /// 列出这个来源里该出现哪些文件，**路径一律是 `{slug}/…`**。
    ///
    /// - ClawHub 只发 `SKILL.md`（技能卡片、`_meta.json` 是平台的元数据，不是技能的
    ///   文件，不装）。大小要读过详情才知道，而详情在下一阶段才读；目录里 `SKILL.md`
    ///   的大小取自正文本身，所以这里给 0 不影响任何显示。
    /// - SkillHub 按技能的 files 接口取它自己的文件：目录阶段就带上了真实大小，
    ///   `assemble` 据此把超过上限的附件挡在清单之外。
    fn list_blobs(&self, repo: &str, _commit: &str) -> Result<Vec<RepoBlob>, CoreError> {
        let spec = Spec::parse(repo)?;
        match (&spec.skill, spec.source) {
            (Some(skill), RegistrySource::Clawhub) => Ok(vec![skill_md_blob(&skill.slug)]),
            (Some(skill), RegistrySource::Skillhub) => Ok(Self::skillhub_blobs(
                &skill.slug,
                &self.skillhub_files(&skill.slug)?,
            )),
            (None, RegistrySource::Clawhub) => Ok(self
                .listing(&spec, repo)?
                .iter()
                .map(|entry| skill_md_blob(&entry.slug))
                .collect()),
            (None, RegistrySource::Skillhub) => {
                let entries = self.listing(&spec, repo)?;
                // 按技能逐个取文件清单（最多 30 个），并发跑：串行就是用户看到的「一直在读取」。
                let fetched = concurrent_map(entries.len(), |index| {
                    let slug = entries[index].slug.clone();
                    self.skillhub_files(&slug).map(|files| (slug, files))
                });
                let mut blobs = Vec::new();
                let mut last_error = None;
                for outcome in fetched {
                    match outcome {
                        Ok((slug, files)) => blobs.extend(Self::skillhub_blobs(&slug, &files)),
                        // 个别技能取不到清单（刚被下架、平台抽风）不该让整页打不开；
                        // 但要是全都没取到，那是市场那边的问题，用户需要知道。
                        Err(error) => last_error = Some(error),
                    }
                }
                if blobs.is_empty() {
                    if let Some(error) = last_error {
                        return Err(error);
                    }
                }
                Ok(blobs)
            }
        }
    }

    /// 读一个文件。`path` 的第一段是 slug，其余是技能目录里的相对路径。
    fn read_file(&self, repo: &str, commit: &str, path: &str) -> Result<Vec<u8>, CoreError> {
        let key = cache_key(repo, commit, path);
        if let Some(bytes) = self.cached_file(&key) {
            return Ok(bytes);
        }
        let spec = Spec::parse(repo)?;
        let (slug, relative) = split_blob_path(path)?;
        if let Some(skill) = &spec.skill {
            if skill.slug != slug {
                return Err(bad_path(path));
            }
        }
        let bytes = match spec.source {
            RegistrySource::Clawhub => {
                if !relative.eq_ignore_ascii_case("SKILL.md") {
                    return Err(bad_path(path));
                }
                let skill = SkillRef {
                    owner: self.owner_of(&slug),
                    slug,
                };
                let (payload, url) = self.clawhub_detail(&skill)?;
                skill_md_of(&payload, &url)?.as_bytes().to_vec()
            }
            RegistrySource::Skillhub => self.skillhub_file(&slug, &relative)?,
        };
        self.remember_file(&key, &bytes);
        Ok(bytes)
    }
}

/* ------------------------------------------------------------------ 工具函数 */

/// 一个 ClawHub 呈交给我们的 `SKILL.md` 正文（详情响应里的 `skill.description`）。
fn skill_md_of<'a>(payload: &'a Value, url: &str) -> Result<&'a str, CoreError> {
    payload
        .get("skill")
        .and_then(|skill| skill.get("description"))
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| bad_response(url, "详情里没有 skill.description（SKILL.md 正文）"))
}

/// ClawHub 的版本号：详情看 `latestVersion.version`，列表条目看 `version` 或 `tags.latest`。
/// 两者都没有就退回更新时间（日期粒度）——总比编一个看起来像提交号的东西强。
fn clawhub_version_of(payload: &Value, url: &str) -> Result<String, CoreError> {
    let explicit = payload
        .get("latestVersion")
        .and_then(|latest| latest.get("version"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| clawhub_version_field(payload));
    if let Some(version) = explicit.filter(|value| !value.trim().is_empty()) {
        return Ok(version.trim().to_owned());
    }
    payload
        .get("skill")
        .and_then(|skill| skill.get("updatedAt"))
        .and_then(Value::as_i64)
        .and_then(compact_date)
        .ok_or_else(|| bad_response(url, "详情里既没有版本号也没有更新时间"))
}

/// 技能目录里的相对路径是不是嵌套的 `SKILL.md`（`examples/demo/SKILL.md` 这种）。
///
/// 技能根那一份（`SKILL.md`，没有目录层）**不算**嵌套：它就是技能自己的文档。
fn nested_skill_md(relative: &str) -> bool {
    relative
        .rsplit_once('/')
        .map(|(_, file)| file.eq_ignore_ascii_case("SKILL.md"))
        .unwrap_or(false)
}

/// 这一条是不是 ClawHub 自己的技能。
///
/// 搜索接口会把别处镜像过来的技能混在一起（`install.kind = "skills-sh"`，指向 skills.sh，
/// 实测搜「pdf」就混着几条）：那些用 ClawHub 的详情接口取不到（`?owner=` 回 404），
/// 装也装不了——不列出来。列表接口的条目没有 `install` 字段，那些都是 ClawHub 自己的。
fn clawhub_native(item: &Value) -> bool {
    match item
        .get("install")
        .and_then(|install| install.get("kind"))
        .and_then(Value::as_str)
    {
        Some(kind) => kind == "clawhub",
        None => true,
    }
}

/// 列表条目里的版本号：`version`（搜索接口）或 `tags.latest`（列表接口）。
fn clawhub_version_field(item: &Value) -> Option<String> {
    item.get("version")
        .and_then(Value::as_str)
        .or_else(|| {
            item.get("tags")
                .and_then(|tags| tags.get("latest"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty())
}

/// 目录页的版本标记：这一页里最新一次更新的日期。
fn listing_marker(source: RegistrySource, entries: &[MarketEntry]) -> String {
    let newest = entries.iter().filter_map(|entry| entry.updated_at).max();
    match newest.and_then(compact_date) {
        Some(stamp) => stamp,
        None => source.id().to_owned(),
    }
}

/// Unix 毫秒 → `20260930`。读不出来就 `None`（不猜时间）。
fn compact_date(millis: i64) -> Option<String> {
    let seconds = millis.checked_div(1000)?;
    let stamp = time::OffsetDateTime::from_unix_timestamp(seconds)
        .ok()?
        .format(&time::format_description::well_known::Rfc3339)
        .ok()?;
    let day: String = stamp.chars().take(10).collect();
    Some(day.replace('-', ""))
}

/// 文件清单的 sha256 组合：内容变了标记就变。
fn files_digest(files: &[MarketFile]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update(b"\0");
        hasher.update(file.sha256.as_deref().unwrap_or_default().as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", &format!("{:x}", hasher.finalize())[..12])
}

/// 拆开 `{slug}/{技能目录里的相对路径}`。
fn split_blob_path(path: &str) -> Result<(String, String), CoreError> {
    let (slug, relative) = path.split_once('/').ok_or_else(|| bad_path(path))?;
    if validate_segment(slug).is_err() || !safe_file_path(relative) {
        return Err(bad_path(path));
    }
    Ok((slug.to_owned(), relative.to_owned()))
}

fn skill_md_blob(slug: &str) -> RepoBlob {
    RepoBlob {
        path: format!("{slug}/SKILL.md"),
        size: 0,
    }
}

/// 缓存键。版本标记也在里面：同一个路径在另一版上是另一份内容。
fn cache_key(repo: &str, commit: &str, path: &str) -> String {
    format!("{}@{commit}:{path}", repo.trim())
}

/// 一段（归属者 / slug）必须是安全的路径片段。
///
/// slug 会变成安装目录名（`{slug}/SKILL.md` 的第一段），所以这里按路径片段的标准卡：
/// 市场返回什么我们控制不了，卡不住就得靠写盘那一层兜着，而那一层报出来的错误用户看不懂。
fn validate_segment(segment: &str) -> Result<String, CoreError> {
    let valid = !segment.is_empty()
        && segment.chars().count() <= MAX_SEGMENT_LEN
        && segment != "."
        && segment != ".."
        && !segment.starts_with('.')
        && segment
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character));
    if !valid {
        return Err(
            CoreError::new(ErrorCode::ValidationFailed, "error.pluginRepoInvalid")
                .with_detail(format!("技能标识里有不支持的字符：{segment}")),
        );
    }
    Ok(segment.to_owned())
}

/// 技能目录里的相对路径能不能安全地拼进目标目录。
fn safe_file_path(path: &str) -> bool {
    !path.is_empty()
        && path.chars().count() <= MAX_SEGMENT_LEN * 3
        && !path.ends_with('/')
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(|character| character.is_control())
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment.chars().count() <= MAX_SEGMENT_LEN
                && segment != "."
                && segment != ".."
        })
}

/// 并发跑一批读取任务，最多 [`LIST_CONCURRENCY`] 个线程。
///
/// 与 `source::read_many` 同一个理由：一个目录页最多 30 个技能，每个都要取文件清单，
/// 串行就是几十个来回——页面看起来就是卡住了。
fn concurrent_map<T: Send>(count: usize, task: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let workers = count.clamp(1, LIST_CONCURRENCY);
    if workers == 1 {
        return (0..count).map(task).collect();
    }
    let mut slots: Vec<Option<T>> = Vec::new();
    slots.resize_with(count, || None);
    let slots = std::sync::Mutex::new(slots);
    std::thread::scope(|scope| {
        let task = &task;
        for worker in 0..workers {
            let slots = &slots;
            scope.spawn(move || {
                let mut index = worker;
                while index < count {
                    let value = task(index);
                    slots.lock().expect("并发结果锁")[index] = Some(value);
                    index += workers;
                }
            });
        }
    });
    slots
        .into_inner()
        .expect("并发结果锁")
        .into_iter()
        .flatten()
        .collect()
}

/// 来源写法不对。
fn invalid_spec(raw: &str) -> CoreError {
    CoreError::new(ErrorCode::ValidationFailed, "error.pluginRepoInvalid").with_detail(format!(
        "市场来源要写成 clawhub、clawhub?q=关键词、clawhub:{{归属者}}/{{技能}}、skillhub 或 skillhub:{{技能}}，收到的是：{raw}"
    ))
}

/// 请求的文件不在我们列出来的清单里。
fn bad_path(path: &str) -> CoreError {
    CoreError::new(
        ErrorCode::ValidationFailed,
        "error.pluginRegistrySkillMissing",
    )
    .with_detail(format!("市场里没有这个文件：{path}"))
}

/// 市场的响应不是我们能读的东西（不是 JSON、缺关键字段、平台自己的错误码）。
fn bad_response(url: &str, why: &str) -> CoreError {
    CoreError::new(ErrorCode::Internal, "error.pluginRegistryBadResponse")
        .with_detail(format!("{url}：{why}"))
}

/// 被市场限流时的可行动说明。
fn rate_limited_detail(label: &str, retry_after: Option<&str>) -> String {
    let mut detail = format!("{label} 暂时限制了本机的访问频率");
    match retry_after.and_then(|value| value.trim().parse::<u64>().ok()) {
        Some(seconds) => detail.push_str(&format!("，上游要求大约 {seconds} 秒后重试")),
        None => detail.push_str("，稍后再试"),
    }
    detail
}

/// SkillHub 文件清单里的一条。
#[derive(Debug, Clone, PartialEq, Eq)]
struct MarketFile {
    path: String,
    size: u64,
    sha256: Option<String>,
}

/* ------------------------------------------------------------------ 测试 */

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::{Ipv4Addr, TcpListener},
        sync::{Arc, Mutex as StdMutex},
    };

    use crate::plugins::source::assemble;

    use super::*;

    /// 一次合成响应：状态码、附加响应头、正文。
    type Reply = (u16, Vec<(&'static str, String)>, String);

    /// 可编排的合成上游：按请求路径（含查询串）给响应，并记下每一个被请求的路径。
    ///
    /// 用真的 socket（写法照 `diagnostics::probe` 与 `source` 的用例）：这里要验证的正好是
    /// **请求打到哪个端点、打了几次**——「第二次读同一个文件不再发请求」只有数真实请求
    /// 才看得出来。
    struct Mock {
        endpoint: String,
        seen: Arc<StdMutex<Vec<String>>>,
    }

    impl Mock {
        fn start(handler: impl Fn(&str) -> Reply + Send + 'static) -> Self {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = Arc::new(StdMutex::new(Vec::new()));
            let sink = seen.clone();
            std::thread::spawn(move || {
                // 逐条处理：每条连接都会读到完整请求并写回完整响应（响应里带
                // `connection: close`），并发的客户端只是排队，不会互相卡住。
                for incoming in listener.incoming() {
                    let Ok(stream) = incoming else { break };
                    let mut stream = stream;
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut path = String::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        let lower = line.to_ascii_lowercase();
                        if lower.starts_with("get ") || lower.starts_with("post ") {
                            path = line
                                .split_whitespace()
                                .nth(1)
                                .unwrap_or_default()
                                .to_owned();
                        }
                    }
                    sink.lock().unwrap().push(path.clone());
                    let (status, headers, body) = handler(&path);
                    let extra: String = headers
                        .into_iter()
                        .map(|(name, value)| format!("{name}: {value}\r\n"))
                        .collect();
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    );
                    let _ = stream.flush();
                }
            });
            Self {
                endpoint: format!("http://127.0.0.1:{port}"),
                seen,
            }
        }

        fn paths(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }

        fn hits(&self, needle: &str) -> usize {
            self.paths()
                .iter()
                .filter(|path| path.contains(needle))
                .count()
        }
    }

    /// 指向合成上游的 ClawHub 抓取器。SkillHub 的入口故意用 RFC 5737 的文档地址：
    /// 用例要是误打到那边，会连不上并如实报错，而不是悄悄通过。
    fn clawhub_fetcher(endpoint: &str) -> RegistryFetcher {
        RegistryFetcher::with_bases(
            "test-agent".to_owned(),
            endpoint.to_owned(),
            "http://198.51.100.7".to_owned(),
        )
    }

    /// 指向合成上游的 SkillHub 抓取器。
    fn skillhub_fetcher(endpoint: &str) -> RegistryFetcher {
        RegistryFetcher::with_bases(
            "test-agent".to_owned(),
            "http://198.51.100.8".to_owned(),
            endpoint.to_owned(),
        )
    }

    /// 把一段文本编成 JSON 字符串（详情响应里 `description` 就是 SKILL.md 全文）。
    fn json_text(text: &str) -> String {
        serde_json::to_string(text).unwrap()
    }

    fn clawhub_detail_body(slug: &str, owner: &str, version: &str, markdown: &str) -> String {
        format!(
            r#"{{"owner":{{"handle":"{owner}"}},"skill":{{"slug":"{slug}","displayName":"{slug}","description":{}}},"latestVersion":{{"version":"{version}"}}}}"#,
            json_text(markdown)
        )
    }

    const CLAWHUB_SEARCH: &str = r#"{"results":[
      {"ownerHandle":"awspace","slug":"pdf","displayName":"Pdf","summary":"PDF 工具箱","version":"0.1.0","updatedAt":1789594554485},
      {"ownerHandle":"momansouri83","slug":"pdf-forms","displayName":"Pdf Forms","summary":"表单填写","version":"2.0.0","updatedAt":1789000000000}
    ]}"#;

    fn skill_markdown(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n正文：{name}\n")
    }

    /// 搜索返回两条 → 目录里两个技能、名字与描述来自响应，`homepage` 是回链地址。
    #[test]
    fn clawhub_search_lists_the_skills_from_the_answer() {
        let first = skill_markdown("pdf", "PDF 工具箱：提取与合并");
        let second = skill_markdown("pdf-forms", "表单填写");
        let (first_body, second_body) = (first.clone(), second.clone());
        let mock = Mock::start(move |path| {
            if path.starts_with("/api/v1/search?q=pdf") {
                (200, Vec::new(), CLAWHUB_SEARCH.to_owned())
            } else if path.starts_with("/api/v1/skills/pdf-forms") {
                (
                    200,
                    Vec::new(),
                    clawhub_detail_body("pdf-forms", "momansouri83", "2.0.0", &second_body),
                )
            } else if path.starts_with("/api/v1/skills/pdf") {
                (
                    200,
                    Vec::new(),
                    clawhub_detail_body("pdf", "awspace", "0.1.0", &first_body),
                )
            } else {
                (404, Vec::new(), "{}".to_owned())
            }
        });
        let fetcher = clawhub_fetcher(&mock.endpoint);
        let spec = "clawhub?q=pdf";

        let commit = fetcher.resolve_commit(spec, None).unwrap();
        let blobs = fetcher.list_blobs(spec, &commit).unwrap();
        assert_eq!(
            blobs
                .iter()
                .map(|blob| blob.path.as_str())
                .collect::<Vec<_>>(),
            vec!["pdf/SKILL.md", "pdf-forms/SKILL.md"],
            "路径形状要让 assemble 认出「名为 slug 的技能目录」"
        );

        let catalog = assemble(&fetcher, spec, &commit, &blobs, 7).unwrap();
        assert_eq!(catalog.skills.len(), 2);
        // 目录按路径排序（`pdf-forms/` 在 `pdf/` 前面），按名字取用哪个技能。
        let pdf = catalog
            .skills
            .iter()
            .find(|skill| skill.dir_name == "pdf")
            .expect("pdf 要在目录里");
        assert_eq!(pdf.document.id, "pdf");
        assert_eq!(
            pdf.document.description.as_deref(),
            Some("PDF 工具箱：提取与合并")
        );
        assert_eq!(
            pdf.files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["SKILL.md"],
            "只装技能自己的文件：平台的元数据（skill-card.md、_meta.json）不进清单"
        );
        assert_eq!(
            catalog
                .skills
                .iter()
                .find(|skill| skill.dir_name == "pdf-forms")
                .map(|skill| skill.document.description.as_deref()),
            Some(Some("表单填写"))
        );
        assert_eq!(
            homepage_of(spec).unwrap(),
            "https://clawhub.ai/skills?q=pdf",
            "目录页给的是平台在该关键词下的页"
        );
        assert_eq!(
            homepage_of("clawhub:awspace/pdf").unwrap(),
            "https://clawhub.ai/awspace/skills/pdf",
            "单个技能给的是技能页——ClawHub 的第三方目录条款要求回链到这里"
        );
    }

    /// 同一个文件读两次只打一次详情接口：预览与安装是同一次浏览里的两个动作。
    #[test]
    fn reading_the_same_file_twice_asks_the_market_once() {
        let body = clawhub_detail_body("pdf", "awspace", "0.1.0", &skill_markdown("pdf", "工具箱"));
        let mock = Mock::start(move |path| {
            if path.starts_with("/api/v1/skills/pdf") {
                (200, Vec::new(), body.clone())
            } else {
                (404, Vec::new(), "{}".to_owned())
            }
        });
        let fetcher = clawhub_fetcher(&mock.endpoint);

        let first = fetcher
            .read_file("clawhub:awspace/pdf", "2", "pdf/SKILL.md")
            .unwrap();
        assert!(String::from_utf8_lossy(&first).contains("工具箱"));
        let after_first = mock.hits("/api/v1/skills/pdf");
        assert_eq!(after_first, 1, "第一次读要真的打详情接口");

        let second = fetcher
            .read_file("clawhub:awspace/pdf", "2", "pdf/SKILL.md")
            .unwrap();
        assert_eq!(second, first);
        assert_eq!(
            mock.hits("/api/v1/skills/pdf"),
            after_first,
            "第二次读同一个文件不该再发请求"
        );
    }

    /// 目录页里读技能正文：路径上没有归属者，靠列表响应里记下的归属者补 `?owner=`。
    #[test]
    fn a_listed_skill_is_read_with_its_owner() {
        let body = clawhub_detail_body("pdf", "awspace", "0.1.0", &skill_markdown("pdf", "工具箱"));
        let mock = Mock::start(move |path| {
            if path.starts_with("/api/v1/search") {
                (200, Vec::new(), CLAWHUB_SEARCH.to_owned())
            } else if path.starts_with("/api/v1/skills/pdf?owner=awspace") {
                (200, Vec::new(), body.clone())
            } else {
                (404, Vec::new(), "{}".to_owned())
            }
        });
        let fetcher = clawhub_fetcher(&mock.endpoint);

        let commit = fetcher.resolve_commit("clawhub?q=pdf", None).unwrap();
        fetcher.list_blobs("clawhub?q=pdf", &commit).unwrap();
        assert!(fetcher
            .read_file("clawhub?q=pdf", &commit, "pdf/SKILL.md")
            .is_ok());
        assert_eq!(
            mock.hits("owner=awspace"),
            1,
            "同名 slug 会 409，所以必须带上列表里记下的归属者"
        );
    }

    /// SkillHub：文件清单进目录时带 `{slug}/` 前缀，大小来自 files 接口。
    #[test]
    fn skillhub_prefixes_every_file_of_a_skill_with_its_slug() {
        let mock = Mock::start(|path| {
            if path.starts_with("/api/skills?pageSize=") {
                (
                    200,
                    Vec::new(),
                    r#"{"code":0,"data":{"total":2,"skills":[
                      {"slug":"contract-review","name":"合同审查","version":"1.0.0","updated_at":1790760472563},
                      {"slug":"pdf-tools","name":"PDF 工具","version":"2.1.0","updated_at":1790000000000}
                    ]}}"#
                        .to_owned(),
                )
            } else if path.ends_with("/files") {
                (
                    200,
                    Vec::new(),
                    r#"{"count":5,"files":[
                      {"path":"SKILL.md","sha256":"aa","size":25279},
                      {"path":"assets/style.css","sha256":"bb","size":15491},
                      {"path":"scripts/run.py","sha256":"cc","size":7035},
                      {"path":"examples/demo/SKILL.md","sha256":"ee","size":900},
                      {"path":"../escape.md","sha256":"dd","size":10}
                    ]}"#
                    .to_owned(),
                )
            } else if path.contains("/file?path=") {
                (200, Vec::new(), "---\nname: body\n---\n正文".to_owned())
            } else {
                (404, Vec::new(), "{}".to_owned())
            }
        });
        let fetcher = skillhub_fetcher(&mock.endpoint);

        let commit = fetcher.resolve_commit("skillhub", None).unwrap();
        let blobs = fetcher.list_blobs("skillhub", &commit).unwrap();
        let paths: Vec<&str> = blobs.iter().map(|blob| blob.path.as_str()).collect();
        assert!(paths.contains(&"contract-review/SKILL.md"));
        assert!(paths.contains(&"contract-review/assets/style.css"));
        assert!(paths.contains(&"pdf-tools/scripts/run.py"));
        assert!(
            !paths.iter().any(|path| path.contains("escape")),
            "越界的路径不进清单：装不上好过在别处写文件"
        );
        assert!(
            !paths.iter().any(|path| path.contains("examples/demo")),
            "嵌套的 SKILL.md 不列：它会变成一个名叫 demo 的假技能，挤掉真技能的名额"
        );
        let style = blobs
            .iter()
            .find(|blob| blob.path == "contract-review/assets/style.css")
            .unwrap();
        assert_eq!(
            style.size, 15491,
            "大小来自 files 接口，不是读正文之后才知道"
        );
        assert_eq!(
            homepage_of("skillhub:contract-review").unwrap(),
            "https://skillhub.cn/skills/contract-review"
        );

        // 两个技能各自的文件清单合成一份目录：技能数是 2，不是「2 个技能 + 2 个假技能」。
        let catalog = assemble(&fetcher, "skillhub", &commit, &blobs, 0).unwrap();
        assert_eq!(catalog.skills.len(), 2);
        assert_eq!(
            catalog
                .skills
                .iter()
                .filter(|skill| skill.dir_name == "demo")
                .count(),
            0,
            "嵌套的 SKILL.md 不该变成一个名叫 demo 的技能"
        );
    }

    /// 单文件接口会 302 到对象存储：不跟随就只会拿到一行 `Found`。
    #[test]
    fn skillhub_follows_the_redirect_to_the_object_storage() {
        let mock = Mock::start(|path| {
            if path.starts_with("/api/v1/skills/pdf-tools/file?path=") {
                (
                    302,
                    vec![("location", "/blob/pdf-tools/SKILL.md".to_owned())],
                    String::new(),
                )
            } else if path == "/blob/pdf-tools/SKILL.md" {
                (200, Vec::new(), "# PDF 工具\n正文".to_owned())
            } else {
                (404, Vec::new(), "{}".to_owned())
            }
        });
        let fetcher = skillhub_fetcher(&mock.endpoint);

        let bytes = fetcher
            .read_file("skillhub", "20260930", "pdf-tools/SKILL.md")
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&bytes), "# PDF 工具\n正文");
        let seen = mock.paths();
        assert!(
            seen.iter().any(|path| path.contains("/file?path=")),
            "先打单文件接口：{seen:?}"
        );
        assert!(
            seen.iter().any(|path| path == "/blob/pdf-tools/SKILL.md"),
            "必须跟随 302 到对象存储：{seen:?}"
        );
    }

    /// 目录页的列表只打一次：`resolve_commit` 与 `list_blobs` 是同一次浏览的两步。
    #[test]
    fn one_browse_asks_for_the_listing_once() {
        let mock = Mock::start(|path| {
            if path.starts_with("/api/skills?pageSize=") {
                (
                    200,
                    Vec::new(),
                    r#"{"code":0,"data":{"total":1,"skills":[{"slug":"solo","version":"1.0.0","updated_at":1790760472563}]}}"#
                        .to_owned(),
                )
            } else if path.ends_with("/files") {
                (
                    200,
                    Vec::new(),
                    r#"{"count":1,"files":[{"path":"SKILL.md","sha256":"aa","size":12}]}"#
                        .to_owned(),
                )
            } else {
                (404, Vec::new(), "{}".to_owned())
            }
        });
        let fetcher = skillhub_fetcher(&mock.endpoint);

        let commit = fetcher.resolve_commit("skillhub", None).unwrap();
        assert_eq!(commit, "20260930", "目录页的标记是这一页最新的更新日期");
        let blobs = fetcher.list_blobs("skillhub", &commit).unwrap();
        assert_eq!(blobs.len(), 1);
        assert_eq!(
            mock.hits("/api/skills?pageSize="),
            1,
            "两步共用同一份列表，不该各打一次"
        );
    }

    /// 市场上没有这个技能时给的是「找不到」，不是「仓库不存在」。
    #[test]
    fn a_missing_skill_is_reported_as_missing() {
        let mock = Mock::start(|_| (404, Vec::new(), r#"{"error":"not found"}"#.to_owned()));
        let fetcher = clawhub_fetcher(&mock.endpoint);

        let error = fetcher
            .read_file("clawhub:awspace/gone", "0", "gone/SKILL.md")
            .unwrap_err();
        assert_eq!(error.message_key, "error.pluginRegistrySkillMissing");
        assert!(
            error.safe_details[0].contains("404"),
            "{:?}",
            error.safe_details
        );
    }

    /// 响应不是 JSON（网关维护页、限流页）时说清楚是哪一步读不懂。
    #[test]
    fn a_non_json_answer_is_reported_with_the_url() {
        let mock = Mock::start(|_| (200, Vec::new(), "<html>维护中</html>".to_owned()));
        let fetcher = clawhub_fetcher(&mock.endpoint);

        let error = fetcher.resolve_commit("clawhub", None).unwrap_err();
        assert_eq!(error.message_key, "error.pluginRegistryBadResponse");
        assert!(
            error.safe_details[0].contains("不是合法 JSON"),
            "{:?}",
            error.safe_details
        );
    }

    /// 被限流时说过「谁限的、上游要求等多久」。
    #[test]
    fn a_rate_limited_market_says_when_to_retry() {
        let mock = Mock::start(|_| (429, vec![("retry-after", "90".to_owned())], String::new()));
        let fetcher = clawhub_fetcher(&mock.endpoint);

        let error = fetcher.resolve_commit("clawhub", None).unwrap_err();
        assert_eq!(error.message_key, "error.pluginRegistryRateLimited");
        let detail = &error.safe_details[0];
        assert!(detail.contains("ClawHub"), "{detail}");
        assert!(detail.contains("90 秒"), "{detail}");
    }

    /// 搜索词编在来源写法里带走；装的时候界面只交回 repo，词不能丢。
    #[test]
    fn the_query_travels_inside_the_source_spec() {
        assert_eq!(compose_spec("clawhub", Some(" pdf ")), "clawhub?q=pdf");
        assert_eq!(
            compose_spec("clawhub?q=pdf", None),
            "clawhub?q=pdf",
            "装技能那一趟没有搜索词，不能把已有的词清掉"
        );
        assert_eq!(
            compose_spec("clawhub?q=pdf", Some("")),
            "clawhub",
            "清空搜索框就回到默认目录"
        );
        assert_eq!(
            compose_spec("skillhub:pdf", Some("表格")),
            "skillhub:pdf",
            "单个技能的写法上不挂搜索词"
        );
        assert_eq!(compose_spec("owner/repo", Some("pdf")), "owner/repo");
    }

    /// 不安全的写法与 slug 在任何网络动作之前就被拒。
    ///
    /// 基地址故意用 RFC 5737 的文档地址：万一这里真的发起请求，用例会卡在连不上，
    /// 而不是悄悄通过——「先校验再联网」这件事只有这么写才测得准。
    #[test]
    fn an_unsafe_spec_is_refused_before_any_network() {
        let fetcher = RegistryFetcher::with_bases(
            "test-agent".to_owned(),
            "http://198.51.100.7".to_owned(),
            "http://198.51.100.8".to_owned(),
        );
        for bad in [
            "clawhub:../etc",
            "clawhub:owner/..",
            "clawhub:owner/.hidden",
            "clawhub:owner/sla sh",
            "clawhub:owner/",
            "clawhub:",
            "clawhub?q",
            "clawhub?keyword=pdf",
            "skillhub:owner/slug",
            "owner/clawhub",
            "github",
        ] {
            let error = fetcher.resolve_commit(bad, None).unwrap_err();
            assert_eq!(error.message_key, "error.pluginRepoInvalid", "{bad}");
        }
        // 市场来源没有分支可以钉。
        assert!(fetcher.resolve_commit("clawhub", Some("main")).is_err());
        assert!(registry_source("clawhubx").is_none());
        assert_eq!(
            registry_source(" clawhub:awspace/pdf "),
            Some(RegistrySource::Clawhub)
        );
        assert_eq!(registry_source("skillhub"), Some(RegistrySource::Skillhub));
        assert!(registry_source("owner/repo").is_none());
    }

    /// 目录页的标记是日期，单个技能的是平台版本号——都不是 git 提交。
    #[test]
    fn version_markers_are_platform_versions_not_git_shas() {
        let body = clawhub_detail_body("pdf", "awspace", "0.1.0", &skill_markdown("pdf", "工具箱"));
        let mock = Mock::start(move |_| (200, Vec::new(), body.clone()));
        let fetcher = clawhub_fetcher(&mock.endpoint);
        assert_eq!(
            fetcher.resolve_commit("clawhub:awspace/pdf", None).unwrap(),
            "0.1.0"
        );
        assert_eq!(compact_date(1_789_594_554_485).as_deref(), Some("20260916"));
        assert!(compact_date(i64::MAX).is_none(), "读不出时间就不猜");
    }
}
