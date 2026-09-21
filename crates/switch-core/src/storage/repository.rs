//! 实体仓库端口与内存实现。
//!
//! 唯一性约束（来自 [数据与接口契约](../../../../docs/architecture/04-data-and-contracts.md)）：
//! `(provider_id, upstream_id, protocol)` 唯一；`catalog_alias` 全局唯一；
//! 精确 ID 不做大小写归一化合并；并发编辑使用 optimistic version。

use crate::domain::credential::Credential;
use crate::domain::error::{CoreError, ErrorCode};
use crate::domain::ids::{CatalogAlias, CredentialId, ModelId, ProviderId};
use crate::domain::model::Model;
use crate::domain::provider::Provider;
use std::collections::HashMap;
use std::sync::Mutex;

/// 实体仓库端口。持久化细节由适配器决定，业务层只见这里的方法。
pub trait Repository: Send + Sync {
    fn list_providers(&self) -> Result<Vec<Provider>, CoreError>;
    fn get_provider(&self, id: &ProviderId) -> Result<Option<Provider>, CoreError>;
    /// 新增或按 optimistic version 更新供应商。
    fn save_provider(
        &self,
        provider: Provider,
        expected_version: u64,
    ) -> Result<Provider, CoreError>;
    fn delete_provider(&self, id: &ProviderId) -> Result<(), CoreError>;

    fn list_credentials(&self, provider_id: &ProviderId) -> Result<Vec<Credential>, CoreError>;
    fn get_credential(&self, id: &CredentialId) -> Result<Option<Credential>, CoreError>;
    fn save_credential(&self, credential: Credential) -> Result<Credential, CoreError>;
    fn delete_credential(&self, id: &CredentialId) -> Result<(), CoreError>;

    fn list_models(&self) -> Result<Vec<Model>, CoreError>;
    fn get_model(&self, id: &ModelId) -> Result<Option<Model>, CoreError>;
    fn save_model(&self, model: Model, expected_version: u64) -> Result<Model, CoreError>;
    fn delete_model(&self, id: &ModelId) -> Result<(), CoreError>;

    /// 读一条应用设置。键不存在返回 `None`——「没设过」与「设成了空」是两件事。
    fn setting(&self, key: &str) -> Result<Option<String>, CoreError>;
    /// 写一条应用设置（覆盖）。
    fn set_setting(&self, key: &str, value: &str) -> Result<(), CoreError>;
}

/// 引用计数：某实体被多少活跃请求/续接引用。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceCount {
    pub models: usize,
    pub credentials: usize,
    pub active_bindings: usize,
}

impl ReferenceCount {
    pub fn blocks_hard_delete(&self) -> bool {
        self.active_bindings > 0 || self.models > 0 || self.credentials > 0
    }
}

/// 内存仓库。除测试外也可作为离线演示后端。
#[derive(Debug, Default)]
pub struct InMemoryRepository {
    providers: Mutex<HashMap<String, Provider>>,
    credentials: Mutex<HashMap<String, Credential>>,
    models: Mutex<HashMap<String, Model>>,
    /// alias -> model_id，保证目录 alias 全局唯一。
    aliases: Mutex<HashMap<String, String>>,
    settings: Mutex<HashMap<String, String>>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// 供应商下的模型与 Key 数量，用于删除前的依赖展示。
    pub fn reference_count(&self, provider_id: &ProviderId) -> Result<ReferenceCount, CoreError> {
        let models = self
            .list_models()?
            .into_iter()
            .filter(|m| m.provider_id == *provider_id)
            .count();
        let credentials = self.list_credentials(provider_id)?.len();
        Ok(ReferenceCount {
            models,
            credentials,
            active_bindings: 0,
        })
    }

    fn check_alias_free(
        &self,
        alias: &CatalogAlias,
        owner: Option<&ModelId>,
    ) -> Result<(), CoreError> {
        let aliases = self.aliases.lock().expect("锁未被污染");
        if let Some(existing) = aliases.get(alias.as_str()) {
            if Some(existing.as_str()) != owner.map(ModelId::as_str) {
                return Err(
                    CoreError::new(ErrorCode::ValidationFailed, "error.duplicateAlias")
                        .with_detail(format!("alias 已被占用：{alias}")),
                );
            }
        }
        Ok(())
    }

