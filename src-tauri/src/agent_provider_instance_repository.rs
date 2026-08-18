//! Repository boundary for operational Provider adapter instances.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::{
    agent_provider_domain::AgentProviderId,
    agent_provider_instance::{
        AgentProviderInstance, AgentProviderInstanceDomainError, AgentProviderInstanceId,
        AgentProviderInstanceLifecycle,
    },
};

#[derive(Debug, Error)]
pub enum AgentProviderInstanceRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] AgentProviderInstanceDomainError),
    #[error("Provider adapter instance is already registered: {0}")]
    AlreadyRegistered(AgentProviderInstanceId),
    #[error("Provider adapter instance must be Registered at revision 1 when inserted")]
    InvalidInitialState,
    #[error("Provider adapter instance was not found: {0}")]
    NotFound(AgentProviderInstanceId),
    #[error("Provider {provider_id} already has live adapter instance {instance_id}")]
    LiveInstanceConflict {
        provider_id: AgentProviderId,
        instance_id: AgentProviderInstanceId,
    },
    #[error("Provider adapter instance identity cannot be changed: {0}")]
    IdentityMutation(AgentProviderInstanceId),
    #[error(
        "Provider adapter instance revision conflict for {instance_id}: expected {expected}, current {current}"
    )]
    RevisionConflict {
        instance_id: AgentProviderInstanceId,
        expected: u64,
        current: u64,
    },
    #[error("Provider adapter instance repository lock failed: {0}")]
    RegistryLock(String),
}

pub trait AgentProviderInstanceRepository: Send + Sync {
    fn insert(
        &self,
        instance: AgentProviderInstance,
    ) -> Result<(), AgentProviderInstanceRepositoryError>;
    fn get(
        &self,
        instance_id: &AgentProviderInstanceId,
    ) -> Result<Option<AgentProviderInstance>, AgentProviderInstanceRepositoryError>;
    fn list(&self) -> Result<Vec<AgentProviderInstance>, AgentProviderInstanceRepositoryError>;
    fn list_for_provider(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Vec<AgentProviderInstance>, AgentProviderInstanceRepositoryError>;
    fn update(
        &self,
        instance: AgentProviderInstance,
        expected_revision: u64,
    ) -> Result<(), AgentProviderInstanceRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryAgentProviderInstanceRepository {
    instances: Arc<RwLock<HashMap<AgentProviderInstanceId, AgentProviderInstance>>>,
}

impl InMemoryAgentProviderInstanceRepository {
    fn validate_live_conflict(
        instances: &HashMap<AgentProviderInstanceId, AgentProviderInstance>,
        candidate: &AgentProviderInstance,
    ) -> Result<(), AgentProviderInstanceRepositoryError> {
        if !candidate.lifecycle().is_live() {
            return Ok(());
        }
        if let Some(existing) = instances.values().find(|existing| {
            existing.id() != candidate.id()
                && existing.provider_id() == candidate.provider_id()
                && existing.lifecycle().is_live()
        }) {
            return Err(AgentProviderInstanceRepositoryError::LiveInstanceConflict {
                provider_id: candidate.provider_id().clone(),
                instance_id: existing.id().clone(),
            });
        }
        Ok(())
    }
}

impl AgentProviderInstanceRepository for InMemoryAgentProviderInstanceRepository {
    fn insert(
        &self,
        instance: AgentProviderInstance,
    ) -> Result<(), AgentProviderInstanceRepositoryError> {
        instance.validate()?;
        if instance.revision() != 1
            || instance.lifecycle() != AgentProviderInstanceLifecycle::Registered
        {
            return Err(AgentProviderInstanceRepositoryError::InvalidInitialState);
        }
        let mut instances = self.instances.write().map_err(|error| {
            AgentProviderInstanceRepositoryError::RegistryLock(error.to_string())
        })?;
        if instances.contains_key(instance.id()) {
            return Err(AgentProviderInstanceRepositoryError::AlreadyRegistered(
                instance.id().clone(),
            ));
        }
        instances.insert(instance.id().clone(), instance);
        Ok(())
    }

    fn get(
        &self,
        instance_id: &AgentProviderInstanceId,
    ) -> Result<Option<AgentProviderInstance>, AgentProviderInstanceRepositoryError> {
        let instances = self.instances.read().map_err(|error| {
            AgentProviderInstanceRepositoryError::RegistryLock(error.to_string())
        })?;
        Ok(instances.get(instance_id).cloned())
    }

    fn list(&self) -> Result<Vec<AgentProviderInstance>, AgentProviderInstanceRepositoryError> {
        let instances = self.instances.read().map_err(|error| {
            AgentProviderInstanceRepositoryError::RegistryLock(error.to_string())
        })?;
        let mut values = instances.values().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(values)
    }

    fn list_for_provider(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Vec<AgentProviderInstance>, AgentProviderInstanceRepositoryError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|instance| instance.provider_id() == provider_id)
            .collect())
    }

    fn update(
        &self,
        instance: AgentProviderInstance,
        expected_revision: u64,
    ) -> Result<(), AgentProviderInstanceRepositoryError> {
        instance.validate()?;
        let mut instances = self.instances.write().map_err(|error| {
            AgentProviderInstanceRepositoryError::RegistryLock(error.to_string())
        })?;
        let current = instances
            .get(instance.id())
            .ok_or_else(|| AgentProviderInstanceRepositoryError::NotFound(instance.id().clone()))?;
        if current.revision() != expected_revision || instance.revision() <= expected_revision {
            return Err(AgentProviderInstanceRepositoryError::RevisionConflict {
                instance_id: instance.id().clone(),
                expected: expected_revision,
                current: current.revision(),
            });
        }
        if current.provider_id() != instance.provider_id()
            || current.adapter_id() != instance.adapter_id()
            || current.created_at() != instance.created_at()
        {
            return Err(AgentProviderInstanceRepositoryError::IdentityMutation(
                instance.id().clone(),
            ));
        }
        Self::validate_live_conflict(&instances, &instance)?;
        instances.insert(instance.id().clone(), instance);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_provider_domain::AgentProviderAdapterId;

    fn instance(id: &str) -> AgentProviderInstance {
        AgentProviderInstance::new(
            AgentProviderInstanceId::new(id).unwrap(),
            AgentProviderId::new("provider:one").unwrap(),
            AgentProviderAdapterId::new(format!("adapter:{id}")).unwrap(),
            10,
        )
        .unwrap()
    }

    #[test]
    fn repository_allows_history_but_only_one_live_instance_per_provider() {
        let repository = InMemoryAgentProviderInstanceRepository::default();
        let first = instance("provider-instance:one");
        repository.insert(first.clone()).unwrap();
        repository
            .update(
                first
                    .transition_to(AgentProviderInstanceLifecycle::Activating, 1, 11)
                    .unwrap(),
                1,
            )
            .unwrap();

        let second = instance("provider-instance:two");
        repository.insert(second.clone()).unwrap();
        assert!(matches!(
            repository.update(
                second
                    .transition_to(AgentProviderInstanceLifecycle::Activating, 1, 12)
                    .unwrap(),
                1,
            ),
            Err(AgentProviderInstanceRepositoryError::LiveInstanceConflict { .. })
        ));
    }
}
