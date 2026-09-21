//! 扩展板块的元数据存储：工具探测缓存、已装技能归属、订阅源与资讯条目。
//!
//! 与实体仓库分开成独立的 trait 和独立的连接，理由和 `OperationStore` 一样：
//! 这几张表服务于三个互不相干的板块，塞进 `Repository` 会让那个已经很克制的
//! 实体接口长出一堆与供应商/模型无关的方法。
//!
//! 每张表都存 JSON 载荷 + 少量索引列：索引列负责排序、过滤与唯一性，
//! 载荷负责领域字段。这样加字段不用改表结构，也不会出现两份真相。

use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;

use crate::{
    content::{FeedItem, FeedSource, ParsedItem},
    domain::error::CoreError,
    plugins::SkillRecord,
    toolhub::detect::ToolState,
};

use super::sqlite::{db_error, decode, encode};

/// v4 新增的四张表。加表只能往后追加，不改历史步骤。
pub const HUB_SCHEMA: &str = "
CREATE TABLE tool_probe_cache (
    tool_id TEXT PRIMARY KEY NOT NULL,
    probed_at INTEGER NOT NULL,
    payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE TABLE installed_skills (
    skill_id TEXT NOT NULL,
    target_tool TEXT NOT NULL,
    installed_at INTEGER NOT NULL,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    PRIMARY KEY (skill_id, target_tool)
);
CREATE TABLE feed_sources (
    id TEXT PRIMARY KEY NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    next_fetch_at INTEGER NOT NULL DEFAULT 0,
    payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE INDEX feed_sources_due ON feed_sources(enabled, next_fetch_at);
CREATE TABLE feed_items (
    url TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL,
    published_at INTEGER NOT NULL,
    first_seen_at INTEGER NOT NULL,
    lang TEXT NOT NULL DEFAULT '',
    payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE INDEX feed_items_time ON feed_items(published_at DESC);
CREATE INDEX feed_items_source ON feed_items(source_id, published_at DESC);
";

/// 三个板块共用的存储接口。
pub trait HubStore: Send + Sync {
    // ---- 工具探测缓存 ----
    fn cached_tool_states(&self) -> Result<Vec<ToolState>, CoreError>;
    fn save_tool_state(&self, state: &ToolState) -> Result<(), CoreError>;

    // ---- 已安装技能 ----
    fn list_skills(&self) -> Result<Vec<SkillRecord>, CoreError>;
    fn save_skill(&self, record: &SkillRecord) -> Result<(), CoreError>;
    fn delete_skill(&self, skill_id: &str, target_tool: &str) -> Result<(), CoreError>;

    // ---- 订阅源 ----
    fn list_feed_sources(&self) -> Result<Vec<FeedSource>, CoreError>;
    fn save_feed_source(&self, source: &FeedSource) -> Result<(), CoreError>;
    fn delete_feed_source(&self, source_id: &str) -> Result<(), CoreError>;

    // ---- 资讯条目 ----
    fn list_feed_items(
        &self,
        source_id: Option<&str>,
        lang: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<FeedItem>, CoreError>;
    /// 写入条目，返回**新增**条数（已存在的按最新内容刷新，不计入新增）。
    fn insert_feed_items(
        &self,
        source_id: &str,
        lang: &str,
        items: &[ParsedItem],
        now: i64,
    ) -> Result<usize, CoreError>;
    /// 删除某个源里不在给定 URL 集合中的条目，返回删除条数。
    fn delete_source_items_not_in(
        &self,
        source_id: &str,
        keep_urls: &[String],
    ) -> Result<usize, CoreError>;
    /// 只保留某个源最新的 `keep` 条（按发布时间倒序），返回删除条数。
    fn trim_source_items(&self, source_id: &str, keep: usize) -> Result<usize, CoreError>;
    /// 用这次抓到的条目**替换**某个源的现存条目，返回新增条数。
    ///
    /// 「替换」而不是「累加」是产品要求（用户原话：「尽量不要存数据，更新时把新的覆盖掉旧的」）：
    /// 本地只留这次抓到的东西，第二天不会翻到前天那条。
    ///
    /// 两个边界写在这里而不是各实现里：
    /// - **条目为空时什么都不做**。订阅源偶发返回 200 + 空正文是可能的，若照常覆盖就把上一份
    ///   快照清空了；这与「失败时保留旧内容」是同一条原则。
    /// - 顺序是「先写入 → 再删这次没见到的 → 最后按条数收口」：先写入才能算出「新增 N 条」，
    ///   而后两步只做减法，跑几遍结果都一样（幂等）。
    fn sync_source_items(
        &self,
        source_id: &str,
        lang: &str,
        items: &[ParsedItem],
        now: i64,
        keep: usize,
    ) -> Result<usize, CoreError> {
        if items.is_empty() {
            return Ok(0);
        }
        let added = self.insert_feed_items(source_id, lang, items, now)?;
        let keep_urls: Vec<String> = items.iter().map(|item| item.url.clone()).collect();
        self.delete_source_items_not_in(source_id, &keep_urls)?;
        self.trim_source_items(source_id, keep)?;
        Ok(added)
    }

    fn count_feed_items(&self) -> Result<usize, CoreError>;
    /// 按天数与总条数裁剪，返回删除条数。
    fn prune_feed_items(
        &self,
        now: i64,
        retention_days: i64,
        keep: usize,
    ) -> Result<usize, CoreError>;
}

// ---------------------------------------------------------------- SQLite

pub struct SqliteHubStore {
    connection: Mutex<Connection>,
}

impl SqliteHubStore {
    /// 与实体仓库、操作记录共用同一个数据库文件，各自持有连接。
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        let mut connection = Connection::open(path).map_err(db_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let current = tx
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .map_err(db_error)?;
        let version = super::migration::run_migrations(current, |step| match step.to {
            1 => tx
                .execute_batch(super::sqlite::INITIAL_SCHEMA)
                .map_err(db_error),
            2 => super::sqlite::migrate_display_name_prefixes(&tx),
            3 => tx
                .execute_batch(super::sqlite::SETTINGS_SCHEMA)
                .map_err(db_error),
            4 => tx.execute_batch(HUB_SCHEMA).map_err(db_error),
            _ => Err(CoreError::internal("未知的数据库升级步骤")),
        })?;
        tx.pragma_update(None, "user_version", version)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn in_memory() -> Result<Self, CoreError> {
        let connection = Connection::open_in_memory().map_err(db_error)?;
        connection
            .execute_batch(super::sqlite::INITIAL_SCHEMA)
            .map_err(db_error)?;
        connection.execute_batch(HUB_SCHEMA).map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, CoreError> {
        self.connection
            .lock()
            .map_err(|_| CoreError::internal("数据库锁不可用"))
    }
}

/// 列表读取：只关心载荷的查询走这一条。
fn load_all<T: DeserializeOwned>(connection: &Connection, sql: &str) -> Result<Vec<T>, CoreError> {
    let mut statement = connection.prepare(sql).map_err(db_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(decode(&row.map_err(db_error)?)?);
    }
    Ok(out)
}

fn count(connection: &Connection, sql: &str) -> Result<usize, CoreError> {
    let value: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .map_err(db_error)?;
    Ok(value.max(0) as usize)
}

impl HubStore for SqliteHubStore {
    fn cached_tool_states(&self) -> Result<Vec<ToolState>, CoreError> {
        let connection = self.lock()?;
        load_all(
            &connection,
            "SELECT payload FROM tool_probe_cache ORDER BY tool_id",
        )
    }

    fn save_tool_state(&self, state: &ToolState) -> Result<(), CoreError> {
        self.lock()?
            .execute(
                "INSERT INTO tool_probe_cache(tool_id,probed_at,payload) VALUES (?1,?2,?3)
                 ON CONFLICT(tool_id) DO UPDATE SET probed_at=excluded.probed_at, payload=excluded.payload",
                params![state.id, state.probed_at, encode(state)?],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn list_skills(&self) -> Result<Vec<SkillRecord>, CoreError> {
        let connection = self.lock()?;
        load_all(
            &connection,
            "SELECT payload FROM installed_skills ORDER BY skill_id, target_tool",
        )
    }

    fn save_skill(&self, record: &SkillRecord) -> Result<(), CoreError> {
        self.lock()?
            .execute(
                "INSERT INTO installed_skills(skill_id,target_tool,installed_at,payload) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(skill_id,target_tool) DO UPDATE SET installed_at=excluded.installed_at, payload=excluded.payload",
                params![
                    record.skill_id,
                    record.target_tool,
                    record.installed_at,
                    encode(record)?
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn delete_skill(&self, skill_id: &str, target_tool: &str) -> Result<(), CoreError> {
        self.lock()?
            .execute(
                "DELETE FROM installed_skills WHERE skill_id=?1 AND target_tool=?2",
                params![skill_id, target_tool],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn list_feed_sources(&self) -> Result<Vec<FeedSource>, CoreError> {
        let connection = self.lock()?;
        load_all(&connection, "SELECT payload FROM feed_sources ORDER BY id")
    }

    fn save_feed_source(&self, source: &FeedSource) -> Result<(), CoreError> {
        self.lock()?
            .execute(
                "INSERT INTO feed_sources(id,enabled,next_fetch_at,payload) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled, next_fetch_at=excluded.next_fetch_at, payload=excluded.payload",
                params![
                    source.id,
                    i64::from(source.enabled),
                    source.next_fetch_at,
                    encode(source)?
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn delete_feed_source(&self, source_id: &str) -> Result<(), CoreError> {
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        tx.execute("DELETE FROM feed_sources WHERE id=?1", [source_id])
            .map_err(db_error)?;
        // 源没了，它的条目也不该留在库里变成孤儿。
        tx.execute("DELETE FROM feed_items WHERE source_id=?1", [source_id])
            .map_err(db_error)?;
        tx.commit().map_err(db_error)
    }

    fn list_feed_items(
        &self,
        source_id: Option<&str>,
        lang: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<FeedItem>, CoreError> {
        let connection = self.lock()?;
        let mut sql = String::from("SELECT payload FROM feed_items WHERE 1=1");
        let mut filters: Vec<String> = Vec::new();
        if let Some(source_id) = source_id {
            filters.push(format!("source_id='{}'", source_id.replace('\'', "")));
        }
        if let Some(lang) = lang {
            if !lang.is_empty() {
                filters.push(format!("lang='{}'", lang.replace('\'', "")));
            }
        }
        for filter in filters {
            sql.push_str(" AND ");
            sql.push_str(&filter);
        }
        sql.push_str(&format!(
            " ORDER BY published_at DESC, url LIMIT {} OFFSET {}",
            limit.min(500),
            offset
        ));
        load_all(&connection, &sql)
    }

    fn insert_feed_items(
        &self,
        source_id: &str,
        lang: &str,
        items: &[ParsedItem],
        now: i64,
    ) -> Result<usize, CoreError> {
        if items.is_empty() {
            return Ok(0);
        }
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let before = count_tx(&tx)?;
        for item in items {
            // first_seen_at 只在首次写入时记。它回答的是「我们哪一刻第一次见到这条」，
            // 每次刷新都覆盖就等于把这个信息抹掉，所以先查一次既有的值。
            let first_seen_at: i64 = tx
                .query_row(
                    "SELECT first_seen_at FROM feed_items WHERE url=?1",
                    [item.url.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db_error)?
                .unwrap_or(now);
            let item = FeedItem {
                url: item.url.clone(),
                source_id: source_id.to_owned(),
                source_label: String::new(),
                title: item.title.clone(),
                summary: item.summary.clone(),
                published_at: item.published_at.unwrap_or(now),
                first_seen_at,
                lang: lang.to_owned(),
                stars: item.stars,
                repo: item.repo.clone(),
            };
            tx.execute(
                // first_seen_at 只在首次写入时记，之后刷新不覆盖：
                // 它是「这条我们是哪一刻第一次看到的」，改了就失去意义。
                "INSERT INTO feed_items(url,source_id,published_at,first_seen_at,lang,payload)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(url) DO UPDATE SET
                    published_at=excluded.published_at,
                    payload=excluded.payload",
                params![
                    item.url,
                    item.source_id,
                    item.published_at,
                    item.first_seen_at,
                    item.lang,
                    encode(&item)?
                ],
            )
            .map_err(db_error)?;
        }
        let after = count_tx(&tx)?;
        tx.commit().map_err(db_error)?;
        Ok(after.saturating_sub(before))
    }

    fn delete_source_items_not_in(
        &self,
        source_id: &str,
        keep_urls: &[String],
    ) -> Result<usize, CoreError> {
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let owned: Vec<String> = {
            let mut statement = tx
                .prepare("SELECT url FROM feed_items WHERE source_id=?1")
                .map_err(db_error)?;
            let rows = statement
                .query_map([source_id], |row| row.get::<_, String>(0))
                .map_err(db_error)?;
            let mut urls = Vec::new();
            for row in rows {
                urls.push(row.map_err(db_error)?);
            }
            urls
        };
        let mut removed = 0;
        for url in owned.iter().filter(|url| !keep_urls.contains(url)) {
            tx.execute("DELETE FROM feed_items WHERE url=?1", [url])
                .map_err(db_error)?;
            removed += 1;
        }
        tx.commit().map_err(db_error)?;
        Ok(removed)
    }

    fn trim_source_items(&self, source_id: &str, keep: usize) -> Result<usize, CoreError> {
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let doomed: Vec<String> = {
            let mut statement = tx
                .prepare(
                    "SELECT url FROM feed_items WHERE source_id=?1
                     ORDER BY published_at DESC, url LIMIT -1 OFFSET ?2",
                )
                .map_err(db_error)?;
            let rows = statement
                .query_map(params![source_id, keep as i64], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(db_error)?;
            let mut urls = Vec::new();
            for row in rows {
                urls.push(row.map_err(db_error)?);
            }
            urls
        };
        let removed = doomed.len();
        for url in &doomed {
            tx.execute("DELETE FROM feed_items WHERE url=?1", [url])
                .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        Ok(removed)
    }

    fn count_feed_items(&self) -> Result<usize, CoreError> {
        let connection = self.lock()?;
        count(&connection, "SELECT COUNT(*) FROM feed_items")
    }

    fn prune_feed_items(
        &self,
        now: i64,
        retention_days: i64,
        keep: usize,
    ) -> Result<usize, CoreError> {
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let cutoff = now - retention_days * 86_400;
        let before = count_tx(&tx)?;
        tx.execute(
            "DELETE FROM feed_items WHERE published_at < ?1",
            params![cutoff],
        )
        .map_err(db_error)?;
        // 再按总数封顶：只保最新的 keep 条。
        tx.execute(
            "DELETE FROM feed_items WHERE url NOT IN (
                SELECT url FROM feed_items ORDER BY published_at DESC LIMIT ?1
             )",
            params![keep as i64],
        )
        .map_err(db_error)?;
        let after = count_tx(&tx)?;
        tx.commit().map_err(db_error)?;
        Ok(before.saturating_sub(after))
    }
}

fn count_tx(tx: &Transaction<'_>) -> Result<usize, CoreError> {
    let value: i64 = tx
        .query_row("SELECT COUNT(*) FROM feed_items", [], |row| row.get(0))
        .map_err(db_error)?;
    Ok(value.max(0) as usize)
}

// ---------------------------------------------------------------- 内存实现

/// 测试与离线演示用的内存实现。行为必须与 SQLite 版一致，
/// 否则测出来的东西不代表真实存储。
#[derive(Default)]
pub struct InMemoryHubStore {
    tools: Mutex<HashMap<String, ToolState>>,
    skills: Mutex<HashMap<(String, String), SkillRecord>>,
    sources: Mutex<HashMap<String, FeedSource>>,
    items: Mutex<HashMap<String, FeedItem>>,
}

impl InMemoryHubStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HubStore for InMemoryHubStore {
    fn cached_tool_states(&self) -> Result<Vec<ToolState>, CoreError> {
        let mut states: Vec<ToolState> = self
            .tools
            .lock()
            .expect("锁未被污染")
            .values()
            .cloned()
            .collect();
        states.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(states)
    }

    fn save_tool_state(&self, state: &ToolState) -> Result<(), CoreError> {
        self.tools
            .lock()
            .expect("锁未被污染")
            .insert(state.id.clone(), state.clone());
        Ok(())
    }

    fn list_skills(&self) -> Result<Vec<SkillRecord>, CoreError> {
        let mut records: Vec<SkillRecord> = self
            .skills
            .lock()
            .expect("锁未被污染")
            .values()
            .cloned()
            .collect();
        records.sort_by(|left, right| {
            left.skill_id
                .cmp(&right.skill_id)
                .then_with(|| left.target_tool.cmp(&right.target_tool))
        });
        Ok(records)
    }

    fn save_skill(&self, record: &SkillRecord) -> Result<(), CoreError> {
        self.skills.lock().expect("锁未被污染").insert(
            (record.skill_id.clone(), record.target_tool.clone()),
            record.clone(),
        );
        Ok(())
    }

    fn delete_skill(&self, skill_id: &str, target_tool: &str) -> Result<(), CoreError> {
        self.skills
            .lock()
            .expect("锁未被污染")
            .remove(&(skill_id.to_owned(), target_tool.to_owned()));
        Ok(())
    }

    fn list_feed_sources(&self) -> Result<Vec<FeedSource>, CoreError> {
        let mut sources: Vec<FeedSource> = self
            .sources
            .lock()
            .expect("锁未被污染")
            .values()
            .cloned()
            .collect();
        sources.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(sources)
    }

    fn save_feed_source(&self, source: &FeedSource) -> Result<(), CoreError> {
        self.sources
            .lock()
            .expect("锁未被污染")
            .insert(source.id.clone(), source.clone());
        Ok(())
    }

    fn delete_feed_source(&self, source_id: &str) -> Result<(), CoreError> {
        self.sources.lock().expect("锁未被污染").remove(source_id);
        self.items
            .lock()
            .expect("锁未被污染")
            .retain(|_, item| item.source_id != source_id);
        Ok(())
    }

    fn list_feed_items(
        &self,
        source_id: Option<&str>,
        lang: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<FeedItem>, CoreError> {
        let mut items: Vec<FeedItem> = self
            .items
            .lock()
            .expect("锁未被污染")
            .values()
            .filter(|item| source_id.map(|id| item.source_id == id).unwrap_or(true))
            .filter(|item| {
                lang.map(|lang| lang.is_empty() || item.lang == lang)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        items.sort_by(|left, right| {
            right
                .published_at
                .cmp(&left.published_at)
                .then_with(|| left.url.cmp(&right.url))
        });
        Ok(items
            .into_iter()
            .skip(offset)
            .take(limit.min(500))
            .collect())
    }

    fn insert_feed_items(
        &self,
        source_id: &str,
        lang: &str,
        items: &[ParsedItem],
        now: i64,
    ) -> Result<usize, CoreError> {
        let mut store = self.items.lock().expect("锁未被污染");
        let mut inserted = 0;
        for item in items {
            match store.get_mut(&item.url) {
                // 已存在：刷新内容，保留 first_seen_at 与来源归属。
                Some(entry) => {
                    entry.title = item.title.clone();
                    entry.summary = item.summary.clone();
                    entry.published_at = item.published_at.unwrap_or(entry.published_at);
                    entry.stars = item.stars;
                    entry.repo = item.repo.clone();
                }
                None => {
                    inserted += 1;
                    store.insert(
                        item.url.clone(),
                        FeedItem {
                            url: item.url.clone(),
                            source_id: source_id.to_owned(),
                            source_label: String::new(),
                            title: item.title.clone(),
                            summary: item.summary.clone(),
                            published_at: item.published_at.unwrap_or(now),
                            first_seen_at: now,
                            lang: lang.to_owned(),
                            stars: item.stars,
                            repo: item.repo.clone(),
                        },
                    );
                }
            }
        }
        Ok(inserted)
    }

    fn delete_source_items_not_in(
        &self,
        source_id: &str,
        keep_urls: &[String],
    ) -> Result<usize, CoreError> {
        let mut store = self.items.lock().expect("锁未被污染");
        let before = store.len();
        store.retain(|url, item| {
            item.source_id != source_id || keep_urls.iter().any(|keep| keep == url)
        });
        Ok(before - store.len())
    }

    fn trim_source_items(&self, source_id: &str, keep: usize) -> Result<usize, CoreError> {
        let mut store = self.items.lock().expect("锁未被污染");
        let before = store.len();
        let mut ordered: Vec<(String, i64)> = store
            .values()
            .filter(|item| item.source_id == source_id)
            .map(|item| (item.url.clone(), item.published_at))
            .collect();
        ordered.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        for (url, _) in ordered.into_iter().skip(keep) {
            store.remove(&url);
        }
        Ok(before - store.len())
    }

    fn count_feed_items(&self) -> Result<usize, CoreError> {
        Ok(self.items.lock().expect("锁未被污染").len())
    }

    fn prune_feed_items(
        &self,
        now: i64,
        retention_days: i64,
        keep: usize,
    ) -> Result<usize, CoreError> {
        let mut store = self.items.lock().expect("锁未被污染");
        let before = store.len();
        let cutoff = now - retention_days * 86_400;
        store.retain(|_, item| item.published_at >= cutoff);
        if store.len() > keep {
            let mut ordered: Vec<(String, i64)> = store
                .values()
                .map(|item| (item.url.clone(), item.published_at))
                .collect();
            ordered.sort_by(|left, right| right.1.cmp(&left.1));
            for (url, _) in ordered.into_iter().skip(keep) {
                store.remove(&url);
            }
        }
        Ok(before - store.len())
    }
}

/// 便于测试断言 SQLite 与内存实现行为一致。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{FeedKind, ParsedItem};

    fn tool_state(id: &str, probed_at: i64) -> ToolState {
        ToolState {
            id: id.to_owned(),
            display_name: id.to_owned(),
            category: crate::toolhub::ToolCategory::Utility,
            description: String::new(),
            status: crate::toolhub::ToolStatus::NotInstalled,
            installed: None,
            website: None,
            docs: None,
            model_config: false,
            skill_target: false,
            agent_usage: Default::default(),
            version_probe_tail: String::new(),
            auth_probe_tail: None,
            notes: Vec::new(),
            probed_at,
            cache_seconds: 300,
        }
    }

    fn source(id: &str) -> FeedSource {
        FeedSource {
            id: id.to_owned(),
            kind: FeedKind::Rss,
            url: "https://example.com/feed".to_owned(),
            label: id.to_owned(),
            lang: "zh".to_owned(),
            enabled: true,
            etag: None,
            last_modified: None,
            last_ok_at: None,
            last_error: None,
            fail_streak: 0,
            next_fetch_at: 0,
            builtin: false,
        }
    }

    fn item(url: &str, published_at: i64) -> ParsedItem {
        ParsedItem {
            title: format!("t {url}"),
            url: url.to_owned(),
            published_at: Some(published_at),
            summary: "s".to_owned(),
            id: url.to_owned(),
            stars: None,
            repo: None,
        }
    }

    fn exercise(store: &dyn HubStore) {
        store.save_tool_state(&tool_state("codex", 100)).unwrap();
        let cached = store.cached_tool_states().unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].probed_at, 100);

        store.save_feed_source(&source("a")).unwrap();
        assert_eq!(store.list_feed_sources().unwrap().len(), 1);

        let inserted = store
            .insert_feed_items("a", "zh", &[item("u1", 10), item("u2", 20)], 500)
            .unwrap();
        assert_eq!(inserted, 2);
        assert_eq!(store.count_feed_items().unwrap(), 2);

        // 再写一次：不算新增，但内容刷新，first_seen_at 不动。
        let again = store
            .insert_feed_items("a", "zh", &[item("u1", 30), item("u3", 40)], 900)
            .unwrap();
        assert_eq!(again, 1);
        assert_eq!(store.count_feed_items().unwrap(), 3);
        let items = store.list_feed_items(None, None, 10, 0).unwrap();
        assert_eq!(items[0].url, "u3", "最新的排最前");
        let u1 = items.iter().find(|entry| entry.url == "u1").unwrap();
        assert_eq!(u1.published_at, 30, "内容要刷新");
        assert_eq!(u1.first_seen_at, 500, "首次见到的时间不能被覆盖");

        // 语言过滤与分页。
        assert_eq!(
            store
                .list_feed_items(None, Some("en"), 10, 0)
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            store.list_feed_items(Some("a"), None, 1, 1).unwrap().len(),
            1
        );

        // 裁剪：只留最新的 2 条。
        store.prune_feed_items(900, 30, 2).unwrap();
        assert_eq!(store.count_feed_items().unwrap(), 2);

        // 整源覆盖（刷新走的就是这条路）：这次没给的条目要消失，并按条数收口。
        // SQLite 与内存两份实现必须行为一致——所以这段写在共用的用例里。
        store.save_feed_source(&source("b")).unwrap();
        store
            .insert_feed_items(
                "b",
                "zh",
                &[item("v1", 10), item("v2", 20), item("v3", 30)],
                1000,
            )
            .unwrap();
        store
            .sync_source_items(
                "b",
                "zh",
                &[item("v9", 100), item("v8", 90), item("v7", 80)],
                1100,
                2,
            )
            .unwrap();
        let synced = store.list_feed_items(Some("b"), None, 10, 0).unwrap();
        assert_eq!(synced.len(), 2, "每源只留最新的 keep 条");
        assert_eq!(
            synced
                .iter()
                .map(|entry| entry.url.as_str())
                .collect::<Vec<_>>(),
            vec!["v9", "v8"],
            "上一轮的条目要被覆盖，而不是累加"
        );
        assert_eq!(
            store.list_feed_items(Some("a"), None, 10, 0).unwrap().len(),
            2,
            "覆盖只影响这一个源"
        );

        // 空集合不覆盖：服务层在「正文里一条都没有」时不该清空本地。
        store.sync_source_items("b", "zh", &[], 1200, 2).unwrap();
        assert_eq!(
            store.list_feed_items(Some("b"), None, 10, 0).unwrap().len(),
            2
        );

        // 删源要连带删条目。
        store.delete_feed_source("a").unwrap();
        store.delete_feed_source("b").unwrap();
        assert_eq!(store.count_feed_items().unwrap(), 0);
    }

    #[test]
    fn sqlite_store_behaves_as_documented() {
        let directory = tempfile::tempdir().unwrap();
        let store = SqliteHubStore::open(&directory.path().join("meta.db")).unwrap();
        exercise(&store);
    }

    #[test]
    fn memory_store_matches_the_sqlite_behaviour() {
        let store = InMemoryHubStore::new();
        exercise(&store);
    }

    #[test]
    fn sqlite_store_survives_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("meta.db");
        {
            let store = SqliteHubStore::open(&path).unwrap();
            store.save_feed_source(&source("persisted")).unwrap();
            store.save_tool_state(&tool_state("codex", 42)).unwrap();
        }
        let store = SqliteHubStore::open(&path).unwrap();
        assert_eq!(store.list_feed_sources().unwrap().len(), 1);
        assert_eq!(store.cached_tool_states().unwrap()[0].probed_at, 42);
    }

    #[test]
    fn pruning_removes_items_older_than_the_retention_window() {
        let store = SqliteHubStore::in_memory().unwrap();
        store.save_feed_source(&source("a")).unwrap();
        // now 的前一天边界：`old` 落在窗口外，`fresh` 落在窗口内。
        let now = 1_000_000;
        store
            .insert_feed_items(
                "a",
                "zh",
                &[item("old", now - 2 * 86_400), item("fresh", now - 3_600)],
                now,
            )
            .unwrap();
        let removed = store.prune_feed_items(now, 1, 100).unwrap();
        assert_eq!(removed, 1);
        let items = store.list_feed_items(None, None, 10, 0).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "fresh");
    }

    #[test]
    fn pruning_caps_the_total_number_of_items() {
        let store = InMemoryHubStore::new();
        store.save_feed_source(&source("a")).unwrap();
        let items: Vec<ParsedItem> = (0..10)
            .map(|index| item(&format!("u{index}"), 1_000 + index as i64))
            .collect();
        store.insert_feed_items("a", "zh", &items, 2_000).unwrap();
        assert_eq!(store.count_feed_items().unwrap(), 10);
        store.prune_feed_items(2_000, 30, 4).unwrap();
        assert_eq!(store.count_feed_items().unwrap(), 4);
    }
}
