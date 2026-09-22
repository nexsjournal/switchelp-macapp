//! 配置应用编排：计划 → CAS → 原子提交 → 等待宿主重载。
//!
//! 规则来自 [配置生命周期](../../../../docs/architecture/02-configuration-lifecycle.md)：
//! 计划不可变且有 TTL；提交前必须做 CAS；没有宿主回执最多到 `AwaitingReload`，
//! 绝不把“本工具保存成功”当成宿主已加载；目录修订与运行策略修订分开发布。
//! 任何阶段都不允许把上游 Key 写进 Codex 配置。

use crate::{
    codex::{
        backup::BackupStore,
        catalog::{CatalogCompiler, CompileOptions},
        coexist,
        config::{
            apply_managed, diff_managed, execute_restore, hash, plan_restore, write_atomic,
            FieldChange, FieldOwnership, ManagedConfig, ManagedProvider, ProviderAuth, PROVIDER_ID,
        },
        detect::CodexInstance,
        plan::{
            build_plan, check_cas, decide_recovery, ApplyOperation, ApplyPlan, ApplyStage,
            CasOutcome, RecoveryDecision, DEFAULT_PLAN_TTL_SECS,
        },
    },
    domain::{
        credential::Credential,
        error::{CoreError, ErrorCode},
        ids::{CredentialId, InstanceId, OperationId, PlanId, RevisionId},
        model::{HostState, Model, ModelLifecycle},
        provider::{AuthKind, Protocol, Provider},
    },
    gateway::{self, GatewayRouter},
    protocols::CHAT_COMPLETIONS_V1,
    storage::{
        operation::{OperationKind, OperationState, PreparedDeployment},
        snapshot::{RouteEntry, RouteSnapshot, RuntimePublication},
        OperationStore, Repository,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// 时间来源。注入后可用确定性时间测试 TTL 与事件顺序。
pub trait Clock: Send + Sync {
    fn now_unix(&self) -> i64;
    fn now(&self) -> String;
}

/// 真实系统时钟。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> i64 {
        time::OffsetDateTime::now_utc().unix_timestamp()
    }

    fn now(&self) -> String {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .expect("UTC 可表示为 RFC3339")
    }
}

/// 当前已生效的配置摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedSummary {
    pub operation_id: String,
    pub instance_id: String,
    /// 已发布的目录修订。
    pub catalog_revision: String,
    /// 受管配置里的默认模型；未记录时为空（不会用当前表单值顶替）。
    pub default_model: Option<String>,
    /// 本次目录包含的 alias 数量。
    pub alias_count: usize,
    /// 事务阶段。`AwaitingReload` 表示已提交但尚未确认宿主已加载。
    pub stage: ApplyStage,
    /// 应用时间（取自事务记录）。
    pub applied_at: String,
}

/// 本机网关与目录的布局。端口与 app 数据目录由装配层探测后注入。
#[derive(Debug, Clone)]
pub struct GatewayLayout {
    pub app_data_dir: PathBuf,
    /// 网关监听端口；文档明确默认值不是强制端口。
    pub port: u16,
    /// auth helper 绝对路径。helper 只输出本机网关令牌，不输出上游 Key。
    pub auth_helper: String,
    /// 目录 `base_instructions`；必须由调用方显式提供。
    pub base_instructions: String,
}

impl GatewayLayout {
    /// 目录修订文件路径：`<appData>/catalogs/<revision>/models.json`。
    pub fn catalog_path(&self, catalog_revision: &str) -> PathBuf {
        self.app_data_dir
            .join("catalogs")
            .join(catalog_revision)
            .join("models.json")
    }

    /// 写入 Codex 配置的 `base_url`：本机网关 + 本实例目录前缀。
    pub fn base_url(&self, instance_id: &InstanceId, catalog_revision: &str) -> String {
        let prefix = RuntimePublication::build_prefix(instance_id, catalog_revision);
        format!("{}{}/v1", gateway::origin(self.port), prefix)
    }

    /// 目录修订 ID。由目录内容摘要派生，内容相同即同一修订。
    fn catalog_revision(&self, catalog_hash: &str) -> String {
        format!("rev_{}", &catalog_hash[..16.min(catalog_hash.len())])
    }

    /// 某个修订的目录目录（`<appData>/catalogs/<revision>`）。
    fn catalog_dir(&self, catalog_revision: &str) -> PathBuf {
        self.app_data_dir.join("catalogs").join(catalog_revision)
    }
}

/// 保留的目录版本代数：当前版本 + 上一代。
///
/// 为什么要留一代：宿主只在启动时读配置，刚应用完的那一瞬间它还带着旧前缀在跑，
/// 旧快照必须继续服务到它真的重启为止（这是 `routing.rs` 写的「旧宿主仍带旧前缀时
/// 按旧快照服务」）。留一代足够覆盖这个窗口，又不会像过去那样**无限累积**——
/// 生产路径上 `retain`/`release`/`retire` 从来没被调用过，本机实测已经堆了 3 份旧目录。
const RETAINED_CATALOG_GENERATIONS: usize = 2;

/// 启动恢复的判定结果，供 UI 说明“为什么还停在等待状态”。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub operation_id: String,
    pub instance_id: String,
    pub decision: RecoveryDecision,
    /// 已按判定执行的修复动作；无需处理时为空。
    pub applied: bool,
}

/// 应用编排服务。壳层只做装配与 DTO 映射，业务判定全部在这里。
pub struct ApplyService {
    /// 提交前自动备份配置文件。未注入时不备份（测试与探针走这条路径）。
    backups: Option<Arc<BackupStore>>,
    repository: Arc<dyn Repository>,
    operations: Arc<dyn OperationStore>,
    router: Arc<GatewayRouter>,
    layout: GatewayLayout,
    clock: Arc<dyn Clock>,
    commits: Mutex<()>,
}

impl ApplyService {
    /// 注入备份存储：之后每次提交前都会先留一份原样副本。
    pub fn with_backups(mut self, backups: Arc<BackupStore>) -> Self {
        self.backups = Some(backups);
        self
    }

