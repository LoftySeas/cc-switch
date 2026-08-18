//! Application service for operational Provider adapter activation.
//!
//! This service coordinates adapter readiness around the existing Agent OS
//! Provider descriptor. It does not own legacy Provider data, credentials,
//! Runtime selection, Model routing, execution, Permission, or Workflow state.

use std::sync::Arc;

use thiserror::Error;

use crate::{
    agent_provider_adapter::{
        AgentProviderAdapterError, AgentProviderLifecycleAdapter,
        AgentProviderLifecycleAdapterRepository,
    },
    agent_provider_domain::{AgentProviderId, ProviderProbe},
    agent_provider_instance::{
        lifecycle_for_probe, AgentProviderInstance, AgentProviderInstanceDomainError,
        AgentProviderInstanceId, AgentProviderInstanceLifecycle,
    },
    agent_provider_instance_repository::{
        AgentProviderInstanceRepository, AgentProviderInstanceRepositoryError,
    },
};

#[derive(Debug, Error)]
pub enum AgentProviderActivationError {
    #[error(transparent)]
    Domain(#[from] AgentProviderInstanceDomainError),
    #[error(transparent)]
    Repository(#[from] AgentProviderInstanceRepositoryError),
    #[error(transparent)]
    Adapter(#[from] AgentProviderAdapterError),
    #[error("Provider adapter instance was not found: {0}")]
    InstanceNotFound(AgentProviderInstanceId),
    #[error("Provider adapter instance is not available: {0}")]
    InstanceNotAvailable(AgentProviderInstanceId),
}

pub struct AgentProviderActivationService<I, A> {
    instances: I,
    adapters: A,
}

impl<I, A> AgentProviderActivationService<I, A>
where
    I: AgentProviderInstanceRepository,
    A: AgentProviderLifecycleAdapterRepository,
{
    pub fn new(instances: I, adapters: A) -> Self {
        Self {
            instances,
            adapters,
        }
    }

    pub fn register_adapter(
        &self,
        adapter: Arc<dyn AgentProviderLifecycleAdapter>,
    ) -> Result<(), AgentProviderActivationError> {
        self.adapters.register_lifecycle(adapter)?;
        Ok(())
    }

    pub fn register_instance(
        &self,
        instance: AgentProviderInstance,
    ) -> Result<AgentProviderInstance, AgentProviderActivationError> {
        let adapter = self.require_adapter(instance.provider_id())?;
        if adapter.descriptor().adapter_id() != instance.adapter_id() {
            return Err(AgentProviderAdapterError::InstanceMismatch {
                instance_id: instance.id().clone(),
                provider_id: instance.provider_id().clone(),
            }
            .into());
        }
        self.instances.insert(instance.clone())?;
        Ok(instance)
    }

    pub fn get(
        &self,
        instance_id: &AgentProviderInstanceId,
    ) -> Result<AgentProviderInstance, AgentProviderActivationError> {
        self.instances
            .get(instance_id)?
            .ok_or_else(|| AgentProviderActivationError::InstanceNotFound(instance_id.clone()))
    }

    pub fn list(&self) -> Result<Vec<AgentProviderInstance>, AgentProviderActivationError> {
        Ok(self.instances.list()?)
    }

    pub fn activate(
        &self,
        instance_id: &AgentProviderInstanceId,
        expected_revision: u64,
        occurred_at: i64,
    ) -> Result<AgentProviderInstance, AgentProviderActivationError> {
        let current = self.get(instance_id)?;
        let activating = current.transition_to(
            AgentProviderInstanceLifecycle::Activating,
            expected_revision,
            occurred_at,
        )?;
        self.instances
            .update(activating.clone(), expected_revision)?;
        let adapter = self.require_adapter(activating.provider_id())?;
        let probe = match adapter.activate(&activating) {
            Ok(probe) => probe,
            Err(error) => {
                self.record_failed(&activating, occurred_at)?;
                return Err(error.into());
            }
        };
        let observed = match self.record_probe(&activating, probe, occurred_at) {
            Ok(observed) => observed,
            Err(error) => {
                let _ = adapter.deactivate(instance_id);
                self.record_failed(&activating, occurred_at)?;
                return Err(error);
            }
        };
        if let Err(error) = self
            .instances
            .update(observed.clone(), activating.revision())
        {
            let _ = adapter.deactivate(instance_id);
            return Err(error.into());
        }
        Ok(observed)
    }

    pub fn refresh_health(
        &self,
        instance_id: &AgentProviderInstanceId,
        expected_revision: u64,
        observed_at: i64,
    ) -> Result<AgentProviderInstance, AgentProviderActivationError> {
        let current = self.get(instance_id)?;
        if current.revision() != expected_revision {
            return Err(AgentProviderInstanceDomainError::RevisionConflict {
                expected: expected_revision,
                current: current.revision(),
            }
            .into());
        }
        if !current.lifecycle().is_available() {
            return Err(AgentProviderActivationError::InstanceNotAvailable(
                instance_id.clone(),
            ));
        }
        let adapter = self.require_adapter(current.provider_id())?;
        let probe = adapter.health(instance_id)?;
        let updated = self.record_probe(&current, probe, observed_at)?;
        self.instances.update(updated.clone(), expected_revision)?;
        Ok(updated)
    }

    pub fn deactivate(
        &self,
        instance_id: &AgentProviderInstanceId,
        expected_revision: u64,
        occurred_at: i64,
    ) -> Result<AgentProviderInstance, AgentProviderActivationError> {
        let current = self.get(instance_id)?;
        let stopping = current.transition_to(
            AgentProviderInstanceLifecycle::Stopping,
            expected_revision,
            occurred_at,
        )?;
        self.instances.update(stopping.clone(), expected_revision)?;
        let adapter = self.require_adapter(stopping.provider_id())?;
        if let Err(error) = adapter.deactivate(instance_id) {
            self.record_failed(&stopping, occurred_at)?;
            return Err(error.into());
        }
        let stopped = stopping.transition_to(
            AgentProviderInstanceLifecycle::Stopped,
            stopping.revision(),
            occurred_at,
        )?;
        self.instances
            .update(stopped.clone(), stopping.revision())?;
        Ok(stopped)
    }

    fn record_probe(
        &self,
        instance: &AgentProviderInstance,
        probe: ProviderProbe,
        observed_at: i64,
    ) -> Result<AgentProviderInstance, AgentProviderActivationError> {
        let target = lifecycle_for_probe(&probe);
        let observed = instance.record_probe(probe, instance.revision(), observed_at)?;
        Ok(observed.transition_to(target, observed.revision(), observed_at)?)
    }

    fn record_failed(
        &self,
        instance: &AgentProviderInstance,
        occurred_at: i64,
    ) -> Result<(), AgentProviderActivationError> {
        let failed = instance.transition_to(
            AgentProviderInstanceLifecycle::Failed,
            instance.revision(),
            occurred_at,
        )?;
        self.instances.update(failed, instance.revision())?;
        Ok(())
    }

    fn require_adapter(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Arc<dyn AgentProviderLifecycleAdapter>, AgentProviderActivationError> {
        self.adapters
            .get_lifecycle(provider_id)?
            .ok_or_else(|| AgentProviderAdapterError::NotRegistered(provider_id.clone()).into())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use serde_json::json;

    use super::*;
    use crate::{
        agent_provider_adapter::{
            InMemoryAgentProviderLifecycleAdapterRepository, LegacyProviderCompatibilityAdapter,
        },
        agent_provider_domain::{
            AgentProviderAdapterId, AgentProviderDescriptor, LegacyProviderReference,
            ProviderAvailability, ProviderCapability, ProviderMetadata,
        },
        agent_provider_instance_repository::InMemoryAgentProviderInstanceRepository,
        database::Database,
        provider::Provider,
    };

    fn descriptor() -> AgentProviderDescriptor {
        AgentProviderDescriptor::new(
            AgentProviderId::new("provider:cod-026").unwrap(),
            AgentProviderAdapterId::new("adapter:cod-026").unwrap(),
            "Existing Provider Compatibility",
            1,
            ProviderMetadata::default(),
            vec![ProviderCapability::new("catalog:metadata", 1, BTreeMap::new()).unwrap()],
        )
        .unwrap()
    }

    fn instance() -> AgentProviderInstance {
        AgentProviderInstance::new(
            AgentProviderInstanceId::new("provider-instance:cod-026").unwrap(),
            AgentProviderId::new("provider:cod-026").unwrap(),
            AgentProviderAdapterId::new("adapter:cod-026").unwrap(),
            10,
        )
        .unwrap()
    }

    #[test]
    fn legacy_provider_adapter_activates_without_copying_or_mutating_provider_data() {
        let database = Arc::new(Database::memory().unwrap());
        database
            .save_provider(
                "claude",
                &Provider::with_id(
                    "legacy-provider".into(),
                    "Legacy Provider".into(),
                    json!({"apiKey": "must-remain-in-legacy-storage"}),
                    None,
                ),
            )
            .unwrap();
        let adapter = Arc::new(
            LegacyProviderCompatibilityAdapter::new(
                descriptor(),
                LegacyProviderReference::new("claude", "legacy-provider").unwrap(),
                database.clone(),
            )
            .unwrap(),
        );
        let service = AgentProviderActivationService::new(
            InMemoryAgentProviderInstanceRepository::default(),
            InMemoryAgentProviderLifecycleAdapterRepository::default(),
        );
        service.register_adapter(adapter).unwrap();
        service.register_instance(instance()).unwrap();

        let ready = service
            .activate(
                &AgentProviderInstanceId::new("provider-instance:cod-026").unwrap(),
                1,
                11,
            )
            .unwrap();
        assert_eq!(ready.lifecycle(), AgentProviderInstanceLifecycle::Ready);
        assert_eq!(
            ready.last_probe().unwrap().availability,
            ProviderAvailability::Registered
        );
        assert_eq!(ready.revision(), 4);

        let healthy = service
            .refresh_health(ready.id(), ready.revision(), 12)
            .unwrap();
        let stopped = service
            .deactivate(healthy.id(), healthy.revision(), 13)
            .unwrap();
        assert_eq!(stopped.lifecycle(), AgentProviderInstanceLifecycle::Stopped);
        assert_eq!(
            database
                .get_provider_by_id("legacy-provider", "claude")
                .unwrap()
                .unwrap()
                .settings_config["apiKey"],
            "must-remain-in-legacy-storage"
        );
    }

    #[test]
    fn missing_legacy_provider_fails_closed_and_records_failed_lifecycle() {
        let adapter = Arc::new(
            LegacyProviderCompatibilityAdapter::new(
                descriptor(),
                LegacyProviderReference::new("claude", "missing-provider").unwrap(),
                Arc::new(Database::memory().unwrap()),
            )
            .unwrap(),
        );
        let service = AgentProviderActivationService::new(
            InMemoryAgentProviderInstanceRepository::default(),
            InMemoryAgentProviderLifecycleAdapterRepository::default(),
        );
        service.register_adapter(adapter).unwrap();
        service.register_instance(instance()).unwrap();
        let id = AgentProviderInstanceId::new("provider-instance:cod-026").unwrap();

        assert!(service.activate(&id, 1, 11).is_err());
        assert_eq!(
            service.get(&id).unwrap().lifecycle(),
            AgentProviderInstanceLifecycle::Failed
        );
    }
}
