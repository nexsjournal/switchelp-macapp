//! 不可变运行快照：目录修订、路由快照与运行发布。
//!
//! 规则来自 [配置生命周期](../../../../docs/architecture/02-configuration-lifecycle.md) 与
//! [总体架构](../../../../docs/architecture/01-system-architecture.md)：
//! 请求开始时持有不可变 Revision；中途 UI 保存不会更换其上游；目录修订与运行策略修订
//! 分开发布，旧目录在仍有引用时保留。

use crate::domain::capability::Support;
use crate::domain::error::{CoreError, ErrorCode};
use crate::domain::ids::{CredentialId, InstanceId, ModelId, ProviderId, RevisionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 目录修订与运行策略修订分开的载体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionKind {
    /// 目录能力修订：改变模型集合或宿主可见能力。
    Catalog,
    /// 运行策略修订：Key 选择、输出上限等，可热发布。
    Policy,
}

/// 不可变修订 manifest。
///
/// 只保存引用与版本号，不保存秘密；`content_hash` 由调用方用编译器产物的字节计算。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: RevisionId,
    pub parent_id: Option<RevisionId>,
    pub kind: RevisionKind,
    pub schema_version: u32,
    pub content_hash: String,
    pub created_at: String,
    /// 该修订纳入目录的模型 alias，按序保存。
    pub catalog_aliases: Vec<String>,
    /// 编译该修订所用的编译器版本，用于稳定快照可追溯。
    pub compiler_version: String,
}

impl Revision {
    /// 校验 manifest 自洽：alias 不重复且非空。
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.content_hash.trim().is_empty() {
            return Err(CoreError::validation("修订缺少内容摘要"));
        }
        let mut seen = std::collections::HashSet::new();
        for alias in &self.catalog_aliases {
            if alias.trim().is_empty() {
                return Err(CoreError::validation("修订包含空 alias"));
            }
            if !seen.insert(alias) {
                return Err(CoreError::validation(format!(
                    "修订包含重复 alias：{alias}"
                )));
            }
        }
        Ok(())
    }

    /// 目录级变更才要求宿主重载；策略修订热发布。
    pub fn requires_host_reload(&self) -> bool {
        self.kind == RevisionKind::Catalog
    }
}

/// 路由快照中的一条解析结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteEntry {
    pub alias: String,
    pub provider_id: ProviderId,
    pub model_id: ModelId,
    pub upstream_id: String,
    pub credential_id: CredentialId,
    pub credential_version: u32,
    /// 该条目的协议适配器标识（版本化）。
    pub protocol_id: String,
    /// 该模型声明的输出上限。网关据此收口 `max_output_tokens`。
    ///
    /// 属于发布时冻结的策略：请求期改表单不会改变在途请求的行为。
    /// 老快照没有这些字段，缺省即“未声明”。
    #[serde(default)]
    pub output_limit: Option<u64>,
    /// 声明可用的思考档位；非空表示档位已声明且映射已版本化。
    #[serde(default)]
    pub reasoning_efforts: Vec<String>,
    /// 目录声明可由宿主原生发送的模态，用于显式拒绝未声明的输入。
    #[serde(default)]
    pub native_modalities: Vec<String>,
    /// 该模型是否声明了上游自己执行的内置工具（`web_search` 等）。
    ///
    /// 老快照没有这项，缺省即未知＝不转发：把一份没有依据的内置工具转给上游，
    /// 换来的是一条与用户操作无关的 400。
    #[serde(default)]
    pub builtin_tools: Support,
}

impl RouteEntry {
    /// 请求期要执行的模型策略。
    pub fn limits(&self) -> crate::protocols::RouteLimits {
        crate::protocols::RouteLimits {
            output_limit: self.output_limit,
            reasoning_efforts: self.reasoning_efforts.clone(),
            builtin_tools: self.builtin_tools,
        }
    }
}