    pub fn new(
        repository: Arc<dyn Repository>,
        operations: Arc<dyn OperationStore>,
        router: Arc<GatewayRouter>,
        layout: GatewayLayout,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            backups: None,
            repository,
            operations,
            router,
            layout,
            clock,
            commits: Mutex::new(()),
        }
    }

    pub fn layout(&self) -> &GatewayLayout {
        &self.layout
    }

    /// 已保存的运行发布；未应用过时为空。
    pub fn publication(&self, operation_id: &str) -> Result<Option<RuntimePublication>, CoreError> {
        Ok(self.status(operation_id)?.publication)
    }

    pub fn status(&self, operation_id: &str) -> Result<OperationState, CoreError> {
        self.operations
            .get(operation_id)?
            .ok_or_else(|| CoreError::not_found("配置事务"))
    }

    pub fn plan(&self, plan_id: &str) -> Result<ApplyPlan, CoreError> {
        self.operations
            .find_by_plan(plan_id)?
            .map(|state| state.plan)
            .ok_or_else(|| CoreError::not_found("应用计划"))
    }

    /// 需要启动恢复的记录。
    pub fn unfinished(&self) -> Result<Vec<OperationState>, CoreError> {
        self.operations.unfinished()
    }

    /// 生成应用计划。只读检测 + 写目录草稿，不修改 Codex 配置。
    pub fn plan_apply(
        &self,
        instance: &CodexInstance,
        default_alias: Option<&str>,
    ) -> Result<ApplyPlan, CoreError> {
        let models = self.repository.list_models()?;
        let providers: HashMap<String, Provider> = self
            .repository
            .list_providers()?
            .into_iter()
            .map(|provider| (provider.id.as_str().to_owned(), provider))
            .collect();
        let credentials = self.credentials_by_id()?;

        let selected: Vec<Model> = models
            .iter()
            .filter(|m| m.in_catalog && m.lifecycle != ModelLifecycle::Disabled)
            .cloned()
            .collect();
        if selected.is_empty() {
            return Err(CoreError::validation(
                "没有已纳入 Codex 的模型；先在模型页勾选要应用的模型",
            )
            .with_recovery("openModels", "action.openModels"));
        }

        // 路由前提：每个纳入目录的模型都必须有可用的供应商路由与 Key 引用。
        let blockers = self.route_blockers(&selected, &providers, &credentials);
        if !blockers.is_empty() {
            return Err(CoreError::validation(blockers.join("；")));
        }

        // 应用阶段要求上下文已声明，不允许用保守值伪装真实上限。
        let options = CompileOptions {
            require_context: true,
            conservative_context_fallback: None,
            base_instructions: self.layout.base_instructions.clone(),
        };
        let compiled = CatalogCompiler::compile(&selected, &options)?;
        let catalog_bytes = compiled.to_json_bytes()?;
        let catalog_hash = hash(&String::from_utf8_lossy(&catalog_bytes));
        // 目录版本必须覆盖「本次要服务的整套状态」，不只是目录文件本身。
        //
        // 路由快照里还含凭据版本、协议、输出上限与模态；这些变了而目录没变时，版本号若保持
        // 不变，`router.publish` 的不可变守卫会以 `error.catalogRevisionConflict` 拒发——
        // 表现就是「换了 Key / 改过策略之后再点应用，界面闪一下，Codex 配置一行都没改」。
        // 把来源摘要一并算进版本号，任何会影响服务的改动都会得到一个真正的新版本。
        let source_hash = source_hash(&selected, &providers, &credentials)?;
        let catalog_revision = self.layout.catalog_revision(&hash(&format!(
            "{}:{}:{}",
            instance.id, catalog_hash, source_hash
        )));
        let catalog_path = self.layout.catalog_path(&catalog_revision);

        let aliases: Vec<String> = compiled
            .catalog
            .models
            .iter()
            .map(|entry| entry.slug.clone())
            .collect();
        let default_alias = match default_alias {
            Some(alias) => {
                if !aliases.iter().any(|a| a == alias) {
                    return Err(CoreError::validation("默认模型不在本次目录中"));
                }
                alias.to_owned()
            }
            None => aliases[0].clone(),
        };

        let managed =
            self.managed_config(instance, &default_alias, &catalog_path, &catalog_revision);
        let snapshot = crate::codex::config::ConfigSnapshot::read(&instance.config_file)?;
        let changes = diff_managed(&snapshot, &managed);
        let mut warnings: Vec<String> = compiled
            .warnings
            .iter()
            .map(|warning| format!("{}：{}", warning.message_key, warning.detail))
            .collect();
        // Chat Completions 适配尚未通过工具调用门禁（PRD 对这条路径的要求是
        // 「未通过则明确标实验状态，不能冒充完整可用」）。它必须在**应用之前**说出来：
        // 用户是在这一步决定要不要让 Codex 走这条路，等到请求失败才发现就晚了。
        if selected
            .iter()
            .filter(|model| model.in_catalog)
            .any(|model| {
                let protocol = model.protocol_override.unwrap_or_else(|| {
                    providers
                        .get(model.provider_id.as_str())
                        .map(|provider| provider.protocol)
                        .unwrap_or(Protocol::Responses)
                });
                protocol_id(protocol) == CHAT_COMPLETIONS_V1
            })
        {
            warnings.push(
                "warning.chatAdapterExperimental：本次有模型走 Chat Completions 适配。                 它会把上游的 chat/completions 双向翻译成 Responses，但工具调用尚未通过门禁、                 没有在真实上游上验证过；文本对话可用，工具与结构化输出可能不可用。"
                    .to_owned(),
            );
        }

        let revision_id = RevisionId::new(catalog_revision.clone());
        let plan = build_plan(
            PlanId::generate(),
            instance.id.clone(),
            revision_id,
            instance.config_file.clone(),
            snapshot.content_hash.clone(),
            snapshot.existed,
            changes,
            catalog_revision.clone(),
            aliases,
            warnings,
            self.clock.now_unix(),
            DEFAULT_PLAN_TTL_SECS,
        );

        // 计划阶段只落库草稿与目录内容；此时不写 Codex 配置。
        self.write_catalog(&catalog_path, &catalog_bytes)?;
        let mut operation =
            ApplyOperation::new(OperationId::generate(), &plan, "", self.clock.now());
        let now = self.clock.now();
        operation.transition(ApplyStage::Validating, now.clone())?;
        operation.transition(ApplyStage::Prepared, now)?;
        let prepared = PreparedDeployment {
            source_hash,
            managed,
            routes: self.build_routes(&plan, &selected)?,
            models: selected,
        };
        self.operations.save(OperationState {
            kind: OperationKind::Apply,
            operation,
            plan: plan.clone(),
            ownership: self.operations.ownership(&instance.id)?,
            catalog_path: catalog_path.display().to_string(),
            catalog_hash,
            publication: None,
            prepared: Some(prepared),
        })?;
        Ok(plan)
    }

    /// 共存模式下真正被写的那份实例：配置根换成应用数据目录里的托管 home。
    ///
    /// 用户真实的 `~/.codex` 因此一个字节都不动——这正是共存模式相对「替换菜单」的全部
    /// 区别：官方模型走原生那根，登录态与历史都在原地。
    pub fn coexist_instance(&self, base: &CodexInstance) -> CodexInstance {
        coexist::managed_instance(base, &self.layout.app_data_dir)
    }

    /// 共存模式的应用计划：目标换成托管 home，其余走同一条管线。
    ///
    /// 首次规划时先把用户当前的配置**复制**一份到托管 home 作为底子：插件、市场、
    /// 项目信任这些设置是用户的 Codex 体验的一部分，共存模式没有理由把它清空。
    /// 复制时去掉我们自己写过的路由字段——那份路由属于原生配置，不属于托管 profile。
    pub fn plan_coexist(
        &self,
        base: &CodexInstance,
        default_alias: Option<&str>,
    ) -> Result<ApplyPlan, CoreError> {
        self.seed_managed_config(base)?;
        let instance = self.coexist_instance(base);
        self.plan_apply(&instance, default_alias)
    }

    /// 托管 home 还没有配置时，从用户的配置复制一份作为底子。
    fn seed_managed_config(&self, base: &CodexInstance) -> Result<(), CoreError> {
        let target = coexist::managed_config_file(&self.layout.app_data_dir);
        if target.exists() {
            return Ok(());
        }
        let platform = crate::platform::Platform::current();
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| CoreError::internal("无法创建托管配置目录"))?;
            // 托管 profile 与用户自己的配置同级敏感：里面有他自己的项目信任、
            // MCP 设置等。目录与文件都按私密权限落盘，不靠「反正父目录是 0700」。
            let _ = crate::platform::restrict(parent, crate::platform::private_dir_mode(platform));
        }
        let source = PathBuf::from(&base.config_file);
        if !source.exists() {
            // 用户还没配置过 Codex：那就从空白开始，没什么可继承的。
            return Ok(());
        }
        let text = std::fs::read_to_string(&source)
            .map_err(|_| CoreError::internal("无法读取原生配置以复制到托管 profile"))?;
        let stripped = crate::codex::config::strip_managed_keys(&text)?;
        std::fs::write(&target, stripped)
            .map_err(|_| CoreError::internal("无法写入托管 profile"))?;
        let _ = crate::platform::restrict(&target, crate::platform::private_file_mode(platform));
        Ok(())
    }

    /// 共生模式是否已经生效这件事由调用方判断；这里只负责读意图。
    pub fn coexist_enabled(&self) -> Result<bool, CoreError> {
        Ok(self
            .repository
            .setting(coexist::SETTING_ENABLED)?
            .as_deref()
            == Some("1"))
    }

    /// 记录共存模式的意图（开关）。事实由宿主进程与 bridge 日志决定，不由这个值决定。
    pub fn set_coexist_enabled(&self, enabled: bool) -> Result<(), CoreError> {
        self.repository
            .set_setting(coexist::SETTING_ENABLED, if enabled { "1" } else { "0" })
    }

    /// 提交计划。CAS 失败或计划过期都必须拒绝，不能拿旧计划硬写。
    pub fn execute_apply(
        &self,
        plan_id: &str,
        plan_hash: &str,
        idempotency_key: &str,
    ) -> Result<String, CoreError> {
        let _commit_guard = self
            .commits
            .lock()
            .map_err(|_| CoreError::internal("提交锁不可用"))?;
        let mut state = self.plan_state(plan_id, plan_hash)?;
        if state.kind != OperationKind::Apply {
            return Err(CoreError::validation("该计划不是应用计划"));
        }
        if let Some(id) = self.check_execution(&state, idempotency_key)? {
            return Ok(id);
        }
        if state.plan.is_expired(self.clock.now_unix()) {
            state
                .operation
                .transition(ApplyStage::Blocked, self.clock.now())?;
            self.operations.save(state)?;
            return Err(
                CoreError::new(ErrorCode::ConfigChanged, "error.planExpired")
                    .with_detail("计划已过期，需要重新生成差异".to_owned())
                    .with_recovery("replan", "action.replan"),
            );
        }

        let snapshot = crate::codex::config::ConfigSnapshot::read(&state.plan.config_path)?;
        let cas = check_cas(
            &state.plan.expected_config_hash,
            state.plan.expected_config_exists,
            &snapshot.content_hash,
            snapshot.existed,
        );
        if !cas.is_match() {
            state
                .operation
                .transition(ApplyStage::Blocked, self.clock.now())?;
            state.operation.stage = ApplyStage::Conflict;
            state
                .operation
                .push_event(ApplyStage::Conflict, HashMap::new(), self.clock.now());
            self.operations.save(state)?;
            return Err(describe_cas_failure(cas));
        }

        let prepared = state.prepared.clone().ok_or_else(|| {
            CoreError::conflict("error.planRequiresRefresh")
                .with_detail("该计划缺少冻结的模型与路由，请重新预览".to_owned())
        })?;
        if self.current_source_hash()? != prepared.source_hash {
            return Err(
                CoreError::new(ErrorCode::ConfigChanged, "error.modelChanged")
                    .with_detail("预览后供应商、Key 或模型已变化，请重新生成差异".to_owned()),
            );
        }
        let bytes = std::fs::read(&state.catalog_path)
            .map_err(|_| CoreError::internal("目录文件在提交前不可读取"))?;
        if hash(&String::from_utf8_lossy(&bytes)) != state.catalog_hash {
            return Err(CoreError::conflict("error.catalogChanged")
                .with_detail("目录文件在预览后被修改".to_owned()));
        }
        let (text, ownership) = apply_managed(&snapshot, &prepared.managed, &state.ownership)?;
        state.operation.idempotency_key = idempotency_key.to_owned();
        state.operation.written_hash = Some(hash(&text));
        state.ownership = ownership;
        state.publication = Some(self.publication_for(&state, &prepared.managed)?);
        state
            .operation
            .transition(ApplyStage::Committing, self.clock.now())?;
        self.operations.save(state.clone())?; // 写前日志：崩溃后可根据目标摘要补记。
                                              // 写之前先备份原文件。备份失败就不写：宁可不提交，也不能在没有退路时改用户配置。
                                              // 备份**排在发布之前**：它失败时什么都没发生过，用户配置与路由都保持原样。
                                              // 托管 home 里的配置是我们自己生成的，没有用户内容可备份；
                                              // 把它塞进用户的备份列表，还会让「还原」页出现一个来源不明的条目。
        let own_generated_file =
            Path::new(&state.plan.config_path).starts_with(&self.layout.app_data_dir);
        if let Some(backups) = &self.backups {
            if !own_generated_file && Path::new(&state.plan.config_path).exists() {
                backups.create(
                    Path::new(&state.plan.config_path),
                    self.clock.now_unix() * 1000,
                    &self.clock.now(),
                )?;
            }
        }
        // 路由发布失败必须发生在修改 Codex 之前；发布的是冻结输入，不读取当前表单值。
        self.router.publish(prepared.routes.clone())?;
        // 写盘失败要把刚发布的版本收回去：路由器是内存态，进程活着它就会一直服务一个
        // 「配置文件里并不存在」的目录版本，而且下次应用的版本号与它对不上——表现是网关
        // 为一个没人指向的版本服务。收不回来（仍有在途引用）时如实写在错误里，不假装已清理。
        if let Err(error) = write_atomic(Path::new(&state.plan.config_path), &text) {
            let reclaimed = self.router.retire(&state.plan.catalog_revision);
            let revision = state.plan.catalog_revision.clone();
            state
                .operation
                .transition(ApplyStage::Failed, self.clock.now())?;
            self.operations.save(state)?;
            return Err(if reclaimed {
                error
            } else {
                error.with_detail(format!(
                    "配置写入失败，且刚发布的目录版本 {revision} 仍有在途引用、未能回收"
                ))
            });
        }
        state
            .operation
            .transition(ApplyStage::AwaitingReload, self.clock.now())?;
        let operation_id = state.operation.id.as_str().to_owned();
        let published_revision = state.plan.catalog_revision.clone();
        self.operations.save(state)?;
        self.mark_models_awaiting_reload(&prepared.models)?;
        self.prune_catalog_revisions(&published_revision);
        Ok(operation_id)
    }

    /// 回收超出保留代数的目录版本：内存里的路由快照 + 磁盘上的目录文件。
    ///
    /// 生产路径上 `retain`/`release`/`retire` 过去从来没被调用过，于是每应用一次就永久
    /// 多一份目录（本机实测堆了 3 份）。这里的判定顺序是刻意的：
    /// 1. 当前版本永远保留；
    /// 2. **仍有引用**（在途请求或续接绑定）的版本一律保留——`retire` 自己也会拒绝，
    ///    这里先筛一遍是为了不把「本该保留」和「没能回收」混为一谈；
    /// 3. 剩下的按目录的修改时间排序，只留最近 `RETAINED_CATALOG_GENERATIONS - 1` 个。
    ///
    /// 修订号是内容摘要，字典序与新旧无关，所以排序必须看时间而不是看名字。
    fn prune_catalog_revisions(&self, current: &str) {
        let mut candidates: Vec<(std::time::SystemTime, String)> = self
            .router
            .revisions()
            .into_iter()
            .filter(|revision| revision != current)
            .filter(|revision| self.router.refs(revision).is_reclaimable())
            .map(|revision| {
                let modified = std::fs::metadata(self.layout.catalog_dir(&revision))
                    .and_then(|meta| meta.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                (modified, revision)
            })
            .collect();
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

        for (_, revision) in candidates
            .into_iter()
            .skip(RETAINED_CATALOG_GENERATIONS - 1)
        {
            if !self.router.retire(&revision) {
                // 竞态：筛选之后来了新引用。`retire` 已经拒绝，留着就是正确结果。
                continue;
            }
            // 内存里回收了，磁盘上那份也要删，否则文件会一直堆着。
            // 删不掉不是致命错误：它既不参与路由，也不影响用户。
            let _ = std::fs::remove_dir_all(self.layout.catalog_dir(&revision));
        }
    }

    /// 宿主加载回执。没有回执时最多停在 `Pending`，不能自称 `Loaded`。
    pub fn confirm_reload(
        &self,
        operation_id: &str,
        loaded: bool,
    ) -> Result<OperationState, CoreError> {
        let mut state = self.status(operation_id)?;
        let next = if loaded {
            ApplyStage::Verified
        } else {
            ApplyStage::Pending
        };
        state.operation.transition(next, self.clock.now())?;
        if loaded {
            self.mark_models_loaded(&self.repository.list_models()?)?;
        }
        self.operations.save(state.clone())?;
        Ok(state)
    }

    /// 宿主回执的自动补记：宿主进程的启动时间**晚于**本次发布时，认为它已经读过这份配置。
    ///
    /// 为什么可以这样判定：Codex 只在启动时读配置（这正是每次应用都要重启它的原因），
    /// 所以「一个在发布之后才起来的宿主进程」本身就是回执——不需要用户再点一次「已重载」。
    /// 以前缺这一步，于是「应用并重启 Codex」成功、配置也真的生效了，事务却永远停在
    /// `AwaitingReload`，界面一直显示「等待重载」，待应用条也一直在。
    ///
    /// 拿不到启动时间（宿主没运行、平台查不到）时**什么都不做**，停在原处等人工确认；
    /// 不用「大概重启过了」去顶替回执。返回被补记的 operation id。
    pub fn reconcile_host_reload(
        &self,
        host_started_at_unix: Option<i64>,
    ) -> Result<Vec<String>, CoreError> {
        let Some(host_started) = host_started_at_unix else {
            return Ok(Vec::new());
        };
        let mut confirmed = Vec::new();
        for mut state in self.operations.unfinished()? {
            if state.operation.stage != ApplyStage::AwaitingReload {
                continue;
            }
            let published_at = match &state.publication {
                Some(publication) => publication.published_at.clone(),
                None => continue,
            };
            let Ok(published) = time::OffsetDateTime::parse(
                &published_at,
                &time::format_description::well_known::Rfc3339,
            ) else {
                continue;
            };
            // 严格晚于发布：同一秒里分不清「发布前启动」还是「发布后启动」，
            // 这种边界留给人工确认，宁可不动也不猜。
            if host_started <= published.unix_timestamp() {
                continue;
            }
            state
                .operation
                .transition(ApplyStage::Verified, self.clock.now())?;
            self.operations.save(state.clone())?;
            confirmed.push(state.operation.id.as_str().to_owned());
        }
        if !confirmed.is_empty() {
            self.mark_models_loaded(&self.repository.list_models()?)?;
        }
        Ok(confirmed)
    }

    /// 还原计划：只撤销本工具写入且未被外部修改的字段。
    pub fn plan_restore(&self, instance: &CodexInstance) -> Result<ApplyPlan, CoreError> {
        let ownership = self.operations.ownership(&instance.id)?;
        if ownership.is_empty() {
            return Err(CoreError::validation(
                "本工具尚未写入过该实例的配置，无需还原",
            ));
        }
        let snapshot = crate::codex::config::ConfigSnapshot::read(&instance.config_file)?;
        let outcomes = plan_restore(&snapshot, &ownership);
        let changes = restore_changes(&snapshot, &ownership, &outcomes);
        let conflicts: Vec<String> = outcomes
            .iter()
            .filter(|outcome| outcome.is_conflict())
            .map(|outcome| format!("{} 已被外部修改，将保留当前值", outcome.key_path()))
            .collect();
        if changes.is_empty() {
            return Err(CoreError::validation(
                "当前配置与本工具写入值一致，无需还原",
            ));
        }
        let plan = build_plan(
            PlanId::generate(),
            instance.id.clone(),
            RevisionId::new("restore"),
            instance.config_file.clone(),
            snapshot.content_hash.clone(),
            snapshot.existed,
            changes,
            "restore".to_owned(),
            Vec::new(),
            conflicts,
            self.clock.now_unix(),
            DEFAULT_PLAN_TTL_SECS,
        );
        let mut operation =
            ApplyOperation::new(OperationId::generate(), &plan, "", self.clock.now());
        let now = self.clock.now();
        operation.transition(ApplyStage::Validating, now.clone())?;
        operation.transition(ApplyStage::Prepared, now)?;
        self.operations.save(OperationState {
            kind: OperationKind::Restore,
            operation,
            plan: plan.clone(),
            ownership,
            catalog_path: String::new(),
            catalog_hash: String::new(),
            publication: None,
            prepared: None,
        })?;
        Ok(plan)
    }

    /// 提交还原计划。冲突字段保持外部值，其余恢复基线。
    pub fn execute_restore(
        &self,
        plan_id: &str,
        plan_hash: &str,
        idempotency_key: &str,
    ) -> Result<String, CoreError> {
        let _commit_guard = self
            .commits
            .lock()
            .map_err(|_| CoreError::internal("提交锁不可用"))?;
        let mut state = self.plan_state(plan_id, plan_hash)?;
        if state.kind != OperationKind::Restore {
            return Err(CoreError::validation("该计划不是还原计划"));
        }
        if let Some(id) = self.check_execution(&state, idempotency_key)? {
            return Ok(id);
        }
        if state.plan.is_expired(self.clock.now_unix()) {
            return Err(
                CoreError::new(ErrorCode::ConfigChanged, "error.planExpired")
                    .with_detail("还原计划已过期，请重新比较".to_owned()),
            );
        }
        let snapshot = crate::codex::config::ConfigSnapshot::read(&state.plan.config_path)?;
        let cas = check_cas(
            &state.plan.expected_config_hash,
            state.plan.expected_config_exists,
            &snapshot.content_hash,
            snapshot.existed,
        );
        if !cas.is_match() {
            state
                .operation
                .transition(ApplyStage::Blocked, self.clock.now())?;
            state.operation.stage = ApplyStage::Conflict;
            state
                .operation
                .push_event(ApplyStage::Conflict, HashMap::new(), self.clock.now());
            self.operations.save(state)?;
            return Err(describe_cas_failure(cas));
        }

        state
            .operation
            .transition(ApplyStage::Committing, self.clock.now())?;
        let (text, _) = execute_restore(&snapshot, &state.ownership)?;
        state.operation.idempotency_key = idempotency_key.to_owned();
        state.operation.written_hash = Some(hash(&text));
        self.operations.save(state.clone())?;
        write_atomic(Path::new(&state.plan.config_path), &text)?;
        // 还原后本工具不再拥有任何受管字段。
        state.ownership = Vec::new();
        state
            .operation
            .transition(ApplyStage::AwaitingReload, self.clock.now())?;
        state
            .operation
            .transition(ApplyStage::Verified, self.clock.now())?;
        let operation_id = state.operation.id.as_str().to_owned();
        self.operations.save(state)?;
        // 还原后宿主状态回落到未纳入目录，避免界面继续显示“等待重载”。
        for mut model in self.repository.list_models()? {
            if model.in_catalog {
                let version = model.version;
                model.host_state = HostState::PendingApply;
                self.repository.save_model(model, version)?;
            }
        }
        Ok(operation_id)
    }

    /// 当前已生效的配置摘要：界面用它回答“现在 Codex 用的是哪个模型、哪个目录版本”。
    ///
    /// 只读取最近一次**已发布且仍生效**的事务，不猜测、不按当前表单重算。
    pub fn applied_summary(&self) -> Result<Option<AppliedSummary>, CoreError> {
        let mut latest: Option<OperationState> = None;
        for state in self.operations.list()? {
            if state.kind != OperationKind::Apply || state.publication.is_none() {
                continue;
            }
            if !matches!(
                state.operation.stage,
                ApplyStage::AwaitingReload | ApplyStage::Pending | ApplyStage::Verified
            ) {
                continue;
            }
            latest = Some(state);
        }
        let Some(state) = latest else {
            return Ok(None);
        };
        let revision = state.plan.catalog_revision.clone();
        Ok(Some(AppliedSummary {
            operation_id: state.operation.id.as_str().to_owned(),
            instance_id: state.plan.instance_id.as_str().to_owned(),
            catalog_revision: revision,
            // 默认模型来自冻结输入，不是当前表单值。
            default_model: state
                .prepared
                .as_ref()
                .and_then(|prepared| prepared.managed.model.clone()),
            alias_count: state.plan.catalog_aliases.len(),
            stage: state.operation.stage,
            // 应用时间取事务最后一个事件的时刻。这里原来写的是 operation id，
            // 界面上「当前生效时间」显示的是一串 op_xxxx。
            applied_at: state
                .operation
                .events
                .last()
                .map(|event| event.timestamp.clone())
                .unwrap_or_default(),
        }))
    }

    /// 启动恢复：只处理本工具未完成的事务，不覆盖外部修改。
    pub fn startup_recovery(&self) -> Result<Vec<RecoveryReport>, CoreError> {
        let _commit_guard = self
            .commits
            .lock()
            .map_err(|_| CoreError::internal("提交锁不可用"))?;
        let mut reports = Vec::new();
        for mut state in self.operations.unfinished()? {
            let snapshot = crate::codex::config::ConfigSnapshot::read(&state.plan.config_path)?;
            let decision = decide_recovery(&state.operation, &snapshot.content_hash);
            let applied = match &decision {
                RecoveryDecision::RecordCommitFromFile { written_hash } => {
                    state.operation.written_hash = Some(written_hash.clone());
                    state
                        .operation
                        .transition(ApplyStage::AwaitingReload, self.clock.now())?;
                    if state.kind == OperationKind::Restore {
                        state.ownership.clear();
                        state
                            .operation
                            .transition(ApplyStage::Verified, self.clock.now())?;
                    }
                    true
                }
                RecoveryDecision::RollBackToBaseline { .. } => {
                    let (text, _) = execute_restore(&snapshot, &state.ownership)?;
                    write_atomic(Path::new(&state.plan.config_path), &text)?;
                    state
                        .operation
                        .transition(ApplyStage::RollingBack, self.clock.now())?;
                    state
                        .operation
                        .transition(ApplyStage::Restored, self.clock.now())?;
                    state.ownership = Vec::new();
                    true
                }
                RecoveryDecision::ConflictWithExternalChange => {
                    state
                        .operation
                        .transition(ApplyStage::RollingBack, self.clock.now())?;
                    state
                        .operation
                        .transition(ApplyStage::Conflict, self.clock.now())?;
                    true
                }
                RecoveryDecision::NoAction if state.operation.stage == ApplyStage::Committing => {
                    state
                        .operation
                        .transition(ApplyStage::Failed, self.clock.now())?;
                    state.operation.error =
                        Some(CoreError::internal("上次提交未写入配置，请重新预览"));
                    true
                }
                RecoveryDecision::AwaitHostReload
                | RecoveryDecision::MarkOrphanCredential { .. }
                | RecoveryDecision::DiscardUnreferencedCatalog { .. }
                | RecoveryDecision::NoAction => false,
            };
            if applied {
                self.operations.save(state.clone())?;
            }
            reports.push(RecoveryReport {
                operation_id: state.operation.id.as_str().to_owned(),
                instance_id: state.operation.instance_id.as_str().to_owned(),
                decision,
                applied,
            });
        }
        // 已完成事务也需要恢复路由；只扫描 unfinished 会使应用重启后目录全部失联。
        for state in self.operations.list()? {
            if state.kind != OperationKind::Apply
                || state.publication.is_none()
                || !matches!(
                    state.operation.stage,
                    ApplyStage::AwaitingReload | ApplyStage::Pending | ApplyStage::Verified
                )
            {
                continue;
            }
            if let Some(prepared) = &state.prepared {
                let intact = std::fs::read_to_string(&state.catalog_path)
                    .map(|text| hash(&text) == state.catalog_hash)
                    .unwrap_or(false);
                if intact {
                    self.router.publish(prepared.routes.clone())?;
                } else {
                    reports.push(RecoveryReport {
                        operation_id: state.operation.id.to_string(),
                        instance_id: state.operation.instance_id.to_string(),
                        decision: RecoveryDecision::ConflictWithExternalChange,
                        applied: false,
                    });
                }
            }
        }
        Ok(reports)
    }

    fn check_execution(
        &self,
        state: &OperationState,
        key: &str,
    ) -> Result<Option<String>, CoreError> {
        if key.trim().is_empty() || key.len() > 128 {
            return Err(CoreError::validation("幂等键为空或过长"));
        }
        if self.operations.list()?.iter().any(|other| {
            other.operation.id != state.operation.id && other.operation.idempotency_key == key
        }) {
            return Err(CoreError::conflict("error.idempotencyKeyReused"));
        }
        match state.operation.stage {
            ApplyStage::AwaitingReload
            | ApplyStage::Pending
            | ApplyStage::Verified
            | ApplyStage::Restored => Ok(Some(state.operation.id.to_string())),
            ApplyStage::Prepared => Ok(None),
            _ => Err(CoreError::conflict("error.operationNotExecutable")
                .with_detail("该计划已经失败、冲突或正在恢复，请重新比较".to_owned())),
        }
    }

    fn current_source_hash(&self) -> Result<String, CoreError> {
        let models: Vec<Model> = self
            .repository
            .list_models()?
            .into_iter()
            .filter(|m| m.in_catalog && m.lifecycle != ModelLifecycle::Disabled)
            .collect();
        let providers = self
            .repository
            .list_providers()?
            .into_iter()
            .map(|p| (p.id.to_string(), p))
            .collect();
        source_hash(&models, &providers, &self.credentials_by_id()?)
    }

    fn plan_state(&self, plan_id: &str, plan_hash: &str) -> Result<OperationState, CoreError> {
        let state = self
            .operations
            .find_by_plan(plan_id)?
            .ok_or_else(|| CoreError::not_found("应用计划"))?;
        if state.plan.plan_hash != plan_hash {
            return Err(CoreError::conflict("error.planHashMismatch")
                .with_detail("计划摘要不一致，已拒绝执行".to_owned())
                .with_recovery("replan", "action.replan"));
        }
        Ok(state)
    }

    fn credentials_by_id(&self) -> Result<HashMap<String, Credential>, CoreError> {
        let mut map = HashMap::new();
        for provider in self.repository.list_providers()? {
            for credential in self.repository.list_credentials(&provider.id)? {
                map.insert(credential.id.as_str().to_owned(), credential);
            }
        }
        Ok(map)
    }

    fn route_blockers(
        &self,
        models: &[Model],
        providers: &HashMap<String, Provider>,
        credentials: &HashMap<String, Credential>,
    ) -> Vec<String> {
        let mut blockers = Vec::new();
        for model in models {
            let Some(provider) = providers.get(model.provider_id.as_str()) else {
                blockers.push(format!("{} 的供应商已不存在", model.display_name));
                continue;
            };
            if !provider.enabled {
                blockers.push(format!("供应商「{}」已停用", provider.name));
                continue;
            }
            if provider.auth_kind == AuthKind::None {
                continue;
            }
            match &provider.active_credential_id {
                None => blockers.push(format!("供应商「{}」尚未选择 API Key", provider.name)),
                Some(id) => match credentials.get(id.as_str()) {
                    None => blockers.push(format!(
                        "供应商「{}」当前 Key 的安全记录不存在",
                        provider.name
                    )),
                    Some(credential) if !credential.status.is_selectable() => {
                        blockers.push(format!(
                            "供应商「{}」当前 Key 状态为 {}，不能用于新请求",
                            provider.name,
                            credential.status.label_key()
                        ))
                    }
                    Some(_) => {}
                },
            }
        }
        blockers.dedup();
        blockers
    }

    fn managed_config(
        &self,
        instance: &CodexInstance,
        default_alias: &str,
        catalog_path: &Path,
        catalog_revision: &str,
    ) -> ManagedConfig {
        managed_config(
            &self.layout,
            &instance.id,
            default_alias,
            catalog_path,
            catalog_revision,
        )
    }

    fn write_catalog(&self, path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
        let parent = path
            .parent()
            .ok_or_else(|| CoreError::validation("目录路径缺少父目录"))?;
        std::fs::create_dir_all(parent)?;
        write_atomic(path, &String::from_utf8_lossy(bytes))?;
        Ok(())
    }

    fn build_routes(&self, plan: &ApplyPlan, models: &[Model]) -> Result<RouteSnapshot, CoreError> {
        let providers = self.repository.list_providers()?;
        let mut routes = Vec::new();
        for model in models.iter().filter(|m| m.in_catalog) {
            let provider = providers
                .iter()
                .find(|provider| provider.id == model.provider_id)
                .ok_or_else(|| CoreError::not_found("供应商"))?;
            let (credential_id, credential_version) = match &provider.active_credential_id {
                Some(id) => {
                    let credential = self
                        .repository
                        .get_credential(id)?
                        .ok_or_else(|| CoreError::not_found("Key"))?;
                    (credential.id.clone(), credential.secret_version)
                }
                None => (CredentialId::new(""), 0),
            };
            routes.push(RouteEntry {
                alias: model.catalog_alias.as_str().to_owned(),
                provider_id: model.provider_id.clone(),
                model_id: model.id.clone(),
                upstream_id: model.upstream_id.clone(),
                credential_id,
                credential_version,
                protocol_id: protocol_id(model.protocol_override.unwrap_or(provider.protocol)),
                // 策略随路由一起冻结：请求期不再读取当前表单值。
                output_limit: model.policy.output_limit.map(|limit| limit.value()),
                reasoning_efforts: model
                    .policy
                    .reasoning
                    .catalog_levels()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                native_modalities: model
                    .policy
                    .catalog_modalities()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                // 内置工具（web_search 等）是上游自己的功能，同样随策略冻结：
                // 请求期回读表单会让在途请求的行为随一次保存而改变。
                builtin_tools: model.policy.tools.builtin_tools,
            });
        }
        RouteSnapshot::new(
            plan.revision_id.clone(),
            plan.instance_id.clone(),
            plan.catalog_revision.clone(),
            routes,
            self.clock.now(),
        )
    }

    fn publication_for(
        &self,
        state: &OperationState,
        managed: &ManagedConfig,
    ) -> Result<RuntimePublication, CoreError> {
        let policy_revision = RevisionId::new(format!(
            "pol_{}",
            hash(
                &serde_json::to_string(managed)
                    .map_err(|_| CoreError::internal("策略序列化失败"))?
            )
        ));
        Ok(RuntimePublication {
            instance_id: state.plan.instance_id.clone(),
            catalog_revision: state.plan.catalog_revision.clone(),
            policy_revision,
            endpoint_prefix: RuntimePublication::build_prefix(
                &state.plan.instance_id,
                &state.plan.catalog_revision,
            ),
            published_at: self.clock.now(),
        })
    }

    fn mark_models_awaiting_reload(&self, models: &[Model]) -> Result<(), CoreError> {
        for model in models.iter().filter(|m| m.in_catalog) {
            let Some(current) = self.repository.get_model(&model.id)? else {
                continue;
            };
            // 提交途中编辑过的草稿仍然待应用，不用旧版本状态覆盖。
            if current.version != model.version {
                continue;
            }
            let mut next = model.clone();
            next.host_state = CatalogCompiler::host_state_after_publish(model.host_state);
            let version = model.version;
            self.repository.save_model(next, version)?;
        }
        Ok(())
    }

    fn mark_models_loaded(&self, models: &[Model]) -> Result<(), CoreError> {
        for model in models.iter().filter(|m| m.in_catalog) {
            let mut next = model.clone();
            next.host_state = HostState::Loaded;
            let version = model.version;
            self.repository.save_model(next, version)?;
        }
        Ok(())
    }
}

