//! SQLite 元数据仓库。关系约束与版本比较和写入位于同一 IMMEDIATE 事务。
//! JSON 保存领域 DTO，索引列负责唯一性与外键；秘密值不属于任何实体。

use super::{migration::run_migrations, operation::OperationState, OperationStore, Repository};
use crate::codex::config::FieldOwnership;
use crate::domain::{
    credential::Credential,
    error::CoreError,
    ids::{CredentialId, InstanceId, ModelId, ProviderId},
    model::{qualified_display_name, HostState, Model},
    provider::{Protocol, Provider},
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

pub struct SqliteRepository {
    connection: Mutex<Connection>,
}

pub(crate) const INITIAL_SCHEMA: &str = "
CREATE TABLE providers (
    id TEXT PRIMARY KEY NOT NULL,
    payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE TABLE credentials (
    id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE RESTRICT,
    payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE INDEX credentials_provider ON credentials(provider_id);
CREATE TABLE models (
    id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE RESTRICT,
    upstream_id TEXT NOT NULL COLLATE BINARY,
    protocol TEXT NOT NULL,
    alias TEXT NOT NULL UNIQUE COLLATE BINARY,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    UNIQUE(provider_id, upstream_id, protocol)
);
CREATE TABLE revisions (id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE TABLE operations (id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload)));
";

/// v3：应用设置表。
///
/// 只放「应用自己的开关」，不放任何实体数据——实体各有专表，加一张宽表最容易长成
/// 谁也说不清的第二份真相。共存模式的开关是第一个用户。
pub(crate) const SETTINGS_SCHEMA: &str = "
CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
";

impl SqliteRepository {
    /// 父目录由宿主建立；不把任意数据库路径自动当成应用数据目录。
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        Self::initialize(Connection::open(path).map_err(db_error)?)
    }

    pub fn in_memory() -> Result<Self, CoreError> {
        Self::initialize(Connection::open_in_memory().map_err(db_error)?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, CoreError> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        // 在写锁中检查版本，多个应用连接不能各自执行初始 migration。
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let current = tx
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .map_err(db_error)?;
        let version = run_migrations(current, |step| match step.to {
            1 => tx.execute_batch(INITIAL_SCHEMA).map_err(db_error),
            2 => migrate_display_name_prefixes(&tx),
            3 => tx.execute_batch(SETTINGS_SCHEMA).map_err(db_error),
            4 => tx.execute_batch(super::hub::HUB_SCHEMA).map_err(db_error),
            _ => Err(CoreError::internal("未知的数据库升级步骤")),
        })?;
        tx.pragma_update(None, "user_version", version)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, CoreError> {
        self.connection
            .lock()
            .map_err(|_| CoreError::internal("数据库锁不可用"))
    }
}

/// v2：给「还是默认形状」的模型显示名补上供应商前缀。
///
/// v2 之前显示名只有上游自己的名字。Codex 的模型菜单是一份**扁平列表**，多个供应商下
/// 的同名模型在里面长得一模一样，选错只会表现为「请求打到了别家」。只补默认形状的
/// 名字——等于上游 ID、等于发现值、或用户从没动过的；真正被用户改写成别的样子的名字
/// 保持不动，迁移不替用户改主意。
pub(crate) fn migrate_display_name_prefixes(tx: &Transaction<'_>) -> Result<(), CoreError> {
    let mut provider_names: HashMap<String, String> = HashMap::new();
    {
        let mut statement = tx
            .prepare("SELECT id, payload FROM providers")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_error)?;
        for row in rows {
            let (id, payload) = row.map_err(db_error)?;
            let provider: Provider = decode(&payload)?;
            provider_names.insert(id, provider.name);
        }
    }

    let mut stored: Vec<(String, String)> = Vec::new();
    {
        let mut statement = tx
            .prepare("SELECT id, payload FROM models")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_error)?;
        for row in rows {
            stored.push(row.map_err(db_error)?);
        }
    }

    for (id, payload) in stored {
        let mut model: Model = decode(&payload)?;
        let Some(provider_name) = provider_names.get(model.provider_id.as_str()) else {
            continue;
        };
        let layer = model.display_name_layer.clone();
        let base = layer
            .user_value
            .clone()
            .or_else(|| layer.discovered.clone())
            .unwrap_or_else(|| model.display_name.clone());
        let default_shaped = base == model.upstream_id
            || layer.discovered.as_deref() == Some(base.as_str())
            || (!layer.overridden && layer.discovered.is_none());
        if !default_shaped {
            continue;
        }
        let qualified = qualified_display_name(provider_name, &base);
        model.display_name = qualified.clone();
        model.display_name_layer.discovered = Some(qualified);
        model.display_name_layer.user_value = None;
        model.display_name_layer.overridden = false;
        if model.in_catalog {
            // 菜单里的名字变了，磁盘上的目录就旧了：退回待应用，让用户自己决定何时生效。
            model.host_state = HostState::PendingApply;
        }
        tx.execute(
            "UPDATE models SET payload = ?1 WHERE id = ?2",
            params![encode(&model)?, id],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value).map_err(|_| CoreError::internal("元数据编码失败"))
}

pub(crate) fn decode<T: DeserializeOwned>(value: &str) -> Result<T, CoreError> {
    serde_json::from_str(value).map_err(|_| CoreError::internal("元数据结构损坏，已停止读取"))
}

pub(crate) fn one<T: DeserializeOwned>(
    connection: &Connection,
    sql: &str,
    id: &str,
) -> Result<Option<T>, CoreError> {
    let value: Option<String> = connection
        .query_row(sql, [id], |row| row.get(0))
        .optional()
        .map_err(db_error)?;
    value.map(|v| decode(&v)).transpose()
}

fn list<T: DeserializeOwned>(
    connection: &Connection,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<T>, CoreError> {
    let mut statement = connection.prepare(sql).map_err(db_error)?;
    let rows = statement
        .query_map(parameters, |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    rows.map(|row| decode(&row.map_err(db_error)?)).collect()
}

pub(crate) fn db_error(error: rusqlite::Error) -> CoreError {
    // 不将 SQL、绑定参数或 SQLite 原始错误文本暴露给 UI。
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::ConstraintViolation) => {
            CoreError::conflict("error.storageConstraint")
                .with_detail("记录被引用，或模型身份、目录别名重复".to_owned())
        }
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            CoreError::conflict("error.storageBusy")
        }
        _ => CoreError::internal("元数据库读写失败"),
    }
}

fn protocol_key(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Responses => "responses",
        Protocol::ChatCompletions => "chat_completions",
    }
}

