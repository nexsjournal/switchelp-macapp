//! 壳无关用例。桌面端不能用完整实体覆盖宿主状态、版本或验证结果。

pub mod apply;
use crate::{
    credentials::{secret_ref, CredentialResolver, ResolvedSecret, SecretVault},
    domain::{
        capability::{InputCapability, InputKind, Support, Verification},
        credential::Credential,
        error::CoreError,
        ids::{CatalogAlias, CredentialId, ModelId, ProviderId},
        model::{HostState, Model, ModelLifecycle, ModelPolicy},
        provider::{AuthKind, Protocol, Provider},
        tokens::TokenCount,
    },
    storage::Repository,
};
pub use apply::{AppliedSummary, ApplyService, Clock, GatewayLayout, RecoveryReport, SystemClock};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDraft {
    pub id: Option<String>,
    pub name: String,
    pub endpoint: String,
    pub protocol: Protocol,
    pub auth_kind: AuthKind,
    pub preset_id: Option<String>,
    pub notes: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelDraft {
    pub id: Option<String>,
    pub provider_id: String,
    pub upstream_id: String,
    /// 新建时留空可由核心生成；目录身份不依赖显示名称。
    pub catalog_alias: String,
    pub display_name: String,
    pub policy: ModelPolicy,
    pub in_catalog: bool,
    pub display_name_overridden: bool,
}

pub struct WorkspaceService {
    pub repository: Arc<dyn Repository>,
    vault: Arc<dyn SecretVault>,
    mutations: Mutex<()>,
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("UTC 可表示为 RFC3339")
}

impl WorkspaceService {
    pub fn new(repository: Arc<dyn Repository>, vault: Arc<dyn SecretVault>) -> Self {
        Self {
            repository,
            vault,
            mutations: Mutex::new(()),
        }
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, CoreError> {
        self.repository.list_providers()
    }
    pub fn list_models(&self) -> Result<Vec<Model>, CoreError> {
        self.repository.list_models()
    }
    pub fn list_credentials(&self, provider_id: &str) -> Result<Vec<Credential>, CoreError> {
        self.repository
            .list_credentials(&ProviderId::new(provider_id))
    }

    pub fn save_provider(
        &self,
        draft: ProviderDraft,
        expected_version: u64,
    ) -> Result<Provider, CoreError> {
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        let mut provider = if let Some(id) = draft.id {
            self.repository
                .get_provider(&ProviderId::new(id))?
                .ok_or_else(|| CoreError::not_found("供应商"))?
        } else {
            if expected_version != 0 {
                return Err(CoreError::conflict("error.providerVersionConflict"));
            }
            Provider::draft(
                ProviderId::generate(),
                &draft.name,
                &draft.endpoint,
                draft.protocol,
                draft.auth_kind,
                now(),
            )?
        };
        provider.name = draft.name;
        provider.endpoint = draft.endpoint;
        provider.protocol = draft.protocol;
        provider.auth_kind = draft.auth_kind;
        provider.preset_id = draft.preset_id;
        provider.notes = draft.notes;
        provider.enabled = draft.enabled;
        provider.updated_at = now();
        self.repository.save_provider(provider, expected_version)
    }

    pub fn add_credential(
        &self,
        provider_id: &str,
        label: &str,
        secret: String,
    ) -> Result<Credential, CoreError> {
        let secret = Zeroizing::new(secret);
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        let provider_id = ProviderId::new(provider_id);
        self.repository
            .get_provider(&provider_id)?
            .ok_or_else(|| CoreError::not_found("供应商"))?;
        let credential =
            Credential::create(CredentialId::generate(), provider_id, label, &secret, now())?;
        self.persist_secret(credential, secret.trim())
    }

    pub fn replace_credential(
        &self,
        id: &str,
        secret: String,
        expected_version: u64,
    ) -> Result<Credential, CoreError> {
        let secret = Zeroizing::new(secret);
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        let mut credential = self
            .repository
            .get_credential(&CredentialId::new(id))?
            .ok_or_else(|| CoreError::not_found("Key"))?;
        if credential.version != expected_version {
            return Err(CoreError::conflict("error.credentialVersionConflict"));
        }
        credential.replace_secret(&secret, now())?;
        self.persist_secret(credential, secret.trim())
    }

    fn persist_secret(
        &self,
        mut credential: Credential,
        secret: &str,
    ) -> Result<Credential, CoreError> {
        // 写入尝试拥有独立条目，跨进程 CAS 失败不能覆盖或删除另一写入者的秘密。
        credential.secret_ref = format!(
            "{}/{}",
            secret_ref(
                credential.provider_id.as_str(),
                credential.id.as_str(),
                credential.secret_version
            ),
            uuid::Uuid::new_v4()
        );
        self.vault.store(&credential.secret_ref, secret)?;
        match self.repository.save_credential(credential.clone()) {
            Ok(saved) => Ok(saved),
            Err(mut error) => {
                if self.vault.delete(&credential.secret_ref).is_err() {
                    error
                        .safe_details
                        .push("未引用的安全条目清理失败；原 Key 保持不变".to_owned());
                }
                Err(error)
            }
        }
    }

    pub fn select_credential(
        &self,
        provider_id: &str,
        credential_id: &str,
    ) -> Result<(), CoreError> {
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        let mut provider = self
            .repository
            .get_provider(&ProviderId::new(provider_id))?
            .ok_or_else(|| CoreError::not_found("供应商"))?;
        let credential = self
            .repository
            .get_credential(&CredentialId::new(credential_id))?
            .ok_or_else(|| CoreError::not_found("Key"))?;
        if credential.provider_id != provider.id {
            return Err(CoreError::validation("不能选择其他供应商的 Key"));
        }
        credential.mark_active()?;
        if !self.vault.exists(&credential.secret_ref)? {
            return Err(CoreError::new(
                crate::domain::error::ErrorCode::CredentialMissing,
                "error.credentialMissing",
            ));
        }
        provider.active_credential_id = Some(credential.id);
        provider.updated_at = now();
        let version = provider.version;
        self.repository.save_provider(provider, version)?;
        Ok(())
    }

    pub fn save_model(&self, draft: ModelDraft, expected_version: u64) -> Result<Model, CoreError> {
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        self.repository
            .get_provider(&ProviderId::new(&draft.provider_id))?
            .ok_or_else(|| CoreError::not_found("供应商"))?;
        let previous = draft
            .id
            .as_ref()
            .map(|id| self.repository.get_model(&ModelId::new(id)))
            .transpose()?
            .flatten();
        if draft.id.is_some() && previous.is_none() {
            return Err(CoreError::not_found("模型"));
        }
        let id = previous
            .as_ref()
            .map(|m| m.id.clone())
            .unwrap_or_else(ModelId::generate);
        let alias = if draft.catalog_alias.is_empty() {
            // 修改上游身份会生成新 alias；不改变已有会话所用的旧身份。
            previous
                .as_ref()
                .filter(|m| m.upstream_id == draft.upstream_id)
                .map(|m| Ok(m.catalog_alias.clone()))
                .unwrap_or_else(|| {
                    CatalogAlias::from_parts(&draft.provider_id, &uuid::Uuid::new_v4().to_string())
                })?
        } else {
            CatalogAlias::parse(&draft.catalog_alias)?
        };
        let mut model = previous.clone().unwrap_or(Model::draft(
            id,
            ProviderId::new(&draft.provider_id),
            &draft.upstream_id,
            &draft.display_name,
            alias.clone(),
            now(),
        )?);
        if model.provider_id.as_str() != draft.provider_id {
            return Err(CoreError::validation("已有模型不能更换供应商"));
        }
        model.upstream_id = draft.upstream_id;
        model.catalog_alias = alias;
        model.display_name = draft.display_name.trim().to_owned();
        // 只有用户确实改过名字才算覆盖层。否则显示名应当跟随上游发现值，
        // 否则一次发现刷新永远覆盖不了“上游官方名”这个更准确的值。
        if draft.display_name_overridden {
            model
                .display_name_layer
                .override_with(model.display_name.clone());
        } else {
            model.display_name_layer.clear_override();
        }
        // 手工编辑只能声明上游能力；宿主/网关能力与测试结论不能由 renderer 自行提升。
        model.policy = normalize_policy(draft.policy)?;
        model.in_catalog = draft.in_catalog;
        model.lifecycle = ModelLifecycle::Saved;
        model.host_state = if model.in_catalog {
            HostState::PendingApply
        } else {
            HostState::NotInCatalog
        };
        if let Some(previous) = previous {
            let catalog_changed = previous.policy != model.policy
                || previous.display_name != model.display_name
                || previous.catalog_alias != model.catalog_alias
                || previous.in_catalog != model.in_catalog;
            if catalog_changed {
                model.capability_revision = previous.capability_revision + 1;
            } else {
                model.host_state = previous.host_state;
            }
        }
        model.updated_at = now();
        self.repository.save_model(model, expected_version)
    }

    /// 删除模型。
    ///
    /// 已纳入目录的模型必须先移出：原生菜单里还挂着它的身份，
    /// 直接删会让宿主指向一个不存在的条目。
    pub fn delete_model(&self, id: &str, expected_version: u64) -> Result<(), CoreError> {
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        let model = self
            .repository
            .get_model(&ModelId::new(id))?
            .ok_or_else(|| CoreError::not_found("模型"))?;
        if model.version != expected_version {
            return Err(CoreError::conflict("error.modelVersionConflict"));
        }
        if model.in_catalog {
            return Err(
                CoreError::validation("该模型已纳入 Codex 目录，请先移出目录再删除")
                    .with_recovery("openModels", "action.openModels"),
            );
        }
        self.repository.delete_model(&model.id)
    }

    /// 删除一个 Key。
    ///
    /// 正在使用的 Key 不能直接删；并且**先撤销安全条目再删元数据**——
    /// 反过来一旦中途失败，会留下“元数据已删但秘密还在凭据库”的组合。
    pub fn delete_credential(&self, id: &str) -> Result<(), CoreError> {
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        let credential = self
            .repository
            .get_credential(&CredentialId::new(id))?
            .ok_or_else(|| CoreError::not_found("Key"))?;
        let provider = self
            .repository
            .get_provider(&credential.provider_id)?
            .ok_or_else(|| CoreError::not_found("供应商"))?;
        // 删「当前 Key」是允许的：只有一个 Key 时无从切换，拒绝删除会让供应商永远删不掉。
        // 删掉之后这个供应商就没有当前 Key，模型探测与应用会照旧拦下来并说明原因。
        if provider.active_credential_id.as_ref() == Some(&credential.id) {
            let mut updated = provider;
            updated.active_credential_id = None;
            updated.updated_at = now();
            let version = updated.version;
            self.repository.save_provider(updated, version)?;
        }
        CredentialResolver::without_cache(self.vault.as_ref()).revoke(&credential)?;
        match self.repository.delete_credential(&credential.id) {
            Ok(()) => Ok(()),
            Err(error) => {
                // 秘密已经撤销，元数据还在：状态是“此 Key 的安全记录不存在”，
                // 界面按 credentialMissing 处理，用户可重试删除。
                let mut error = error;
                error
                    .safe_details
                    .push("安全条目已撤销，但元数据未删除，请重试".to_owned());
                Err(error)
            }
        }
    }

    /// 删除供应商。还有 Key 或模型时明确拒绝，不做静默级联删除。
    pub fn delete_provider(&self, id: &str) -> Result<(), CoreError> {
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        let provider_id = ProviderId::new(id);
        let provider = self
            .repository
            .get_provider(&provider_id)?
            .ok_or_else(|| CoreError::not_found("供应商"))?;
        // 有依赖也允许删：界面上会先把「将一并删除 N 个 Key、M 个模型」说清楚再确认。
        // 拒绝删除而依赖又删不掉（见上面删 Key 的死结）会把用户卡死在角落里。
        // 先清掉「当前 Key」：仓库层还会拦一次「有人在用这个 Key」，那是删凭据的最后一道守卫。
        if provider.active_credential_id.is_some() {
            let mut cleared = provider.clone();
            cleared.active_credential_id = None;
            cleared.updated_at = now();
            let version = cleared.version;
            self.repository.save_provider(cleared, version)?;
        }
        for credential in self.repository.list_credentials(&provider_id)? {
            CredentialResolver::without_cache(self.vault.as_ref()).revoke(&credential)?;
            self.repository.delete_credential(&credential.id)?;
        }
        for model in self
            .repository
            .list_models()?
            .into_iter()
            .filter(|model| model.provider_id == provider_id)
        {
            self.repository.delete_model(&model.id)?;
        }
        self.repository.delete_provider(&provider_id)
    }

    /// 解析某个 Key 的明文，仅供本机网关与探测这一条路径使用。
    ///
    /// 界面永远拿不到它：返回值不进入任何 DTO，用完即随 `ResolvedSecret` 归零。
    /// 不使用缓存，避免明文在长时间存活的进程里驻留。
    pub fn resolve_secret(&self, credential_id: &str) -> Result<ResolvedSecret, CoreError> {
        let credential = self
            .repository
            .get_credential(&CredentialId::new(credential_id))?
            .ok_or_else(|| CoreError::not_found("Key"))?;
        CredentialResolver::without_cache(self.vault.as_ref()).resolve(&credential)
    }

    /// 记录一次模型发现的结果。
    ///
    /// 只更新**发现层**：用户手工改过的显示名不会被刷新覆盖（R07）。
    /// 返回被更新的模型数量；发现结果里没有的模型保持原样。
    pub fn record_discovery(
        &self,
        provider_id: &str,
        discovered: &[(String, String)],
    ) -> Result<usize, CoreError> {
        let _guard = self
            .mutations
            .lock()
            .map_err(|_| CoreError::internal("写入锁不可用"))?;
        let mut updated = 0;
        for mut model in self.repository.list_models()? {
            if model.provider_id.as_str() != provider_id {
                continue;
            }
            let Some((_, name)) = discovered
                .iter()
                .find(|(upstream_id, _)| upstream_id == &model.upstream_id)
            else {
                continue;
            };
            let version = model.version;
            let discovered_changed =
                model.display_name_layer.discovered.as_deref() != Some(name.as_str());
            model.display_name_layer.discovered = Some(name.clone());
            // 未覆盖时显示名跟随发现值；覆盖过就保持用户值。
            let name_followed = !model.display_name_layer.overridden && model.display_name != *name;
            if name_followed {
                model.display_name = name.clone();
            }
            if discovered_changed || name_followed {
                model.updated_at = now();
                self.repository.save_model(model, version)?;
                updated += 1;
            }
        }
        Ok(updated)
    }
}

fn normalize_policy(mut policy: ModelPolicy) -> Result<ModelPolicy, CoreError> {
    for value in [
        policy.context_limit,
        policy.output_limit,
        policy.compact_limit,
    ]
    .into_iter()
    .flatten()
    {
        TokenCount::new(value.value())?;
        if value.value() == 0 {
            return Err(CoreError::validation("Token 上限必须大于 0；未知时请留空"));
        }
    }
    let mut inputs = Vec::new();
    for kind in InputKind::ALL {
        let matching: Vec<_> = policy.inputs.iter().filter(|i| i.kind == kind).collect();
        if matching.len() > 1 {
            return Err(CoreError::validation("输入能力类别重复"));
        }
        let upstream = matching.first().map_or(Support::Unknown, |i| i.upstream);
        let (gateway, host) = match kind {
            InputKind::Text | InputKind::Image => (Support::Supported, Support::Supported),
            InputKind::Audio => (Support::Unknown, Support::Unknown),
            _ => (Support::Unsupported, Support::Unsupported),
        };
        inputs.push(InputCapability::new(
            kind,
            upstream,
            gateway,
            host,
            Verification::Declared,
        ));
    }
    policy.inputs = inputs;
    policy.tools.verification = Verification::Declared;
    policy.reasoning.mapping_id = match policy.reasoning.control {
        crate::domain::reasoning::ReasoningControl::Effort => {
            Some("reasoning.effort.v1".to_owned())
        }
        _ => None,
    };
    policy.validate(false)?;
    Ok(policy)
}