fn source_hash(
    models: &[Model],
    providers: &HashMap<String, Provider>,
    credentials: &HashMap<String, Credential>,
) -> Result<String, CoreError> {
    let mut models: Vec<_> = models.iter().collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    let mut providers: Vec<_> = providers.values().collect();
    providers.sort_by(|a, b| a.id.cmp(&b.id));
    let mut credentials: Vec<_> = credentials.values().collect();
    credentials.sort_by(|a, b| a.id.cmp(&b.id));
    let json = serde_json::to_string(&(models, providers, credentials))
        .map_err(|_| CoreError::internal("计划输入编码失败"))?;
    Ok(hash(&json))
}

fn managed_config(
    layout: &GatewayLayout,
    instance_id: &InstanceId,
    default_alias: &str,
    catalog_path: &Path,
    catalog_revision: &str,
) -> ManagedConfig {
    ManagedConfig {
        model: Some(default_alias.to_owned()),
        model_provider: Some(PROVIDER_ID.to_owned()),
        model_catalog_json: Some(catalog_path.display().to_string()),
        provider: Some(ManagedProvider {
            base_url: layout.base_url(instance_id, catalog_revision),
            wire_api: "responses".to_owned(),
            auth: ProviderAuth::Command {
                command: layout.auth_helper.clone(),
                timeout_ms: 5000,
                refresh_interval_ms: 300_000,
            },
        }),
        // 逐模型上下文由目录承载，不再写全局覆盖。
        model_context_window: None,
        model_reasoning_effort: None,
    }
}

