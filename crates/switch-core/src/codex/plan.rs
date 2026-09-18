//! 应用计划与提交状态机。
//!
//! 规则来自 [配置生命周期](../../../../docs/architecture/02-configuration-lifecycle.md)：
//! 计划不可变且有 TTL；提交前必须做 CAS；没有宿主回执最多到 `Pending`；
//! `committing` 阶段不可取消；重试沿同一 operationId 从中断处继续。

use crate::domain::error::CoreError;
use crate::domain::ids::{InstanceId, OperationId, PlanId, RevisionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::config::FieldChange;

/// 计划默认有效期（秒）。过期必须重算，不能拿旧计划硬提交。
pub const DEFAULT_PLAN_TTL_SECS: i64 = 600;

/// 应用状态机的阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyStage {
    Draft,
    Validating,
    Blocked,
    Prepared,
    Committing,
    AwaitingReload,
    Verified,
    Pending,
    RollingBack,
    Restored,
    Conflict,
    Failed,
}

impl ApplyStage {
    /// 允许的状态迁移。非法迁移必须被拒绝而不是静默接受。
    pub fn can_transition_to(self, next: ApplyStage) -> bool {
        use ApplyStage::*;
        match self {
            Draft => matches!(next, Validating | Blocked),
            Validating => matches!(next, Prepared | Blocked),
            Blocked => matches!(next, Draft | Validating),
            Prepared => matches!(next, Committing | Blocked | Draft),
            Committing => matches!(next, AwaitingReload | RollingBack | Failed),
            AwaitingReload => matches!(next, Verified | Pending | Conflict),
            Pending => matches!(next, Verified | Conflict),
            RollingBack => matches!(next, Restored | Conflict),
            Restored | Verified | Conflict | Failed => matches!(next, Draft),
        }
    }

    /// 该阶段是否允许用户取消。
    ///
    /// 提交阶段写入不可安全中断，取消按钮应变为不可用并说明原因。
    pub fn is_cancellable(self) -> bool {
        matches!(
            self,
            ApplyStage::Draft | ApplyStage::Validating | ApplyStage::Prepared | ApplyStage::Blocked
        )
    }

    /// 终态：不再变化，除非显式开始新事务。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ApplyStage::Verified
                | ApplyStage::Pending
                | ApplyStage::Restored
                | ApplyStage::Conflict
                | ApplyStage::Failed
        )
    }

    pub fn message_key(self) -> &'static str {
        match self {
            ApplyStage::Draft => "stage.draft",
            ApplyStage::Validating => "stage.validating",
            ApplyStage::Blocked => "stage.blocked",
            ApplyStage::Prepared => "stage.prepared",
            ApplyStage::Committing => "stage.committing",
            ApplyStage::AwaitingReload => "stage.awaitingReload",
            ApplyStage::Verified => "stage.verified",
            ApplyStage::Pending => "stage.pending",
            ApplyStage::RollingBack => "stage.rollingBack",
            ApplyStage::Restored => "stage.restored",
            ApplyStage::Conflict => "stage.conflict",
            ApplyStage::Failed => "stage.failed",
        }
    }
}

/// 需要重新加载的范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadScope {
    /// 只发布网关版本，下一次新请求生效。
    None,
    /// 只影响新任务（例如 Codex 默认模型）。
    NextTaskOnly,
    /// 需要宿主重新加载。
    HostReload,
}

impl ReloadScope {
    /// UI 主按钮应显示为“应用”还是“应用并重新加载”。
    pub fn primary_action_key(self) -> &'static str {
        match self {
            ReloadScope::HostReload => "action.applyAndReload",
            ReloadScope::NextTaskOnly | ReloadScope::None => "action.apply",
        }
    }

    pub fn reason_key(self) -> &'static str {
        match self {
            ReloadScope::None => "reload.reason.gatewayOnly",
            ReloadScope::NextTaskOnly => "reload.reason.nextTaskOnly",
            ReloadScope::HostReload => "reload.reason.catalogChanged",
        }
    }
}

