//! Explicit Model resolution contract and immutable resolution repository.
//!
//! Resolution validates identities supplied by a caller. It never searches for,
//! ranks, prefers, or automatically selects a Model, Provider, or Runtime.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    agent_provider_domain::AgentProviderId,
    agent_provider_instance::AgentProviderInstanceId,
    model_domain::{ModelAvailability, ModelDescriptor, ModelId},
    runtime_domain::RuntimeId,
    runtime_instance_domain::RuntimeInstanceId,
};

const MAX_ID_LENGTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelResolutionDomainError {
    #[error("Model resolution ID is empty")]
    EmptyId,
    #[error("Model resolution ID exceeds {0} characters")]
    IdTooLong(usize),
    #[error("Model resolution ID contains whitespace or control characters")]
    InvalidId,
    #[error("Model resolution timestamp must not be negative")]
    InvalidTimestamp,
    #[error("Model capability requirement version must be positive")]
    InvalidCapabilityVersion,
    #[error("Model capability requirement name is invalid")]
    InvalidCapabilityName,
    #[error("Model capability requirement metadata is invalid")]
    InvalidCapabilityMetadata,
    #[error("Duplicate Model capability requirement: {0}")]
    DuplicateCapabilityRequirement(String),
    #[error("Model resolution identities must remain distinct")]
    IdentityCollision,
    #[error("Resolved Model does not match the explicit request")]
    ResolutionMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelResolutionId(String);

impl ModelResolutionId {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelResolutionDomainError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(ModelResolutionDomainError::EmptyId);
        }
        if value.chars().count() > MAX_ID_LENGTH {
            return Err(ModelResolutionDomainError::IdTooLong(MAX_ID_LENGTH));
        }
        if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
            return Err(ModelResolutionDomainError::InvalidId);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModelResolutionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelResolutionCapabilityRequirement {
    name: String,
    minimum_version: u16,
    required_metadata: BTreeMap<String, String>,
}

impl ModelResolutionCapabilityRequirement {
    pub fn new(
        name: impl Into<String>,
        minimum_version: u16,
        required_metadata: BTreeMap<String, String>,
    ) -> Result<Self, ModelResolutionDomainError> {
        let requirement = Self {
            name: name.into(),
            minimum_version,
            required_metadata,
        };
        requirement.validate()?;
        Ok(requirement)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn minimum_version(&self) -> u16 {
        self.minimum_version
    }

    pub fn required_metadata(&self) -> &BTreeMap<String, String> {
        &self.required_metadata
    }

    fn validate(&self) -> Result<(), ModelResolutionDomainError> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > MAX_ID_LENGTH
            || self.name.chars().any(char::is_whitespace)
            || self.name.chars().any(char::is_control)
        {
            return Err(ModelResolutionDomainError::InvalidCapabilityName);
        }
        if self.minimum_version == 0 {
            return Err(ModelResolutionDomainError::InvalidCapabilityVersion);
        }
        if self.required_metadata.iter().any(|(key, value)| {
            key.trim().is_empty()
                || value.trim().is_empty()
                || key.chars().any(char::is_whitespace)
                || key.chars().any(char::is_control)
                || value.chars().any(char::is_control)
        }) {
            return Err(ModelResolutionDomainError::InvalidCapabilityMetadata);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelResolutionRequest {
    resolution_id: ModelResolutionId,
    runtime_instance_id: RuntimeInstanceId,
    provider_instance_id: AgentProviderInstanceId,
    model_id: ModelId,
    availability_id: crate::model_domain::ModelAvailabilityId,
    capability_requirements: Vec<ModelResolutionCapabilityRequirement>,
    requested_at: i64,
}

impl ModelResolutionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resolution_id: ModelResolutionId,
        runtime_instance_id: RuntimeInstanceId,
        provider_instance_id: AgentProviderInstanceId,
        model_id: ModelId,
        availability_id: crate::model_domain::ModelAvailabilityId,
        capability_requirements: Vec<ModelResolutionCapabilityRequirement>,
        requested_at: i64,
    ) -> Result<Self, ModelResolutionDomainError> {
        let request = Self {
            resolution_id,
            runtime_instance_id,
            provider_instance_id,
            model_id,
            availability_id,
            capability_requirements,
            requested_at,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn resolution_id(&self) -> &ModelResolutionId {
        &self.resolution_id
    }
    pub fn runtime_instance_id(&self) -> &RuntimeInstanceId {
        &self.runtime_instance_id
    }
    pub fn provider_instance_id(&self) -> &AgentProviderInstanceId {
        &self.provider_instance_id
    }
    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }
    pub fn availability_id(&self) -> &crate::model_domain::ModelAvailabilityId {
        &self.availability_id
    }
    pub fn capability_requirements(&self) -> &[ModelResolutionCapabilityRequirement] {
        &self.capability_requirements
    }
    pub fn requested_at(&self) -> i64 {
        self.requested_at
    }

    pub fn validate(&self) -> Result<(), ModelResolutionDomainError> {
        ModelResolutionId::new(self.resolution_id.as_str())?;
        if self.requested_at < 0 {
            return Err(ModelResolutionDomainError::InvalidTimestamp);
        }
        let identities = [
            self.resolution_id.as_str(),
            self.runtime_instance_id.as_str(),
            self.provider_instance_id.as_str(),
            self.model_id.as_str(),
            self.availability_id.as_str(),
        ];
        if identities.iter().copied().collect::<HashSet<_>>().len() != identities.len() {
            return Err(ModelResolutionDomainError::IdentityCollision);
        }
        let mut requirements = HashSet::new();
        for requirement in &self.capability_requirements {
            requirement.validate()?;
            if !requirements.insert(requirement.name()) {
                return Err(ModelResolutionDomainError::DuplicateCapabilityRequirement(
                    requirement.name().to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModel {
    resolution_id: ModelResolutionId,
    runtime_instance_id: RuntimeInstanceId,
    runtime_id: RuntimeId,
    provider_instance_id: AgentProviderInstanceId,
    provider_id: AgentProviderId,
    model: ModelDescriptor,
    availability: ModelAvailability,
    resolved_at: i64,
}

impl ResolvedModel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request: &ModelResolutionRequest,
        runtime_id: RuntimeId,
        provider_id: AgentProviderId,
        model: ModelDescriptor,
        availability: ModelAvailability,
        resolved_at: i64,
    ) -> Result<Self, ModelResolutionDomainError> {
        request.validate()?;
        if resolved_at < request.requested_at()
            || model.model_id() != request.model_id()
            || availability.id() != request.availability_id()
            || availability.model_id() != request.model_id()
            || availability.provider_id() != &provider_id
            || runtime_id.as_str() == provider_id.as_str()
            || runtime_id.as_str() == model.model_id().as_str()
        {
            return Err(ModelResolutionDomainError::ResolutionMismatch);
        }
        model
            .validate()
            .map_err(|_| ModelResolutionDomainError::ResolutionMismatch)?;
        availability
            .validate()
            .map_err(|_| ModelResolutionDomainError::ResolutionMismatch)?;
        Ok(Self {
            resolution_id: request.resolution_id().clone(),
            runtime_instance_id: request.runtime_instance_id().clone(),
            runtime_id,
            provider_instance_id: request.provider_instance_id().clone(),
            provider_id,
            model,
            availability,
            resolved_at,
        })
    }

    pub fn resolution_id(&self) -> &ModelResolutionId {
        &self.resolution_id
    }
    pub fn runtime_instance_id(&self) -> &RuntimeInstanceId {
        &self.runtime_instance_id
    }
    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }
    pub fn provider_instance_id(&self) -> &AgentProviderInstanceId {
        &self.provider_instance_id
    }
    pub fn provider_id(&self) -> &AgentProviderId {
        &self.provider_id
    }
    pub fn model(&self) -> &ModelDescriptor {
        &self.model
    }
    pub fn availability(&self) -> &ModelAvailability {
        &self.availability
    }
    pub fn resolved_at(&self) -> i64 {
        self.resolved_at
    }
}

/// Application-facing contract. Implementations may validate an explicit
/// request, but must not infer or substitute any identity.
pub trait ModelResolutionContract {
    type Error;
    fn resolve_explicit(
        &self,
        request: ModelResolutionRequest,
        resolved_at: i64,
    ) -> Result<ResolvedModel, Self::Error>;
}

#[derive(Debug, Error)]
pub enum ModelResolutionRepositoryError {
    #[error("Model resolution is already recorded: {0}")]
    AlreadyRecorded(ModelResolutionId),
    #[error("Model resolution repository lock failed: {0}")]
    RegistryLock(String),
}

pub trait ModelResolutionRepository: Send + Sync {
    fn insert(&self, resolution: ResolvedModel) -> Result<(), ModelResolutionRepositoryError>;
    fn get(
        &self,
        id: &ModelResolutionId,
    ) -> Result<Option<ResolvedModel>, ModelResolutionRepositoryError>;
    fn list(&self) -> Result<Vec<ResolvedModel>, ModelResolutionRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryModelResolutionRepository {
    resolutions: Arc<RwLock<HashMap<ModelResolutionId, ResolvedModel>>>,
}

impl ModelResolutionRepository for InMemoryModelResolutionRepository {
    fn insert(&self, resolution: ResolvedModel) -> Result<(), ModelResolutionRepositoryError> {
        let mut values = self
            .resolutions
            .write()
            .map_err(|error| ModelResolutionRepositoryError::RegistryLock(error.to_string()))?;
        if values.contains_key(resolution.resolution_id()) {
            return Err(ModelResolutionRepositoryError::AlreadyRecorded(
                resolution.resolution_id().clone(),
            ));
        }
        values.insert(resolution.resolution_id().clone(), resolution);
        Ok(())
    }

    fn get(
        &self,
        id: &ModelResolutionId,
    ) -> Result<Option<ResolvedModel>, ModelResolutionRepositoryError> {
        let values = self
            .resolutions
            .read()
            .map_err(|error| ModelResolutionRepositoryError::RegistryLock(error.to_string()))?;
        Ok(values.get(id).cloned())
    }

    fn list(&self) -> Result<Vec<ResolvedModel>, ModelResolutionRepositoryError> {
        let values = self
            .resolutions
            .read()
            .map_err(|error| ModelResolutionRepositoryError::RegistryLock(error.to_string()))?;
        let mut values = values.values().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| {
            left.resolution_id()
                .as_str()
                .cmp(right.resolution_id().as_str())
        });
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        requirements: Vec<ModelResolutionCapabilityRequirement>,
    ) -> Result<ModelResolutionRequest, ModelResolutionDomainError> {
        ModelResolutionRequest::new(
            ModelResolutionId::new("resolution:one")?,
            RuntimeInstanceId::new("runtime-instance:one").unwrap(),
            AgentProviderInstanceId::new("provider-instance:one").unwrap(),
            ModelId::new("model:one").unwrap(),
            crate::model_domain::ModelAvailabilityId::new("availability:one").unwrap(),
            requirements,
            10,
        )
    }

    #[test]
    fn request_contains_only_explicit_boundary_identities() {
        let serialized = serde_json::to_value(request(vec![]).unwrap()).unwrap();
        assert_eq!(serialized["modelId"], "model:one");
        assert!(serialized.get("agentId").is_none());
        assert!(serialized.get("executionId").is_none());
        assert!(serialized.get("roleId").is_none());
    }

    #[test]
    fn duplicate_capability_requirements_are_rejected() {
        let requirement =
            ModelResolutionCapabilityRequirement::new("text:generate", 1, BTreeMap::new()).unwrap();
        assert!(matches!(
            request(vec![requirement.clone(), requirement]),
            Err(ModelResolutionDomainError::DuplicateCapabilityRequirement(
                _
            ))
        ));
    }
}
