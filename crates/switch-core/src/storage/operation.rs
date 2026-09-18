//! 配置事务的操作记录：计划、阶段、字段所有权与运行发布。
//!
//! 只保存摘要、引用与受管非秘密字段（见
//! [数据与接口契约](../../../../docs/architecture/04-data-and-contracts.md)）。
//! 记录是**可判定恢复**的唯一来源：窗口重开后先读快照，再订阅事件。

use crate::codex::config::{FieldOwnership, ManagedConfig};
use crate::codex::plan::{ApplyOperation, ApplyPlan};
use crate::domain::error::CoreError;
use crate::domain::ids::InstanceId;
use crate::storage::snapshot::{RuntimePublication, RouteSnapshot};
use crate::domain::model::Model;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// 事务类型：应用受管配置，或还原基线。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Apply,
    Restore,
}

/// 预览时冻结的提交输入。只含模型元数据、凭据引用，不含秘密。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedDeployment {
    pub source_hash: String,
    pub managed: ManagedConfig,
    pub routes: RouteSnapshot,
    pub models: Vec<Model>,
}

/// 一次配置事务的完整落库内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationState {
    pub kind: OperationKind,
    pub operation: ApplyOperation,
    /// 不可变计划；执行与恢复都按同一份计划判定，不重算。
    pub plan: ApplyPlan,
    /// 本工具在该实例上累计的字段所有权基线（合并后全量）。
    pub ownership: Vec<FieldOwnership>,
    /// 本次事务生成的目录文件绝对路径。
    pub catalog_path: String,
    /// 目录内容摘要；“目录已生成但配置未切换”时据此判断是否需要清理。
    pub catalog_hash: String,
    /// 已发布的运行快照；未发布时为空。
    pub publication: Option<RuntimePublication>,
    /// 老版本计划缺少冻结输入，必须重新预览，不能按当前数据猜测执行。
    #[serde(default)]
    pub prepared: Option<PreparedDeployment>,
}

impl OperationState {
    pub fn instance_id(&self) -> &InstanceId {
        &self.operation.instance_id
    }

    pub fn plan_id(&self) -> &str {
        self.plan.id.as_str()
    }

    /// 已进入终态的事务不再参与启动恢复。
    pub fn finished(&self) -> bool {
        self.operation.stage.is_terminal()
    }
}

/// 操作记录端口。
pub trait OperationStore: Send + Sync {
    fn save(&self, state: OperationState) -> Result<(), CoreError>;
    fn get(&self, operation_id: &str) -> Result<Option<OperationState>, CoreError>;
    /// 按计划 ID 查找。执行阶段只持有 planId 与 planHash。
    fn find_by_plan(&self, plan_id: &str) -> Result<Option<OperationState>, CoreError>;
    fn list(&self) -> Result<Vec<OperationState>, CoreError>;
    /// 需要启动恢复的记录：尚未进入终态的事务。
    fn unfinished(&self) -> Result<Vec<OperationState>, CoreError>;
    /// 该实例最近一次提交的字段所有权，用于还原时的三方比较。
    fn ownership(&self, instance_id: &InstanceId) -> Result<Vec<FieldOwnership>, CoreError>;
}

/// 内存实现。测试与离线演示使用。
#[derive(Debug, Default)]
pub struct MemoryOperationStore {
    states: Mutex<Vec<OperationState>>,
}

impl MemoryOperationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.states.lock().expect("锁未被污染").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl OperationStore for MemoryOperationStore {
    fn save(&self, state: OperationState) -> Result<(), CoreError> {
        let mut states = self.states.lock().expect("锁未被污染");
        match states
            .iter_mut()
            .find(|existing| existing.operation.id == state.operation.id)
        {
            Some(slot) => {
                // 已完成的事务不能被回退为未完成，避免恢复流程把终态改回去。
                if slot.finished() && !state.finished() {
                    return Err(CoreError::conflict("error.operationAlreadyFinished"));
                }
                *slot = state;
            }
            None => states.push(state),
        }
        Ok(())
    }

    fn get(&self, operation_id: &str) -> Result<Option<OperationState>, CoreError> {
        Ok(self
            .states
            .lock()
            .expect("锁未被污染")
            .iter()
            .find(|state| state.operation.id.as_str() == operation_id)
            .cloned())
    }

    fn find_by_plan(&self, plan_id: &str) -> Result<Option<OperationState>, CoreError> {
        Ok(self
            .states
            .lock()
            .expect("锁未被污染")
            .iter()
            .find(|state| state.plan_id() == plan_id)
            .cloned())
    }