impl Repository for SqliteRepository {
    fn list_providers(&self) -> Result<Vec<Provider>, CoreError> {
        list(
            &*self.lock()?,
            "SELECT payload FROM providers ORDER BY json_extract(payload, '$.name'), id",
            [],
        )
    }

    fn get_provider(&self, id: &ProviderId) -> Result<Option<Provider>, CoreError> {
        one(
            &*self.lock()?,
            "SELECT payload FROM providers WHERE id = ?1",
            id.as_str(),
        )
    }

    fn save_provider(
        &self,
        mut provider: Provider,
        expected_version: u64,
    ) -> Result<Provider, CoreError> {
        provider.validate()?;
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let previous: Option<Provider> = one(
            &tx,
            "SELECT payload FROM providers WHERE id = ?1",
            provider.id.as_str(),
        )?;
        match &previous {
            Some(existing) => {
                existing.check_version(expected_version)?;
                provider.version = existing.version + 1;
                provider.created_at = existing.created_at.clone();
            }
            None if expected_version == 0 => provider.version = 1,
            None => return Err(CoreError::conflict("error.providerNotFound")),
        }
        if let Some(active) = &provider.active_credential_id {
            let credential: Credential = one(
                &tx,
                "SELECT payload FROM credentials WHERE id = ?1",
                active.as_str(),
            )?
            .ok_or_else(|| CoreError::not_found("Key"))?;
            if credential.provider_id != provider.id {
                return Err(CoreError::validation("不能选择其他供应商的 Key"));
            }
            credential.mark_active()?;
        }
        tx.execute("INSERT INTO providers(id,payload) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
            params![provider.id.as_str(), encode(&provider)?]).map_err(db_error)?;
        // 继承协议的模型索引同步更新；冲突时整个供应商变更回滚。
        tx.execute("UPDATE models SET protocol=?1 WHERE provider_id=?2 AND json_extract(payload, '$.protocolOverride') IS NULL",
            params![protocol_key(provider.protocol), provider.id.as_str()]).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(provider)
    }