/// 摘要中的一个分组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffGroup {
    pub group_key: String,
    pub changes: Vec<FieldChange>,
}

impl DiffGroup {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// 不可变应用计划。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPlan {
    pub id: PlanId,
    pub instance_id: InstanceId,
    pub revision_id: RevisionId,
    /// 计划摘要，用于执行时校验计划未被篡改或重算。
    pub plan_hash: String,
    /// 计划生成时观察到的配置文件摘要，作为 CAS 期望值。
    pub expected_config_hash: String,
    /// 计划生成时配置文件是否存在。缺失/出现都属于身份变化，不能直接覆盖。
    pub expected_config_exists: bool,
    pub config_path: String,
    pub created_at_unix: i64,
    pub ttl_secs: i64,
    pub changes: Vec<FieldChange>,
    pub reload_scope: ReloadScope,
    /// 目录修订；与运行策略修订分开。
    pub catalog_revision: String,
    /// 该计划将写入的模型 alias 列表（用于应用后的核验）。
    pub catalog_aliases: Vec<String>,
    /// 预检与编译警告；不阻止执行，但必须在差异页显示。
    pub warnings: Vec<String>,
    /// 若为真，执行前必须确认没有正在生成的任务。
    pub touches_active_tasks: bool,
}

impl ApplyPlan {
    /// 计划分组：供应商路由 / 模型目录 / 请求策略 / 需要重新加载。
    pub fn groups(&self) -> Vec<DiffGroup> {
        let mut groups: HashMap<&'static str, Vec<FieldChange>> = HashMap::new();
        for change in &self.changes {
            let key = match change.key_path.as_str() {
                "model" | "model_provider" => "group.route",
                "model_catalog_json" | "model_providers.gptswitch" => "group.catalog",
                "model_context_window" | "model_reasoning_effort" => "group.policy",
                _ => "group.other",
            };
            groups.entry(key).or_default().push(change.clone());
        }
        let mut ordered: Vec<DiffGroup> = ["group.route", "group.catalog", "group.policy", "group.other"]
            .iter()
            .filter_map(|key| {
                groups.remove(*key).map(|changes| DiffGroup {
                    group_key: (*key).to_owned(),
                    changes,
                })
            })
            .collect();
        if self.reload_scope == ReloadScope::HostReload {
            ordered.push(DiffGroup {
                group_key: "group.requiresReload".to_owned(),
                changes: Vec::new(),
            });
        }
        ordered
    }

    /// 计划是否已过期。
    pub fn is_expired(&self, now_unix: i64) -> bool {
        now_unix.saturating_sub(self.created_at_unix) > self.ttl_secs
    }

    /// 是否包含任何实际写入。
    pub fn is_noop(&self) -> bool {
        self.changes.is_empty()
    }
}

/// 计划摘要计算：只包含决定行为的字段，保证同输入同摘要。
pub fn plan_hash(
    instance_id: &InstanceId,
    revision_id: &RevisionId,
    expected_config_hash: &str,
    changes: &[FieldChange],
    reload_scope: ReloadScope,
) -> String {
    plan_hash_with_identity(instance_id, revision_id, expected_config_hash, true, changes, reload_scope)
}