fn protocol_id(protocol: Protocol) -> String {
    match protocol {
        Protocol::Responses => "responses.v1".to_owned(),
        Protocol::ChatCompletions => "chat_completions.v1".to_owned(),
    }
}

fn describe_cas_failure(cas: CasOutcome) -> CoreError {
    match cas {
        CasOutcome::Changed { .. } => {
            CoreError::new(ErrorCode::ConfigChanged, "error.configChanged")
                .with_detail("配置文件在计划生成后被其他程序修改".to_owned())
                .with_recovery("recompare", "action.recompare")
        }
        CasOutcome::IdentityChanged { detail } => {
            CoreError::new(ErrorCode::ConfigChanged, "error.configChanged")
                .with_detail(detail)
                .with_recovery("recompare", "action.recompare")
        }
        CasOutcome::Match => CoreError::internal("CAS 结果不一致"),
    }
}

/// 把三方还原结果转换为 UI 可展示的字段差异。
fn restore_changes(
    snapshot: &crate::codex::config::ConfigSnapshot,
    ownership: &[FieldOwnership],
    outcomes: &[crate::codex::config::RestoreOutcome],
) -> Vec<FieldChange> {
    outcomes
        .iter()
        .filter(|outcome| {
            !matches!(
                outcome,
                crate::codex::config::RestoreOutcome::Unchanged { .. }
            )
        })
        .filter(|outcome| !outcome.is_conflict())
        .map(|outcome| {
            let key_path = outcome.key_path().to_owned();
            let before = snapshot.managed_value(&key_path);
            let after = ownership
                .iter()
                .find(|record| record.key_path == key_path)
                .and_then(|record| {
                    matches!(
                        outcome,
                        crate::codex::config::RestoreOutcome::Restore { .. }
                    )
                    .then(|| record.baseline_value.clone())
                    .flatten()
                });
            FieldChange {
                key_path,
                before,
                after,
                reason_key: "reason.restore".to_owned(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_revision_is_derived_from_content_hash() {
        let layout = GatewayLayout {
            app_data_dir: PathBuf::from("/tmp/gptswitch"),
            port: gateway::DEFAULT_PORT,
            auth_helper: "/tmp/auth".to_owned(),
            base_instructions: "test".to_owned(),
        };
        let revision = layout.catalog_revision("0123456789abcdef0123");
        assert_eq!(revision, "rev_0123456789abcdef");
        assert_eq!(
            layout.catalog_path(&revision),
            PathBuf::from("/tmp/gptswitch/catalogs/rev_0123456789abcdef/models.json")
        );
    }

    #[test]
    fn base_url_binds_loopback_instance_and_catalog_revision() {
        let layout = GatewayLayout {
            app_data_dir: PathBuf::from("/tmp/gptswitch"),
            port: 18765,
            auth_helper: "/tmp/auth".to_owned(),
            base_instructions: "test".to_owned(),
        };
        let url = layout.base_url(&InstanceId::new("inst_1"), "rev_0007");
        assert!(url.starts_with("http://127.0.0.1:18765/i/inst_1/c/rev_0007/v1"));
        assert!(!url.contains("0.0.0.0"));
    }
}
