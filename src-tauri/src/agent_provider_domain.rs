//! Agent OS Provider domain contracts.
//!
//! These types are intentionally separate from the existing CC Switch
//! `Provider` configuration record. They describe stable infrastructure
//! identities without carrying credentials, model identities, or execution.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    model_domain::{ModelAvailabilityId, ModelId},
    runtime_domain::RuntimeExecutionId,
};

const MAX_ID_LENGTH: usize = 128;
const MAX_TEXT_LENGTH: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentProviderDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Provider contract version must be positive")]
    InvalidContractVersion,
    #[error("Provider capability version must be positive")]
    InvalidCapabilityVersion,
    #[error("Duplicate Provider capability: {0}")]
    DuplicateCapability(String),
    #[error("Provider and Provider adapter identities must be distinct")]
    IdentityCollision,
    #[error("Prepared Provider binding timestamp must not be negative")]
    InvalidBindingTimestamp,
    #[error("Prepared Provider binding reference is invalid")]
    InvalidBindingReference,
    #[error("Execution, Provider, Model, and availability binding identities must be distinct")]
    BindingIdentityCollision,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, AgentProviderDomainError> {
                Ok(Self(validate_identifier($field, value.into())?))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            fn validate(&self) -> Result<(), AgentProviderDomainError> {
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

typed_id!(AgentProviderId, "Agent OS Provider ID");
typed_id!(AgentProviderAdapterId, "Agent OS Provider adapter ID");

fn validate_identifier(
    field: &'static str,
    value: String,
) -> Result<String, AgentProviderDomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AgentProviderDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(AgentProviderDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(AgentProviderDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn validate_text(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, AgentProviderDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(AgentProviderDomainError::Empty { field });
    }
    if value.chars().count() > MAX_TEXT_LENGTH {
        return Err(AgentProviderDomainError::TooLong {
            field,
            max: MAX_TEXT_LENGTH,
        });
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapability {
    name: String,
    version: u16,
    metadata: BTreeMap<String, String>,
}

impl ProviderCapability {
    pub fn new(
        name: impl Into<String>,
        version: u16,
        metadata: BTreeMap<String, String>,
    ) -> Result<Self, AgentProviderDomainError> {
        let capability = Self {
            name: validate_identifier("Provider capability name", name.into())?,
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

    fn validate(&self) -> Result<(), AgentProviderDomainError> {
        validate_identifier("Provider capability name", self.name.clone())?;
        if self.version == 0 {
            return Err(AgentProviderDomainError::InvalidCapabilityVersion);
        }
        for (key, value) in &self.metadata {
            validate_identifier("Provider capability metadata key", key.clone())?;
            validate_text("Provider capability metadata value", value.clone())?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMetadata {
    category: Option<String>,
    documentation_url: Option<String>,
}

impl ProviderMetadata {
    pub fn new(
        category: Option<String>,
        documentation_url: Option<String>,
    ) -> Result<Self, AgentProviderDomainError> {
        let metadata = Self {
            category: category
                .map(|value| validate_text("Provider category", value))
                .transpose()?,
            documentation_url: documentation_url
                .map(|value| validate_text("Provider documentation URL", value))
                .transpose()?,
        };
        Ok(metadata)
    }

    pub fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }

    pub fn documentation_url(&self) -> Option<&str> {
        self.documentation_url.as_deref()
    }

    fn validate(&self) -> Result<(), AgentProviderDomainError> {
        if let Some(category) = &self.category {
            validate_text("Provider category", category.clone())?;
        }
        if let Some(documentation_url) = &self.documentation_url {
            validate_text("Provider documentation URL", documentation_url.clone())?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderDescriptor {
    provider_id: AgentProviderId,
    adapter_id: AgentProviderAdapterId,
    display_name: String,
    contract_version: u16,
    metadata: ProviderMetadata,
    capabilities: Vec<ProviderCapability>,
}

impl AgentProviderDescriptor {
    pub fn new(
        provider_id: AgentProviderId,
        adapter_id: AgentProviderAdapterId,
        display_name: impl Into<String>,
        contract_version: u16,
        metadata: ProviderMetadata,
        capabilities: Vec<ProviderCapability>,
    ) -> Result<Self, AgentProviderDomainError> {
        let descriptor = Self {
            provider_id,
            adapter_id,
            display_name: validate_text("Provider display name", display_name)?,
            contract_version,
            metadata,
            capabilities,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn provider_id(&self) -> &AgentProviderId {
        &self.provider_id
    }

    pub fn adapter_id(&self) -> &AgentProviderAdapterId {
        &self.adapter_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn contract_version(&self) -> u16 {
        self.contract_version
    }

    pub fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    pub fn capabilities(&self) -> &[ProviderCapability] {
        &self.capabilities
    }

    pub fn validate(&self) -> Result<(), AgentProviderDomainError> {
        self.provider_id.validate()?;
        self.adapter_id.validate()?;
        if self.provider_id.as_str() == self.adapter_id.as_str() {
            return Err(AgentProviderDomainError::IdentityCollision);
        }
        validate_text("Provider display name", self.display_name.clone())?;
        self.metadata.validate()?;
        if self.contract_version == 0 {
            return Err(AgentProviderDomainError::InvalidContractVersion);
        }

        let mut names = HashSet::new();
        for capability in &self.capabilities {
            capability.validate()?;
            if !names.insert(capability.name()) {
                return Err(AgentProviderDomainError::DuplicateCapability(
                    capability.name().to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyProviderReference {
    app_type: String,
    provider_id: String,
}

impl LegacyProviderReference {
    pub fn new(
        app_type: impl Into<String>,
        provider_id: impl Into<String>,
    ) -> Result<Self, AgentProviderDomainError> {
        Ok(Self {
            app_type: validate_identifier("Legacy Provider app type", app_type.into())?,
            provider_id: validate_identifier("Legacy Provider ID", provider_id.into())?,
        })
    }

    pub fn app_type(&self) -> &str {
        &self.app_type
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAvailability {
    Registered,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbe {
    pub provider_id: AgentProviderId,
    pub availability: ProviderAvailability,
    pub diagnostics: Vec<String>,
}

/// Execution-scoped request presented to a Provider integration adapter after
/// Model routing. It carries identities only and never contains credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBindingRequest {
    execution_id: RuntimeExecutionId,
    provider_id: AgentProviderId,
    model_id: ModelId,
    availability_id: ModelAvailabilityId,
    provider_model_reference: String,
}

impl ProviderBindingRequest {
    pub fn new(
        execution_id: RuntimeExecutionId,
        provider_id: AgentProviderId,
        model_id: ModelId,
        availability_id: ModelAvailabilityId,
        provider_model_reference: impl Into<String>,
    ) -> Result<Self, AgentProviderDomainError> {
        if execution_id.as_str() == provider_id.as_str()
            || execution_id.as_str() == model_id.as_str()
            || execution_id.as_str() == availability_id.as_str()
            || provider_id.as_str() == model_id.as_str()
            || provider_id.as_str() == availability_id.as_str()
            || model_id.as_str() == availability_id.as_str()
        {
            return Err(AgentProviderDomainError::BindingIdentityCollision);
        }
        Ok(Self {
            execution_id,
            provider_id,
            model_id,
            availability_id,
            provider_model_reference: validate_identifier(
                "Provider Model reference",
                provider_model_reference.into(),
            )?,
        })
    }

    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }

    pub fn provider_id(&self) -> &AgentProviderId {
        &self.provider_id
    }

    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    pub fn availability_id(&self) -> &ModelAvailabilityId {
        &self.availability_id
    }

    pub fn provider_model_reference(&self) -> &str {
        &self.provider_model_reference
    }
}

/// Opaque, non-secret compatibility reference prepared for exactly one
/// Execution. Runtime adapters may consume the reference through an approved
/// host integration, but the binding itself performs no Provider API call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedProviderBinding {
    execution_id: RuntimeExecutionId,
    provider_id: AgentProviderId,
    model_id: ModelId,
    availability_id: ModelAvailabilityId,
    provider_model_reference: String,
    integration_reference: String,
    prepared_at: i64,
}

impl PreparedProviderBinding {
    pub fn new(
        request: ProviderBindingRequest,
        integration_reference: impl Into<String>,
        prepared_at: i64,
    ) -> Result<Self, AgentProviderDomainError> {
        let integration_reference = integration_reference.into();
        let integration_reference = integration_reference.trim();
        if integration_reference.is_empty()
            || integration_reference.chars().any(char::is_control)
            || integration_reference.chars().count() > MAX_TEXT_LENGTH
        {
            return Err(AgentProviderDomainError::InvalidBindingReference);
        }
        if prepared_at < 0 {
            return Err(AgentProviderDomainError::InvalidBindingTimestamp);
        }
        Ok(Self {
            execution_id: request.execution_id,
            provider_id: request.provider_id,
            model_id: request.model_id,
            availability_id: request.availability_id,
            provider_model_reference: request.provider_model_reference,
            integration_reference: integration_reference.to_string(),
            prepared_at,
        })
    }

    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }

    pub fn provider_id(&self) -> &AgentProviderId {
        &self.provider_id
    }

    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    pub fn availability_id(&self) -> &ModelAvailabilityId {
        &self.availability_id
    }

    pub fn provider_model_reference(&self) -> &str {
        &self.provider_model_reference
    }

    pub fn integration_reference(&self) -> &str {
        &self.integration_reference
    }

    pub fn prepared_at(&self) -> i64 {
        self.prepared_at
    }
}

impl ProviderProbe {
    pub fn validate(&self) -> Result<(), AgentProviderDomainError> {
        self.provider_id.validate()?;
        for diagnostic in &self.diagnostics {
            validate_text("Provider diagnostic", diagnostic.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(name: &str) -> ProviderCapability {
        ProviderCapability::new(name, 1, BTreeMap::new()).expect("valid capability")
    }

    #[test]
    fn descriptor_rejects_duplicate_capabilities() {
        let result = AgentProviderDescriptor::new(
            AgentProviderId::new("provider:one").expect("valid provider ID"),
            AgentProviderAdapterId::new("adapter:one").expect("valid adapter ID"),
            "Provider One",
            1,
            ProviderMetadata::default(),
            vec![capability("catalog:read"), capability("catalog:read")],
        );

        assert!(matches!(
            result,
            Err(AgentProviderDomainError::DuplicateCapability(_))
        ));
    }

    #[test]
    fn legacy_reference_is_not_provider_identity() {
        let provider_id = AgentProviderId::new("provider:stable").expect("valid provider ID");
        let legacy = LegacyProviderReference::new("claude", "legacy-1").expect("valid reference");

        assert_ne!(provider_id.as_str(), legacy.provider_id());
        assert_eq!(legacy.app_type(), "claude");
    }

    #[test]
    fn provider_and_adapter_identity_cannot_collide() {
        let result = AgentProviderDescriptor::new(
            AgentProviderId::new("provider:same").expect("valid provider ID"),
            AgentProviderAdapterId::new("provider:same").expect("valid adapter ID"),
            "Provider",
            1,
            ProviderMetadata::default(),
            vec![],
        );

        assert!(matches!(
            result,
            Err(AgentProviderDomainError::IdentityCollision)
        ));
    }

    #[test]
    fn execution_provider_model_and_availability_binding_identities_cannot_collide() {
        let result = ProviderBindingRequest::new(
            RuntimeExecutionId::new("identity:same").unwrap(),
            AgentProviderId::new("identity:same").unwrap(),
            ModelId::new("model:one").unwrap(),
            ModelAvailabilityId::new("availability:one").unwrap(),
            "native-model",
        );
        assert!(matches!(
            result,
            Err(AgentProviderDomainError::BindingIdentityCollision)
        ));
    }
}
