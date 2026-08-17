//! Agent OS Provider catalog service.
//!
//! This service exposes registration, discovery, and read-only probing only.

use std::sync::Arc;

use crate::agent_provider_adapter::{
    AgentProviderAdapter, AgentProviderAdapterError, AgentProviderAdapterRepository,
};
use crate::agent_provider_domain::{AgentProviderDescriptor, AgentProviderId, ProviderProbe};

pub struct AgentProviderService<R> {
    repository: R,
}

impl<R> AgentProviderService<R>
where
    R: AgentProviderAdapterRepository,
{
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn register(
        &self,
        adapter: Arc<dyn AgentProviderAdapter>,
    ) -> Result<(), AgentProviderAdapterError> {
        self.repository.register(adapter)
    }

    pub fn get(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<AgentProviderDescriptor, AgentProviderAdapterError> {
        self.require_adapter(provider_id)
            .map(|adapter| adapter.descriptor().clone())
    }

    pub fn list(&self) -> Result<Vec<AgentProviderDescriptor>, AgentProviderAdapterError> {
        self.repository.list().map(|adapters| {
            adapters
                .into_iter()
                .map(|adapter| adapter.descriptor().clone())
                .collect()
        })
    }

    pub fn probe(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<ProviderProbe, AgentProviderAdapterError> {
        let adapter = self.require_adapter(provider_id)?;
        let probe = adapter.probe()?;
        probe.validate()?;
        if &probe.provider_id != provider_id {
            return Err(AgentProviderAdapterError::IdentityMismatch {
                expected: provider_id.clone(),
                observed: probe.provider_id,
            });
        }
        Ok(probe)
    }

    fn require_adapter(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Arc<dyn AgentProviderAdapter>, AgentProviderAdapterError> {
        self.repository
            .get(provider_id)?
            .ok_or_else(|| AgentProviderAdapterError::NotRegistered(provider_id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::agent_provider_adapter::InMemoryAgentProviderAdapterRepository;
    use crate::agent_provider_domain::{
        AgentProviderAdapterId, ProviderAvailability, ProviderMetadata,
    };

    struct StubAdapter {
        descriptor: AgentProviderDescriptor,
        observed_id: AgentProviderId,
    }

    impl StubAdapter {
        fn new(provider_id: &str, observed_id: &str) -> Self {
            Self {
                descriptor: AgentProviderDescriptor::new(
                    AgentProviderId::new(provider_id).expect("valid Provider ID"),
                    AgentProviderAdapterId::new(format!("adapter:{provider_id}"))
                        .expect("valid adapter ID"),
                    provider_id,
                    1,
                    ProviderMetadata::default(),
                    vec![],
                )
                .expect("valid descriptor"),
                observed_id: AgentProviderId::new(observed_id).expect("valid observed ID"),
            }
        }
    }

    impl AgentProviderAdapter for StubAdapter {
        fn descriptor(&self) -> &AgentProviderDescriptor {
            &self.descriptor
        }

        fn probe(&self) -> Result<ProviderProbe, AgentProviderAdapterError> {
            Ok(ProviderProbe {
                provider_id: self.observed_id.clone(),
                availability: ProviderAvailability::Registered,
                diagnostics: vec![],
            })
        }
    }

    #[test]
    fn service_lists_descriptors_without_legacy_configuration() {
        let service = AgentProviderService::new(InMemoryAgentProviderAdapterRepository::default());
        service
            .register(Arc::new(StubAdapter::new("provider:one", "provider:one")))
            .expect("adapter registers");

        let descriptors = service.list().expect("descriptors list");
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].provider_id().as_str(), "provider:one");
    }

    #[test]
    fn service_rejects_probe_identity_mismatch() {
        let service = AgentProviderService::new(InMemoryAgentProviderAdapterRepository::default());
        service
            .register(Arc::new(StubAdapter::new("provider:one", "provider:other")))
            .expect("adapter registers");

        assert!(matches!(
            service.probe(&AgentProviderId::new("provider:one").expect("valid Provider ID")),
            Err(AgentProviderAdapterError::IdentityMismatch { .. })
        ));
    }
}
