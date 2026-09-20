//! 路由准入：把请求前缀 + alias 解析为固定的请求路由。
//!
//! 核心约束（[网关与协议](../../../../docs/architecture/03-gateway-and-protocols.md)）：
//! 目录前缀决定可用的 alias 集合；旧宿主仍带旧前缀时按旧快照服务，
//! 绝不静默回落到最新版本；请求开始即固定 `routeRevision + credentialVersion + protocolVersion`。

use crate::domain::error::{CoreError, ErrorCode};
use crate::domain::ids::{CredentialId, InstanceId, ModelId, ProviderId, RevisionId};
use crate::storage::snapshot::{RevisionRefs, RouteEntry, RouteSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 准入失败原因。与 `CoreError` 分开，便于在网关内部按类别分支处理。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AdmissionError {
    /// 该目录版本从未发布或已被回收。
    UnknownPrefix { catalog_revision: String },
    /// 目录版本存在，但不包含该 alias。
    UnknownAlias {
        alias: String,
        catalog_revision: String,
    },
    /// 前缀中的实例与快照所属实例不一致。
    ///
    /// `expected` 是**调用方要求的**那个实例，`actual` 是实际解析出来的那个。
    /// 两个调用点的语义不同（一个要求的是令牌所属实例、一个要求的是 URL 前缀里的实例），
    /// 所以文案必须中性——以前写成「令牌属于 {expected}」，在 URL 前缀那个调用点上
    /// 恰好把两个实例名说反了。
    InstanceMismatch { expected: String, actual: String },
}

impl AdmissionError {
    pub fn code(&self) -> ErrorCode {
        match self {
            AdmissionError::UnknownPrefix { .. } | AdmissionError::UnknownAlias { .. } => {
                ErrorCode::RouteMismatch
            }
            AdmissionError::InstanceMismatch { .. } => ErrorCode::Unauthorized,
        }
    }

    pub fn message_key(&self) -> &'static str {
        match self {
            AdmissionError::UnknownPrefix { .. } => "error.unknownCatalogRevision",
            AdmissionError::UnknownAlias { .. } => "error.unknownAlias",
            AdmissionError::InstanceMismatch { .. } => "error.instanceMismatch",
        }
    }

    pub fn to_core_error(&self) -> CoreError {
        let detail = match self {
            AdmissionError::UnknownPrefix { catalog_revision } => {
                format!("该目录版本未发布：{catalog_revision}")
            }
            AdmissionError::UnknownAlias {
                alias,
                catalog_revision,
            } => format!("目录版本 {catalog_revision} 不包含 alias {alias}"),
            AdmissionError::InstanceMismatch { expected, actual } => {
                format!("实例不匹配：这里要求的是 {expected}，但该目录版本属于 {actual}")
            }
        };
        CoreError::new(self.code(), self.message_key()).with_detail(detail)
    }
}

/// 请求级不可变路由：请求开始后不受后续保存影响。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRoute {
    pub instance_id: InstanceId,
    pub catalog_revision: String,
    pub revision_id: RevisionId,
    pub alias: String,
    pub provider_id: ProviderId,
    pub model_id: ModelId,
    /// 上游精确 ID，保留大小写、斜杠与 Unicode。
    pub upstream_id: String,
    pub credential_id: CredentialId,
    pub credential_version: u32,
    /// 版本化协议适配器标识。
    pub protocol_id: String,
    /// 该模型声明的输出上限；请求期用于收口 `max_output_tokens`。
    pub output_limit: Option<u64>,
    /// 声明可用的思考档位。
    pub reasoning_efforts: Vec<String>,
    /// 声明可由宿主原生发送的模态。
    pub native_modalities: Vec<String>,
}

impl RequestRoute {
    /// 请求期要执行的模型策略，全部来自发布时冻结的路由快照。
    pub fn limits(&self) -> crate::protocols::RouteLimits {
        crate::protocols::RouteLimits {
            output_limit: self.output_limit,
            reasoning_efforts: self.reasoning_efforts.clone(),
        }
    }
}

