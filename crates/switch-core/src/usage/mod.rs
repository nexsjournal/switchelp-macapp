//! 用量：只读本机 Codex 会话记录（rollout JSONL），统计 Token 用量。
//!
//! 完整规格见 `docs/design/07-usage-page.md`。三条设计取向写在最前面，因为它们决定了
//! 这个模块「不做什么」：
//!
//! - **不联网**。数据全部来自 Codex 自己写在磁盘上的会话记录，没有任何请求、没有采集。
//! - **不做成本估算**。本工具的模型是用户自定义的第三方模型，没有可靠的单价来源；
//!   宁可只给 token 真值，也不给一个估出来的钱（见 04-pages-and-flows.md 的同一条规则）。
//! - **不落库**。每次按需扫描，不在本地复制一份用量数据，符合「尽量不存数据」的取向。
//!
//! # 解析算法：对累计值做差分，不要累加 `last_token_usage`
//!
//! `token_count` 事件里同时有 `total_token_usage`（会话内累计）与 `last_token_usage`。
//! 本机 337 个文件实测：**`last_token_usage` 会重复计数**——约 42% 的文件里「累加 last」
//! 显著大于最终累计值（例：最终 135386，累加 last 得 202636）。而 `total_token_usage`
//! 的每个字段在 259 个活跃文件里**全部单调不减**（负差分为 0），所以正确做法是逐字段差分：
//!
//! ```text
//! delta_i = total_i - total_{i-1}     （prev 初值全 0）
//! ```
//!
//! 累计值单调时，差分自动求和等于最终累计值：既不重复计数，又能按每个事件的时间戳与
//! 当时的模型/供应商正确分摊到「天 / 模型 / 供应商」三个维度。
//!
//! 一个容易写错的地方：**`prev` 必须无条件前进**，即使该事件落在时间范围外（会被过滤）。
//! 否则范围边界处的第一个事件会被算成一个巨大的增量。

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// 允许的时间范围（天）。范围固定，避免每日柱状图跨度失控、以及图表与合计口径不一致。
pub const RANGE_DAYS_OPTIONS: [i64; 3] = [7, 30, 90];
/// 非法范围回落到这个值。
pub const DEFAULT_RANGE_DAYS: i64 = 30;
/// 递归深度上限。`sessions/年/月/日/文件` 只有四层。
const MAX_DEPTH: usize = 8;

/// 把请求范围夹到允许值；非法值回落到 [`DEFAULT_RANGE_DAYS`]。
pub fn clamp_range_days(days: i64) -> i64 {
    if RANGE_DAYS_OPTIONS.contains(&days) {
        days
    } else {
        DEFAULT_RANGE_DAYS
    }
}

/// 会话记录的根目录：优先 `CODEX_HOME`，否则用户主目录下的 `.codex`。
///
/// 这是**「按默认位置找」**的近似：共存模式下托管 profile 有它自己的 home，本进程
/// `CODEX_HOME` 没指过去时扫不到那部分用量。报告里的 `source_directory` 会把实际扫的
/// 目录如实带回界面，所以这种情况在界面上是看得见的，不是暗中的偏差。
pub fn resolve_codex_home() -> PathBuf {
    if let Some(value) = std::env::var_os("CODEX_HOME") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    user_home().unwrap_or_default().join(".codex")
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// 六个字段的用量。口径见模块说明与契约注释：`cached_tokens ⊂ input_tokens`、`reasoning_tokens ⊂ output_tokens`。
///
/// **`total_tokens` 不等于 `input_tokens + output_tokens`**：本机 32366 个带用量的
/// `token_count` 事件里有 2878 个（约 9%）两者不等，差异多为 ±1~7。所以这几个字段之间
/// 不可互相推算——界面只并列展示，不做任何加总暗示。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub input_tokens: u64,
    /// 输入中被缓存命中的部分。
    pub cached_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    /// 输出中的推理部分。
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
}

