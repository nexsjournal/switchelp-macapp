//! 系统代理与回环地址的冲突：宿主为什么连不上本机网关。
//!
//! 现象：开着系统代理的用户在 Codex 里发消息，会稳定拿到
//! `unexpected status 502 Bad Gateway: Unknown error, url: http://127.0.0.1:<端口>/i/...`，
//! 而同一套 Key、同一套地址在别的客户端里好好的。
//!
//! 成因在**宿主自己的 HTTP 客户端**，不在网关：
//! - 宿主的 `base_url` 指向 `http://127.0.0.1:<端口>`（回环地址）；
//! - 宿主的客户端（reqwest）在 macOS 上经 SystemConfiguration 读系统代理，而
//!   **不读例外列表**——macOS 的例外列表里本来就有 `127.0.0.1`，照样拦不住；
//! - 于是这个请求被交给代理；代理按规则把它转给远端节点，远端当然连不到用户自己的
//!   127.0.0.1，回一个**空正文的 502**，宿主把空正文渲染成 `Unknown error`。
//!
//! 唯一可靠的杠杆是环境变量：宿主认 `NO_PROXY`。所以这里只做两件事——**观察**系统代理
//! （给界面一个可核实的事实，而不是让用户对着 502 猜），以及产出「绕过回环」的两个变量。
//! 网关侧没有可修的余地：那个请求根本没到过网关，网关也不会返回 502（见 `gateway::server`
//! 的状态码映射）。

use std::process::Command;

use super::{CommandSpec, Platform};

/// 必须绕过代理的主机。本机网关只绑回环地址，这两个名字是它的全部写法。
pub const LOOPBACK_BYPASS: &str = "127.0.0.1,localhost";

/// 系统代理的观察结果。
///
/// 只取 HTTP 一侧：宿主到本机网关是 `http://`，`HTTPSProxy` 影响不到它。把两者混成一个
/// `enabled` 会让界面在不相关的场景里报错，所以宁可窄一点。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemProxy {
    pub http_enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl SystemProxy {
    /// 系统代理会把发往回环地址的请求也带走吗？
    ///
    /// 只看「HTTP 代理是否启用」——**不看例外列表**。这不是偷懒：例外列表里有
    /// `127.0.0.1`（macOS 默认就有）也不起作用，宿主的客户端根本不读它。实测见本模块测试里
    /// 那段真实的 `scutil --proxy` 输出：例外里有 127.0.0.1，宿主仍然把请求发给了代理。
    pub fn hijacks_loopback(&self) -> bool {
        self.http_enabled
    }

    /// 给界面看的代理地址，形如 `127.0.0.1:7890`。
    pub fn endpoint(&self) -> Option<String> {
        let host = self.host.as_deref().filter(|value| !value.is_empty())?;
        Some(match self.port {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        })
    }
}

/// 解析 `scutil --proxy` 的输出。
///
/// 不解析那段 `<dictionary>` 的嵌套结构，只按行找三个扁平键：`scutil` 的输出不是
/// 机器接口，但它这三个键的写法多年没变，而系统里也没有第二个不用绑定 SystemConfiguration
/// 就能读到「当前生效代理」的地方。键必须**整词匹配**：`HTTPSProxy` 以 `HTTP` 开头，
/// 用 `starts_with` 会把 https 的代理当成 http 的。
pub fn parse_scutil_proxy(text: &str) -> SystemProxy {
    let mut proxy = SystemProxy::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "HTTPEnable" => proxy.http_enabled = value == "1",
            "HTTPProxy" => proxy.host = Some(value.to_owned()),
            "HTTPPort" => proxy.port = value.parse::<u16>().ok(),
            _ => {}
        }
    }
    proxy
}

/// 读当前生效的系统代理。
///
/// macOS 走 `scutil --proxy`。走绝对路径 `/usr/sbin/scutil`：GUI 进程的 `PATH` 未必含
/// `/usr/sbin`，而这是我们在**没有代理的情况下也要正确回答「有没有代理」**的地方，
/// 不能因为查不到就当成没开。
///
/// 其余平台只看环境变量：Windows 的 WinINET 与 Linux 的桌面代理设置都不是宿主会读的来源
/// （宿主只认 `http_proxy` 一类环境变量），拿系统设置去判断反而会说错。
pub fn read_system_proxy(platform: Platform) -> SystemProxy {
    if platform != Platform::Macos {
        let value = std::env::var("HTTP_PROXY")
            .or_else(|_| std::env::var("http_proxy"))
            .ok();
        return from_env_proxy(value.as_deref());
    }
    let program = if std::path::Path::new("/usr/sbin/scutil").exists() {
        "/usr/sbin/scutil"
    } else {
        "scutil"
    };
    let output = Command::new(program).arg("--proxy").output();
    match output {
        Ok(output) if output.status.success() => {
            parse_scutil_proxy(&String::from_utf8_lossy(&output.stdout))
        }
        // 查不到就说查不到：当作「没开」会让界面在一个我们其实不知道的状态上下结论。
        _ => SystemProxy::default(),
    }
}