impl RequestRoute {
    fn from_entry(
        instance_id: InstanceId,
        catalog_revision: &str,
        revision_id: RevisionId,
        entry: &RouteEntry,
    ) -> Self {
        Self {
            instance_id,
            catalog_revision: catalog_revision.to_owned(),
            revision_id,
            alias: entry.alias.clone(),
            provider_id: entry.provider_id.clone(),
            model_id: entry.model_id.clone(),
            upstream_id: entry.upstream_id.clone(),
            credential_id: entry.credential_id.clone(),
            credential_version: entry.credential_version,
            protocol_id: entry.protocol_id.clone(),
            output_limit: entry.output_limit,
            reasoning_efforts: entry.reasoning_efforts.clone(),
            native_modalities: entry.native_modalities.clone(),
        }
    }

    /// 该路由是否与另一条路由等价。用于重试时判定“仍是同一路由”。
    pub fn is_same_route(&self, other: &RequestRoute) -> bool {
        self.instance_id == other.instance_id
            && self.catalog_revision == other.catalog_revision
            && self.revision_id == other.revision_id
            && self.alias == other.alias
            && self.upstream_id == other.upstream_id
            && self.credential_version == other.credential_version
            && self.protocol_id == other.protocol_id
    }

    /// 是否可跨凭据重放。已提交的流一律不可（文档：流一旦提交不得跨 Key 重试）。
    pub fn allows_credential_swap(&self, _streamed: bool) -> bool {
        false
    }
}

/// 准入结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Admission {
    pub route: RequestRoute,
    pub snapshot_revision: RevisionId,
}

/// 已发布路由快照的注册表。
#[derive(Debug, Default)]
pub struct GatewayRouter {
    snapshots: Mutex<HashMap<String, Arc<RouteSnapshot>>>,
    refs: Mutex<HashMap<String, RevisionRefs>>,
}