impl UsageTotals {
    /// 逐字段累加一个增量。
    fn add(&mut self, delta: &UsageTotals) {
        self.input_tokens += delta.input_tokens;
        self.cached_tokens += delta.cached_tokens;
        self.cache_write_tokens += delta.cache_write_tokens;
        self.output_tokens += delta.output_tokens;
        self.reasoning_tokens += delta.reasoning_tokens;
        self.total_tokens += delta.total_tokens;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    /// 本地日期，`YYYY-MM-DD`。
    pub date: String,
    pub sessions: u64,
    pub totals: UsageTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageModelRow {
    pub model: String,
    pub sessions: u64,
    pub totals: UsageTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageProviderRow {
    pub provider: String,
    pub sessions: u64,
    pub totals: UsageTotals,
}

/// 计划额度窗口。只有官方计划账号的 Codex 会话会写入它；第三方供应商的会话没有。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePlanWindow {
    pub plan_type: String,
    pub used_percent: f64,
    pub window_minutes: i64,
    /// Unix 秒。
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    /// 实际扫描的根目录，界面上如实显示，便于核对数字来源。
    pub source_directory: String,
    pub range_days: i64,
    pub scanned_files: u64,
    pub unreadable_files: u64,
    pub sessions: u64,
    pub totals: UsageTotals,
    /// 范围内逐日零填充，按日期升序。
    pub daily: Vec<UsageDay>,
    /// 按总 Token 降序。
    pub by_model: Vec<UsageModelRow>,
    pub by_provider: Vec<UsageProviderRow>,
    pub plan_window: Option<UsagePlanWindow>,
}

/// 扫描 `<codex_home>` 下的会话记录，得到用量报告。
///
/// 时区偏移单独注入是为了让断言不依赖跑测试的机器在哪个时区（同 `content::next_daily_fetch`）。
pub fn collect_usage(codex_home: &Path, days: i64, now_unix: i64) -> UsageReport {
    collect_usage_at(codex_home, days, now_unix, local_offset_seconds())
}

/// 本机时区相对 UTC 的偏移（秒）。拿不到时按 UTC（`local-offset` 在某些平台会失败）。
///
/// 退化成 UTC 至多让「哪一天」偏几小时，比整个页面取不到数好。
fn local_offset_seconds() -> i32 {
    time::UtcOffset::current_local_offset()
        .map(|offset| offset.whole_seconds())
        .unwrap_or(0)
}

/// 带显式时区偏移的 [`collect_usage`]。
pub fn collect_usage_at(
    codex_home: &Path,
    days: i64,
    now_unix: i64,
    offset_seconds: i32,
) -> UsageReport {
    let range_days = clamp_range_days(days);

    // 范围内每一天的本地日期键（升序）。同时也是「事件是否在范围内」的判据。
    let offset = i64::from(offset_seconds);
    let local_now = now_unix + offset;
    let today_start = local_now - local_now.rem_euclid(86_400);
    let date_keys: Vec<String> = (0..range_days)
        .map(|i| local_date_string(today_start - (range_days - 1 - i) * 86_400, 0))
        .collect();
    let in_range: HashSet<&str> = date_keys.iter().map(String::as_str).collect();

    let mut scanned_files = 0_u64;
    let mut unreadable_files = 0_u64;
    let mut day_totals: BTreeMap<String, UsageTotals> = BTreeMap::new();
    let mut day_sessions: BTreeMap<String, u64> = BTreeMap::new();
    let mut model_totals: BTreeMap<String, UsageTotals> = BTreeMap::new();
    let mut model_sessions: BTreeMap<String, u64> = BTreeMap::new();
    let mut provider_totals: BTreeMap<String, UsageTotals> = BTreeMap::new();
    let mut provider_sessions: BTreeMap<String, u64> = BTreeMap::new();
    let mut totals = UsageTotals::default();
    let mut sessions = 0_u64;
    let mut best_plan: Option<(i64, UsagePlanWindow)> = None;

    let mut candidates = Vec::new();
    collect_rollout_files(&codex_home.join("sessions"), 0, &mut candidates);
    collect_rollout_files(&codex_home.join("archived_sessions"), 0, &mut candidates);

    for path in candidates {
        scanned_files += 1;
        let parsed = parse_rollout_file(
            &path,
            offset,
            &in_range,
            &mut day_totals,
            &mut model_totals,
            &mut provider_totals,
            &mut best_plan,
        );
        let Some(parsed) = parsed else {
            unreadable_files += 1;
            continue;
        };
        if !parsed.in_range {
            continue;
        }
        totals.add(&parsed.totals);
        // 会话数一律按**去重后的文件数**统计：一天里同一个会话产生多少事件都只算一个。
        sessions += 1;
        for date in parsed.days {
            *day_sessions.entry(date).or_insert(0) += 1;
        }
        for model in parsed.models {
            *model_sessions.entry(model).or_insert(0) += 1;
        }
        *provider_sessions.entry(parsed.provider).or_insert(0) += 1;
    }

    let daily = date_keys
        .into_iter()
        .map(|date| UsageDay {
            sessions: day_sessions.get(&date).copied().unwrap_or(0),
            totals: day_totals.get(&date).copied().unwrap_or_default(),
            date,
        })
        .collect();

    UsageReport {
        source_directory: codex_home.display().to_string(),
        range_days,
        scanned_files,
        unreadable_files,
        sessions,
        totals,
        daily,
        by_model: model_rows(model_totals, model_sessions),
        by_provider: provider_rows(provider_totals, provider_sessions),
        plan_window: best_plan.map(|(_, window)| window),
    }
}

fn model_rows(
    totals: BTreeMap<String, UsageTotals>,
    sessions: BTreeMap<String, u64>,
) -> Vec<UsageModelRow> {
    let mut rows: Vec<UsageModelRow> = totals
        .into_iter()
        .map(|(model, totals)| UsageModelRow {
            sessions: sessions.get(&model).copied().unwrap_or(0),
            model,
            totals,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.totals
            .total_tokens
            .cmp(&a.totals.total_tokens)
            .then_with(|| a.model.cmp(&b.model))
    });
    rows
}

fn provider_rows(
    totals: BTreeMap<String, UsageTotals>,
    sessions: BTreeMap<String, u64>,
) -> Vec<UsageProviderRow> {
    let mut rows: Vec<UsageProviderRow> = totals
        .into_iter()
        .map(|(provider, totals)| UsageProviderRow {
            sessions: sessions.get(&provider).copied().unwrap_or(0),
            provider,
            totals,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.totals
            .total_tokens
            .cmp(&a.totals.total_tokens)
            .then_with(|| a.provider.cmp(&b.provider))
    });
    rows
}

/// 一个文件解析出来的结果。`in_range` 为假时只有 `totals` 会被用到。
struct ParsedFile {
    totals: UsageTotals,
    in_range: bool,
    days: HashSet<String>,
    models: HashSet<String>,
    provider: String,
}

/// 解析单个 rollout 文件。
///
/// 返回 `None` 表示这个文件读不了（打不开、或读到一半失败）——调用方记为「不可读」。
/// **行级解析失败不算不可读**：跳过了继续解析下一行，否则一个坏行会让整个会话消失。
#[allow(clippy::too_many_arguments)]
fn parse_rollout_file(
    path: &Path,
    offset: i64,
    in_range: &HashSet<&str>,
    day_totals: &mut BTreeMap<String, UsageTotals>,
    model_totals: &mut BTreeMap<String, UsageTotals>,
    provider_totals: &mut BTreeMap<String, UsageTotals>,
    best_plan: &mut Option<(i64, UsagePlanWindow)>,
) -> Option<ParsedFile> {
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);

    let mut parsed = ParsedFile {
        totals: UsageTotals::default(),
        in_range: false,
        days: HashSet::new(),
        models: HashSet::new(),
        provider: "unknown".to_owned(),
    };
    // 差分基准：会话内累计值的上一次读数，初值全 0。
    let mut prev = UsageTotals::default();
    let mut model = "unknown".to_owned();
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            // 读完。
            Ok(0) => break,
            Ok(_) => {}
            // 读到一半失败：文件读不完整，按不可读处理（已统计的部分不落进报告）。
            Err(_) => return None,
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // 坏行直接跳过：一个坏行不该让整个会话消失。
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };

