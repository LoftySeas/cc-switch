//! Model capability matching and routing policy domain.
//!
//! Routing resolves a Model and Provider availability for one Execution. It
//! never invokes the Model and it does not bind either identity to an Agent.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    agent_provider_domain::AgentProviderId,
    execution_domain::ExecutionModelBinding,
    model_domain::{ModelAvailability, ModelDescriptor, ModelId},
};

const MAX_NAME_LENGTH: usize = 128;
const MAX_METADATA_LENGTH: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelRoutingDomainError {
    #[error("Model capability requirement name is empty")]
    EmptyCapability,
    #[error("Model capability requirement name is invalid")]
    InvalidCapability,
    #[error("Model capability requirement version must be positive")]
    InvalidCapabilityVersion,
    #[error("Model capability metadata is invalid")]
    InvalidCapabilityMetadata,
    #[error("Duplicate Model capability requirement: {0}")]
    DuplicateCapability(String),
    #[error("Duplicate Model routing policy identity: {0}")]
    DuplicatePolicyIdentity(String),
    #[error("Preferred Model or Provider must be included in its allowed set")]
    PreferenceOutsideAllowedSet,
    #[error("Model availability maximum age must be positive")]
    InvalidMaximumAge,
    #[error("Model routing timestamp must not be negative")]
    InvalidRoutingTimestamp,
    #[error("Resolved Model route identities do not match")]
    RouteIdentityMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilityRequirement {
    name: String,
    minimum_version: u16,
    required_metadata: BTreeMap<String, String>,
}

impl ModelCapabilityRequirement {
    pub fn new(
        name: impl Into<String>,
        minimum_version: u16,
        required_metadata: BTreeMap<String, String>,
    ) -> Result<Self, ModelRoutingDomainError> {
        let name = name.into();
        let name = name.trim();
        if name.is_empty() {
            return Err(ModelRoutingDomainError::EmptyCapability);
        }
        if name.chars().count() > MAX_NAME_LENGTH
            || name.chars().any(char::is_whitespace)
            || name.chars().any(char::is_control)
        {
            return Err(ModelRoutingDomainError::InvalidCapability);
        }
        if minimum_version == 0 {
            return Err(ModelRoutingDomainError::InvalidCapabilityVersion);
        }
        if required_metadata.iter().any(|(key, value)| {
            key.trim().is_empty()
                || key.chars().count() > MAX_NAME_LENGTH
                || key.chars().any(char::is_whitespace)
                || key.chars().any(char::is_control)
                || value.trim().is_empty()
                || value.chars().count() > MAX_METADATA_LENGTH
                || value.chars().any(char::is_control)
        }) {
            return Err(ModelRoutingDomainError::InvalidCapabilityMetadata);
        }
        Ok(Self {
            name: name.to_string(),
            minimum_version,
            required_metadata,
        })
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
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoutingPolicy {
    allowed_model_ids: Vec<ModelId>,
    allowed_provider_ids: Vec<AgentProviderId>,
    preferred_model_ids: Vec<ModelId>,
    preferred_provider_ids: Vec<AgentProviderId>,
    maximum_availability_age: Option<i64>,
}

impl ModelRoutingPolicy {
    pub fn new(
        allowed_model_ids: Vec<ModelId>,
        allowed_provider_ids: Vec<AgentProviderId>,
        preferred_model_ids: Vec<ModelId>,
        preferred_provider_ids: Vec<AgentProviderId>,
        maximum_availability_age: Option<i64>,
    ) -> Result<Self, ModelRoutingDomainError> {
        let policy = Self {
            allowed_model_ids,
            allowed_provider_ids,
            preferred_model_ids,
            preferred_provider_ids,
            maximum_availability_age,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn allowed_model_ids(&self) -> &[ModelId] {
        &self.allowed_model_ids
    }

    pub fn allowed_provider_ids(&self) -> &[AgentProviderId] {
        &self.allowed_provider_ids
    }

    pub fn preferred_model_ids(&self) -> &[ModelId] {
        &self.preferred_model_ids
    }

    pub fn preferred_provider_ids(&self) -> &[AgentProviderId] {
        &self.preferred_provider_ids
    }

    pub fn maximum_availability_age(&self) -> Option<i64> {
        self.maximum_availability_age
    }

    fn validate(&self) -> Result<(), ModelRoutingDomainError> {
        fn unique<T: Eq + std::hash::Hash + std::fmt::Display>(
            values: &[T],
        ) -> Result<(), ModelRoutingDomainError> {
            let mut seen = HashSet::new();
            for value in values {
                if !seen.insert(value) {
                    return Err(ModelRoutingDomainError::DuplicatePolicyIdentity(
                        value.to_string(),
                    ));
                }
            }
            Ok(())
        }
        unique(&self.allowed_model_ids)?;
        unique(&self.allowed_provider_ids)?;
        unique(&self.preferred_model_ids)?;
        unique(&self.preferred_provider_ids)?;
        if !self.allowed_model_ids.is_empty()
            && self
                .preferred_model_ids
                .iter()
                .any(|id| !self.allowed_model_ids.contains(id))
        {
            return Err(ModelRoutingDomainError::PreferenceOutsideAllowedSet);
        }
        if !self.allowed_provider_ids.is_empty()
            && self
                .preferred_provider_ids
                .iter()
                .any(|id| !self.allowed_provider_ids.contains(id))
        {
            return Err(ModelRoutingDomainError::PreferenceOutsideAllowedSet);
        }
        if self.maximum_availability_age.is_some_and(|age| age <= 0) {
            return Err(ModelRoutingDomainError::InvalidMaximumAge);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRouteRequest {
    requirements: Vec<ModelCapabilityRequirement>,
    policy: ModelRoutingPolicy,
    routed_at: i64,
}

impl ModelRouteRequest {
    pub fn new(
        requirements: Vec<ModelCapabilityRequirement>,
        policy: ModelRoutingPolicy,
        routed_at: i64,
    ) -> Result<Self, ModelRoutingDomainError> {
        if routed_at < 0 {
            return Err(ModelRoutingDomainError::InvalidRoutingTimestamp);
        }
        let mut names = HashSet::new();
        for requirement in &requirements {
            if !names.insert(requirement.name()) {
                return Err(ModelRoutingDomainError::DuplicateCapability(
                    requirement.name().to_string(),
                ));
            }
        }
        policy.validate()?;
        Ok(Self {
            requirements,
            policy,
            routed_at,
        })
    }

    pub fn requirements(&self) -> &[ModelCapabilityRequirement] {
        &self.requirements
    }

    pub fn policy(&self) -> &ModelRoutingPolicy {
        &self.policy
    }

    pub fn routed_at(&self) -> i64 {
        self.routed_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModelRoute {
    model: ModelDescriptor,
    availability: ModelAvailability,
    matched_capabilities: Vec<String>,
    routed_at: i64,
}

impl ResolvedModelRoute {
    pub fn new(
        model: ModelDescriptor,
        availability: ModelAvailability,
        matched_capabilities: Vec<String>,
        routed_at: i64,
    ) -> Result<Self, ModelRoutingDomainError> {
        if routed_at < 0 {
            return Err(ModelRoutingDomainError::InvalidRoutingTimestamp);
        }
        if model.model_id() != availability.model_id() {
            return Err(ModelRoutingDomainError::RouteIdentityMismatch);
        }
        Ok(Self {
            model,
            availability,
            matched_capabilities,
            routed_at,
        })
    }

    pub fn model(&self) -> &ModelDescriptor {
        &self.model
    }

    pub fn availability(&self) -> &ModelAvailability {
        &self.availability
    }

    pub fn matched_capabilities(&self) -> &[String] {
        &self.matched_capabilities
    }

    pub fn routed_at(&self) -> i64 {
        self.routed_at
    }

    pub fn execution_binding(&self) -> ExecutionModelBinding {
        ExecutionModelBinding::provider_model(
            self.model.model_id().clone(),
            self.availability.provider_id().clone(),
            self.availability.id().clone(),
        )
    }
}