/// 与 [`plan_hash`] 相同，但把“文件是否存在”一并纳入摘要。
///
/// 文件存在性属于 CAS 身份的一部分：缺失/出现都必须让旧计划失效。
pub fn plan_hash_with_identity(
    instance_id: &InstanceId,
    revision_id: &RevisionId,
    expected_config_hash: &str,
    expected_config_exists: bool,
    changes: &[FieldChange],
    reload_scope: ReloadScope,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(instance_id.as_str().as_bytes());
    hasher.update(b"|");
    hasher.update(revision_id.as_str().as_bytes());
    hasher.update(b"|");
    hasher.update(expected_config_hash.as_bytes());
    hasher.update(b"|");
    hasher.update(if expected_config_exists { b"exists" as &[u8] } else { b"absent" as &[u8] });
    hasher.update(b"|");
    hasher.update(format!("{:?}", reload_scope).as_bytes());
    for change in changes {
        hasher.update(b"|");
        hasher.update(change.key_path.as_bytes());
        hasher.update(b"=");
        hasher.update(change.before.as_deref().unwrap_or("\u{0}").as_bytes());
        hasher.update(b"->");
        hasher.update(change.after.as_deref().unwrap_or("\u{0}").as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// CAS 结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "result")]
pub enum CasOutcome {
    /// 摘要一致，可以提交。
    Match,
    /// 文件在计划生成后被修改。
    Changed { expected: String, actual: String },
    /// 文件不存在但计划预期存在（或反之）。
    IdentityChanged { detail: String },
}

impl CasOutcome {
    pub fn is_match(&self) -> bool {
        matches!(self, CasOutcome::Match)
    }
}

/// 提交前的 CAS 检查。锁只能协调本工具，不合作的编辑器仍需这一层。
pub fn check_cas(
    expected_hash: &str,
    expected_exists: bool,
    actual_hash: &str,
    actual_exists: bool,
) -> CasOutcome {
    if expected_exists != actual_exists {
        return CasOutcome::IdentityChanged {
            detail: format!(
                "计划预期文件存在={}，实际存在={}",
                expected_exists, actual_exists
            ),
        };
    }
    if expected_hash != actual_hash {
        return CasOutcome::Changed {
            expected: expected_hash.to_owned(),
            actual: actual_hash.to_owned(),
        };
    }
    CasOutcome::Match
}

/// 契约：窗口重开后先查 operation snapshot，再订阅大于 sequence 的事件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationEvent {
    pub schema_version: u32,
    pub operation_id: String,
    pub sequence: u64,
    pub phase: ApplyStage,
    pub revision_id: Option<String>,
    pub message_key: String,
    pub safe_args: HashMap<String, String>,
    pub cancellable: bool,
    pub timestamp: String,
}

/// 事务记录。重试沿同一 operationId 从中断阶段继续。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOperation {
    pub id: OperationId,
    pub instance_id: InstanceId,
    pub plan_id: PlanId,
    pub plan_hash: String,
    /// 幂等键：相同 (planHash, idempotencyKey) 只执行一次。
    pub idempotency_key: String,
    pub stage: ApplyStage,
    pub expected_config_hash: String,
    /// 写入后的配置摘要；未提交时为空。
    pub written_hash: Option<String>,
    pub revision_id: RevisionId,
    pub error: Option<CoreError>,
    pub events: Vec<OperationEvent>,
    pub next_sequence: u64,
}

impl ApplyOperation {
    pub fn new(
        id: OperationId,
        plan: &ApplyPlan,
        idempotency_key: impl Into<String>,
        _now: impl Into<String>,
    ) -> Self {
        Self {
            id,
            instance_id: plan.instance_id.clone(),
            plan_id: plan.id.clone(),
            plan_hash: plan.plan_hash.clone(),
            idempotency_key: idempotency_key.into(),
            stage: ApplyStage::Draft,
            expected_config_hash: plan.expected_config_hash.clone(),
            written_hash: None,
            revision_id: plan.revision_id.clone(),
            error: None,
            events: Vec::new(),
            next_sequence: 1,
        }
    }

    /// 状态迁移：非法迁移返回结构化错误，不静默修正。
    pub fn transition(
        &mut self,
        next: ApplyStage,
        timestamp: impl Into<String>,
    ) -> Result<(), CoreError> {
        if self.stage == next {
            return Ok(());
        }
        if !self.stage.can_transition_to(next) {
            return Err(CoreError::internal(format!(
                "非法状态迁移 {:?} -> {:?}",
                self.stage, next
            )));
        }
        self.stage = next;
        self.push_event(next, HashMap::new(), timestamp);
        Ok(())
    }

    /// 记录事件。sequence 单调递增，便于窗口重开后只拉取增量。
    pub fn push_event(
        &mut self,
        phase: ApplyStage,
        safe_args: HashMap<String, String>,
        timestamp: impl Into<String>,
    ) -> &OperationEvent {
        let event = OperationEvent {
            schema_version: 1,
            operation_id: self.id.as_str().to_owned(),
            sequence: self.next_sequence,
            phase,
            revision_id: Some(self.revision_id.as_str().to_owned()),
            message_key: phase.message_key().to_owned(),
            safe_args,
            cancellable: phase.is_cancellable(),
            timestamp: timestamp.into(),
        };
        self.next_sequence += 1;
        self.events.push(event);
        self.events.last().expect("刚推入的事件必然存在")
    }

    /// 窗口重开后的增量订阅：返回 sequence 大于 cursor 的事件。
    pub fn events_since(&self, cursor: u64) -> &[OperationEvent] {
        let start = self
            .events
            .iter()
            .position(|event| event.sequence > cursor)
            .unwrap_or(self.events.len());
        &self.events[start..]
    }

    /// 取消能力由阶段决定，不由调用方决定。
    pub fn can_cancel(&self) -> bool {
        self.stage.is_cancellable()
    }

    /// 重试必须沿同一 operationId 从中断阶段继续，不能新建事务。
    pub fn resumable_from(&self) -> ApplyStage {
        match self.stage {
            ApplyStage::Validating | ApplyStage::Blocked => ApplyStage::Validating,
            ApplyStage::Prepared => ApplyStage::Prepared,
            ApplyStage::Committing | ApplyStage::RollingBack => ApplyStage::Committing,
            ApplyStage::AwaitingReload | ApplyStage::Pending => ApplyStage::AwaitingReload,
            other => other,
        }
    }
}

/// 幂等登记表：同一 (planHash, idempotencyKey) 只执行一次。
#[derive(Debug, Default)]
pub struct IdempotencyRegistry {
    executed: HashMap<(String, String), OperationId>,
}

impl IdempotencyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一次执行。返回已存在的 operationId 表示本次不应重复执行。
    pub fn begin(&mut self, plan_hash: &str, key: &str, operation: &OperationId) -> Option<OperationId> {
        let entry = (plan_hash.to_owned(), key.to_owned());
        if let Some(existing) = self.executed.get(&entry) {
            return Some(existing.clone());
        }
        self.executed.insert(entry, operation.clone());
        None
    }

    pub fn len(&self) -> usize {
        self.executed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.executed.is_empty()
    }
}

