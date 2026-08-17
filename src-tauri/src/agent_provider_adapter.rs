//! Agent OS Provider adapter and compatibility boundaries.
//!
//! The legacy adapter observes existing CC Switch Provider registration through
//! a non-secret summary. It never replaces legacy persistence, copies
//! credentials, invokes an upstream API, or exposes a Model catalog.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::agent_provider_domain::{
    AgentProviderDescriptor, AgentProviderDomainError, AgentProviderId, LegacyProviderReference,
    ProviderAvailability, ProviderProbe,
};
use crate::database::Database;

#[derive(Debug, Error)]
pub enum AgentProviderAdapterError {
    #[error(transparent)]
    InvalidDomain(#[from] AgentProviderDomainError),
    #[error("Agent OS Provider adapter is already registered: {0}")]
    AlreadyRegistered(AgentProviderId),
    #[error("Agent OS Provider adapter is not registered: {0}")]
    NotRegistered(AgentProviderId),
    #[error("Agent OS Provider adapter registry lock failed: {0}")]
    RegistryLock(String),
    #[error("Legacy Provider source lookup failed: {0}")]
    LegacySource(String),
    #[error(
        "Legacy Provider source returned identity {observed} for requested Provider {expected}"
    )]
    LegacyIdentityMismatch { expected: String, observed: String },
    #[error("Provider adapter returned identity {observed} for requested Provider {expected}")]
    IdentityMismatch {
        expected: AgentProviderId,
        observed: AgentProviderId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyProviderSummary {
    pub provider_id: String,
    pub display_name: String,
    pub category: Option<String>,
}

/// Read-only compatibility seam around the current Provider source of truth.
/// Implementations return only non-secret metadata.
pub trait LegacyProviderSource: Send + Sync {
    fn find_summary(
        &self,
        reference: &LegacyProviderReference,
    ) -> Result<Option<LegacyProviderSummary>, AgentProviderAdapterError>;
}

impl LegacyProviderSource for Database {
    fn find_summary(
        &self,
        reference: &LegacyProviderReference,
    ) -> Result<Option<LegacyProviderSummary>, AgentProviderAdapterError> {
        self.get_provider_by_id(reference.provider_id(), reference.app_type())
            .map(|provider| {
                provider.map(|provider| LegacyProviderSummary {
                    provider_id: provider.id,
                    display_name: provider.name,
                    category: provider.category,
                })
            })
            .map_err(|error| AgentProviderAdapterError::LegacySource(error.to_string()))
    }
}

pub trait AgentProviderAdapter: Send + Sync {
    fn descriptor(&self) -> &AgentProviderDescriptor;

    /// Observe source registration only. This operation must not call a model
    /// API, mutate existing configuration, or resolve credentials.
    fn probe(&self) -> Result<ProviderProbe, AgentProviderAdapterError>;

    fn legacy_reference(&self) -> Option<&LegacyProviderReference> {
        None
    }
}

pub trait AgentProviderAdapterRepository: Send + Sync {
    fn register(
        &self,
        adapter: Arc<dyn AgentProviderAdapter>,
    ) -> Result<(), AgentProviderAdapterError>;
    fn get(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Option<Arc<dyn AgentProviderAdapter>>, AgentProviderAdapterError>;
    fn list(&self) -> Result<Vec<Arc<dyn AgentProviderAdapter>>, AgentProviderAdapterError>;
}

#[derive(Clone, Default)]
pub struct InMemoryAgentProviderAdapterRepository {
    adapters: Arc<RwLock<HashMap<AgentProviderId, Arc<dyn AgentProviderAdapter>>>>,
}

impl AgentProviderAdapterRepository for InMemoryAgentProviderAdapterRepository {
    fn register(
        &self,
        adapter: Arc<dyn AgentProviderAdapter>,
    ) -> Result<(), AgentProviderAdapterError> {
        adapter.descriptor().validate()?;
        let provider_id = adapter.descriptor().provider_id().clone();
        let mut adapters = self
            .adapters
            .write()
            .map_err(|error| AgentProviderAdapterError::RegistryLock(error.to_string()))?;
        if adapters.contains_key(&provider_id) {
            return Err(AgentProviderAdapterError::AlreadyRegistered(provider_id));
        }
        adapters.insert(provider_id, adapter);
        Ok(())
    }

    fn get(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Option<Arc<dyn AgentProviderAdapter>>, AgentProviderAdapterError> {
        let adapters = self
            .adapters
            .read()
            .map_err(|error| AgentProviderAdapterError::RegistryLock(error.to_string()))?;
        Ok(adapters.get(provider_id).cloned())
    }

    fn list(&self) -> Result<Vec<Arc<dyn AgentProviderAdapter>>, AgentProviderAdapterError> {
        let adapters = self
            .adapters
            .read()
            .map_err(|error| AgentProviderAdapterError::RegistryLock(error.to_string()))?;
        let mut adapters = adapters.values().cloned().collect::<Vec<_>>();
        adapters.sort_by(|left, right| {
            left.descriptor()
                .provider_id()
                .as_str()
                .cmp(right.descriptor().provider_id().as_str())
        });
        Ok(adapters)
    }
}

pub struct LegacyProviderCompatibilityAdapter<S> {
    descriptor: AgentProviderDescriptor,
    reference: LegacyProviderReference,
    source: Arc<S>,
}

impl<S> LegacyProviderCompatibilityAdapter<S>
where
    S: LegacyProviderSource,
{
    pub fn new(
        descriptor: AgentProviderDescriptor,
        reference: LegacyProviderReference,
        source: Arc<S>,
    ) -> Result<Self, AgentProviderAdapterError> {
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            reference,
            source,
        })
    }
}

impl<S> AgentProviderAdapter for LegacyProviderCompatibilityAdapter<S>
where
    S: LegacyProviderSource,
{
    fn descriptor(&self) -> &AgentProviderDescriptor {
        &self.descriptor
    }

    fn probe(&self) -> Result<ProviderProbe, AgentProviderAdapterError> {
        let summary = self.source.find_summary(&self.reference)?;
        if let Some(summary) = &summary {
            if summary.provider_id != self.reference.provider_id() {
                return Err(AgentProviderAdapterError::LegacyIdentityMismatch {
                    expected: self.reference.provider_id().to_string(),
                    observed: summary.provider_id.clone(),
                });
            }
        }
        let probe = ProviderProbe {
            provider_id: self.descriptor.provider_id().clone(),
            availability: if summary.is_some() {
                ProviderAvailability::Registered
            } else {
                ProviderAvailability::Missing
            },
            diagnostics: summary
                .map(|summary| {
                    vec![format!(
                        "Legacy Provider '{}' is registered",
                        summary.display_name
                    )]
                })
                .unwrap_or_else(|| vec!["Legacy Provider reference is not registered".to_string()]),
        };
        probe.validate()?;
        Ok(probe)
    }

    fn legacy_reference(&self) -> Option<&LegacyProviderReference> {
        Some(&self.reference)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::agent_provider_domain::{
        AgentProviderAdapterId, ProviderCapability, ProviderMetadata,
    };
    use crate::provider::Provider;

    fn descriptor(id: &str) -> AgentProviderDescriptor {
        AgentProviderDescriptor::new(
            AgentProviderId::new(id).expect("valid provider ID"),
            AgentProviderAdapterId::new(format!("adapter:{id}")).expect("valid adapter ID"),
            "Compatibility Provider",
            1,
            ProviderMetadata::default(),
            vec![
                ProviderCapability::new("catalog:metadata", 1, BTreeMap::new())
                    .expect("valid capability"),
            ],
        )
        .expect("valid descriptor")
    }

    #[test]
    fn legacy_adapter_observes_database_without_exposing_credentials() {
        let database = Arc::new(Database::memory().expect("in-memory database"));
        let legacy = Provider::with_id(
            "legacy-1".to_string(),
            "Existing Provider".to_string(),
            json!({"apiKey": "must-not-cross-boundary"}),
            Some("https://docs.example.test".to_string()),
        );
        database
            .save_provider("claude", &legacy)
            .expect("legacy provider persists");
        let adapter = LegacyProviderCompatibilityAdapter::new(
            descriptor("provider:stable"),
            LegacyProviderReference::new("claude", "legacy-1").expect("valid reference"),
            database.clone(),
        )
        .expect("adapter builds");

        let probe = adapter.probe().expect("probe succeeds");
        assert_eq!(probe.availability, ProviderAvailability::Registered);
        let serialized = serde_json::to_string(&probe).expect("probe serializes");
        assert!(!serialized.contains("must-not-cross-boundary"));
        assert_ne!(probe.provider_id.as_str(), "legacy-1");
        let preserved = database
            .get_provider_by_id("legacy-1", "claude")
            .expect("legacy lookup succeeds")
            .expect("legacy provider remains");
        assert_eq!(
            preserved.settings_config["apiKey"],
            "must-not-cross-boundary"
        );
    }

    #[test]
    fn registry_rejects_duplicate_provider_identity() {
        let database = Arc::new(Database::memory().expect("in-memory database"));
        let repository = InMemoryAgentProviderAdapterRepository::default();
        let first = LegacyProviderCompatibilityAdapter::new(
            descriptor("provider:stable"),
            LegacyProviderReference::new("claude", "legacy-1").expect("valid reference"),
            database.clone(),
        )
        .expect("adapter builds");
        repository
            .register(Arc::new(first))
            .expect("first adapter registers");
        let duplicate = LegacyProviderCompatibilityAdapter::new(
            descriptor("provider:stable"),
            LegacyProviderReference::new("claude", "legacy-1").expect("valid reference"),
            database,
        )
        .expect("adapter builds");

        assert!(matches!(
            repository.register(Arc::new(duplicate)),
            Err(AgentProviderAdapterError::AlreadyRegistered(_))
        ));
    }
}
