//! Repository boundary for Runtime activation instances.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::{
    runtime_domain::RuntimeId,
    runtime_instance_domain::{
        RuntimeInstance, RuntimeInstanceDomainError, RuntimeInstanceId, RuntimeInstanceLifecycle,
    },
};

#[derive(Debug, Error)]
pub enum RuntimeInstanceRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] RuntimeInstanceDomainError),
    #[error("Runtime instance is already registered: {0}")]
    AlreadyRegistered(RuntimeInstanceId),
    #[error("Runtime instance must be Registered at revision 1 when inserted")]
    InvalidInitialState,
    #[error("Runtime instance was not found: {0}")]
    NotFound(RuntimeInstanceId),
    #[error("Runtime {runtime_id} already has live instance {instance_id}")]
    LiveInstanceConflict {
        runtime_id: RuntimeId,
        instance_id: RuntimeInstanceId,
    },
    #[error("Runtime instance identity cannot be changed: {0}")]
    IdentityMutation(RuntimeInstanceId),
    #[error(
        "Runtime instance revision conflict for {instance_id}: expected {expected}, current {current}"
    )]
    RevisionConflict {
        instance_id: RuntimeInstanceId,
        expected: u64,
        current: u64,
    },
    #[error("Runtime instance repository lock failed: {0}")]
    RegistryLock(String),
}

pub trait RuntimeInstanceRepository: Send + Sync {
    fn insert(&self, instance: RuntimeInstance) -> Result<(), RuntimeInstanceRepositoryError>;
    fn get(
        &self,
        instance_id: &RuntimeInstanceId,
    ) -> Result<Option<RuntimeInstance>, RuntimeInstanceRepositoryError>;
    fn list(&self) -> Result<Vec<RuntimeInstance>, RuntimeInstanceRepositoryError>;
    fn list_for_runtime(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Vec<RuntimeInstance>, RuntimeInstanceRepositoryError>;
    fn update(
        &self,
        instance: RuntimeInstance,
        expected_revision: u64,
    ) -> Result<(), RuntimeInstanceRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryRuntimeInstanceRepository {
    instances: Arc<RwLock<HashMap<RuntimeInstanceId, RuntimeInstance>>>,
}

impl InMemoryRuntimeInstanceRepository {
    fn validate_live_conflict(
        instances: &HashMap<RuntimeInstanceId, RuntimeInstance>,
        candidate: &RuntimeInstance,
    ) -> Result<(), RuntimeInstanceRepositoryError> {
        if !candidate.lifecycle().is_live() {
            return Ok(());
        }
        if let Some(existing) = instances.values().find(|existing| {
            existing.id() != candidate.id()
                && existing.runtime_id() == candidate.runtime_id()
                && existing.lifecycle().is_live()
        }) {
            return Err(RuntimeInstanceRepositoryError::LiveInstanceConflict {
                runtime_id: candidate.runtime_id().clone(),
                instance_id: existing.id().clone(),
            });
        }
        Ok(())
    }
}

impl RuntimeInstanceRepository for InMemoryRuntimeInstanceRepository {
    fn insert(&self, instance: RuntimeInstance) -> Result<(), RuntimeInstanceRepositoryError> {
        instance.validate()?;
        if instance.revision() != 1 || instance.lifecycle() != RuntimeInstanceLifecycle::Registered
        {
            return Err(RuntimeInstanceRepositoryError::InvalidInitialState);
        }
        let mut instances = self
            .instances
            .write()
            .map_err(|error| RuntimeInstanceRepositoryError::RegistryLock(error.to_string()))?;
        if instances.contains_key(instance.id()) {
            return Err(RuntimeInstanceRepositoryError::AlreadyRegistered(
                instance.id().clone(),
            ));
        }
        instances.insert(instance.id().clone(), instance);
        Ok(())
    }

    fn get(
        &self,
        instance_id: &RuntimeInstanceId,
    ) -> Result<Option<RuntimeInstance>, RuntimeInstanceRepositoryError> {
        let instances = self
            .instances
            .read()
            .map_err(|error| RuntimeInstanceRepositoryError::RegistryLock(error.to_string()))?;
        let instance = instances.get(instance_id).cloned();
        if let Some(instance) = &instance {
            instance.validate()?;
        }
        Ok(instance)
    }

    fn list(&self) -> Result<Vec<RuntimeInstance>, RuntimeInstanceRepositoryError> {
        let instances = self
            .instances
            .read()
            .map_err(|error| RuntimeInstanceRepositoryError::RegistryLock(error.to_string()))?;
        let mut values = instances.values().cloned().collect::<Vec<_>>();
        for instance in &values {
            instance.validate()?;
        }
        values.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(values)
    }

    fn list_for_runtime(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Vec<RuntimeInstance>, RuntimeInstanceRepositoryError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|instance| instance.runtime_id() == runtime_id)
            .collect())
    }

    fn update(
        &self,
        instance: RuntimeInstance,
        expected_revision: u64,
    ) -> Result<(), RuntimeInstanceRepositoryError> {
        instance.validate()?;
        let mut instances = self
            .instances
            .write()
            .map_err(|error| RuntimeInstanceRepositoryError::RegistryLock(error.to_string()))?;
        let current = instances
            .get(instance.id())
            .ok_or_else(|| RuntimeInstanceRepositoryError::NotFound(instance.id().clone()))?;
        if current.revision() != expected_revision {
            return Err(RuntimeInstanceRepositoryError::RevisionConflict {
                instance_id: instance.id().clone(),
                expected: expected_revision,
                current: current.revision(),
            });
        }
        if instance.revision() <= expected_revision {
            return Err(RuntimeInstanceRepositoryError::RevisionConflict {
                instance_id: instance.id().clone(),
                expected: expected_revision + 1,
                current: instance.revision(),
            });
        }
        if current.runtime_id() != instance.runtime_id()
            || current.adapter_id() != instance.adapter_id()
            || current.created_at() != instance.created_at()
        {
            return Err(RuntimeInstanceRepositoryError::IdentityMutation(
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
    use crate::{
        runtime_domain::RuntimeAdapterId, runtime_instance_domain::RuntimeInstanceLifecycle,
    };

    fn instance(id: &str, runtime_id: &str) -> RuntimeInstance {
        RuntimeInstance::new(
            RuntimeInstanceId::new(id).unwrap(),
            RuntimeId::new(runtime_id).unwrap(),
            RuntimeAdapterId::new(format!("adapter:{id}")).unwrap(),
            10,
        )
        .unwrap()
    }

    #[test]
    fn repository_allows_history_but_only_one_live_instance_per_runtime() {
        let repository = InMemoryRuntimeInstanceRepository::default();
        let first = instance("instance:one", "runtime:one");
        repository.insert(first.clone()).unwrap();
        let activating = first
            .transition_to(RuntimeInstanceLifecycle::Activating, 1, 11)
            .unwrap();
        repository.update(activating, 1).unwrap();

        let second = instance("instance:two", "runtime:one");
        repository.insert(second.clone()).unwrap();
        let second_activating = second
            .transition_to(RuntimeInstanceLifecycle::Activating, 1, 12)
            .unwrap();
        assert!(matches!(
            repository.update(second_activating, 1),
            Err(RuntimeInstanceRepositoryError::LiveInstanceConflict { .. })
        ));
    }
}