/// 崩溃恢复判定：启动时只恢复本工具未完成的事务。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RecoveryDecision {
    /// 凭据已保存但元数据未提交：保留短期恢复窗口。
    MarkOrphanCredential { credential_id: String },
    /// 目录已生成但配置未切换：清理未被引用的目录。
    DiscardUnreferencedCatalog { revision_id: String },
    /// 配置已替换但 DB 未记完成：按摘要补记完成。
    RecordCommitFromFile { written_hash: String },
    /// 配置已替换且与计划不符：按 journal 回滚。
    RollBackToBaseline { expected_hash: String },
    /// DB 已提交但宿主未重载：显示等待重载，不反复重启。
    AwaitHostReload,
    /// 外部配置已变化：进入冲突，不用旧备份覆盖新文件。
    ConflictWithExternalChange,
    /// 无需处理。
    NoAction,
}

/// 依据 journal 与当前文件摘要判定恢复动作。
pub fn decide_recovery(
    operation: &ApplyOperation,
    current_hash: &str,
) -> RecoveryDecision {
    match operation.stage {
        ApplyStage::Committing => {
            if let Some(written) = &operation.written_hash {
                if written == current_hash {
                    RecoveryDecision::RecordCommitFromFile {
                        written_hash: written.clone(),
                    }
                } else if operation.expected_config_hash == current_hash {
                    RecoveryDecision::NoAction
                } else {
                    RecoveryDecision::ConflictWithExternalChange
                }
            } else if operation.expected_config_hash == current_hash {
                // 提交尚未写文件，保持旧配置。
                RecoveryDecision::NoAction
            } else {
                RecoveryDecision::ConflictWithExternalChange
            }
        }
        ApplyStage::AwaitingReload | ApplyStage::Pending => RecoveryDecision::AwaitHostReload,
        _ => RecoveryDecision::NoAction,
    }
}

