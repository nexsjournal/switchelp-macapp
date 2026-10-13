//! 对话记录与配置路由的对齐（`state_5.sqlite` 的 `threads` 表）。
//!
//! 为什么需要它：codex 把每条对话的 `model_provider` **固化在对话记录里**，
//! 切换配置（原配置 ↔ 自定义配置）不会改它。于是：
//!
//! - 在账号路由下创建的对话，在我们接管之后仍按账号发送。选了我们目录里的模型
//!   （`gs/…`）再发送，请求会打到 ChatGPT 后端，换回的是 400：
//!   `The '<model>' model is not supported when using Codex with a ChatGPT account.`
//!   用户看到的版本是「切到自定义配置后，同一个对话里发消息报错」。
//! - 反过来，接管期间创建的对话记着 `gptswitch`；还原之后配置里已经没有这个
//!   provider，续接直接失败（表现为对话一直闪、打不开）。
//!
//! 所以路由变了以后，这张表要跟着对齐。只动两列，语义各一条：
//!
//! - 接管：`gs/…` 的对话 → 网关 provider。原生模型名不动（账号路由与网关透传都能送）。
//! - 还原：所有网关 provider 的对话 → 还原后配置里的 provider；`gs/…` 的对话连模型
//!   一起换成还原后配置里的模型（网关模型走不了账号路由，不换就是下一次发送必 400）。
//!
//! 这是尽力而为的一步：表不存在、库文件不存在、库被写锁占住——一律静默跳过。
//! 失败时的行为与没有这一步时完全一致，绝不改变已经提交的配置结果。

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};

/// 我们目录里的模型前缀。与目录编译（`codex::catalog`）发布的 slug 前缀同源。
const GATEWAY_MODEL_PREFIX: &str = "gs/";

/// 配置里没写 `model_provider` 时，codex 用的是内置的那一个。
const FALLBACK_PROVIDER: &str = "openai";

/// 接管后的对齐：把网关模型的对话钉到网关 provider 上。
///
/// 返回改动的行数；`None` 表示没找到可对齐的库（没什么可做的）。
pub fn sync_for_takeover(home: &Path, gateway_provider: &str) -> Option<usize> {
    with_threads(home, |connection| {
        connection
            .execute(
                "UPDATE threads SET model_provider = ?1 \
                 WHERE model LIKE ?2 AND model_provider <> ?1",
                params![gateway_provider, gateway_model_pattern()],
            )
            .ok()
    })
}

/// 还原后的对齐：把接管期间创建的对话交还给还原后的配置。
///
/// `provider` 与 `model` 取自还原后写盘的配置文本（见 [`route_from_config_text`]）。
/// 模型传 `None` 表示配置里没写——把网关模型的对话置空，让宿主回落到它自己的默认。
pub fn sync_for_restore(
    home: &Path,
    gateway_provider: &str,
    provider: &str,
    model: Option<&str>,
) -> Option<usize> {
    with_threads(home, |connection| {
        let mut changed = connection
            .execute(
                "UPDATE threads SET model_provider = ?1 WHERE model_provider = ?2",
                params![provider, gateway_provider],
            )
            .ok()?;
        // 网关模型在账号路由上不存在：不换掉就是「发送必被上游拒绝」。
        changed += connection
            .execute(
                "UPDATE threads SET model = ?1 WHERE model LIKE ?2",
                params![model, gateway_model_pattern()],
            )
            .ok()?;
        Some(changed)
    })
}

/// 还原后的配置文本里，宿主会用的 provider 与模型。
///
/// provider 缺省是 codex 的内置 `openai`；模型缺省返回 `None`。解析不了（不该发生：
/// 文本是我们自己渲染的）时返回 `None`——调用方据此跳过同步，而不是猜一套值写进去。
pub fn route_from_config_text(text: &str) -> Option<(String, Option<String>)> {
    let document = text.parse::<toml_edit::DocumentMut>().ok()?;
    let provider = document
        .get("model_provider")
        .and_then(|item| item.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(FALLBACK_PROVIDER)
        .to_owned();
    let model = document
        .get("model")
        .and_then(|item| item.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    Some((provider, model))
}

fn gateway_model_pattern() -> String {
    format!("{GATEWAY_MODEL_PREFIX}%")
}

/// 打开库并确认 `threads` 表在。任何一步不成立都返回 `None`：这一步不做任何假设。
fn with_threads<T>(home: &Path, work: impl FnOnce(&Connection) -> Option<T>) -> Option<T> {
    let database = state_db(home)?;
    let connection =
        Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_WRITE).ok()?;
    // 宿主在写库是常态（WAL 允许并发，写锁是短促的）；等不到就放弃。
    let _ = connection.busy_timeout(Duration::from_secs(2));
    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'threads'",
            [],
            |row| row.get(0),
        )
        .ok()?;
    if table_count == 0 {
        return None;
    }
    work(&connection)
}

