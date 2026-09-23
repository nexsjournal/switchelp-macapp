//! 桌面壳状态：核心服务、实例缓存与用户主目录的装配点。
//!
//! 壳只持有 `Arc` 与服务句柄，业务判定全部在 `switch-core`：这里不复制
//! 任何模型、供应商或事务规则，也不直接读写 Codex 配置。

use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use switch_core::{
    application::{ApplyService, WorkspaceService},
    codex::backup::BackupStore,
    codex::detect::{CodexInstance, DetectInput, InstanceDetector, RealFs},
    content::{ContentService, FeedFetcher, HttpFeedFetcher},
    credentials::SecretVault,
    diagnostics::{DiagnosticLog, Probes},
    domain::error::CoreError,
    gateway::Gateway,
    platform::{proxy, Platform, ProcessProbe, SystemProcessProbe},
    plugins::PluginService,
    storage::HubStore,
    toolhub::ToolHubService,
};

/// 系统代理与回环地址的关系，以及我们就此做过的事。
///
/// 宿主到本机网关是 `http://127.0.0.1:<端口>`，而宿主自己的 HTTP 客户端会把系统代理
/// 套到这条请求上，代理又到不了用户的回环地址，于是回一个**空正文的 502**——
/// 在 Codex 里就显示成「unexpected status 502 Bad Gateway: Unknown error」。
/// 成因与依据见 `switch_core::platform::proxy` 的模块说明。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemProxyReport {
    /// 系统里有没有开 HTTP 代理。
    pub http_enabled: bool,
    /// 代理地址，形如 `127.0.0.1:7890`；取不到时为 null。
    pub endpoint: Option<String>,
    /// 「绕过回环」是否已经在生效。为真时，之后启动的 Codex 不会再被代理拦下。
    pub bypass_applied: bool,
}

/// 启动时的两个「可能失败」的装配结果。
///
/// 放在一个结构里而不是给 `new` 加参数：这两件事的失败都不致命（状态里保留原因、
/// 界面照实显示），把它们并成一个入参也让调用点的意图更清楚——启动装配有哪些可选项。
pub struct StartupParts {
    pub gateway: Result<Arc<Gateway>, CoreError>,
    pub bridge: Result<PathBuf, CoreError>,
    /// 工具清单。加载失败时整个工具管理板块为不可用，因此原因要保留到状态里。
    pub catalog: Result<Arc<ToolHubService>, CoreError>,
}

/// 一次进程生命周期内共享的壳状态。
pub struct DesktopState {
    pub workspace: Arc<WorkspaceService>,
    pub apply: Arc<ApplyService>,
    home: PathBuf,
    /// 应用数据目录：诊断包等本工具自己的产物写在这里。
    app_data_dir: PathBuf,
    /// 最近一次检测结果。窗口重开或页面刷新都复用这里，不要求用户重新检测。
    instances: Mutex<Vec<CodexInstance>>,
    /// 本机网关。绑定失败时为 None，失败原因单独保留——
    /// 界面必须看到“网关没起来”，而不是看到一个空状态。
    gateway: Option<Arc<Gateway>>,
    gateway_error: Option<String>,
    /// 随包分发的 bridge 有没有装好。失败原因单独保留：共存模式必须能如实说出
    /// 「为什么打不开」，而不是显示一个点了没反应的开关。
    bridge: Result<PathBuf, String>,
    /// 与网关共享的诊断日志：网关写、界面读。
    diagnostics: Arc<DiagnosticLog>,
    /// 连接与模型探测。持可取消集合，跨命令保持同一个实例。
    probes: Arc<Probes>,
    /// 配置备份存储。
    backups: Arc<BackupStore>,
    /// 扩展板块的元数据存储（工具探测缓存、已装技能、订阅源与资讯条目）。
    hub: Arc<dyn HubStore>,
    /// 工具管理。清单加载失败时保留原因，界面显示「清单不可用」而不是空表。
    tools: Result<Arc<ToolHubService>, String>,
    /// 插件中心。
    plugins: Arc<PluginService>,
    /// 抓取器与凭据库：内容服务每次按需装配，这样换了令牌立刻生效。
    feed_fetcher: Arc<dyn FeedFetcher>,
    vault: Arc<dyn SecretVault>,
    /// 系统代理的观察结果与已做的绕过。启动时算一次：代理是会话级设置，
    /// 进程生命周期里重算没有意义，只会让同一个事实在两个时刻显示成两句话。
    system_proxy: SystemProxyReport,
}