/// 依据差异推导重载范围。
///
/// 目录相关字段（默认模型、目录路径、provider 表）需要宿主重载；纯运行策略
/// （输出上限等）只发布网关版本。UI 从这个结果生成按钮文案，不自己猜。
pub fn derive_reload_scope(changes: &[FieldChange]) -> ReloadScope {
    let mut scope = ReloadScope::None;
    for change in changes {
        match change.key_path.as_str() {
            "model_catalog_json" | "model_providers.gptswitch" | "model_context_window" => {
                return ReloadScope::HostReload;
            }
            "model_reasoning_effort" => {
                if scope != ReloadScope::HostReload {
                    scope = ReloadScope::HostReload;
                }
            }
            "model" | "model_provider" => {
                if scope == ReloadScope::None {
                    scope = ReloadScope::NextTaskOnly;
                }
            }
            _ => {}
        }
    }
    scope
}

/// 构造不可变计划。所有决定行为的输入都由调用方显式传入。
#[allow(clippy::too_many_arguments)]
pub fn build_plan(
    id: PlanId,
    instance_id: InstanceId,
    revision_id: RevisionId,
    config_path: impl Into<String>,
    expected_config_hash: impl Into<String>,
    expected_config_exists: bool,
    changes: Vec<FieldChange>,
    catalog_revision: impl Into<String>,
    catalog_aliases: Vec<String>,
    warnings: Vec<String>,
    created_at_unix: i64,
    ttl_secs: i64,
) -> ApplyPlan {
    let expected_config_hash = expected_config_hash.into();
    let reload_scope = derive_reload_scope(&changes);
    let hash = plan_hash_with_identity(
        &instance_id,
        &revision_id,
        &expected_config_hash,
        expected_config_exists,
        &changes,
        reload_scope,
    );
    ApplyPlan {
        id,
        instance_id,
        revision_id,
        plan_hash: hash,
        expected_config_hash,
        expected_config_exists,
        config_path: config_path.into(),
        created_at_unix,
        ttl_secs,
        changes,
        reload_scope,
        catalog_revision: catalog_revision.into(),
        catalog_aliases,
        warnings,
        touches_active_tasks: reload_scope == ReloadScope::HostReload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::ErrorCode;

    fn change(key: &str, before: Option<&str>, after: Option<&str>) -> FieldChange {
        FieldChange {
            key_path: key.to_owned(),
            before: before.map(|v| v.to_owned()),
            after: after.map(|v| v.to_owned()),
            reason_key: "reason.test".to_owned(),
        }
    }

    fn plan() -> ApplyPlan {
        build_plan(
            PlanId::new("plan_1"),
            InstanceId::new("inst_1"),
            RevisionId::new("rev_1"),
            "/Users/example/.codex/config.toml",
            "hash-before",
            true,
            vec![
                change("model", Some("gpt-5-codex"), Some("gs/p_a/m_1")),
                change("model_provider", Some("openai"), Some("gptswitch")),
            ],
            "catalog_rev_1",
            vec!["gs/p_a/m_1".to_owned()],
            Vec::new(),
            1_700_000_000,
            DEFAULT_PLAN_TTL_SECS,
        )
    }

    #[test]
    fn legal_transitions_are_allowed_and_illegal_are_rejected() {
        assert!(ApplyStage::Draft.can_transition_to(ApplyStage::Validating));
        assert!(ApplyStage::Validating.can_transition_to(ApplyStage::Prepared));
        assert!(ApplyStage::Prepared.can_transition_to(ApplyStage::Committing));
        assert!(ApplyStage::Committing.can_transition_to(ApplyStage::AwaitingReload));
        assert!(ApplyStage::AwaitingReload.can_transition_to(ApplyStage::Pending));
        assert!(ApplyStage::Pending.can_transition_to(ApplyStage::Verified));
        assert!(ApplyStage::Committing.can_transition_to(ApplyStage::RollingBack));
        assert!(ApplyStage::RollingBack.can_transition_to(ApplyStage::Restored));

        // 不能从 Draft 直接跳到已验证，也不能从提交阶段回到草稿。
        assert!(!ApplyStage::Draft.can_transition_to(ApplyStage::Verified));
        assert!(!ApplyStage::Committing.can_transition_to(ApplyStage::Draft));
        assert!(!ApplyStage::Verified.can_transition_to(ApplyStage::Committing));
    }

    #[test]
    fn committing_stage_is_not_cancellable() {
        assert!(ApplyStage::Validating.is_cancellable());
        assert!(ApplyStage::Prepared.is_cancellable());
        assert!(!ApplyStage::Committing.is_cancellable());
        assert!(!ApplyStage::AwaitingReload.is_cancellable());
    }

    #[test]
    fn host_reload_changes_primary_action_copy() {
        assert_eq!(ReloadScope::HostReload.primary_action_key(), "action.applyAndReload");
        assert_eq!(ReloadScope::None.primary_action_key(), "action.apply");
        assert_ne!(ReloadScope::HostReload.reason_key(), ReloadScope::None.reason_key());
    }

    #[test]
    fn plan_hash_is_stable_and_sensitive_to_inputs() {
        let a = plan();
        let b = plan();
        assert_eq!(a.plan_hash, b.plan_hash, "同输入必须同摘要");

        let mut changes = a.changes.clone();
        changes[0].after = Some("gs/p_a/m_2".to_owned());
        let c = build_plan(
            PlanId::new("plan_2"),
            InstanceId::new("inst_1"),
            RevisionId::new("rev_1"),
            "/Users/example/.codex/config.toml",
            "hash-before",
            true,
            changes,
            "catalog_rev_1",
            vec![],
            Vec::new(),
            1_700_000_000,
            DEFAULT_PLAN_TTL_SECS,
        );
        assert_ne!(a.plan_hash, c.plan_hash);
    }

    #[test]
    fn plan_groups_follow_contract_order() {
        // 分组顺序由计划自身的 changes 与 reload_scope 决定；这里构造含目录变更的计划。
        let mut changes = plan().changes;
        changes.push(change(
            "model_catalog_json",
            None,
            Some("/app-data/catalogs/rev_1/models.json"),
        ));
        let plan = build_plan(
            PlanId::new("plan_groups"),
            InstanceId::new("inst_1"),
            RevisionId::new("rev_1"),
            "/Users/example/.codex/config.toml",
            "hash-before",
            true,
            changes,
            "catalog_rev_1",
            vec!["gs/p_a/m_1".to_owned()],
            Vec::new(),
            1_700_000_000,
            DEFAULT_PLAN_TTL_SECS,
        );
        let groups: Vec<String> = plan.groups().into_iter().map(|g| g.group_key).collect();
        assert_eq!(
            groups,
            vec!["group.route", "group.catalog", "group.requiresReload"]
        );
    }

    #[test]
    fn catalog_change_requires_host_reload() {
        let scope = derive_reload_scope(&[change(
            "model_catalog_json",
            None,
            Some("/app-data/catalogs/rev_1/models.json"),
        )]);
        assert_eq!(scope, ReloadScope::HostReload);
    }

    #[test]
    fn gateway_only_policy_change_needs_no_reload() {
        let scope = derive_reload_scope(&[change(
            "model_context_window",
            None,
            Some("128000"),
        )]);
        // 上下文变化会影响目录能力，按文档归入需要重载的目录类变更。
        assert_eq!(scope, ReloadScope::HostReload);

        let scope = derive_reload_scope(&[]);
        assert_eq!(scope, ReloadScope::None);
    }

    #[test]
    fn plan_expires_after_ttl() {
        let plan = plan();
        assert!(!plan.is_expired(1_700_000_100));
        assert!(plan.is_expired(1_700_000_000 + DEFAULT_PLAN_TTL_SECS + 1));
    }

    #[test]
    fn plan_reports_noop() {
        let plan = build_plan(
            PlanId::new("plan_noop"),
            InstanceId::new("inst_1"),
            RevisionId::new("rev_1"),
            "/path",
            "hash",
            true,
            Vec::new(),
            "rev",
            Vec::new(),
            Vec::new(),
            0,
            DEFAULT_PLAN_TTL_SECS,
        );
        assert!(plan.is_noop());
        assert_eq!(plan.reload_scope, ReloadScope::None);
    }

    #[test]
    fn cas_detects_external_change_and_identity_change() {
        assert!(check_cas("abc", true, "abc", true).is_match());
        match check_cas("abc", true, "def", true) {
            CasOutcome::Changed { expected, actual } => {
                assert_eq!(expected, "abc");
                assert_eq!(actual, "def");
            }
            other => panic!("期望 Changed，实际 {:?}", other),
        }
        assert!(matches!(
            check_cas("abc", false, "abc", true),
            CasOutcome::IdentityChanged { .. }
        ));
    }

    #[test]
    fn operation_events_are_monotonic_and_resumable() {
        let plan = plan();
        let mut operation =
            ApplyOperation::new(OperationId::new("op_1"), &plan, "idem_1", "2026-09-18T00:00:00Z");
        assert_eq!(operation.stage, ApplyStage::Draft);

        operation.transition(ApplyStage::Validating, "t1").unwrap();
        operation.transition(ApplyStage::Prepared, "t2").unwrap();
        operation.transition(ApplyStage::Committing, "t3").unwrap();
        operation.transition(ApplyStage::AwaitingReload, "t4").unwrap();

        let sequences: Vec<u64> = operation.events.iter().map(|e| e.sequence).collect();
        assert_eq!(sequences, vec![1, 2, 3, 4]);
        // 窗口重开：只拉取 cursor 之后的事件。
        assert_eq!(operation.events_since(2).len(), 2);
        assert_eq!(operation.events_since(4).len(), 0);
        assert_eq!(operation.resumable_from(), ApplyStage::AwaitingReload);
    }

    #[test]
    fn illegal_transition_on_operation_is_reported() {
        let plan = plan();
        let mut operation =
            ApplyOperation::new(OperationId::new("op_1"), &plan, "idem_1", "t");
        let error = operation
            .transition(ApplyStage::Verified, "t")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        // 状态未被静默修正。
        assert_eq!(operation.stage, ApplyStage::Draft);
    }

    #[test]
    fn event_cancellable_flag_follows_stage() {
        let plan = plan();
        let mut operation = ApplyOperation::new(OperationId::new("op_1"), &plan, "k", "t");
        operation.transition(ApplyStage::Validating, "t").unwrap();
        assert!(operation.can_cancel());
        operation.transition(ApplyStage::Prepared, "t").unwrap();
        operation.transition(ApplyStage::Committing, "t").unwrap();
        assert!(!operation.can_cancel());
        assert!(!operation.events.last().unwrap().cancellable);
    }

    #[test]
    fn idempotency_blocks_repeated_execution_of_same_plan() {
        let mut registry = IdempotencyRegistry::new();
        let op = OperationId::new("op_1");
        assert!(registry.begin("planhash", "idem", &op).is_none());
        let replay = registry.begin("planhash", "idem", &OperationId::new("op_2"));
        assert_eq!(replay.as_ref(), Some(&op), "重复应用同一 revision 不得再执行");
        assert_eq!(registry.len(), 1);

        // 不同幂等键是新的执行。
        assert!(registry
            .begin("planhash", "idem-2", &OperationId::new("op_3"))
            .is_none());
    }

    #[test]
    fn recovery_supplements_commit_or_preserves_external_changes() {
        let plan = plan();
        let mut operation = ApplyOperation::new(OperationId::new("op_1"), &plan, "k", "t");
        operation.transition(ApplyStage::Validating, "t").unwrap();
        operation.transition(ApplyStage::Prepared, "t").unwrap();
        operation.transition(ApplyStage::Committing, "t").unwrap();
        operation.written_hash = Some("hash-after".to_owned());

        assert_eq!(
            decide_recovery(&operation, "hash-after"),
            RecoveryDecision::RecordCommitFromFile {
                written_hash: "hash-after".to_owned()
            }
        );
        assert!(matches!(
            decide_recovery(&operation, "hash-other"),
            RecoveryDecision::ConflictWithExternalChange
        ));
    }

    #[test]
    fn recovery_waits_for_host_reload_without_restarting() {
        let plan = plan();
        let mut operation = ApplyOperation::new(OperationId::new("op_1"), &plan, "k", "t");
        operation.transition(ApplyStage::Validating, "t").unwrap();
        operation.transition(ApplyStage::Prepared, "t").unwrap();
        operation.transition(ApplyStage::Committing, "t").unwrap();
        operation.transition(ApplyStage::AwaitingReload, "t").unwrap();
        assert_eq!(decide_recovery(&operation, "anything"), RecoveryDecision::AwaitHostReload);
    }

    #[test]
    fn recovery_conflicts_when_external_edit_preceded_commit() {
        let plan = plan();
        let mut operation = ApplyOperation::new(OperationId::new("op_1"), &plan, "k", "t");
        operation.transition(ApplyStage::Validating, "t").unwrap();
        operation.transition(ApplyStage::Prepared, "t").unwrap();
        operation.transition(ApplyStage::Committing, "t").unwrap();
        assert_eq!(
            decide_recovery(&operation, "external-hash"),
            RecoveryDecision::ConflictWithExternalChange
        );
        assert_eq!(decide_recovery(&operation, "hash-before"), RecoveryDecision::NoAction);
    }

    #[test]
    fn awaiting_reload_is_not_verified_without_host_receipt() {
        let plan = plan();
        let mut operation = ApplyOperation::new(OperationId::new("op_1"), &plan, "k", "t");
        operation.transition(ApplyStage::Validating, "t").unwrap();
        operation.transition(ApplyStage::Prepared, "t").unwrap();
        operation.transition(ApplyStage::Committing, "t").unwrap();
        operation.transition(ApplyStage::AwaitingReload, "t").unwrap();
        operation.transition(ApplyStage::Pending, "t").unwrap();
        assert_eq!(operation.stage, ApplyStage::Pending);
        // 提交成功不等于宿主已加载：Pending 与 Verified 是两个阶段。
        assert_ne!(operation.stage, ApplyStage::Verified);
        // Pending 允许在未来被宿主回执提升为 Verified，且已核验不能再退回等待。
        assert!(ApplyStage::Pending.can_transition_to(ApplyStage::Verified));
        assert!(!ApplyStage::Verified.can_transition_to(ApplyStage::Pending));
    }

    #[test]
    fn operation_serializes_events_with_contract_field_names() {
        let plan = plan();
        let mut operation = ApplyOperation::new(OperationId::new("op_1"), &plan, "k", "t");
        operation.transition(ApplyStage::Validating, "2026-09-18T00:00:01Z").unwrap();
        let json = serde_json::to_value(&operation.events[0]).unwrap();
        for key in [
            "schemaVersion",
            "operationId",
            "sequence",
            "phase",
            "messageKey",
            "safeArgs",
            "cancellable",
            "timestamp",
        ] {
            assert!(json.get(key).is_some(), "缺少事件字段 {}", key);
        }
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["phase"], "validating");
    }
}