/// codex 的状态库文件名带版本号（当前是 `state_5.sqlite`），且同一目录可能残留旧版本。
/// 取编号最大的那个——与 codex 自己用新版行为一致。
fn state_db(home: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(home).ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(version) = name
            .strip_prefix("state_")
            .and_then(|rest| rest.strip_suffix(".sqlite"))
            .and_then(|digits| digits.parse::<u64>().ok())
        else {
            continue;
        };
        if best
            .as_ref()
            .map(|(current, _)| version > *current)
            .unwrap_or(true)
        {
            best = Some((version, entry.path()));
        }
    }
    best.map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个带 `threads` 表的库；只建这条测试要用到的列。
    fn seed(home: &Path, version: u64, rows: &[(&str, Option<&str>, &str)]) -> PathBuf {
        let path = home.join(format!("state_{version}.sqlite"));
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, model TEXT, model_provider TEXT NOT NULL)",
                [],
            )
            .unwrap();
        for (id, model, provider) in rows {
            connection
                .execute(
                    "INSERT INTO threads (id, model, model_provider) VALUES (?1, ?2, ?3)",
                    params![id, model, provider],
                )
                .unwrap();
        }
        drop(connection);
        path
    }

    /// 读回全量行，按 id 排序，便于整表断言。
    fn rows(db: &Path) -> Vec<(String, Option<String>, String)> {
        let connection = Connection::open(db).unwrap();
        let mut statement = connection
            .prepare("SELECT id, model, model_provider FROM threads ORDER BY id")
            .unwrap();
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows
    }

    #[test]
    fn takeover_pins_only_the_gateway_models() {
        let dir = tempfile::tempdir().unwrap();
        let db = seed(
            dir.path(),
            5,
            &[
                ("gs-a", Some("gs/prov/model-a"), "custom"),
                ("native-a", Some("gpt-5.6-sol"), "custom"),
                ("gs-b", Some("gs/prov/model-b"), "gptswitch"),
            ],
        );
        let changed = sync_for_takeover(dir.path(), "gptswitch");
        assert_eq!(
            changed,
            Some(1),
            "只该动「网关模型 + 非网关 provider」那一行"
        );
        assert_eq!(
            rows(&db),
            vec![
                (
                    "gs-a".to_owned(),
                    Some("gs/prov/model-a".to_owned()),
                    "gptswitch".to_owned()
                ),
                (
                    "gs-b".to_owned(),
                    Some("gs/prov/model-b".to_owned()),
                    "gptswitch".to_owned()
                ),
                (
                    "native-a".to_owned(),
                    Some("gpt-5.6-sol".to_owned()),
                    "custom".to_owned()
                ),
            ]
        );
        assert_eq!(
            sync_for_takeover(dir.path(), "gptswitch"),
            Some(0),
            "再跑一次是幂等的"
        );
    }

    #[test]
    fn restore_returns_gateway_conversations_to_the_account_route() {
        let dir = tempfile::tempdir().unwrap();
        let db = seed(
            dir.path(),
            5,
            &[
                ("gs-a", Some("gs/prov/model-a"), "gptswitch"),
                ("gs-b", Some("gs/prov/model-b"), "custom"),
                ("native-a", Some("gpt-6.1-sol"), "gptswitch"),
            ],
        );
        let changed = sync_for_restore(dir.path(), "gptswitch", "custom", Some("gpt-6.1-sol"));
        // provider 两行（gs-a、native-a）+ 模型两行（gs-a、gs-b）。
        assert_eq!(changed, Some(4));
        assert_eq!(
            rows(&db),
            vec![
                (
                    "gs-a".to_owned(),
                    Some("gpt-6.1-sol".to_owned()),
                    "custom".to_owned()
                ),
                (
                    "gs-b".to_owned(),
                    Some("gpt-6.1-sol".to_owned()),
                    "custom".to_owned()
                ),
                (
                    "native-a".to_owned(),
                    Some("gpt-6.1-sol".to_owned()),
                    "custom".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn restore_without_a_model_in_the_config_clears_the_gateway_model() {
        let dir = tempfile::tempdir().unwrap();
        let db = seed(
            dir.path(),
            5,
            &[("gs-a", Some("gs/prov/model-a"), "gptswitch")],
        );
        sync_for_restore(dir.path(), "gptswitch", "openai", None).unwrap();
        assert_eq!(
            rows(&db),
            vec![("gs-a".to_owned(), None, "openai".to_owned())],
            "配置里没有模型就置空，让宿主回落默认，而不是继续指向走不通的旧值"
        );
    }

    #[test]
    fn a_home_without_a_state_database_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(sync_for_takeover(dir.path(), "gptswitch"), None);
        assert_eq!(
            sync_for_restore(dir.path(), "gptswitch", "openai", None),
            None
        );
    }

    #[test]
    fn a_database_without_a_threads_table_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state_5.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("CREATE TABLE unrelated (id TEXT)", [])
            .unwrap();
        drop(connection);
        assert_eq!(sync_for_takeover(dir.path(), "gptswitch"), None);
    }

    #[test]
    fn only_the_newest_state_database_is_touched() {
        let dir = tempfile::tempdir().unwrap();
        let old = seed(
            dir.path(),
            4,
            &[("gs-a", Some("gs/prov/model-a"), "custom")],
        );
        let new = seed(
            dir.path(),
            5,
            &[("gs-a", Some("gs/prov/model-a"), "custom")],
        );
        assert_eq!(sync_for_takeover(dir.path(), "gptswitch"), Some(1));
        assert_eq!(rows(&old)[0].2, "custom", "旧版本库必须原样不动");
        assert_eq!(rows(&new)[0].2, "gptswitch");
    }

    #[test]
    fn route_from_config_text_reads_provider_and_model() {
        assert_eq!(
            route_from_config_text("model_provider = \"custom\"\nmodel = \"gpt-6.1-sol\"\n"),
            Some(("custom".to_owned(), Some("gpt-6.1-sol".to_owned())))
        );
        assert_eq!(
            route_from_config_text("approval_policy = \"on-request\"\n"),
            Some(("openai".to_owned(), None)),
            "没写 provider 时是内置的那一个"
        );
        assert_eq!(
            route_from_config_text("model = \"  \"\n"),
            Some(("openai".to_owned(), None)),
            "空白值等于没写"
        );
        assert_eq!(route_from_config_text("不是 toml"), None);
    }
}
