//! 桌面壳状态：核心服务、实例缓存与用户主目录的装配点。
//!
//! 壳只持有 `Arc` 与服务句柄，业务判定全部在 `switch-core`：这里不复制
//! 任何模型、供应商或事务规则，也不直接读写 Codex 配置。

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use switch_core::{
    application::{ApplyService, WorkspaceService},
    codex::backup::BackupStore,
    codex::detect::{CodexInstance, DetectInput, InstanceDetector, RealFs},
    diagnostics::{DiagnosticLog, Probes},
    domain::error::CoreError,
    gateway::Gateway,
};

/// 启动时的两个「可能失败」的装配结果。
///
/// 放在一个结构里而不是给 `new` 加参数：这两件事的失败都不致命（状态里保留原因、
/// 界面照实显示），把它们并成一个入参也让调用点的意图更清楚——启动装配有哪些可选项。
pub struct StartupParts {
    pub gateway: Result<Arc<Gateway>, CoreError>,
    pub bridge: Result<PathBuf, CoreError>,
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
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

impl DesktopState {
    pub fn new(
        workspace: Arc<WorkspaceService>,
        apply: Arc<ApplyService>,
        home: PathBuf,
        app_data_dir: PathBuf,
        parts: StartupParts,
        diagnostics: Arc<DiagnosticLog>,
        backups: Arc<BackupStore>,
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
        }
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
