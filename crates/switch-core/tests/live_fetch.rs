//! 真的打一次外网：内容源与 GitHub 仓库。
//!
//! 默认 `#[ignore]`：它依赖本机网络、代理设置**以及第三方站点的可用性**，不该在 CI 里跑
//! （CI 上没有代理，这些站点也不一定可达）。要在目标机器上显式验收时执行：
//!
//! ```sh
//! cargo test -p switch-core --test live_fetch -- --ignored --nocapture
//! ```
//!
//! 它守的是 `platform::proxy::outbound_proxy` 这件事：**开着系统代理的机器上，抓取要走代理**。
//! 2026-09-23 在本机实测：直连 `linux.do` 与 `raw.githubusercontent.com` 都超时（12s + 000），
//! 经 Clash Verge(7897) 都是 200；而两个抓取器当时只用 `Proxy::try_from_env()`，看不到 macOS
//! 的系统代理——于是内容中心一直显示「某个源连续失败 N 次」、插件中心永远停在「正在读取仓库…」。
//!
//! 判定要打印数字（状态码、条目数、文件数），不要只说「没报错」。

use switch_core::content::{FeedFetcher, HttpFeedFetcher, DEFAULT_SOURCES, DEFAULT_USER_AGENT};
use switch_core::platform::{proxy, Platform};
use switch_core::plugins::source::{assemble, skill_paths};
use switch_core::plugins::{GithubFetcher, RepoFetcher};

/// 内容源：内置的每一个都要真的答上来。
#[test]
#[ignore = "打真实外网：需要本机网络与代理可用"]
fn every_builtin_content_source_answers() {
    let platform = Platform::current();
    println!(
        "出网代理: {:?}",
        proxy::read_system_proxy(platform).outbound_endpoint()
    );
    let fetcher = HttpFeedFetcher::new();
    let mut failures = Vec::new();
    for (id, kind, url, label, _lang) in DEFAULT_SOURCES {
        // GitHub 搜索源的第 3 个字段是 today/week/month，不是地址：它走搜索接口，
        // 由 `every_builtin_skill_source_can_be_read` 之外的另一条路覆盖。
        if kind != switch_core::content::FeedKind::Rss {
            println!("· {label:<24} {id:<14} 跳过（非 RSS 源）");
            continue;
        }
        let outcome = fetcher.get(url, None, None, DEFAULT_USER_AGENT, None);
        match outcome {
            Ok(response) if (200..300).contains(&response.status) => {
                let items = response.body.matches("<item").count()
                    + response.body.matches("<entry").count();
                println!(
                    "✓ {label:<24} {id:<14} HTTP {} · {items} 条",
                    response.status
                );
                if items == 0 {
                    failures.push(format!("{label}: 200 但解析不出条目（{url}）"));
                }
            }
            Ok(response) => {
                println!("✗ {label:<24} {id:<14} HTTP {}", response.status);
                failures.push(format!("{label}: HTTP {}（{url}）", response.status));
            }
            Err(error) => {
                println!("✗ {label:<24} {id:<14} 抓取失败: {error:?}");
                failures.push(format!("{label}: 抓取失败（{url}）"));
            }
        }
    }
    assert!(failures.is_empty(), "有源没答上来：{failures:#?}");
}

