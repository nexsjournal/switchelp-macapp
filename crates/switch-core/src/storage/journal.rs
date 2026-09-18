//! 配置事务 journal。
//!
//! journal 记录“准备 → 加锁重读 → 提交 → 重载 → 核验”每一步的可判定证据：
//! 期望摘要、写入摘要、阶段、时间。崩溃后据此决定补记完成、回滚，或转入冲突。

use crate::codex::plan::ApplyStage;
use crate::domain::error::CoreError;
use crate::domain::ids::{InstanceId, OperationId, RevisionId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// journal 阶段：比应用状态机更贴近文件系统事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalStage {
    /// 目录已生成，尚未切换配置。
    CatalogWritten,
    /// 配置已替换，DB 未记完成。
    ConfigReplaced,
    /// DB 已提交，宿主未重载。
    Committed,
    /// 宿主已确认加载。
    Verified,
    /// 已回滚到基线。
    RolledBack,
    /// 因外部修改转入冲突，保留当前文件。
    Conflicted,
}

impl JournalStage {
    /// 该阶段是否仍可能需要在启动时恢复。
    pub fn needs_recovery(self) -> bool {
        matches!(
            self,
            JournalStage::CatalogWritten | JournalStage::ConfigReplaced | JournalStage::Committed
        )
    }

    /// 崩溃恢复表里的对应策略 key，供 UI 说明。
    pub fn recovery_note_key(self) -> &'static str {
        match self {
            JournalStage::CatalogWritten => "journal.note.catalogOnly",
            JournalStage::ConfigReplaced => "journal.note.configReplaced",
            JournalStage::Committed => "journal.note.awaitingReload",
            JournalStage::Verified => "journal.note.verified",
            JournalStage::RolledBack => "journal.note.rolledBack",
            JournalStage::Conflicted => "journal.note.conflicted",
        }
    }
}

/// 一条 journal 记录。只保存摘要与引用，不保存配置原文或秘密。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub operation_id: OperationId,
    pub instance_id: InstanceId,
    pub revision_id: RevisionId,
    pub stage: JournalStage,
    /// 事务开始前的配置摘要。
    pub expected_config_hash: String,
    /// 本工具写入后的配置摘要；未写入时为空。
    pub written_hash: Option<String>,
    /// 私有备份引用（可能含秘密，绝不混入诊断包）。
    pub backup_ref: Option<String>,
    /// 未被其他操作引用的目录修订，用于清理。
    pub orphan_catalogs: Vec<String>,
    pub updated_at_unix: i64,
    /// 是否已完成（终态）。已完成的记录不再参与启动恢复。
    pub finished: bool,
}

impl JournalEntry {
    pub fn new(
        operation_id: OperationId,
        instance_id: InstanceId,
        revision_id: RevisionId,
        expected_config_hash: impl Into<String>,
        now_unix: i64,
    ) -> Self {
        Self {
            operation_id,
            instance_id,
            revision_id,
            stage: JournalStage::CatalogWritten,
            expected_config_hash: expected_config_hash.into(),
            written_hash: None,
            backup_ref: None,
            orphan_catalogs: Vec::new(),
            updated_at_unix: now_unix,
            finished: false,
        }
    }

    /// 记录配置替换结果。
    pub fn mark_config_replaced(&mut self, written_hash: impl Into<String>, now_unix: i64) {
        self.written_hash = Some(written_hash.into());
        self.stage = JournalStage::ConfigReplaced;
        self.updated_at_unix = now_unix;
    }

    pub fn mark_committed(&mut self, now_unix: i64) {
        self.stage = JournalStage::Committed;
        self.updated_at_unix = now_unix;
    }

    pub fn mark_verified(&mut self, now_unix: i64) {
        self.stage = JournalStage::Verified;
        self.finished = true;
        self.updated_at_unix = now_unix;
    }

    pub fn mark_rolled_back(&mut self, now_unix: i64) {
        self.stage = JournalStage::RolledBack;
        self.finished = true;
        self.updated_at_unix = now_unix;
    }

    pub fn mark_conflicted(&mut self, now_unix: i64) {
        self.stage = JournalStage::Conflicted;
        self.finished = true;
        self.updated_at_unix = now_unix;
    }

    /// 由应用状态机同步 journal 阶段。
    pub fn sync_from_stage(&mut self, stage: ApplyStage, now_unix: i64) {
        match stage {
            ApplyStage::Verified => self.mark_verified(now_unix),
            ApplyStage::Restored => self.mark_rolled_back(now_unix),
            ApplyStage::Conflict => self.mark_conflicted(now_unix),
            ApplyStage::AwaitingReload | ApplyStage::Pending => self.mark_committed(now_unix),
            _ => {}
        }
    }

    /// 是否需要拉起一次启动恢复。
    pub fn needs_startup_recovery(&self) -> bool {
        !self.finished && self.stage.needs_recovery()
    }

    /// 当前阶段对应的恢复说明 key，供 UI 解释“为什么还停在等待状态”。
    pub fn recovery_note_key(&self) -> &'static str {
        self.stage.recovery_note_key()
    }
}