    fn delete_provider(&self, id: &ProviderId) -> Result<(), CoreError> {
        let affected = self
            .lock()?
            .execute("DELETE FROM providers WHERE id=?1", [id.as_str()])
            .map_err(db_error)?;
        if affected == 0 {
            return Err(CoreError::not_found("供应商"));
        }
        Ok(())
    }

    fn list_credentials(&self, provider_id: &ProviderId) -> Result<Vec<Credential>, CoreError> {
        list(&*self.lock()?, "SELECT payload FROM credentials WHERE provider_id=?1 ORDER BY json_extract(payload, '$.createdAt'), id", [provider_id.as_str()])
    }

    fn get_credential(&self, id: &CredentialId) -> Result<Option<Credential>, CoreError> {
        one(
            &*self.lock()?,
            "SELECT payload FROM credentials WHERE id=?1",
            id.as_str(),
        )
    }

    fn save_credential(&self, credential: Credential) -> Result<Credential, CoreError> {
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let previous: Option<Credential> = one(
            &tx,
            "SELECT payload FROM credentials WHERE id=?1",
            credential.id.as_str(),
        )?;
        if let Some(existing) = previous {
            if credential.version != existing.version + 1
                || credential.provider_id != existing.provider_id
            {
                return Err(CoreError::conflict("error.credentialVersionConflict"));
            }
        } else if credential.version != 1 {
            return Err(CoreError::conflict("error.credentialNotFound"));
        }
        tx.execute("INSERT INTO credentials(id,provider_id,payload) VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
            params![credential.id.as_str(), credential.provider_id.as_str(), encode(&credential)?]).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(credential)
    }

    fn delete_credential(&self, id: &CredentialId) -> Result<(), CoreError> {
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM providers WHERE json_extract(payload, '$.activeCredentialId')=?1)",
            [id.as_str()], |row| row.get(0)).map_err(db_error)?;
        if active {
            return Err(CoreError::conflict("error.credentialInUse"));
        }
        if tx
            .execute("DELETE FROM credentials WHERE id=?1", [id.as_str()])
            .map_err(db_error)?
            == 0
        {
            return Err(CoreError::not_found("Key"));
        }
        tx.commit().map_err(db_error)
    }

    fn list_models(&self) -> Result<Vec<Model>, CoreError> {
        list(
            &*self.lock()?,
            "SELECT payload FROM models ORDER BY alias",
            [],
        )
    }

    fn get_model(&self, id: &ModelId) -> Result<Option<Model>, CoreError> {
        one(
            &*self.lock()?,
            "SELECT payload FROM models WHERE id=?1",
            id.as_str(),
        )
    }

    fn save_model(&self, mut model: Model, expected_version: u64) -> Result<Model, CoreError> {
        model.validate_draft()?;
        model.policy.validate(false)?;
        // serde 的透明包装类型并不会自动执行 parse 校验。
        crate::domain::ids::CatalogAlias::parse(model.catalog_alias.as_str())?;
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let provider: Provider = one(
            &tx,
            "SELECT payload FROM providers WHERE id=?1",
            model.provider_id.as_str(),
        )?
        .ok_or_else(|| CoreError::not_found("供应商"))?;
        let previous: Option<Model> = one(
            &tx,
            "SELECT payload FROM models WHERE id=?1",
            model.id.as_str(),
        )?;
        match previous {
            Some(existing) => {
                if existing.version != expected_version {
                    return Err(CoreError::conflict("error.modelVersionConflict"));
                }
                if existing.provider_id != model.provider_id {
                    return Err(CoreError::validation("已有模型不能更换供应商"));
                }
                if existing.upstream_id != model.upstream_id
                    && existing.catalog_alias == model.catalog_alias
                {
                    return Err(CoreError::validation("更换上游模型需要新的目录别名"));
                }
                model.version = existing.version + 1;
                model.created_at = existing.created_at;
            }
            None if expected_version == 0 => model.version = 1,
            None => return Err(CoreError::conflict("error.modelNotFound")),
        }
        tx.execute("INSERT INTO models(id,provider_id,upstream_id,protocol,alias,payload) VALUES (?1,?2,?3,?4,?5,?6)
            ON CONFLICT(id) DO UPDATE SET upstream_id=excluded.upstream_id,protocol=excluded.protocol,alias=excluded.alias,payload=excluded.payload",
            params![model.id.as_str(), model.provider_id.as_str(), model.upstream_id,
                protocol_key(model.protocol_override.unwrap_or(provider.protocol)), model.catalog_alias.as_str(), encode(&model)?]).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(model)
    }

    fn delete_model(&self, id: &ModelId) -> Result<(), CoreError> {
        if self
            .lock()?
            .execute("DELETE FROM models WHERE id=?1", [id.as_str()])
            .map_err(db_error)?
            == 0
        {
            return Err(CoreError::not_found("模型"));
        }
        Ok(())
    }

    fn setting(&self, key: &str) -> Result<Option<String>, CoreError> {
        self.lock()?
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(db_error)
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.lock()?
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )
            .map_err(db_error)?;
        Ok(())
    }
}