/// 不可变路由快照：请求开始即固定，之后不受 UI 保存影响。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSnapshot {
    pub revision_id: RevisionId,
    pub instance_id: InstanceId,
    /// 目录前缀中使用的修订标识；与 `revision_id` 一起决定 Base URL。
    pub catalog_revision: String,
    pub routes: Vec<RouteEntry>,
    pub created_at: String,
}

impl RouteSnapshot {
    /// 由条目构造，同时校验 alias 唯一。
    pub fn new(
        revision_id: RevisionId,
        instance_id: InstanceId,
        catalog_revision: impl Into<String>,
        routes: Vec<RouteEntry>,
        created_at: impl Into<String>,
    ) -> Result<Self, CoreError> {
        let mut seen = std::collections::HashSet::new();
        for entry in &routes {
            if !seen.insert(entry.alias.as_str()) {
                return Err(
                    CoreError::new(ErrorCode::ValidationFailed, "error.duplicateAlias")
                        .with_detail(format!("路由快照包含重复 alias：{}", entry.alias)),
                );
            }
        }
        Ok(Self {
            revision_id,
            instance_id,
            catalog_revision: catalog_revision.into(),
            routes,
            created_at: created_at.into(),
        })
    }

    /// 按 alias 解析路由。未知 alias 必须拒绝，不能回落到“最新配置”。
    pub fn resolve(&self, alias: &str) -> Result<&RouteEntry, CoreError> {
        self.routes
            .iter()
            .find(|entry| entry.alias == alias)
            .ok_or_else(|| {
                CoreError::new(ErrorCode::RouteMismatch, "error.unknownAlias")
                    .with_detail(format!("该目录版本不包含 alias：{alias}"))
            })
    }

    /// 该快照是否允许服务指定目录前缀。
    ///
    /// 旧宿主未重载时仍带旧前缀，网关按前缀选路由集合，不会把旧请求送进新能力配置。
    pub fn serves_prefix(&self, prefix: &str) -> bool {
        self.catalog_revision == prefix
    }

    /// alias 到条目索引，便于热路径 O(1) 查询而不每次线性扫描。
    pub fn index(&self) -> HashMap<&str, &RouteEntry> {
        self.routes.iter().map(|e| (e.alias.as_str(), e)).collect()
    }
}

/// 运行发布：某实例当前对外提供的修订组合。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePublication {
    pub instance_id: InstanceId,
    /// 对外服务的目录修订。
    pub catalog_revision: String,
    /// 对外服务的策略修订。
    pub policy_revision: RevisionId,
    /// 网关地址前缀，形如 `/i/{instanceId}/c/{catalogRevision}`。
    pub endpoint_prefix: String,
    pub published_at: String,
}

/// 路径段的转义集合：保留 RFC 3986 unreserved（`A-Za-z0-9-._~`）。
///
/// 实例 ID 与目录修订形如 `inst_0123ab`/`rev_0123ab`，下划线属于 unreserved，
/// 编码后会让配置里的 `base_url` 难以人工核对；文档示例同样保持可读形态。
/// 分隔符与百分号仍然转义，避免实例 ID 自带 `/` 时改变前缀分段。
const PATH_SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

impl RuntimePublication {
    /// 生成受认证的网关前缀。前缀确定目录版本，请求不能覆盖。
    pub fn build_prefix(instance_id: &InstanceId, catalog_revision: &str) -> String {
        format!(
            "/i/{}/c/{}",
            percent_encoding::utf8_percent_encode(instance_id.as_str(), PATH_SEGMENT),
            percent_encoding::utf8_percent_encode(catalog_revision, PATH_SEGMENT)
        )
    }

    pub fn full_base_url(&self, gateway_origin: &str) -> String {
        format!(
            "{}{}/v1",
            gateway_origin.trim_end_matches('/'),
            self.endpoint_prefix
        )
    }