/// 插件中心的预置来源：能解析出提交、列出文件、读到 SKILL.md。
#[test]
#[ignore = "打真实外网：需要本机网络与代理可用"]
fn every_builtin_skill_source_can_be_read() {
    let fetcher = GithubFetcher::new(
        std::env::var("SWITCHELP_GITHUB_TOKEN").ok(),
        DEFAULT_USER_AGENT.to_owned(),
    );
    let mut failures = Vec::new();
    for (repo, label, _description) in switch_core::plugins::DEFAULT_SOURCES {
        let commit = match fetcher.resolve_commit(repo, None) {
            Ok(commit) => commit,
            // 未认证的 GitHub 接口每小时 60 次，反复跑探针就会用尽。那是外部配额，
            // 不是产品缺陷——跳过并说明，但别的错照样算失败。
            Err(error) if error.message_key == "error.pluginRateLimited" => {
                println!("跳过：GitHub 未认证配额已用尽（填令牌后再跑）");
                return;
            }
            Err(error) => {
                println!("✗ {label:<26} 解析提交失败: {error:?}");
                failures.push(format!("{label}: 解析提交失败"));
                continue;
            }
        };
        match fetcher.list_blobs(repo, &commit) {
            Ok(blobs) => {
                let skills = skill_paths(&blobs);
                println!(
                    "✓ {label:<26} {repo:<20} 文件 {} · SKILL.md {} 个",
                    blobs.len(),
                    skills.len()
                );
                if skills.is_empty() {
                    failures.push(format!("{label}: 一个 SKILL.md 都没有"));
                    continue;
                }
                // 真的读一个：目录能列出来但正文读不到（raw 主机不通）正是要守的那种坏法。
                match fetcher.read_file(repo, &commit, &skills[0]) {
                    Ok(bytes) if !bytes.is_empty() => {
                        println!("    读到 {}（{} 字节）", skills[0], bytes.len());
                    }
                    Ok(_) => failures.push(format!("{label}: {} 是空文件", skills[0])),
                    Err(error) => {
                        println!("✗ 读不到正文: {error:?}");
                        failures.push(format!("{label}: 读不到 SKILL.md 正文"));
                    }
                }
            }
            Err(error) => {
                println!("✗ {label:<26} 列文件失败: {error:?}");
                failures.push(format!("{label}: 列文件失败"));
            }
        }
    }
    assert!(failures.is_empty(), "有来源读不出来：{failures:#?}");
}

/// 插件中心真正的入口：把仓库读成一份**技能目录**。
///
/// 前面那个用例只证明「能列文件、能读一个文件」；这一层才是界面按钮走的那条路
/// （`assemble` 会列出每个技能目录里的文件、拼出目录清单）。「正在读取仓库…」卡住的
/// 病根就在这里——`assemble` 之前要先把整棵 tree 与正文拉回来。
#[test]
#[ignore = "打真实外网：需要本机网络与代理可用"]
fn the_plugin_catalog_can_be_built_for_a_default_source() {
    let fetcher = GithubFetcher::new(
        std::env::var("SWITCHELP_GITHUB_TOKEN").ok(),
        DEFAULT_USER_AGENT.to_owned(),
    );
    let (repo, _label, _description) = switch_core::plugins::DEFAULT_SOURCES[0];
    // 未认证的 GitHub 接口每小时只有 60 次：反复跑这个探针会把它用尽。
    // 那是外部配额，不是缺陷——但只放过这一种错，别的错照样红。
    let rate_limited =
        |error: &switch_core::CoreError| error.message_key == "error.pluginRateLimited";
    let commit = match fetcher.resolve_commit(repo, None) {
        Ok(commit) => commit,
        Err(error) if rate_limited(&error) => {
            println!("跳过：GitHub 未认证配额已用尽（填令牌后再跑）");
            return;
        }
        Err(error) => panic!("解析提交失败: {error:?}"),
    };
    let blobs = match fetcher.list_blobs(repo, &commit) {
        Ok(blobs) => blobs,
        Err(error) if rate_limited(&error) => {
            println!("跳过：GitHub 未认证配额已用尽（填令牌后再跑）");
            return;
        }
        Err(error) => panic!("列文件失败: {error:?}"),
    };
    let started = std::time::Instant::now();
    let catalog = assemble(&fetcher, repo, &commit, &blobs, 0).expect("拼目录");
    println!(
        "{repo} @{} · 技能 {} 个 · 用时 {:.1}s",
        &commit[..8.min(commit.len())],
        catalog.skills.len(),
        started.elapsed().as_secs_f32()
    );
    for skill in catalog.skills.iter().take(3) {
        println!("  · {}（{} 个文件）", skill.source_path, skill.files.len());
    }
    assert!(!catalog.skills.is_empty(), "目录里必须至少有一个技能");
    // 界面上是「点了按钮几秒内出清单」，不是几分钟。给一个宽松但真实的上限。
    assert!(
        started.elapsed().as_secs() < 60,
        "拼目录用了 {:?}，界面上会像卡住",
        started.elapsed()
    );
}
