//! 元数据存储：实体仓库、操作记录、schema migration 与事务 journal。
//!
//! 三条硬约束（来自 [数据与接口契约](../../../../docs/architecture/04-data-and-contracts.md)）：
//! - 明文 Key 永不进入元数据存储，只保存引用、版本与掩码。
//! - migration 单调递增，升级失败即回滚，不对旧库做部分迁移。
//! - 操作记录让中断的配置事务可判定、可恢复，而不是“重新对齐全部供应商”。

pub mod hub;
pub mod journal;
pub mod migration;
pub mod operation;
pub mod repository;
pub mod snapshot;
pub mod sqlite;

pub use hub::{HubStore, InMemoryHubStore, SqliteHubStore};
pub use journal::{JournalEntry, JournalStage, JournalStore};
pub use migration::{run_migrations, SchemaVersion, CURRENT_SCHEMA_VERSION};
pub use operation::{MemoryOperationStore, OperationKind, OperationState, OperationStore};
pub use repository::{InMemoryRepository, ReferenceCount, Repository};
pub use snapshot::{Revision, RouteSnapshot, RuntimePublication};
pub use sqlite::{SqliteOperationStore, SqliteRepository};