    fn check_model_identity(&self, model: &Model) -> Result<(), CoreError> {
        let identity = model.identity_key();
        for existing in self.models.lock().expect("锁未被污染").values() {
            if existing.id == model.id {
                continue;
            }
            if existing.identity_key() == identity {
                return Err(CoreError::new(
                    ErrorCode::ValidationFailed,
                    "error.duplicateModelIdentity",
                )
                .with_detail(format!(
                    "同一供应商下已存在该上游模型与协议：{}",
                    model.upstream_id
                )));
            }
        }
        Ok(())
    }

    fn bind_alias(&self, alias: &CatalogAlias, model_id: &ModelId) {
        self.aliases
            .lock()
            .expect("锁未被污染")
            .insert(alias.as_str().to_owned(), model_id.as_str().to_owned());
    }
}

impl Repository for InMemoryRepository {
    fn list_providers(&self) -> Result<Vec<Provider>, CoreError> {
        let mut items: Vec<Provider> = self
            .providers
            .lock()
            .expect("锁未被污染")
            .values()
            .cloned()
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(items)
    }

    fn get_provider(&self, id: &ProviderId) -> Result<Option<Provider>, CoreError> {
        Ok(self
            .providers
            .lock()
            .expect("锁未被污染")
            .get(id.as_str())
            .cloned())
    }

    fn save_provider(
        &self,
        mut provider: Provider,
        expected_version: u64,
    ) -> Result<Provider, CoreError> {
        provider.validate()?;
        let mut providers = self.providers.lock().expect("锁未被污染");
        match providers.get(provider.id.as_str()) {
            Some(existing) => {
                // 更新路径必须带当前版本，冲突时返回结构化错误而不是覆盖。
                existing.check_version(expected_version)?;
                provider.version = existing.version + 1;
                provider.created_at = existing.created_at.clone();
            }
            None => {
                if expected_version != 0 {
                    return Err(CoreError::conflict("error.providerNotFound"));
                }
                provider.version = 1;
            }
        }
        providers.insert(provider.id.as_str().to_owned(), provider.clone());
        Ok(provider)
    }

    fn delete_provider(&self, id: &ProviderId) -> Result<(), CoreError> {
        let references = self.reference_count(id)?;
        if references.blocks_hard_delete() {
            return Err(
                CoreError::conflict("error.providerInUse").with_detail(format!(
                    "该供应商仍被 {} 个模型、{} 个 Key 引用",
                    references.models, references.credentials
                )),
            );
        }
        self.providers
            .lock()
            .expect("锁未被污染")
            .remove(id.as_str())
            .ok_or_else(|| CoreError::not_found("供应商"))?;
        Ok(())
    }

    fn list_credentials(&self, provider_id: &ProviderId) -> Result<Vec<Credential>, CoreError> {
        let mut items: Vec<Credential> = self
            .credentials
            .lock()
            .expect("锁未被污染")
            .values()
            .filter(|c| c.provider_id == *provider_id)
            .cloned()
            .collect();
        items.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(items)
    }

    fn get_credential(&self, id: &CredentialId) -> Result<Option<Credential>, CoreError> {
        Ok(self
            .credentials
            .lock()
            .expect("锁未被污染")
            .get(id.as_str())
            .cloned())
    }

    fn save_credential(&self, credential: Credential) -> Result<Credential, CoreError> {
        if self
            .providers
            .lock()
            .expect("锁未被污染")
            .get(credential.provider_id.as_str())
            .is_none()
        {
            return Err(CoreError::not_found("供应商"));
        }
        self.credentials
            .lock()
            .expect("锁未被污染")
            .insert(credential.id.as_str().to_owned(), credential.clone());
        Ok(credential)
    }