    /// 解析形如 `/i/{instanceId}/c/{catalogRevision}` 的前缀。
    pub fn parse_prefix(path: &str) -> Option<(String, String)> {
        let rest = path.strip_prefix("/i/")?;
        let (instance, rest) = rest.split_once("/c/")?;
        let instance = decode(instance)?;
        // 只取第一段作为目录版本：真实 URL 形如
        // `/i/{instanceId}/c/{catalogRevision}/v1/responses`，后续段不属于前缀。
        let (revision_segment, _tail) = rest.split_once('/').unwrap_or((rest, ""));
        let revision = decode(revision_segment)?;
        if instance.is_empty() || revision.is_empty() {
            return None;
        }
        Some((instance, revision))
    }

    /// 相同目录前缀的重复发布必须是无写入的幂等操作。
    pub fn is_same_publication(&self, other: &RuntimePublication) -> bool {
        self.instance_id == other.instance_id
            && self.catalog_revision == other.catalog_revision
            && self.policy_revision == other.policy_revision
            && self.endpoint_prefix == other.endpoint_prefix
    }
}

fn decode(value: &str) -> Option<String> {
    percent_encoding::percent_decode_str(value)
        .decode_utf8()
        .ok()
        .map(|cow| cow.into_owned())
}

/// 修订引用计数：旧目录在仍有客户端或续接引用时必须保留。
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionRefs {
    pub clients: usize,
    pub continuations: usize,
}

impl RevisionRefs {
    pub fn total(&self) -> usize {
        self.clients + self.continuations
    }

    /// 引用归零后才可以清理旧目录。
    pub fn is_reclaimable(&self) -> bool {
        self.total() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_keeps_unreserved_characters_readable() {
        let prefix =
            RuntimePublication::build_prefix(&InstanceId::new("inst_0123ab"), "rev_0123ab");
        assert_eq!(prefix, "/i/inst_0123ab/c/rev_0123ab");
    }

    #[test]
    fn prefix_escapes_path_separators_and_percent() {
        // 实例 ID 自带分隔符时不能改变前缀分段，否则会解析出错误的目录版本。
        let prefix = RuntimePublication::build_prefix(&InstanceId::new("a/b"), "rev%2F7");
        assert_eq!(prefix, "/i/a%2Fb/c/rev%252F7");
        assert_eq!(
            RuntimePublication::parse_prefix(&format!("{prefix}/v1/responses")),
            Some(("a/b".to_owned(), "rev%2F7".to_owned()))
        );
    }

    #[test]
    fn prefix_round_trips_unicode_instance_ids() {
        let prefix = RuntimePublication::build_prefix(&InstanceId::new("实例-甲"), "rev_1");
        assert_eq!(
            RuntimePublication::parse_prefix(&format!("{prefix}/v1/models")),
            Some(("实例-甲".to_owned(), "rev_1".to_owned()))
        );
    }

    #[test]
    fn parse_prefix_rejects_missing_or_empty_segments() {
        assert_eq!(RuntimePublication::parse_prefix("/v1/responses"), None);
        assert_eq!(RuntimePublication::parse_prefix("/i//c/rev_1"), None);
        assert_eq!(RuntimePublication::parse_prefix("/i/inst_1/c/"), None);
    }

    #[test]
    fn full_base_url_appends_v1_under_the_prefixed_origin() {
        let publication = RuntimePublication {
            instance_id: InstanceId::new("inst_0123ab"),
            catalog_revision: "rev_0123ab".to_owned(),
            policy_revision: RevisionId::new("pol_1"),
            endpoint_prefix: RuntimePublication::build_prefix(
                &InstanceId::new("inst_0123ab"),
                "rev_0123ab",
            ),
            published_at: "2026-09-18T00:00:00Z".to_owned(),
        };
        assert_eq!(
            publication.full_base_url("http://127.0.0.1:18765/"),
            "http://127.0.0.1:18765/i/inst_0123ab/c/rev_0123ab/v1"
        );
    }
}
