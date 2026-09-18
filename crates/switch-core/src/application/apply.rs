//! 配置应用编排：计划 → CAS → 原子提交 → 等待宿主重载。
//!
//! 规则来自 [配置生命周期](../../../../docs/architecture/02-configuration-lifecycle.md)：
//! 计划不可变且有 TTL；提交前必须做 CAS；没有宿主回执最多到 `AwaitingReload`，
//! 绝不把“本工具保存成功”当成宿主已加载；目录修订与运行策略修订分开发布。
//! 任何阶段都不允许把上游 Key 写进 Codex 配置。

use crate::{
    codex::{
        catalog::{CatalogCompiler, CompileOptions},
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
}

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
    repository: Arc<dyn Repository>,
    operations: Arc<dyn OperationStore>,
    router: Arc<GatewayRouter>,
    layout: GatewayLayout,
    clock: Arc<dyn Clock>,
    commits: Mutex<()>,
}

impl ApplyService {
    pub fn new(
        repository: Arc<dyn Repository>,
        operations: Arc<dyn OperationStore>,
        router: Arc<GatewayRouter>,
        layout: GatewayLayout,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self { repository, operations, router, layout, clock, commits: Mutex::new(()) }
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
        let catalog_revision = self.layout.catalog_revision(&hash(&format!("{}:{}", instance.id, catalog_hash)));
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

        let managed = self.managed_config(instance, &default_alias, &catalog_path, &catalog_revision);
        let snapshot = crate::codex::config::ConfigSnapshot::read(&instance.config_file)?;
        let changes = diff_managed(&snapshot, &managed);
        let warnings: Vec<String> = compiled
            .warnings
            .iter()
            .map(|warning| format!("{}：{}", warning.message_key, warning.detail))
            .collect();

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
            source_hash: source_hash(&selected, &providers, &credentials)?,
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

    /// 提交计划。CAS 失败或计划过期都必须拒绝，不能拿旧计划硬写。
    pub fn execute_apply(
        &self,
        plan_id: &str,
        plan_hash: &str,
        idempotency_key: &str,
    ) -> Result<String, CoreError> {
        let _commit_guard = self.commits.lock().map_err(|_| CoreError::internal("提交锁不可用"))?;
        let mut state = self.plan_state(plan_id, plan_hash)?;
        if state.kind != OperationKind::Apply {
            return Err(CoreError::validation("该计划不是应用计划"));
        }
        if let Some(id) = self.check_execution(&state, idempotency_key)? { return Ok(id); }
        if state.plan.is_expired(self.clock.now_unix()) {
            state.operation.transition(ApplyStage::Blocked, self.clock.now())?;
            self.operations.save(state)?;
            return Err(CoreError::new(ErrorCode::ConfigChanged, "error.planExpired")
                .with_detail("计划已过期，需要重新生成差异".to_owned())
                .with_recovery("replan", "action.replan"));
        }

        let snapshot = crate::codex::config::ConfigSnapshot::read(&state.plan.config_path)?;
        let cas = check_cas(
            &state.plan.expected_config_hash,
            state.plan.expected_config_exists,
            &snapshot.content_hash,
            snapshot.existed,
        );
        if !cas.is_match() {
            state.operation.transition(ApplyStage::Blocked, self.clock.now())?;
            state.operation.stage = ApplyStage::Conflict;
            state.operation.push_event(ApplyStage::Conflict, HashMap::new(), self.clock.now());
            self.operations.save(state)?;
            return Err(describe_cas_failure(cas));
        }

        let prepared = state.prepared.clone().ok_or_else(|| CoreError::conflict("error.planRequiresRefresh")
            .with_detail("该计划缺少冻结的模型与路由，请重新预览".to_owned()))?;
        if self.current_source_hash()? != prepared.source_hash {
            return Err(CoreError::new(ErrorCode::ConfigChanged, "error.modelChanged")
                .with_detail("预览后供应商、Key 或模型已变化，请重新生成差异".to_owned()));
        }
        let bytes = std::fs::read(&state.catalog_path).map_err(|_| CoreError::internal("目录文件在提交前不可读取"))?;
        if hash(&String::from_utf8_lossy(&bytes)) != state.catalog_hash {
            return Err(CoreError::conflict("error.catalogChanged").with_detail("目录文件在预览后被修改".to_owned()));
        }
        let (text, ownership) = apply_managed(&snapshot, &prepared.managed, &state.ownership)?;
        // 路由发布失败必须发生在修改 Codex 之前；发布的是冻结输入，不读取当前表单值。
        self.router.publish(prepared.routes.clone())?;
        state.operation.idempotency_key = idempotency_key.to_owned();
        state.operation.written_hash = Some(hash(&text));
        state.ownership = ownership;
        state.publication = Some(self.publication_for(&state, &prepared.managed)?);
        state.operation.transition(ApplyStage::Committing, self.clock.now())?;
        self.operations.save(state.clone())?; // 写前日志：崩溃后可根据目标摘要补记。
        write_atomic(Path::new(&state.plan.config_path), &text)?;
        state.operation.transition(ApplyStage::AwaitingReload, self.clock.now())?;
        let operation_id = state.operation.id.as_str().to_owned();
        self.operations.save(state)?;
        self.mark_models_awaiting_reload(&prepared.models)?;
        Ok(operation_id)
    }

    /// 宿主加载回执。没有回执时最多停在 `Pending`，不能自称 `Loaded`。
    pub fn confirm_reload(&self, operation_id: &str, loaded: bool) -> Result<OperationState, CoreError> {
        let mut state = self.status(operation_id)?;
        let next = if loaded { ApplyStage::Verified } else { ApplyStage::Pending };
        state.operation.transition(next, self.clock.now())?;
        if loaded {
            self.mark_models_loaded(&self.repository.list_models()?)?;
        }
        self.operations.save(state.clone())?;
        Ok(state)
    }

    /// 还原计划：只撤销本工具写入且未被外部修改的字段。
    pub fn plan_restore(&self, instance: &CodexInstance) -> Result<ApplyPlan, CoreError> {
        let ownership = self.operations.ownership(&instance.id)?;
        if ownership.is_empty() {
            return Err(CoreError::validation("本工具尚未写入过该实例的配置，无需还原"));
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
            return Err(CoreError::validation("当前配置与本工具写入值一致，无需还原"));
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
        let mut operation = ApplyOperation::new(OperationId::generate(), &plan, "", self.clock.now());
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
        let _commit_guard = self.commits.lock().map_err(|_| CoreError::internal("提交锁不可用"))?;
        let mut state = self.plan_state(plan_id, plan_hash)?;
        if state.kind != OperationKind::Restore {
            return Err(CoreError::validation("该计划不是还原计划"));
        }
        if let Some(id) = self.check_execution(&state, idempotency_key)? { return Ok(id); }
        if state.plan.is_expired(self.clock.now_unix()) {
            return Err(CoreError::new(ErrorCode::ConfigChanged, "error.planExpired")
                .with_detail("还原计划已过期，请重新比较".to_owned()));
        }
        let snapshot = crate::codex::config::ConfigSnapshot::read(&state.plan.config_path)?;
        let cas = check_cas(
            &state.plan.expected_config_hash,
            state.plan.expected_config_exists,
            &snapshot.content_hash,
            snapshot.existed,
        );
        if !cas.is_match() {
            state.operation.transition(ApplyStage::Blocked, self.clock.now())?;
            state.operation.stage = ApplyStage::Conflict;
            state.operation.push_event(ApplyStage::Conflict, HashMap::new(), self.clock.now());
            self.operations.save(state)?;
            return Err(describe_cas_failure(cas));
        }

        state.operation.transition(ApplyStage::Committing, self.clock.now())?;
        let (text, _) = execute_restore(&snapshot, &state.ownership)?;
        state.operation.idempotency_key = idempotency_key.to_owned();
        state.operation.written_hash = Some(hash(&text));
        self.operations.save(state.clone())?;
        write_atomic(Path::new(&state.plan.config_path), &text)?;
        // 还原后本工具不再拥有任何受管字段。
        state.ownership = Vec::new();
        state.operation.transition(ApplyStage::AwaitingReload, self.clock.now())?;
        state.operation.transition(ApplyStage::Verified, self.clock.now())?;
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

    /// 启动恢复：只处理本工具未完成的事务，不覆盖外部修改。
    pub fn startup_recovery(&self) -> Result<Vec<RecoveryReport>, CoreError> {
        let _commit_guard = self.commits.lock().map_err(|_| CoreError::internal("提交锁不可用"))?;
        let mut reports = Vec::new();
        for mut state in self.operations.unfinished()? {
            let snapshot = crate::codex::config::ConfigSnapshot::read(&state.plan.config_path)?;
            let decision = decide_recovery(&state.operation, &snapshot.content_hash);
            let applied = match &decision {
                RecoveryDecision::RecordCommitFromFile { written_hash } => {
                    state.operation.written_hash = Some(written_hash.clone());
                    state.operation.transition(ApplyStage::AwaitingReload, self.clock.now())?;
                    if state.kind == OperationKind::Restore {
                        state.ownership.clear();
                        state.operation.transition(ApplyStage::Verified, self.clock.now())?;
                    }
                    true
                }
                RecoveryDecision::RollBackToBaseline { .. } => {
                    let (text, _) = execute_restore(&snapshot, &state.ownership)?;
                    write_atomic(Path::new(&state.plan.config_path), &text)?;
                    state.operation.transition(ApplyStage::RollingBack, self.clock.now())?;
                    state.operation.transition(ApplyStage::Restored, self.clock.now())?;
                    state.ownership = Vec::new();
                    true
                }
                RecoveryDecision::ConflictWithExternalChange => {
                    state.operation.transition(ApplyStage::RollingBack, self.clock.now())?;
                    state.operation.transition(ApplyStage::Conflict, self.clock.now())?;
                    true
                }
                RecoveryDecision::NoAction if state.operation.stage == ApplyStage::Committing => {
                    state.operation.transition(ApplyStage::Failed, self.clock.now())?;
                    state.operation.error = Some(CoreError::internal("上次提交未写入配置，请重新预览"));
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
            if state.kind != OperationKind::Apply || state.publication.is_none() || !matches!(state.operation.stage,
                ApplyStage::AwaitingReload | ApplyStage::Pending | ApplyStage::Verified) { continue; }
            if let Some(prepared) = &state.prepared {
                let intact = std::fs::read_to_string(&state.catalog_path)
                    .map(|text| hash(&text) == state.catalog_hash).unwrap_or(false);
                if intact { self.router.publish(prepared.routes.clone())?; }
                else {
                    reports.push(RecoveryReport { operation_id: state.operation.id.to_string(), instance_id: state.operation.instance_id.to_string(),
                        decision: RecoveryDecision::ConflictWithExternalChange, applied: false });
                }
            }
        }
        Ok(reports)
    }

    fn check_execution(&self, state: &OperationState, key: &str) -> Result<Option<String>, CoreError> {
        if key.trim().is_empty() || key.len() > 128 { return Err(CoreError::validation("幂等键为空或过长")); }
        if self.operations.list()?.iter().any(|other| other.operation.id != state.operation.id && other.operation.idempotency_key == key) {
            return Err(CoreError::conflict("error.idempotencyKeyReused"));
        }
        match state.operation.stage {
            ApplyStage::AwaitingReload | ApplyStage::Pending | ApplyStage::Verified | ApplyStage::Restored => Ok(Some(state.operation.id.to_string())),
            ApplyStage::Prepared => Ok(None),
            _ => Err(CoreError::conflict("error.operationNotExecutable")
                .with_detail("该计划已经失败、冲突或正在恢复，请重新比较".to_owned())),
        }
    }

    fn current_source_hash(&self) -> Result<String, CoreError> {
        let models: Vec<Model> = self.repository.list_models()?.into_iter()
            .filter(|m| m.in_catalog && m.lifecycle != ModelLifecycle::Disabled).collect();
        let providers = self.repository.list_providers()?.into_iter().map(|p| (p.id.to_string(), p)).collect();
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
                    None => blockers.push(format!("供应商「{}」当前 Key 的安全记录不存在", provider.name)),
                    Some(credential) if !credential.status.is_selectable() => blockers.push(format!(
                        "供应商「{}」当前 Key 状态为 {}，不能用于新请求",
                        provider.name,
                        credential.status.label_key()
                    )),
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

    fn publication_for(&self, state: &OperationState, managed: &ManagedConfig) -> Result<RuntimePublication, CoreError> {
        let policy_revision = RevisionId::new(format!(
            "pol_{}",
            &hash(&serde_json::to_string(managed).map_err(|_| CoreError::internal("策略序列化失败"))?)
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
            let Some(current) = self.repository.get_model(&model.id)? else { continue; };
            // 提交途中编辑过的草稿仍然待应用，不用旧版本状态覆盖。
            if current.version != model.version { continue; }
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

fn source_hash(models: &[Model], providers: &HashMap<String, Provider>, credentials: &HashMap<String, Credential>) -> Result<String, CoreError> {
    let mut models: Vec<_> = models.iter().collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    let mut providers: Vec<_> = providers.values().collect();
    providers.sort_by(|a, b| a.id.cmp(&b.id));
    let mut credentials: Vec<_> = credentials.values().collect();
    credentials.sort_by(|a, b| a.id.cmp(&b.id));
    let json = serde_json::to_string(&(models, providers, credentials)).map_err(|_| CoreError::internal("计划输入编码失败"))?;
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
        CasOutcome::Changed { .. } => CoreError::new(ErrorCode::ConfigChanged, "error.configChanged")
            .with_detail("配置文件在计划生成后被其他程序修改".to_owned())
            .with_recovery("recompare", "action.recompare"),
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
            !matches!(outcome, crate::codex::config::RestoreOutcome::Unchanged { .. })
        })
        .filter(|outcome| !outcome.is_conflict())
        .map(|outcome| {
            let key_path = outcome.key_path().to_owned();
            let before = snapshot.managed_value(&key_path);
            let after = ownership
                .iter()
                .find(|record| record.key_path == key_path)
                .and_then(|record| {
                    matches!(outcome, crate::codex::config::RestoreOutcome::Restore { .. })
                        .then(|| record.baseline_value.clone())
                        .flatten()
                });
            FieldChange { key_path, before, after, reason_key: "reason.restore".to_owned() }
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