    fn delete_credential(&self, id: &CredentialId) -> Result<(), CoreError> {
        self.credentials
            .lock()
            .expect("锁未被污染")
            .remove(id.as_str())
            .ok_or_else(|| CoreError::not_found("Key"))?;
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<Model>, CoreError> {
        let models = self.models.lock().expect("锁未被污染");
        let mut items: Vec<Model> = models.values().cloned().collect();
        items.sort_by(|a, b| a.catalog_alias.as_str().cmp(b.catalog_alias.as_str()));
        Ok(items)
    }

    fn get_model(&self, id: &ModelId) -> Result<Option<Model>, CoreError> {
        Ok(self
            .models
            .lock()
            .expect("锁未被污染")
            .get(id.as_str())
            .cloned())
    }

    fn save_model(&self, model: Model, expected_version: u64) -> Result<Model, CoreError> {
        model.validate_draft()?;
        self.check_alias_free(&model.catalog_alias, Some(&model.id))?;
        self.check_model_identity(&model)?;

        let mut model = model;
        let mut models = self.models.lock().expect("锁未被污染");
        match models.get(model.id.as_str()) {
            Some(existing) => {
                if existing.version != expected_version {
                    return Err(CoreError::conflict("error.modelVersionConflict"));
                }
                model.version = existing.version + 1;
                model.created_at = existing.created_at.clone();
            }
            None => {
                if expected_version != 0 {
                    return Err(CoreError::conflict("error.modelNotFound"));
                }
                model.version = 1;
            }
        }
        models.insert(model.id.as_str().to_owned(), model.clone());
        drop(models);
        self.bind_alias(&model.catalog_alias, &model.id);
        Ok(model)
    }

    fn delete_model(&self, id: &ModelId) -> Result<(), CoreError> {
        let removed = self
            .models
            .lock()
            .expect("锁未被污染")
            .remove(id.as_str())
            .ok_or_else(|| CoreError::not_found("模型"))?;
        self.aliases
            .lock()
            .expect("锁未被污染")
            .remove(removed.catalog_alias.as_str());
        Ok(())
    }

    fn setting(&self, key: &str) -> Result<Option<String>, CoreError> {
        Ok(self.settings.lock().expect("锁未被污染").get(key).cloned())
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.settings
            .lock()
            .expect("锁未被污染")
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::provider::{AuthKind, Protocol};

    fn provider(id: &str, name: &str) -> Provider {
        Provider::draft(
            ProviderId::new(id),
            name,
            "https://api.example.com/v1",
            Protocol::Responses,
            AuthKind::ApiKey,
            "2026-09-18T00:00:00Z",
        )
        .unwrap()
    }

    fn model(id: &str, provider_id: &str, upstream: &str, alias: &str) -> Model {
        Model::draft(
            ModelId::new(id),
            ProviderId::new(provider_id),
            upstream,
            "显示名",
            CatalogAlias::parse(alias).unwrap(),
            "2026-09-18T00:00:00Z",
        )
        .unwrap()
    }

    fn credential(id: &str, provider_id: &str) -> Credential {
        Credential::create(
            CredentialId::new(id),
            ProviderId::new(provider_id),
            "工作 Key",
            "sk-live-0123456789abcdef",
            "2026-09-18T00:00:00Z",
        )
        .unwrap()
    }

    #[test]
    fn saves_and_lists_providers_with_stable_order() {
        let repo = InMemoryRepository::new();
        repo.save_provider(provider("p_b", "Beta"), 0).unwrap();
        repo.save_provider(provider("p_a", "Alpha"), 0).unwrap();
        let providers = repo.list_providers().unwrap();
        let names: Vec<&str> = providers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn update_requires_matching_version() {
        let repo = InMemoryRepository::new();
        let saved = repo.save_provider(provider("p_a", "Alpha"), 0).unwrap();
        assert_eq!(saved.version, 1);

        let mut draft = saved.clone();
        draft.name = "Alpha 2".to_owned();
        let updated = repo.save_provider(draft.clone(), 1).unwrap();
        assert_eq!(updated.version, 2);
        assert_eq!(
            updated.created_at, saved.created_at,
            "created_at 不应被更新覆盖"
        );

        // 用过期版本再写必须冲突。
        let error = repo.save_provider(draft, 1).unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
    }

    #[test]
    fn creating_with_nonzero_expected_version_is_a_conflict() {
        let repo = InMemoryRepository::new();
        let error = repo.save_provider(provider("p_new", "New"), 3).unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
    }

    #[test]
    fn duplicate_alias_is_rejected_globally() {
        let repo = InMemoryRepository::new();
        repo.save_provider(provider("p_a", "A"), 0).unwrap();
        repo.save_provider(provider("p_b", "B"), 0).unwrap();

        repo.save_model(model("m_1", "p_a", "vendor/x", "gs/p_a/m_1"), 0)
            .unwrap();
        // 不同供应商、相同 alias 也必须被拒绝。
        let error = repo
            .save_model(model("m_2", "p_b", "vendor/y", "gs/p_a/m_1"), 0)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ValidationFailed);
    }

    #[test]
    fn same_alias_for_same_model_on_update_is_allowed() {
        let repo = InMemoryRepository::new();
        repo.save_provider(provider("p_a", "A"), 0).unwrap();
        let saved = repo
            .save_model(model("m_1", "p_a", "vendor/x", "gs/p_a/m_1"), 0)
            .unwrap();
        let mut draft = saved.clone();
        draft.display_name = "新显示名".to_owned();
        assert!(repo.save_model(draft, 1).is_ok());
    }

    #[test]
    fn duplicate_model_identity_is_rejected_but_case_is_preserved() {
        let repo = InMemoryRepository::new();
        repo.save_provider(provider("p_a", "A"), 0).unwrap();
        repo.save_model(model("m_1", "p_a", "vendor/Model-X", "gs/p_a/m_1"), 0)
            .unwrap();
        // 同一 (provider, upstream, protocol) 重复。
        let error = repo
            .save_model(model("m_2", "p_a", "vendor/Model-X", "gs/p_a/m_2"), 0)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ValidationFailed);

        // 仅大小写不同视为不同模型。
        assert!(repo
            .save_model(model("m_3", "p_a", "vendor/model-x", "gs/p_a/m_3"), 0)
            .is_ok());
    }

    #[test]
    fn deleting_model_releases_its_alias() {
        let repo = InMemoryRepository::new();
        repo.save_provider(provider("p_a", "A"), 0).unwrap();
        repo.save_model(model("m_1", "p_a", "vendor/x", "gs/p_a/m_1"), 0)
            .unwrap();
        repo.delete_model(&ModelId::new("m_1")).unwrap();
        assert!(repo
            .save_model(model("m_2", "p_a", "vendor/y", "gs/p_a/m_1"), 0)
            .is_ok());
    }

    #[test]
    fn provider_with_children_cannot_be_deleted() {
        let repo = InMemoryRepository::new();
        repo.save_provider(provider("p_a", "A"), 0).unwrap();
        repo.save_model(model("m_1", "p_a", "vendor/x", "gs/p_a/m_1"), 0)
            .unwrap();
        let error = repo.delete_provider(&ProviderId::new("p_a")).unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(error.safe_details[0].contains("仍被"));

        repo.save_credential(credential("c_1", "p_a")).unwrap();
        let references = repo.reference_count(&ProviderId::new("p_a")).unwrap();
        assert_eq!(references.models, 1);
        assert_eq!(references.credentials, 1);
        assert!(references.blocks_hard_delete());
    }

    #[test]
    fn credential_requires_existing_provider() {
        let repo = InMemoryRepository::new();
        let error = repo
            .save_credential(credential("c_1", "p_missing"))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);

        repo.save_provider(provider("p_a", "A"), 0).unwrap();
        assert!(repo.save_credential(credential("c_1", "p_a")).is_ok());
    }