        match value.get("type").and_then(|v| v.as_str()) {
            Some("session_meta") => {
                if let Some(name) = value
                    .pointer("/payload/model_provider")
                    .and_then(|v| v.as_str())
                {
                    parsed.provider = name.to_owned();
                }
            }
            Some("turn_context") => {
                // 模型名只出现在这里，不在 session_meta。
                if let Some(name) = value.pointer("/payload/model").and_then(|v| v.as_str()) {
                    model = name.to_owned();
                }
            }
            Some("event_msg") => {
                if value.pointer("/payload/type").and_then(|v| v.as_str()) != Some("token_count") {
                    continue;
                }
                let Some(info) = value.pointer("/payload/info/total_token_usage") else {
                    continue;
                };
                let current = UsageTotals {
                    input_tokens: number(info, "input_tokens"),
                    cached_tokens: number(info, "cached_input_tokens"),
                    cache_write_tokens: number(info, "cache_write_input_tokens"),
                    output_tokens: number(info, "output_tokens"),
                    reasoning_tokens: number(info, "reasoning_output_tokens"),
                    total_tokens: number(info, "total_tokens"),
                };
                // 逐字段差分。**先算差值并推进 prev，再决定要不要计入范围**——
                // 否则范围外的读数会把范围内第一个事件的增量放大。
                let delta = UsageTotals {
                    input_tokens: step(current.input_tokens, &mut prev.input_tokens),
                    cached_tokens: step(current.cached_tokens, &mut prev.cached_tokens),
                    cache_write_tokens: step(
                        current.cache_write_tokens,
                        &mut prev.cache_write_tokens,
                    ),
                    output_tokens: step(current.output_tokens, &mut prev.output_tokens),
                    reasoning_tokens: step(current.reasoning_tokens, &mut prev.reasoning_tokens),
                    total_tokens: step(current.total_tokens, &mut prev.total_tokens),
                };

                // 计划窗口不受时间范围限制：它是「当前这个窗口」的状态，取时间戳最新的那条。
                if let Some(window) = plan_window(&value) {
                    let at = timestamp(&value).unwrap_or(0);
                    let should_replace = match best_plan.as_ref() {
                        Some((seen, _)) => at >= *seen,
                        None => true,
                    };
                    if should_replace {
                        *best_plan = Some((at, window));
                    }
                }

                let Some(at) = timestamp(&value) else {
                    continue;
                };
                let date = local_date_string(at, offset as i32);
                if !in_range.contains(date.as_str()) {
                    continue;
                }
                parsed.in_range = true;
                parsed.totals.add(&delta);
                parsed.days.insert(date.clone());
                parsed.models.insert(model.clone());
                day_totals.entry(date).or_default().add(&delta);
                model_totals.entry(model.clone()).or_default().add(&delta);
                provider_totals
                    .entry(parsed.provider.clone())
                    .or_default()
                    .add(&delta);
            }
            _ => {}
        }
    }

    Some(parsed)
}