/// journal 存储端口。
pub trait JournalStore: Send + Sync {
    fn append(&self, entry: JournalEntry) -> Result<(), CoreError>;
    fn update(&self, entry: JournalEntry) -> Result<(), CoreError>;
    fn list(&self) -> Result<Vec<JournalEntry>, CoreError>;
    fn get(&self, operation_id: &str) -> Result<Option<JournalEntry>, CoreError>;
}

/// 内存实现。测试与离线演示使用。
#[derive(Debug, Default)]
pub struct MemoryJournal {
    entries: std::sync::Mutex<Vec<JournalEntry>>,
}

impl MemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// 仅返回需要启动恢复的记录。
    pub fn pending_recovery(&self) -> Result<Vec<JournalEntry>, CoreError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(JournalEntry::needs_startup_recovery)
            .collect())
    }
}

impl JournalStore for MemoryJournal {
    fn append(&self, entry: JournalEntry) -> Result<(), CoreError> {
        let mut entries = self.entries.lock().expect("锁未被污染");
        if entries
            .iter()
            .any(|existing| existing.operation_id == entry.operation_id)
        {
            return Err(CoreError::conflict("error.operationAlreadyRecorded"));
        }
        entries.push(entry);
        Ok(())
    }

    fn update(&self, entry: JournalEntry) -> Result<(), CoreError> {
        let mut entries = self.entries.lock().expect("锁未被污染");
        let slot = entries
            .iter_mut()
            .find(|existing| existing.operation_id == entry.operation_id)
            .ok_or_else(|| CoreError::not_found("journal 记录"))?;
        // 已完成的记录不可被回退，避免“恢复完又被旧事件改回去”。
        if slot.finished && !entry.finished {
            return Err(CoreError::conflict("error.journalAlreadyFinished"));
        }
        *slot = entry;
        Ok(())
    }

    fn list(&self) -> Result<Vec<JournalEntry>, CoreError> {
        Ok(self.entries.lock().expect("锁未被污染").clone())
    }

    fn get(&self, operation_id: &str) -> Result<Option<JournalEntry>, CoreError> {
        Ok(self
            .entries
            .lock()
            .expect("锁未被污染")
            .iter()
            .find(|entry| entry.operation_id.as_str() == operation_id)
            .cloned())
    }
}

/// 文件型 journal：每行一条 JSON，追加友好且崩溃后仍可读回。
///
/// 只写入摘要与引用，不写配置原文，因此不需要额外加密。
#[derive(Debug, Clone)]
pub struct FileJournal {
    path: PathBuf,
}

impl FileJournal {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_all(&self) -> Result<Vec<JournalEntry>, CoreError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(&self.path)?;
        let mut entries: Vec<JournalEntry> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let entry: JournalEntry = serde_json::from_str(line)
                .map_err(|error| CoreError::internal(format!("journal 解析失败: {error}")))?;
            entries.push(entry);
        }
        Ok(entries)
    }

    fn rewrite(&self, entries: &[JournalEntry]) -> Result<(), CoreError> {
        let mut text = String::new();
        for entry in entries {
            text.push_str(
                &serde_json::to_string(entry)
                    .map_err(|error| CoreError::internal(format!("journal 序列化失败: {error}")))?,
            );
            text.push('\n');
        }
        crate::codex::config::write_atomic(&self.path, &text)
    }
}

impl JournalStore for FileJournal {
    fn append(&self, entry: JournalEntry) -> Result<(), CoreError> {
        let mut entries = self.read_all()?;
        if entries
            .iter()
            .any(|existing| existing.operation_id == entry.operation_id)
        {
            return Err(CoreError::conflict("error.operationAlreadyRecorded"));
        }
        entries.push(entry);
        self.rewrite(&entries)
    }

    fn update(&self, entry: JournalEntry) -> Result<(), CoreError> {
        let mut entries = self.read_all()?;
        let slot = entries
            .iter_mut()
            .find(|existing| existing.operation_id == entry.operation_id)
            .ok_or_else(|| CoreError::not_found("journal 记录"))?;
        if slot.finished && !entry.finished {
            return Err(CoreError::conflict("error.journalAlreadyFinished"));
        }
        *slot = entry;
        self.rewrite(&entries)
    }

    fn list(&self) -> Result<Vec<JournalEntry>, CoreError> {
        self.read_all()
    }

