//! Agent OS Model catalog domain.
//!
//! Model identity and capability declarations remain independent from Provider,
//! Runtime, Agent, Role, Permission, routing, and execution. Availability is an
//! explicit relationship object rather than a field that changes Model identity.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::agent_provider_domain::AgentProviderId;

const MAX_ID_LENGTH: usize = 128;
const MAX_TEXT_LENGTH: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Model capability version must be positive")]
    InvalidCapabilityVersion,
    #[error("Model numeric metadata must be positive: {0}")]
    InvalidNumericMetadata(&'static str),
    #[error("Duplicate Model capability: {0}")]
    DuplicateCapability(String),
    #[error("Model, Provider, and availability identities must be distinct")]
    IdentityCollision,
    #[error("Model availability observation timestamp must not be negative")]
    InvalidObservationTimestamp,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ModelDomainError> {
                Ok(Self(validate_identifier($field, value.into())?))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            fn validate(&self) -> Result<(), ModelDomainError> {
                validate_identifier($field, self.0.clone()).map(|_| ())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

typed_id!(ModelId, "Model ID");
typed_id!(ModelAvailabilityId, "Model availability ID");

fn validate_identifier(field: &'static str, value: String) -> Result<String, ModelDomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ModelDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(ModelDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(ModelDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn validate_text(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, ModelDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(ModelDomainError::Empty { field });
    }
    if value.chars().count() > MAX_TEXT_LENGTH {
        return Err(ModelDomainError::TooLong {
            field,
            max: MAX_TEXT_LENGTH,
        });
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapability {
    name: String,
    version: u16,
    metadata: BTreeMap<String, String>,
}

impl ModelCapability {
    pub fn new(
        name: impl Into<String>,
        version: u16,
        metadata: BTreeMap<String, String>,
    ) -> Result<Self, ModelDomainError> {
        let capability = Self {
            name: validate_identifier("Model capability name", name.into())?,
            version,
            metadata,
        };
        capability.validate()?;
        Ok(capability)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }

    fn validate(&self) -> Result<(), ModelDomainError> {
        validate_identifier("Model capability name", self.name.clone())?;
        if self.version == 0 {
            return Err(ModelDomainError::InvalidCapabilityVersion);
        }
        for (key, value) in &self.metadata {
            validate_identifier("Model capability metadata key", key.clone())?;
            validate_text("Model capability metadata value", value.clone())?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadata {
    family: Option<String>,
    version: Option<String>,
    context_window: Option<u64>,
    max_output_tokens: Option<u64>,
}

impl ModelMetadata {
    pub fn new(
        family: Option<String>,
        version: Option<String>,
        context_window: Option<u64>,
        max_output_tokens: Option<u64>,
    ) -> Result<Self, ModelDomainError> {
        let metadata = Self {
            family: family
                .map(|value| validate_text("Model family", value))
                .transpose()?,
            version: version
                .map(|value| validate_text("Model version", value))
                .transpose()?,
            context_window,
            max_output_tokens,
        };
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub fn context_window(&self) -> Option<u64> {
        self.context_window
    }

    pub fn max_output_tokens(&self) -> Option<u64> {
        self.max_output_tokens
    }

    fn validate(&self) -> Result<(), ModelDomainError> {
        if let Some(family) = &self.family {
            validate_text("Model family", family.clone())?;
        }
        if let Some(version) = &self.version {
            validate_text("Model version", version.clone())?;
        }
        if self.context_window == Some(0) {
            return Err(ModelDomainError::InvalidNumericMetadata("contextWindow"));
        }
        if self.max_output_tokens == Some(0) {
            return Err(ModelDomainError::InvalidNumericMetadata("maxOutputTokens"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    model_id: ModelId,
    display_name: String,
    metadata: ModelMetadata,
    capabilities: Vec<ModelCapability>,
}

impl ModelDescriptor {
    pub fn new(
        model_id: ModelId,
        display_name: impl Into<String>,
        metadata: ModelMetadata,
        capabilities: Vec<ModelCapability>,
    ) -> Result<Self, ModelDomainError> {
        let descriptor = Self {
            model_id,
            display_name: validate_text("Model display name", display_name)?,
            metadata,
            capabilities,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    pub fn capabilities(&self) -> &[ModelCapability] {
        &self.capabilities
    }

    pub fn validate(&self) -> Result<(), ModelDomainError> {
        self.model_id.validate()?;
        validate_text("Model display name", self.display_name.clone())?;
        self.metadata.validate()?;
        let mut names = HashSet::new();
        for capability in &self.capabilities {
            capability.validate()?;
            if !names.insert(capability.name()) {
                return Err(ModelDomainError::DuplicateCapability(
                    capability.name().to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailabilityStatus {
    Declared,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAvailability {
    id: ModelAvailabilityId,
    model_id: ModelId,
    provider_id: AgentProviderId,
    provider_model_reference: String,
    status: ModelAvailabilityStatus,
    observed_at: i64,
}

impl ModelAvailability {
    pub fn new(
        id: ModelAvailabilityId,
        model_id: ModelId,
        provider_id: AgentProviderId,
        provider_model_reference: impl Into<String>,
        status: ModelAvailabilityStatus,
        observed_at: i64,
    ) -> Result<Self, ModelDomainError> {
        let availability = Self {
            id,
            model_id,
            provider_id,
            provider_model_reference: validate_identifier(
                "Provider Model reference",
                provider_model_reference.into(),
            )?,
            status,
            observed_at,
        };
        availability.validate()?;
        Ok(availability)
    }

    pub fn id(&self) -> &ModelAvailabilityId {
        &self.id
    }

    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    pub fn provider_id(&self) -> &AgentProviderId {
        &self.provider_id
    }

    pub fn provider_model_reference(&self) -> &str {
        &self.provider_model_reference
    }

    pub fn status(&self) -> ModelAvailabilityStatus {
        self.status
    }

    pub fn observed_at(&self) -> i64 {
        self.observed_at
    }

    pub fn validate(&self) -> Result<(), ModelDomainError> {
        self.id.validate()?;
        self.model_id.validate()?;
        validate_identifier(
            "Agent OS Provider ID",
            self.provider_id.as_str().to_string(),
        )?;
        validate_identifier(
            "Provider Model reference",
            self.provider_model_reference.clone(),
        )?;
        if self.id.as_str() == self.model_id.as_str()
            || self.id.as_str() == self.provider_id.as_str()
            || self.model_id.as_str() == self.provider_id.as_str()
        {
            return Err(ModelDomainError::IdentityCollision);
        }
        if self.observed_at < 0 {
            return Err(ModelDomainError::InvalidObservationTimestamp);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(name: &str) -> ModelCapability {
        ModelCapability::new(name, 1, BTreeMap::new()).expect("valid capability")
    }

    #[test]
    fn descriptor_has_no_provider_or_agent_identity() {
        let descriptor = ModelDescriptor::new(
            ModelId::new("model:reasoning").expect("valid model ID"),
            "Reasoning Model",
            ModelMetadata::new(
                Some("reasoning".to_string()),
                None,
                Some(32_000),
                Some(8_000),
            )
            .expect("valid metadata"),
            vec![capability("reasoning:structured")],
        )
        .expect("valid descriptor");

        let serialized = serde_json::to_value(descriptor).expect("descriptor serializes");
        assert!(serialized.get("providerId").is_none());
        assert!(serialized.get("agentId").is_none());
    }

    #[test]
    fn availability_preserves_distinct_provider_and_model_identity() {
        let availability = ModelAvailability::new(
            ModelAvailabilityId::new("availability:one").expect("valid availability ID"),
            ModelId::new("model:one").expect("valid model ID"),
            AgentProviderId::new("provider:one").expect("valid provider ID"),
            "native-model-one",
            ModelAvailabilityStatus::Declared,
            1_000,
        )
        .expect("valid availability");

        assert_ne!(
            availability.model_id().as_str(),
            availability.provider_id().as_str()
        );
        assert_eq!(availability.provider_model_reference(), "native-model-one");
    }

    #[test]
    fn duplicate_model_capability_is_rejected() {
        let result = ModelDescriptor::new(
            ModelId::new("model:one").expect("valid model ID"),
            "Model One",
            ModelMetadata::default(),
            vec![capability("text:generate"), capability("text:generate")],
        );
        assert!(matches!(
            result,
            Err(ModelDomainError::DuplicateCapability(_))
        ));
    }
}