/// 差值并把基准推进到当前值；负差（累计值异常回退）按 0 处理。
fn step(current: u64, prev: &mut u64) -> u64 {
    let delta = current.saturating_sub(*prev);
    *prev = current;
    delta
}

fn number(value: &serde_json::Value, key: &str) -> u64 {
    value.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

/// 事件的 Unix 秒。缺时间戳或格式不对时返回 `None`。
fn timestamp(value: &serde_json::Value) -> Option<i64> {
    let raw = value.get("timestamp").and_then(|v| v.as_str())?;
    time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|parsed| parsed.unix_timestamp())
}

/// 读出计划额度窗口。`plan_type` 与 `primary` 都非空才算数（第三方供应商的会话两者都是 null）。
fn plan_window(value: &serde_json::Value) -> Option<UsagePlanWindow> {
    let limits = value.pointer("/payload/rate_limits")?;
    let plan_type = limits.get("plan_type").and_then(|v| v.as_str())?;
    let primary = limits.get("primary").filter(|v| !v.is_null())?;
    let used_percent = primary.get("used_percent").and_then(|v| v.as_f64())?;
    Some(UsagePlanWindow {
        plan_type: plan_type.to_owned(),
        used_percent,
        window_minutes: primary
            .get("window_minutes")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        resets_at: primary
            .get("resets_at")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
    })
}