/// 宿主到本机网关是回环地址：绕过代理只能靠这两个环境变量，两个拼写都带上。
///
/// 不依赖「检测到代理」：代理可能是刚开的、也可能没读到，而多带一个只影响回环地址的
/// 例外在任何时候都不会有害。已有的值先合并进来，不覆盖用户自己的例外名单。
pub fn loopback_bypass_env(platform: Platform) -> Vec<(String, String)> {
    let merged = proxy::merge_bypass(
        proxy::session_bypass_value(platform).as_deref(),
        proxy::LOOPBACK_BYPASS,
    );
    vec![
        ("NO_PROXY".to_owned(), merged.clone()),
        ("no_proxy".to_owned(), merged),
    ]
}

/// 启动时把「绕过回环」安顿好，并留下一个可核实的观察结果。
///
/// 要做两件事，因为宿主有两条启动路径：我们重启它时会给它带上环境变量；而用户重启电脑后
/// 常常直接从程序坞打开 Codex，那条路不经过我们——那一份只能写进**登录会话**
/// （`launchctl setenv`，只影响当前登录会话，退出登录即失效，不写任何文件）。
///
/// 只有真的开着 HTTP 代理时才去动会话：没开代理的时候写这个变量没有任何收益，
/// 却会往用户的会话里多塞一个环境变量。写失败也不算错——重启宿主那条路仍然带着注入。
fn prepare_system_proxy() -> SystemProxyReport {
    let platform = Platform::current();
    let observed = proxy::read_system_proxy(platform);
    let mut bypass_applied = proxy::session_bypass_effective(platform, proxy::LOOPBACK_BYPASS);
    if observed.hijacks_loopback() && !bypass_applied {
        let probe = SystemProcessProbe;
        for spec in proxy::session_bypass(platform, proxy::LOOPBACK_BYPASS) {
            probe.spawn_detached(&spec);
        }
        // 重读会话，而不是相信 `launchctl setenv` 的退出码：界面要说的是「以后启动的
        // Codex 不会再被拦」，这句话只有读回来确实带上回环地址时才成立。
        bypass_applied = proxy::session_bypass_effective(platform, proxy::LOOPBACK_BYPASS);
    }
    SystemProxyReport {
        http_enabled: observed.hijacks_loopback(),
        endpoint: observed.endpoint(),
        bypass_applied,
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

impl DesktopState {
    /// 应用装配点：启动时把各个服务一次装齐。
    ///
    /// 参数多是这个位置的固有形态（每个板块一个依赖），所以显式放行那条 lint：
    /// 把其中几个塞进 `StartupParts` 只会让「谁装配谁」更难读，而不是更好读。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace: Arc<WorkspaceService>,
        apply: Arc<ApplyService>,
        home: PathBuf,
        app_data_dir: PathBuf,
        parts: StartupParts,
        diagnostics: Arc<DiagnosticLog>,
        backups: Arc<BackupStore>,
        hub: Arc<dyn HubStore>,
        plugins: Arc<PluginService>,
        vault: Arc<dyn SecretVault>,
    ) -> Self {
        let (gateway, gateway_error) = match parts.gateway {
            Ok(gateway) => (Some(gateway), None),
            Err(error) => (
                None,
                Some(
                    error
                        .safe_details
                        .first()
                        .cloned()
                        .unwrap_or_else(|| error.message_key.clone()),
                ),
            ),
        };
        let bridge = parts.bridge.map_err(|error| {
            error
                .safe_details
                .first()
                .cloned()
                .unwrap_or_else(|| error.message_key.clone())
        });
        let tools = parts.catalog.map_err(|error| {
            error
                .safe_details
                .first()
                .cloned()
                .unwrap_or_else(|| error.message_key.clone())
        });
        let system_proxy = prepare_system_proxy();
        Self {
            workspace,
            apply,
            home,
            app_data_dir,
            instances: Mutex::new(Vec::new()),
            gateway,
            gateway_error,
            bridge,
            diagnostics,
            probes: Arc::new(Probes::new()),
            backups,
            hub,
            tools,
            plugins,
            feed_fetcher: Arc::new(HttpFeedFetcher::new()),
            vault,
            system_proxy,
        }
    }

    /// 系统代理的观察结果与已做的绕过。
    pub fn system_proxy(&self) -> SystemProxyReport {
        self.system_proxy.clone()
    }

    /// 工具管理服务；清单没加载成功时给出原因。
    pub fn tools(&self) -> Result<&Arc<ToolHubService>, CoreError> {
        self.tools.as_ref().map_err(|reason| {
            CoreError::new(
                switch_core::ErrorCode::CatalogSchemaMismatch,
                "error.toolCatalogUnavailable",
            )
            .with_detail(reason.clone())
        })
    }

    pub fn plugins(&self) -> &Arc<PluginService> {
        &self.plugins
    }

    /// 按需装配内容服务：令牌从凭据库现读，改完立即生效。
    ///
    /// 抓取节奏不再可配：固定在本地 06:00 与 18:00 各一次（见
    /// `switch_core::content::DAILY_FETCH_HOURS`），时刻表由核心持有。
    pub fn content(&self) -> ContentService {
        ContentService::new(
            self.hub.clone(),
            self.feed_fetcher.clone(),
            self.github_token(),
        )
    }

    /// GitHub 令牌。取不到（凭据库未授权、条目不存在）时按「没有令牌」处理——
    /// 匿名访问本来就能用，只是频率低一些，不该因此让整个资讯页失败。
    pub fn github_token(&self) -> Option<String> {
        self.vault
            .load(switch_core::content::GITHUB_TOKEN_REF)
            .ok()
            .flatten()
            .filter(|token| !token.trim().is_empty())
            .or_else(|| {
                std::env::var("SWITCHELP_GITHUB_TOKEN")
                    .ok()
                    .filter(|token| !token.trim().is_empty())
            })
    }

    pub fn vault(&self) -> &Arc<dyn SecretVault> {
        &self.vault
    }

    /// 应用数据目录。托管 profile、bridge 与备份都在这儿。
    pub fn app_data_dir(&self) -> PathBuf {
        self.app_data_dir.clone()
    }

    /// 诊断包输出目录。
    pub fn exports_dir(&self) -> PathBuf {
        self.app_data_dir.join("exports")
    }

    pub fn diagnostics(&self) -> Arc<DiagnosticLog> {
        self.diagnostics.clone()
    }

    pub fn probes(&self) -> Arc<Probes> {
        self.probes.clone()
    }

    pub fn backups(&self) -> Arc<BackupStore> {
        self.backups.clone()
    }

    pub fn gateway(&self) -> Option<&Arc<Gateway>> {
        self.gateway.as_ref()
    }

    pub fn gateway_error(&self) -> Option<&str> {
        self.gateway_error.as_deref()
    }

    /// 已安装的 bridge 路径；没装成时给出原因。
    pub fn bridge(&self) -> Result<&PathBuf, &str> {
        self.bridge.as_ref().map_err(String::as_str)
    }

    /// 只读检测可用实例。显式路径优先，其余走平台候选。
    pub fn detect_instances(
        &self,
        explicit_path: Option<String>,
    ) -> Result<Vec<CodexInstance>, CoreError> {
        let mut input = DetectInput::for_macos(self.home.clone());
        if let Some(path) = explicit_path.filter(|value| !value.trim().is_empty()) {
            input.app_path = Some(PathBuf::from(path));
        }
        let found = InstanceDetector::detect(&input, &RealFs, None, now_unix())?;
        let mut cached = self
            .instances
            .lock()
            .map_err(|_| CoreError::internal("实例缓存锁不可用"))?;
        *cached = found.clone();
        Ok(found)
    }

    /// 按 id 取实例。缓存未命中时重新检测一次，而不是让用户重走检测流程。
    pub fn instance(&self, instance_id: &str) -> Result<CodexInstance, CoreError> {
        let cached = self
            .instances
            .lock()
            .map_err(|_| CoreError::internal("实例缓存锁不可用"))?
            .iter()
            .find(|instance| instance.id.as_str() == instance_id)
            .cloned();
        if let Some(instance) = cached {
            return Ok(instance);
        }
        self.detect_instances(None)?
            .into_iter()
            .find(|instance| instance.id.as_str() == instance_id)
            .ok_or_else(|| CoreError::not_found("Codex 实例"))
    }
}