/// SQLite 版操作记录存储。与实体仓库共用同一数据库文件，但各自持有连接。
///
/// 只写入非秘密内容：计划、阶段、字段所有权与目录摘要。
pub struct SqliteOperationStore {
    connection: Mutex<Connection>,
}

impl SqliteOperationStore {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        let mut connection = Connection::open(path).map_err(db_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let current = tx
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .map_err(db_error)?;
        let version = run_migrations(current, |step| match step.to {
            1 => tx.execute_batch(INITIAL_SCHEMA).map_err(db_error),
            2 => migrate_display_name_prefixes(&tx),
            3 => tx.execute_batch(SETTINGS_SCHEMA).map_err(db_error),
            4 => tx.execute_batch(super::hub::HUB_SCHEMA).map_err(db_error),
            _ => Err(CoreError::internal("未知的数据库升级步骤")),
        })?;
        tx.pragma_update(None, "user_version", version)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn in_memory() -> Result<Self, CoreError> {
        let connection = Connection::open_in_memory().map_err(db_error)?;
        connection.execute_batch(INITIAL_SCHEMA).map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, CoreError> {
        self.connection
            .lock()
            .map_err(|_| CoreError::internal("数据库锁不可用"))
    }
}

impl OperationStore for SqliteOperationStore {
    fn save(&self, state: OperationState) -> Result<(), CoreError> {
        let mut connection = self.lock()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT payload FROM operations WHERE id = ?1",
                [state.operation.id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(payload) = previous {
            let existing: OperationState = decode(&payload)?;
            // 已完成的事务不能被回退为未完成，避免恢复流程把终态改回去。
            if existing.finished() && !state.finished() {
                return Err(CoreError::conflict("error.operationAlreadyFinished"));
            }
        }
        tx.execute(
            "INSERT INTO operations(id,payload) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
            params![state.operation.id.as_str(), encode(&state)?],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)
    }

    fn get(&self, operation_id: &str) -> Result<Option<OperationState>, CoreError> {
        one(
            &*self.lock()?,
            "SELECT payload FROM operations WHERE id=?1",
            operation_id,
        )
    }

    fn find_by_plan(&self, plan_id: &str) -> Result<Option<OperationState>, CoreError> {
        one(
            &*self.lock()?,
            "SELECT payload FROM operations WHERE json_extract(payload, '$.plan.id')=?1 ORDER BY rowid DESC LIMIT 1",
            plan_id,
        )
    }

    fn list(&self) -> Result<Vec<OperationState>, CoreError> {
        list(
            &*self.lock()?,
            "SELECT payload FROM operations ORDER BY rowid",
            [],
        )
    }

    fn unfinished(&self) -> Result<Vec<OperationState>, CoreError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|state| !state.finished())
            .collect())
    }

    fn ownership(&self, instance_id: &InstanceId) -> Result<Vec<FieldOwnership>, CoreError> {
        let states: Vec<OperationState> = list(
            &*self.lock()?,
            "SELECT payload FROM operations WHERE json_extract(payload, '$.operation.instanceId')=?1 ORDER BY rowid",
            [instance_id.as_str()],
        )?;
        Ok(states
            .iter()
            .filter(|state| {
                state.operation.written_hash.is_some()
                    && matches!(
                        state.operation.stage,
                        crate::codex::plan::ApplyStage::AwaitingReload
                            | crate::codex::plan::ApplyStage::Pending
                            | crate::codex::plan::ApplyStage::Verified
                            | crate::codex::plan::ApplyStage::Restored
                    )
            })
            .next_back()
            .map(|state| state.ownership.clone())
            .unwrap_or_default())
    }
}