    fn list(&self) -> Result<Vec<OperationState>, CoreError> {
        Ok(self.states.lock().expect("锁未被污染").clone())
    }

    fn unfinished(&self) -> Result<Vec<OperationState>, CoreError> {
        Ok(self
            .states
            .lock()
            .expect("锁未被污染")
            .iter()
            .filter(|state| !state.finished())
            .cloned()
            .collect())
    }

    fn ownership(&self, instance_id: &InstanceId) -> Result<Vec<FieldOwnership>, CoreError> {
        Ok(self
            .states
            .lock()
            .expect("锁未被污染")
            .iter()
            .filter(|state| state.instance_id() == instance_id)
            .filter(|state| state.operation.written_hash.is_some() && matches!(state.operation.stage,
                crate::codex::plan::ApplyStage::AwaitingReload | crate::codex::plan::ApplyStage::Pending
                | crate::codex::plan::ApplyStage::Verified | crate::codex::plan::ApplyStage::Restored))
            .last()
            .map(|state| state.ownership.clone())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::plan::{build_plan, ApplyOperation, ApplyStage, DEFAULT_PLAN_TTL_SECS};
    use crate::domain::ids::{OperationId, PlanId, RevisionId};

    fn plan() -> ApplyPlan {
        build_plan(
            PlanId::new("plan_1"),
            InstanceId::new("inst_1"),
            RevisionId::new("rev_1"),
            "/tmp/config.toml",
            "hash-before",
            true,
            Vec::new(),
            "rev_1",
            Vec::new(),
            Vec::new(),
            1_700_000_000,
            DEFAULT_PLAN_TTL_SECS,
        )
    }

    fn state(stage: ApplyStage) -> OperationState {
        let plan = plan();
        let mut operation = ApplyOperation::new(
            OperationId::new("op_1"),
            &plan,
            "idem-1",
            "2026-09-18T00:00:00Z",
        );
        operation.stage = stage;
        // 所有权只从“确实写过配置”的事务里读，因此夹具必须带上写入摘要。
        operation.written_hash = Some("written-hash".to_owned());
        OperationState {
            kind: OperationKind::Apply,
            operation,
            plan,
            ownership: vec![FieldOwnership::new("model", Some("gpt-5-codex".to_owned()))],
            catalog_path: "/tmp/models.json".to_owned(),
            catalog_hash: "catalog-hash".to_owned(),
            publication: None,
            prepared: None,
        }
    }

    #[test]
    fn saving_replaces_the_same_operation_and_lists_unfinished_only() {
        let store = MemoryOperationStore::new();
        store.save(state(ApplyStage::Prepared)).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.unfinished().unwrap().len(), 1);

        store.save(state(ApplyStage::AwaitingReload)).unwrap();
        assert_eq!(store.len(), 1, "同一 operationId 只保留一条记录");
        assert_eq!(
            store.get("op_1").unwrap().unwrap().operation.stage,
            ApplyStage::AwaitingReload
        );

        store.save(state(ApplyStage::Verified)).unwrap();
        assert!(store.unfinished().unwrap().is_empty(), "终态不再参与启动恢复");
    }

    #[test]
    fn finished_operation_cannot_regress_to_unfinished() {
        let store = MemoryOperationStore::new();
        store.save(state(ApplyStage::Verified)).unwrap();
        let error = store.save(state(ApplyStage::Prepared)).unwrap_err();
        assert_eq!(error.code, crate::domain::error::ErrorCode::Conflict);
    }

    #[test]
    fn plan_lookup_returns_the_same_record() {
        let store = MemoryOperationStore::new();
        store.save(state(ApplyStage::Prepared)).unwrap();
        let found = store.find_by_plan("plan_1").unwrap().unwrap();
        assert_eq!(found.operation.id.as_str(), "op_1");
        assert!(store.find_by_plan("plan_missing").unwrap().is_none());
    }

    #[test]
    fn ownership_is_read_back_per_instance() {
        let store = MemoryOperationStore::new();
        store.save(state(ApplyStage::Verified)).unwrap();
        let ownership = store.ownership(&InstanceId::new("inst_1")).unwrap();
        assert_eq!(ownership.len(), 1);
        assert_eq!(ownership[0].key_path, "model");
        assert!(store.ownership(&InstanceId::new("inst_other")).unwrap().is_empty());
    }
}