/// 从 `http_proxy` 环境变量解析代理地址。`http://127.0.0.1:7890` 与 `127.0.0.1:7890`
/// 两种写法都认（前者是惯例，后者在代理软件生成的配置里也常见）。
pub fn from_env_proxy(value: Option<&str>) -> SystemProxy {
    let Some(raw) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return SystemProxy::default();
    };
    // 去掉 scheme 与可能的凭据段：我们只要地址，用来给用户看是哪条代理。
    let without_scheme = raw.split_once("://").map_or(raw, |(_, rest)| rest);
    let authority = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .rsplit('@')
        .next()
        .unwrap_or(without_scheme);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok()),
        None => (authority, None),
    };
    SystemProxy {
        http_enabled: !host.is_empty(),
        host: (!host.is_empty()).then(|| host.to_owned()),
        port,
    }
}

/// 把「绕过回环」并进已有的 `NO_PROXY`。
///
/// 不覆盖用户原有的值：他们的例外名单里可能还有内网域名，替换掉等于替他们改了代理行为。
/// 我们只往里加两条，加过就不重复。
pub fn merge_bypass(existing: Option<&str>, hosts: &str) -> String {
    let mut entries: Vec<String> = existing
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    for host in hosts.split(',').map(str::trim).filter(|v| !v.is_empty()) {
        if !entries.iter().any(|entry| entry.eq_ignore_ascii_case(host)) {
            entries.push(host.to_owned());
        }
    }
    entries.join(",")
}

/// 让**之后启动的**宿主也拿到绕过。
///
/// 为什么光注入启动参数不够：宿主被本工具重启时才拿到我们的环境变量，而用户重启电脑后
/// 常常直接从程序坞打开 Codex，那条路不经过本工具。macOS 的 GUI 进程由 launchd 启动，
/// 环境来自登录会话本身，所以这里改的是会话（`launchctl setenv`）。
///
/// 代价与边界：只影响当前登录会话，退出登录即失效；不写任何文件、不需要授权、不碰
/// 用户已有的值（值已在调用方合并好）。两个拼写都设置，因为不同客户端认不同的那个。
pub fn session_bypass(platform: Platform, merged: &str) -> Vec<CommandSpec> {
    if platform != Platform::Macos {
        return Vec::new();
    }
    ["NO_PROXY", "no_proxy"]
        .iter()
        .map(|key| CommandSpec {
            program: "launchctl".to_owned(),
            args: vec!["setenv".to_owned(), (*key).to_owned(), merged.to_owned()],
            env: Vec::new(),
        })
        .collect()
}