    fn get(&self, operation_id: &str) -> Result<Option<JournalEntry>, CoreError> {
        Ok(self
            .read_all()?
            .into_iter()
            .find(|entry| entry.operation_id.as_str() == operation_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(op: &str, revision: &str) -> JournalEntry {
        JournalEntry::new(
            OperationId::new(op),
            InstanceId::new("inst_1"),
            RevisionId::new(revision),
            "hash-before",
            1_700_000_000,
        )
    }

    #[test]
    fn new_entry_awaits_recovery_at_catalog_stage() {
        let record = entry("op_1", "rev_1");
        assert_eq!(record.stage, JournalStage::CatalogWritten);
        assert!(record.needs_startup_recovery());
        assert!(!record.finished);
    }

    #[test]
    fn finished_entries_stop_participating_in_recovery() {
        let mut record = entry("op_1", "rev_1");
        record.mark_verified(1_700_000_010);
        assert!(!record.needs_startup_recovery());
        assert!(record.finished);

        let mut record = entry("op_2", "rev_1");
        record.mark_config_replaced("hash-after", 1_700_000_010);
        record.mark_conflicted(1_700_000_020);
        assert!(!record.needs_startup_recovery());
    }

    #[test]
    fn committed_stage_still_needs_recovery_until_verified() {
        let mut record = entry("op_1", "rev_1");
        record.mark_config_replaced("hash-after", 1);
        record.mark_committed(2);
        assert_eq!(record.stage, JournalStage::Committed);
        assert!(record.needs_startup_recovery());
        assert_eq!(record.recovery_note_key(), "journal.note.awaitingReload");
    }

    #[test]
    fn stage_sync_follows_apply_state_machine() {
        let mut record = entry("op_1", "rev_1");
        record.sync_from_stage(ApplyStage::Committing, 1);
        assert_eq!(
            record.stage,
            JournalStage::CatalogWritten,
            "提交中不改变 journal 阶段"
        );

        record.sync_from_stage(ApplyStage::AwaitingReload, 2);
        assert_eq!(record.stage, JournalStage::Committed);

        record.sync_from_stage(ApplyStage::Pending, 3);
        assert_eq!(
            record.stage,
            JournalStage::Committed,
            "等待重载仍是 Committed"
        );

        record.sync_from_stage(ApplyStage::Verified, 4);
        assert_eq!(record.stage, JournalStage::Verified);
        assert!(record.finished);
    }

    #[test]
    fn memory_journal_rejects_duplicate_operations() {
        let journal = MemoryJournal::new();
        journal.append(entry("op_1", "rev_1")).unwrap();
        let error = journal.append(entry("op_1", "rev_1")).unwrap_err();
        assert_eq!(error.code, crate::domain::error::ErrorCode::Conflict);
        assert_eq!(journal.list().unwrap().len(), 1);
    }

    #[test]
    fn memory_journal_lists_only_pending_recovery() {
        let journal = MemoryJournal::new();
        journal.append(entry("op_1", "rev_1")).unwrap();
        let mut finished = entry("op_2", "rev_1");
        finished.mark_verified(1);
        journal.append(finished).unwrap();

        let pending = journal.pending_recovery().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].operation_id.as_str(), "op_1");
    }

    #[test]
    fn finished_records_cannot_be_regressed() {
        let journal = MemoryJournal::new();
        let mut record = entry("op_1", "rev_1");
        record.mark_verified(10);
        journal.append(record.clone()).unwrap();

        let mut regressed = record.clone();
        regressed.finished = false;
        regressed.stage = JournalStage::CatalogWritten;
        let error = journal.update(regressed).unwrap_err();
        assert_eq!(error.code, crate::domain::error::ErrorCode::Conflict);
    }

    #[test]
    fn update_unknown_operation_is_reported() {
        let journal = MemoryJournal::new();
        let error = journal.update(entry("op_missing", "rev_1")).unwrap_err();
        assert_eq!(error.code, crate::domain::error::ErrorCode::NotFound);
    }

    #[test]
    fn file_journal_survives_reopen_and_keeps_stages() {
        let dir = std::env::temp_dir().join(format!("gptswitch-journal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("journal.jsonl");

        {
            let journal = FileJournal::new(&path);
            let mut record = entry("op_1", "rev_1");
            record.mark_config_replaced("hash-after", 5);
            journal.append(record).unwrap();
        }

        let reopened = FileJournal::new(&path);
        let record = reopened.get("op_1").unwrap().unwrap();
        assert_eq!(record.stage, JournalStage::ConfigReplaced);
        assert_eq!(record.written_hash.as_deref(), Some("hash-after"));
        assert!(record.needs_startup_recovery());

        // 完成后再读回：不再参与恢复。
        let mut record = record;
        record.mark_verified(6);
        reopened.update(record).unwrap();
        assert!(
            FileJournal::new(&path)
                .get("op_1")
                .unwrap()
                .unwrap()
                .finished
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_journal_never_stores_config_body_or_secret() {
        let dir = std::env::temp_dir().join(format!("gptswitch-journal-s-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("journal.jsonl");
        let journal = FileJournal::new(&path);
        let mut record = entry("op_1", "rev_1");
        record.backup_ref = Some("private/backups/op_1.toml.enc".to_owned());
        record.orphan_catalogs = vec!["rev_0006".to_owned()];
        journal.append(record).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("sk-"));
        assert!(!text.contains("model ="));
        assert!(text.contains("backupRef"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_journal_serializes_with_contract_field_names() {
        let record = entry("op_1", "rev_1");
        let json = serde_json::to_value(&record).unwrap();
        for key in [
            "operationId",
            "instanceId",
            "revisionId",
            "stage",
            "expectedConfigHash",
            "writtenHash",
            "backupRef",
            "orphanCatalogs",
            "updatedAtUnix",
            "finished",
        ] {
            assert!(json.get(key).is_some(), "缺少 journal 字段 {key}");
        }
        assert_eq!(json["stage"], "catalog_written");
    }
}