    #[test]
    fn deleting_missing_entities_is_reported_not_silent() {
        let repo = InMemoryRepository::new();
        assert_eq!(
            repo.delete_credential(&CredentialId::new("c_x"))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            repo.delete_model(&ModelId::new("m_x")).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(
            repo.delete_provider(&ProviderId::new("p_x"))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn provider_validation_applies_on_save() {
        let repo = InMemoryRepository::new();
        let mut bad = provider("p_a", "A");
        bad.endpoint = "http://api.example.com/v1".to_owned();
        let error = repo.save_provider(bad, 0).unwrap_err();
        assert_eq!(error.code, ErrorCode::ValidationFailed);
    }

    #[test]
    fn models_are_listed_in_alias_order() {
        let repo = InMemoryRepository::new();
        repo.save_provider(provider("p_a", "A"), 0).unwrap();
        repo.save_model(model("m_2", "p_a", "vendor/b", "gs/p_a/m_2"), 0)
            .unwrap();
        repo.save_model(model("m_1", "p_a", "vendor/a", "gs/p_a/m_1"), 0)
            .unwrap();
        let models = repo.list_models().unwrap();
        let aliases: Vec<&str> = models.iter().map(|m| m.catalog_alias.as_str()).collect();
        assert_eq!(aliases, vec!["gs/p_a/m_1", "gs/p_a/m_2"]);
    }
}