/// 登录会话里当前的 `NO_PROXY`。取不到时返回 `None`（调用方按「原来是空的」处理）。
pub fn session_bypass_value(platform: Platform) -> Option<String> {
    if platform != Platform::Macos {
        return std::env::var("NO_PROXY")
            .or_else(|_| std::env::var("no_proxy"))
            .ok();
    }
    let output = Command::new("launchctl")
        .args(["getenv", "NO_PROXY"])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// 绕过是否**已经**生效：会话里的值真的含这些主机。
///
/// 重读会话而不是相信 `launchctl setenv` 的退出码：界面要说的是「以后启动的 Codex 不会再
/// 被代理拦下」，这句话只有在读回来确实含回环地址时才成立。
pub fn session_bypass_effective(platform: Platform, hosts: &str) -> bool {
    match session_bypass_value(platform) {
        Some(value) => hosts
            .split(',')
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .all(|host| {
                value
                    .split(',')
                    .map(str::trim)
                    .any(|entry| entry.eq_ignore_ascii_case(host))
            }),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实输出（本机开着系统代理时抓的）。只有例外列表里那条私网段换成了 RFC 5737 的
    /// 文档地址——发布前扫描不允许私网地址入库，而它与本用例无关。
    /// 注意例外列表里**有** 127.0.0.1——它正是拦不住的那一环。
    const SCUTIL_WITH_PROXY: &str = r#"<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
    1 : localhost
    2 : 127.0.0.1
    3 : 198.51.100.0/24
  }
  ExcludeSimpleHostnames : 0
  HTTPEnable : 1
  HTTPPort : 7890
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7890
  HTTPSProxy : 127.0.0.1
  ProxyAutoConfigEnable : 0
  SOCKSEnable : 1
  SOCKSPort : 7890
  SOCKSProxy : 127.0.0.1
}"#;

    #[test]
    fn parse_reads_the_http_proxy_and_ignores_the_https_one() {
        let proxy = parse_scutil_proxy(SCUTIL_WITH_PROXY);
        assert!(proxy.http_enabled);
        assert_eq!(proxy.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(proxy.port, Some(7890));
        assert_eq!(proxy.endpoint().as_deref(), Some("127.0.0.1:7890"));
    }

    /// 这是整个模块的依据：macOS 的例外列表默认就含 127.0.0.1，而宿主的客户端不读它。
    /// 如果哪天有人把 `hijacks_loopback` 改成「例外里有回环就返回 false」，这条会红。
    #[test]
    fn loopback_is_still_hijacked_even_though_the_exception_list_names_it() {
        assert!(SCUTIL_WITH_PROXY.contains("127.0.0.1"));
        assert!(parse_scutil_proxy(SCUTIL_WITH_PROXY).hijacks_loopback());
    }

    #[test]
    fn disabled_or_absent_http_proxy_is_not_a_hijack() {
        let off = "<dictionary> {\n  HTTPEnable : 0\n  HTTPPort : 7890\n  HTTPProxy : 127.0.0.1\n}";
        assert!(!parse_scutil_proxy(off).hijacks_loopback());
        assert!(!parse_scutil_proxy("").hijacks_loopback());
    }

    /// 只开 SOCKS / 只开 HTTPS 时不该报——宿主到本机网关是 http://。
    #[test]
    fn https_only_configuration_is_not_reported_as_http_hijack() {
        let https_only =
            "<dictionary> {\n  HTTPSEnable : 1\n  HTTPSPort : 7890\n  HTTPSProxy : 127.0.0.1\n}";
        let proxy = parse_scutil_proxy(https_only);
        assert!(!proxy.hijacks_loopback());
        assert!(!proxy.http_enabled, "HTTPSProxy 不能被当成 HTTPProxy");
    }

    #[test]
    fn merge_keeps_existing_entries_and_adds_ours_once() {
        assert_eq!(merge_bypass(None, LOOPBACK_BYPASS), "127.0.0.1,localhost");
        assert_eq!(
            merge_bypass(Some("*.internal, 198.51.100.0/24"), LOOPBACK_BYPASS),
            "*.internal,198.51.100.0/24,127.0.0.1,localhost"
        );
        // 已经有了就不重复追加：这个函数会被反复调用（每次重启宿主一次）。
        assert_eq!(
            merge_bypass(Some("127.0.0.1,localhost"), LOOPBACK_BYPASS),
            "127.0.0.1,localhost"
        );
        assert_eq!(
            merge_bypass(Some("LOCALHOST"), LOOPBACK_BYPASS),
            "LOCALHOST,127.0.0.1"
        );
    }

    #[test]
    fn session_bypass_sets_both_spellings_on_macos_only() {
        let specs = session_bypass(Platform::Macos, "127.0.0.1,localhost");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].program, "launchctl");
        assert_eq!(
            specs[0].args,
            vec!["setenv", "NO_PROXY", "127.0.0.1,localhost"]
        );
        assert_eq!(
            specs[1].args,
            vec!["setenv", "no_proxy", "127.0.0.1,localhost"]
        );
        // 别的平台宿主只看进程环境，动会话没有意义。
        assert!(session_bypass(Platform::Windows, LOOPBACK_BYPASS).is_empty());
        assert!(session_bypass(Platform::Linux, LOOPBACK_BYPASS).is_empty());
    }

    #[test]
    fn env_proxy_accepts_every_shape_the_proxy_clients_write() {
        let parsed = from_env_proxy(Some("http://127.0.0.1:7890"));
        assert!(parsed.hijacks_loopback());
        assert_eq!(parsed.endpoint().as_deref(), Some("127.0.0.1:7890"));

        // 不带 scheme、带凭据、带路径：都只取地址那一段。
        assert_eq!(
            from_env_proxy(Some("127.0.0.1:7897")).endpoint().as_deref(),
            Some("127.0.0.1:7897")
        );
        assert_eq!(
            from_env_proxy(Some("http://user:pass@198.51.100.7:3128/"))
                .endpoint()
                .as_deref(),
            Some("198.51.100.7:3128")
        );

        // 没设 / 空串 / 只有 scheme：都算没开，不能凭空报一条「代理拦住了你」。
        assert!(!from_env_proxy(None).hijacks_loopback());
        assert!(!from_env_proxy(Some("   ")).hijacks_loopback());
        assert!(!from_env_proxy(Some("http://")).hijacks_loopback());
    }
}