impl GatewayRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 发布一个目录修订。
    ///
    /// 同一目录前缀重复发布必须是幂等的：内容一致直接成功，内容不同则冲突，
    /// 避免“同名不同内容”的快照被悄悄替换后服务到错误的模型集合。
    pub fn publish(&self, snapshot: RouteSnapshot) -> Result<(), CoreError> {
        snapshot
            .revision_id
            .as_str()
            .is_empty()
            .then_some(())
            .map_or(Ok(()), |_| Err(CoreError::validation("修订 ID 不能为空")))?;
        if snapshot.catalog_revision.trim().is_empty() {
            return Err(CoreError::validation("目录版本标识不能为空"));
        }

        let key = snapshot.catalog_revision.clone();
        let mut snapshots = self.snapshots.lock().expect("锁未被污染");
        if let Some(existing) = snapshots.get(&key) {
            let mut comparable = snapshot.clone();
            comparable.created_at = existing.created_at.clone();
            if existing.as_ref() == &comparable {
                return Ok(());
            }
            return Err(CoreError::conflict("error.catalogRevisionConflict")
                .with_detail(format!("目录版本 {key} 已发布且内容不同，拒绝覆盖")));
        }
        snapshots.insert(key.clone(), Arc::new(snapshot));
        drop(snapshots);
        self.refs
            .lock()
            .expect("锁未被污染")
            .entry(key)
            .or_default();
        Ok(())
    }

    /// 按 URL 前缀解析准入。前缀缺失或未发布都必须拒绝，不能回落到最新版本。
    pub fn admission_from_path(
        &self,
        path: &str,
        alias: &str,
        expected_instance: &InstanceId,
    ) -> Result<Admission, AdmissionError> {
        let (instance, revision) = crate::storage::snapshot::RuntimePublication::parse_prefix(path)
            .ok_or_else(|| AdmissionError::UnknownPrefix {
                catalog_revision: path.to_owned(),
            })?;
        if instance != expected_instance.as_str() {
            return Err(AdmissionError::InstanceMismatch {
                expected: expected_instance.as_str().to_owned(),
                actual: instance,
            });
        }
        self.admission(&revision, alias, expected_instance)
    }

    /// 按目录版本与 alias 解析准入。
    pub fn admission(
        &self,
        catalog_revision: &str,
        alias: &str,
        expected_instance: &InstanceId,
    ) -> Result<Admission, AdmissionError> {
        let snapshot = {
            let snapshots = self.snapshots.lock().expect("锁未被污染");
            snapshots.get(catalog_revision).cloned()
        };
        let snapshot = snapshot.ok_or_else(|| AdmissionError::UnknownPrefix {
            catalog_revision: catalog_revision.to_owned(),
        })?;

        if &snapshot.instance_id != expected_instance {
            return Err(AdmissionError::InstanceMismatch {
                expected: expected_instance.as_str().to_owned(),
                actual: snapshot.instance_id.as_str().to_owned(),
            });
        }

        let entry = snapshot
            .routes
            .iter()
            .find(|entry| entry.alias == alias)
            .ok_or_else(|| AdmissionError::UnknownAlias {
                alias: alias.to_owned(),
                catalog_revision: catalog_revision.to_owned(),
            })?;

        Ok(Admission {
            route: RequestRoute::from_entry(
                snapshot.instance_id.clone(),
                &snapshot.catalog_revision,
                snapshot.revision_id.clone(),
                entry,
            ),
            snapshot_revision: snapshot.revision_id.clone(),
        })
    }

    /// 登记一个活跃引用（客户端连接或续接绑定）。
    pub fn retain(&self, catalog_revision: &str, continuations: usize) {
        let mut refs = self.refs.lock().expect("锁未被污染");
        let entry = refs.entry(catalog_revision.to_owned()).or_default();
        entry.clients += 1;
        entry.continuations += continuations;
    }

    /// 释放一个活跃引用。
    pub fn release(&self, catalog_revision: &str, continuations: usize) {
        let mut refs = self.refs.lock().expect("锁未被污染");
        if let Some(entry) = refs.get_mut(catalog_revision) {
            entry.clients = entry.clients.saturating_sub(1);
            entry.continuations = entry.continuations.saturating_sub(continuations);
        }
    }

    /// 某目录版本是否仍被引用（含续接）。
    pub fn refs(&self, catalog_revision: &str) -> RevisionRefs {
        self.refs
            .lock()
            .expect("锁未被污染")
            .get(catalog_revision)
            .cloned()
            .unwrap_or_default()
    }

    /// 回收一个目录修订。
    ///
    /// 仍有活跃引用或续接绑定时必须保留，返回 `false`；只有引用归零才真正移除。
    pub fn retire(&self, catalog_revision: &str) -> bool {
        if !self.refs(catalog_revision).is_reclaimable() {
            return false;
        }
        self.refs
            .lock()
            .expect("锁未被污染")
            .remove(catalog_revision);
        self.snapshots
            .lock()
            .expect("锁未被污染")
            .remove(catalog_revision)
            .is_some()
    }

    /// 当前已发布的目录版本，按字典序排列以保证输出稳定。
    pub fn revisions(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .snapshots
            .lock()
            .expect("锁未被污染")
            .keys()
            .cloned()
            .collect();
        keys.sort_unstable();
        keys
    }

    /// 某目录版本里全部 alias，按快照内顺序返回。
    ///
    /// 只用于诊断与 `/v1/models` 展示；路由决策仍然只走 `admission`，
    /// 不允许调用方拿这份列表自己挑模型。
    pub fn aliases(&self, catalog_revision: &str) -> Option<Vec<String>> {
        let snapshots = self.snapshots.lock().expect("锁未被污染");
        snapshots.get(catalog_revision).map(|snapshot| {
            snapshot
                .routes
                .iter()
                .map(|entry| entry.alias.clone())
                .collect()
        })
    }

    /// 与 `admission` 同样严格的 alias 列表：目录版本必须存在，**且属于 `expected_instance`**。
    ///
    /// `/v1/models` 过去走 `aliases()`，只查版本在不在、不查实例归属，于是
    /// `/i/<任意实例>/c/<真实版本>/v1/models` 都能列出别人的模型。展示接口不该比推理接口更松。
    pub fn aliases_checked(
        &self,
        catalog_revision: &str,
        expected_instance: &InstanceId,
    ) -> Result<Vec<String>, AdmissionError> {
        let snapshots = self.snapshots.lock().expect("锁未被污染");
        let snapshot =
            snapshots
                .get(catalog_revision)
                .ok_or_else(|| AdmissionError::UnknownPrefix {
                    catalog_revision: catalog_revision.to_owned(),
                })?;
        if &snapshot.instance_id != expected_instance {
            return Err(AdmissionError::InstanceMismatch {
                expected: expected_instance.as_str().to_owned(),
                actual: snapshot.instance_id.as_str().to_owned(),
            });
        }
        Ok(snapshot
            .routes
            .iter()
            .map(|entry| entry.alias.clone())
            .collect())
    }

    pub fn len(&self) -> usize {
        self.snapshots.lock().expect("锁未被污染").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::snapshot::RouteEntry;

    fn entry(alias: &str, upstream: &str) -> RouteEntry {
        RouteEntry {
            alias: alias.to_owned(),
            provider_id: ProviderId::new("p_a"),
            model_id: ModelId::new(format!("m_{}", alias.replace('/', "_"))),
            upstream_id: upstream.to_owned(),
            credential_id: CredentialId::new("c_1"),
            credential_version: 1,
            protocol_id: "responses.v1".to_owned(),
            output_limit: None,
            reasoning_efforts: Vec::new(),
            native_modalities: Vec::new(),
        }
    }

    fn snapshot(revision: &str, aliases: &[&str]) -> RouteSnapshot {
        RouteSnapshot::new(
            RevisionId::new(format!("rev_{revision}")),
            InstanceId::new("inst_1"),
            revision,
            aliases.iter().map(|a| entry(a, "vendor/x")).collect(),
            "2026-09-18T00:00:00Z",
        )
        .unwrap()
    }

    fn instance() -> InstanceId {
        InstanceId::new("inst_1")
    }

    #[test]
    fn publishes_and_admits_known_route() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();

        let admission = router
            .admission("rev_0007", "gs/p_a/m_1", &instance())
            .unwrap();
        assert_eq!(admission.route.alias, "gs/p_a/m_1");
        assert_eq!(admission.route.catalog_revision, "rev_0007");
        assert_eq!(admission.route.credential_version, 1);
        assert_eq!(admission.route.protocol_id, "responses.v1");
        assert_eq!(admission.snapshot_revision.as_str(), "rev_rev_0007");
    }

    #[test]
    fn unknown_prefix_is_rejected_and_never_falls_back_to_latest() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();

        let error = router
            .admission("rev_0006", "gs/p_a/m_1", &instance())
            .unwrap_err();
        assert_eq!(
            error,
            AdmissionError::UnknownPrefix {
                catalog_revision: "rev_0006".to_owned()
            }
        );
        assert_eq!(error.code(), ErrorCode::RouteMismatch);
        assert!(error.to_core_error().safe_details[0].contains("rev_0006"));
    }

    #[test]
    fn unknown_alias_is_rejected() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();

        let error = router
            .admission("rev_0007", "gs/p_a/m_9", &instance())
            .unwrap_err();
        assert!(matches!(error, AdmissionError::UnknownAlias { .. }));
        assert_eq!(error.code(), ErrorCode::RouteMismatch);
    }

    #[test]
    fn old_prefix_keeps_serving_its_own_snapshot() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0006", &["gs/p_a/m_1"]))
            .unwrap();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1", "gs/p_a/m_2"]))
            .unwrap();

        // 新前缀能看到新模型。
        assert!(router
            .admission("rev_0007", "gs/p_a/m_2", &instance())
            .is_ok());
        // 旧前缀仍只服务旧集合。
        assert!(router
            .admission("rev_0006", "gs/p_a/m_2", &instance())
            .is_err());
        assert_eq!(
            router
                .admission("rev_0006", "gs/p_a/m_1", &instance())
                .unwrap()
                .route
                .catalog_revision,
            "rev_0006"
        );
    }

    #[test]
    fn republishing_identical_snapshot_is_idempotent() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();
        assert!(router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .is_ok());
        assert_eq!(router.len(), 1);
    }

    #[test]
    fn republishing_different_content_under_same_prefix_conflicts() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();
        let error = router
            .publish(snapshot("rev_0007", &["gs/p_a/m_2"]))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        // 原快照未被覆盖。
        assert!(router
            .admission("rev_0007", "gs/p_a/m_1", &instance())
            .is_ok());
        assert!(router
            .admission("rev_0007", "gs/p_a/m_2", &instance())
            .is_err());
    }

    #[test]
    fn empty_revision_or_prefix_is_rejected() {
        let router = GatewayRouter::new();
        let mut blank_revision = snapshot("rev_0007", &[]);
        blank_revision.revision_id = RevisionId::new("");
        assert_eq!(
            router.publish(blank_revision).unwrap_err().code,
            ErrorCode::ValidationFailed
        );

        let mut blank_prefix = snapshot("rev_0007", &[]);
        blank_prefix.catalog_revision = "  ".to_owned();
        assert_eq!(
            router.publish(blank_prefix).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn instance_mismatch_is_reported_as_unauthorized() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();

        let error = router
            .admission("rev_0007", "gs/p_a/m_1", &InstanceId::new("inst_2"))
            .unwrap_err();
        assert!(matches!(error, AdmissionError::InstanceMismatch { .. }));
        assert_eq!(error.code(), ErrorCode::Unauthorized);
    }

    #[test]
    fn admission_from_path_parses_prefixed_url() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();

        let path = "/i/inst_1/c/rev_0007/v1/responses";
        let admission = router
            .admission_from_path(path, "gs/p_a/m_1", &instance())
            .unwrap();
        assert_eq!(admission.route.instance_id, instance());

        // 前缀里的实例与令牌不符时拒绝。
        let other = router.admission_from_path(path, "gs/p_a/m_1", &InstanceId::new("inst_2"));
        assert!(matches!(
            other.unwrap_err(),
            AdmissionError::InstanceMismatch { .. }
        ));

        // 非前缀路径拒绝。
        assert!(router
            .admission_from_path("/v1/responses", "gs/p_a/m_1", &instance())
            .is_err());
    }

    #[test]
    fn retire_keeps_revision_while_refs_remain() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();

        router.retain("rev_0007", 2);
        assert_eq!(router.refs("rev_0007").total(), 3);
        assert!(!router.retire("rev_0007"), "仍有引用时必须保留");
        assert!(router
            .admission("rev_0007", "gs/p_a/m_1", &instance())
            .is_ok());

        router.release("rev_0007", 2);
        assert!(router.retire("rev_0007"), "引用归零后应可回收");
        assert!(router.is_empty());
        assert!(router
            .admission("rev_0007", "gs/p_a/m_1", &instance())
            .is_err());
    }

    #[test]
    fn release_never_underflows() {
        let router = GatewayRouter::new();
        router.publish(snapshot("rev_0007", &[])).unwrap();
        router.release("rev_0007", 5);
        assert_eq!(router.refs("rev_0007").total(), 0);
        assert!(router.retire("rev_0007"));
    }

    #[test]
    fn revisions_are_listed_in_stable_order() {
        let router = GatewayRouter::new();
        router.publish(snapshot("rev_0007", &[])).unwrap();
        router.publish(snapshot("rev_0006", &[])).unwrap();
        assert_eq!(router.revisions(), vec!["rev_0006", "rev_0007"]);
        assert_eq!(router.len(), 2);
    }

    #[test]
    fn request_route_identity_ignores_display_but_tracks_credentials() {
        let router = GatewayRouter::new();
        router
            .publish(snapshot("rev_0007", &["gs/p_a/m_1"]))
            .unwrap();
        let first = router
            .admission("rev_0007", "gs/p_a/m_1", &instance())
            .unwrap()
            .route;
        assert!(first.is_same_route(&first.clone()));

        let mut changed = first.clone();
        changed.credential_version = 2;
        assert!(
            !first.is_same_route(&changed),
            "Key 版本变化必须视为不同路由"
        );

        let mut renamed = first.clone();
        renamed.upstream_id = "vendor/other".to_owned();
        assert!(!first.is_same_route(&renamed));

        // 任何情况下都不允许跨凭据重放。
        assert!(!first.allows_credential_swap(false));
        assert!(!first.allows_credential_swap(true));
    }

    #[test]
    fn duplicate_alias_inside_a_snapshot_is_rejected_at_construction() {
        let error = RouteSnapshot::new(
            RevisionId::new("rev"),
            InstanceId::new("inst_1"),
            "rev_0007",
            vec![
                entry("gs/p_a/m_1", "vendor/a"),
                entry("gs/p_a/m_1", "vendor/b"),
            ],
            "now",
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::ValidationFailed);
    }
}