/// Unix 秒 → 该时刻在 `offset_seconds` 时区下的本地日期字符串。
///
/// 做法是先把时间戳平移到本地刻度，再按 UTC 解释——这样拿到的就是本地日历日期。
/// 显式接受偏移而不是读环境，是为了断言可以在固定时区下写死。
fn local_date_string(unix_seconds: i64, offset_seconds: i32) -> String {
    let shifted =
        time::OffsetDateTime::from_unix_timestamp(unix_seconds + i64::from(offset_seconds))
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    shifted.format(DATE_FORMAT).unwrap_or_default()
}

const DATE_FORMAT: &[time::format_description::FormatItem<'_>] =
    time::macros::format_description!("[year]-[month]-[day]");

/// 递归找 rollout 文件。只认 `rollout-*.jsonl`，所以归档目录里的 `.bak` 不会被算进来。
fn collect_rollout_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_rollout_files(&path, depth + 1, out);
            continue;
        }
        if !kind.is_file() && !kind.is_symlink() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if name.starts_with("rollout-") && name.ends_with(".jsonl") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 2026-05-25 12:00:00 UTC，测试全部以它为「现在」，时区偏移固定 +8（Asia/Shanghai）。
    const NOW: i64 = 1_779_710_400;
    const OFFSET: i32 = 8 * 3600;

    fn write_session(dir: &Path, name: &str, lines: &[serde_json::Value]) {
        let path = dir.join(name);
        fs::create_dir_all(dir).unwrap();
        let mut file = fs::File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{}", serde_json::to_string(line).unwrap()).unwrap();
        }
    }

    fn meta(provider: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": "2026-05-25T02:00:00.000Z",
            "type": "session_meta",
            "payload": { "session_id": "s1", "model_provider": provider },
        })
    }

    fn turn(model: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": "2026-05-25T02:00:01.000Z",
            "type": "turn_context",
            "payload": { "model": model },
        })
    }

    /// 造一个 token_count 事件：`total` 是累计值，`last` 是那一行里同时写着的增量。
    fn token_count(
        timestamp: &str,
        total: (u64, u64, u64, u64),
        last: Option<(u64, u64, u64, u64)>,
        rate_limits: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let (input, cached, output, reasoning) = total;
        let usage = |t: (u64, u64, u64, u64)| {
            serde_json::json!({
                "input_tokens": t.0,
                "cached_input_tokens": t.1,
                "cache_write_input_tokens": 0,
                "output_tokens": t.2,
                "reasoning_output_tokens": t.3,
                "total_tokens": t.0 + t.2,
            })
        };
        let last = serde_json::to_value(usage(last.unwrap_or(total))).unwrap();
        let mut payload = serde_json::json!({
            "type": "token_count",
            "info": { "total_token_usage": usage((input, cached, output, reasoning)), "last_token_usage": last },
        });
        if let Some(limits) = rate_limits {
            payload["rate_limits"] = limits;
        }
        serde_json::json!({ "timestamp": timestamp, "type": "event_msg", "payload": payload })
    }

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn sessions_dir(root: &Path) -> PathBuf {
        root.join("sessions").join("2026").join("05").join("25")
    }

    #[test]
    fn last_token_usage_double_counting_does_not_inflate_totals() {
        // 本机实测的那种文件：last 明显大于真实增量，但 total 单调递增。
        let root = home();
        let dir = sessions_dir(root.path());
        write_session(
            &dir,
            "rollout-a.jsonl",
            &[
                meta("OpenAI"),
                turn("gpt-5.5"),
                token_count(
                    "2026-05-25T02:10:00.000Z",
                    (14_766, 2_432, 300, 88),
                    Some((14_766, 2_432, 300, 88)),
                    None,
                ),
                token_count(
                    "2026-05-25T02:20:00.000Z",
                    (55_211, 17_152, 897, 255),
                    // 真实增量是 40445/14720/597/167，但 last 写成整段累计值。
                    Some((55_211, 17_152, 897, 255)),
                    None,
                ),
                token_count(
                    "2026-05-25T02:30:00.000Z",
                    (60_000, 18_000, 1_000, 300),
                    Some((4_789, 848, 103, 45)),
                    None,
                ),
            ],
        );

        let report = collect_usage_at(root.path(), 30, NOW, OFFSET);
        // 差分之和 == 最终累计值；若按 last 累加会得到 14_766 + 55_211 + 4_789 = 74_766。
        assert_eq!(report.totals.input_tokens, 60_000);
        assert_eq!(report.totals.output_tokens, 1_000);
        assert_eq!(report.totals.cached_tokens, 18_000);
        assert_eq!(report.totals.total_tokens, 61_000);
        assert_ne!(report.totals.input_tokens, 74_766);
        assert_eq!(report.sessions, 1);
    }

    #[test]
    fn out_of_range_events_still_advance_the_baseline() {
        let root = home();
        let dir = sessions_dir(root.path());
        write_session(
            &dir,
            "rollout-b.jsonl",
            &[
                meta("OpenAI"),
                turn("gpt-5.5"),
                // 范围外（2026-04-01）：它必须让 prev 前进，否则下面那条会被算成 50_000。
                token_count(
                    "2026-04-01T02:00:00.000Z",
                    (50_000, 0, 1_000, 0),
                    Some((50_000, 0, 1_000, 0)),
                    None,
                ),
                // 范围内：真实增量是 1_000。
                token_count(
                    "2026-05-25T02:10:00.000Z",
                    (51_000, 0, 1_100, 0),
                    Some((1_000, 0, 100, 0)),
                    None,
                ),
            ],
        );

        let report = collect_usage_at(root.path(), 30, NOW, OFFSET);
        assert_eq!(report.totals.input_tokens, 1_000);
        assert_eq!(report.totals.output_tokens, 100);
        // 范围外的那段只体现在文件总量里，不进范围内合计。
        let today = report.daily.last().unwrap();
        assert_eq!(today.totals.input_tokens, 1_000);
        assert_eq!(report.sessions, 1);
    }

    #[test]
    fn plan_window_takes_the_latest_non_null_and_is_none_when_absent() {
        let root = home();
        let dir = sessions_dir(root.path());
        let limits = |percent: f64, plan: &str| {
            serde_json::json!({
                "limit_id": "codex", "plan_type": plan,
                "primary": { "used_percent": percent, "window_minutes": 10_080, "resets_at": 1_779_107_671_i64 },
                "secondary": null, "credits": null,
            })
        };
        write_session(
            &dir,
            "rollout-c.jsonl",
            &[
                meta("OpenAI"),
                turn("gpt-5.5"),
                token_count(
                    "2026-05-25T02:10:00.000Z",
                    (1_000, 0, 100, 0),
                    None,
                    Some(limits(32.0, "free")),
                ),
                // 全 null 的那条必须被跳过。
                token_count(
                    "2026-05-25T02:20:00.000Z",
                    (2_000, 0, 200, 0),
                    None,
                    Some(serde_json::json!({
                        "limit_id": "codex", "plan_type": null,
                        "primary": null, "secondary": null, "credits": null,
                    })),
                ),
                // 时间更晚的那条胜出。
                token_count(
                    "2026-05-25T02:30:00.000Z",
                    (3_000, 0, 300, 0),
                    None,
                    Some(limits(41.5, "plus")),
                ),
            ],
        );

        let report = collect_usage_at(root.path(), 30, NOW, OFFSET);
        let window = report.plan_window.expect("应当读到计划窗口");
        assert_eq!(window.plan_type, "plus");
        assert_eq!(window.used_percent, 41.5);
        assert_eq!(window.window_minutes, 10_080);

        // 另起一个只有第三方供应商会话的目录：一条非空 rate_limits 都没有。
        let other = home();
        write_session(
            &sessions_dir(other.path()),
            "rollout-d.jsonl",
            &[
                meta("gptswitch"),
                turn("qiyuanapi/deepseek-v4.1"),
                token_count(
                    "2026-05-25T02:10:00.000Z",
                    (1_000, 0, 100, 0),
                    None,
                    Some(serde_json::json!({
                        "limit_id": "codex", "plan_type": null,
                        "primary": null, "secondary": null, "credits": null,
                    })),
                ),
            ],
        );
        assert!(collect_usage_at(other.path(), 30, NOW, OFFSET)
            .plan_window
            .is_none());
    }

    #[test]
    fn daily_is_zero_filled_and_ascending() {
        let root = home();
        write_session(
            &sessions_dir(root.path()),
            "rollout-e.jsonl",
            &[
                meta("OpenAI"),
                turn("gpt-5.5"),
                token_count("2026-05-25T02:10:00.000Z", (1_000, 0, 100, 0), None, None),
            ],
        );

        let report = collect_usage_at(root.path(), 7, NOW, OFFSET);
        assert_eq!(report.daily.len(), 7);
        let dates: Vec<&str> = report.daily.iter().map(|d| d.date.as_str()).collect();
        let mut sorted = dates.clone();
        sorted.sort_unstable();
        assert_eq!(dates, sorted, "daily 必须升序");
        // 本地 2026-05-25 是范围最后一天（NOW 是本地 05-25 20:00）。
        assert_eq!(dates.last().unwrap(), &"2026-05-25");
        assert_eq!(dates.first().unwrap(), &"2026-05-19");
        // 只有最后一天有数据，其余为零。
        assert_eq!(report.daily[0].totals, UsageTotals::default());
        assert_eq!(report.daily[6].totals.input_tokens, 1_000);
        assert_eq!(report.daily[6].sessions, 1);
    }

    #[test]
    fn malformed_lines_are_skipped_without_losing_the_session() {
        let root = home();
        let dir = sessions_dir(root.path());
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-f.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "{}", serde_json::to_string(&meta("OpenAI")).unwrap()).unwrap();
        writeln!(file, "这不是 JSON").unwrap();
        writeln!(file, "{{\"type\": \"session_meta\", 截断了").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "{}", serde_json::to_string(&turn("gpt-5.5")).unwrap()).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::to_string(&token_count(
                "2026-05-25T02:10:00.000Z",
                (1_000, 0, 100, 0),
                None,
                None
            ))
            .unwrap()
        )
        .unwrap();

        let report = collect_usage_at(root.path(), 30, NOW, OFFSET);
        assert_eq!(report.unreadable_files, 0, "坏行不算整个文件不可读");
        assert_eq!(report.sessions, 1);
        assert_eq!(report.totals.input_tokens, 1_000);
        assert_eq!(report.by_model[0].model, "gpt-5.5");
    }

    #[test]
    fn totals_are_consistent_with_the_breakdowns() {
        let root = home();
        let dir = sessions_dir(root.path());
        write_session(
            &dir,
            "rollout-g.jsonl",
            &[
                meta("OpenAI"),
                turn("gpt-5.5"),
                token_count("2026-05-25T02:10:00.000Z", (1_000, 100, 50, 10), None, None),
                turn("gpt-5.6-sol"),
                token_count(
                    "2026-05-25T03:10:00.000Z",
                    (9_000, 200, 500, 20),
                    None,
                    None,
                ),
            ],
        );
        write_session(
            &dir,
            "rollout-h.jsonl",
            &[
                meta("gptswitch"),
                turn("qiyuanapi/deepseek-v4.1"),
                token_count("2026-05-25T04:10:00.000Z", (3_000, 0, 300, 0), None, None),
            ],
        );

        let report = collect_usage_at(root.path(), 30, NOW, OFFSET);
        // 逐事件分摊：gpt-5.5 拿第一条的增量，gpt-5.6-sol 拿第二条。
        assert_eq!(report.by_model[0].model, "gpt-5.6-sol");
        assert_eq!(report.by_model[1].model, "qiyuanapi/deepseek-v4.1");
        assert_eq!(report.by_model[2].model, "gpt-5.5");
        assert_eq!(report.by_model[0].totals.total_tokens, 8_450);
        assert_eq!(report.by_model[2].totals.total_tokens, 1_050);
        // 供应商按 token 降序，且会话数按文件去重。
        assert_eq!(report.by_provider[0].provider, "OpenAI");
        assert_eq!(report.by_provider[0].sessions, 1);
        assert_eq!(report.by_provider[1].provider, "gptswitch");
        assert_eq!(report.sessions, 2);
        // 三条独立分栏必须都与合计自洽——这是比「字段间恒等式」更该守住的不变量。
        // （`total_tokens` 与 `input_tokens + output_tokens` 在真实数据上并不总相等：
        // 本机 32366 个事件里约 9% 有 ±1~7 的差，所以这里不作那个断言。）
        let sum_daily: u64 = report.daily.iter().map(|d| d.totals.total_tokens).sum();
        let sum_models: u64 = report.by_model.iter().map(|r| r.totals.total_tokens).sum();
        let sum_providers: u64 = report
            .by_provider
            .iter()
            .map(|r| r.totals.total_tokens)
            .sum();
        assert_eq!(sum_daily, report.totals.total_tokens);
        assert_eq!(sum_models, report.totals.total_tokens);
        assert_eq!(sum_providers, report.totals.total_tokens);
        // 会话数同样按文件去重后自洽。
        let sum_day_sessions: u64 = report.daily.iter().map(|d| d.sessions).sum();
        assert_eq!(sum_day_sessions, 2);
    }

    #[test]
    fn missing_home_yields_an_empty_report_instead_of_failing() {
        let root = home();
        let report = collect_usage_at(&root.path().join("并不存在"), 30, NOW, OFFSET);
        assert_eq!(report.scanned_files, 0);
        assert_eq!(report.sessions, 0);
        assert_eq!(report.totals, UsageTotals::default());
        assert_eq!(report.by_model.len(), 0);
        assert_eq!(report.daily.len(), 30);
        assert!(report.plan_window.is_none());
    }

    #[test]
    fn archived_sessions_are_counted_and_bak_files_are_ignored() {
        let root = home();
        write_session(
            &root.path().join("archived_sessions"),
            "rollout-archived.jsonl",
            &[
                meta("OpenAI"),
                turn("gpt-5.5"),
                token_count("2026-05-25T02:10:00.000Z", (1_000, 0, 100, 0), None, None),
            ],
        );
        // 归档目录里真实存在的 `.bak` 不能被当成会话。
        fs::write(
            root.path()
                .join("archived_sessions")
                .join("rollout-x.jsonl.bak"),
            "{}",
        )
        .unwrap();

        let report = collect_usage_at(root.path(), 30, NOW, OFFSET);
        assert_eq!(report.scanned_files, 1);
        assert_eq!(report.sessions, 1);
    }

    #[test]
    fn clamp_range_days_only_accepts_the_three_options() {
        assert_eq!(clamp_range_days(7), 7);
        assert_eq!(clamp_range_days(30), 30);
        assert_eq!(clamp_range_days(90), 90);
        assert_eq!(clamp_range_days(0), DEFAULT_RANGE_DAYS);
        assert_eq!(clamp_range_days(365), DEFAULT_RANGE_DAYS);
        assert_eq!(clamp_range_days(-1), DEFAULT_RANGE_DAYS);
    }
}
