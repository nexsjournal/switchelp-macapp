//! Schema migration。
//!
//! 规则：版本号单调递增；每一步要么完整应用要么完全不应用；升级失败时
//! 保留原版本，避免“半迁移”的库被后续代码误读。

use crate::domain::error::CoreError;
use serde::{Deserialize, Serialize};

/// 当前期望的 schema 版本。
pub const CURRENT_SCHEMA_VERSION: u32 = 4;

/// 一次 migration 步骤。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaVersion {
    pub from: u32,
    pub to: u32,
    pub description: &'static str,
}

/// 内置 migration 序列。追加新版本时只能往后加，不能改写历史步骤。
pub const MIGRATIONS: [SchemaVersion; 4] = [
    SchemaVersion {
        from: 0,
        to: 1,
        description: "初始实体表：providers / credentials / models / revisions / operations",
    },
    SchemaVersion {
        from: 1,
        to: 2,
        description: "模型显示名补上供应商前缀：扁平菜单里跨供应商的同名模型要能区分",
    },
    SchemaVersion {
        from: 2,
        to: 3,
        description: "应用设置表：共存模式（Bridge）之类的应用级开关",
    },
    SchemaVersion {
        from: 3,
        to: 4,
        description: "扩展板块表：工具探测缓存、已装技能归属、订阅源与资讯条目",
    },
];

/// 依次应用 migration，返回最终版本。
///
/// `apply` 由调用方提供，负责真正落库；这里只保证顺序、单调性与失败即停。
pub fn run_migrations<F>(current: u32, mut apply: F) -> Result<u32, CoreError>
where
    F: FnMut(&SchemaVersion) -> Result<(), CoreError>,
{
    if current > CURRENT_SCHEMA_VERSION {
        return Err(CoreError::internal(format!(
            "元数据库版本 {current} 高于本应用支持的 {CURRENT_SCHEMA_VERSION}，拒绝降级读取"
        )));
    }

    let mut version = current;
    for step in MIGRATIONS.iter() {
        if step.from != version {
            continue;
        }
        apply(step)?;
        version = step.to;
    }

    if version != CURRENT_SCHEMA_VERSION {
        return Err(CoreError::internal(format!(
            "migration 后版本为 {version}，期望 {CURRENT_SCHEMA_VERSION}"
        )));
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_pending_migrations_in_order() {
        let mut applied: Vec<u32> = Vec::new();
        let version = run_migrations(0, |step| {
            applied.push(step.to);
            Ok(())
        })
        .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        assert_eq!(applied, vec![1, 2, 3, 4]);
    }

    #[test]
    fn already_current_database_applies_nothing() {
        let mut applied: Vec<u32> = Vec::new();
        let version = run_migrations(CURRENT_SCHEMA_VERSION, |step| {
            applied.push(step.to);
            Ok(())
        })
        .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        assert!(applied.is_empty(), "已是当前版本时不能重复迁移");
    }

    #[test]
    fn failure_stops_and_does_not_report_a_version() {
        let result = run_migrations(0, |_| Err(CoreError::internal("磁盘满")));
        assert!(result.is_err(), "升级失败不得返回新版本");
    }

    #[test]
    fn newer_database_is_refused() {
        let error = run_migrations(CURRENT_SCHEMA_VERSION + 1, |_| Ok(())).unwrap_err();
        assert!(error.safe_details[0].contains("拒绝降级"));
    }

    #[test]
    fn migration_history_is_monotonic() {
        for window in MIGRATIONS.windows(2) {
            assert_eq!(window[0].to, window[1].from, "migration 必须首尾相接");
            assert!(window[0].to > window[0].from);
        }
    }
}
