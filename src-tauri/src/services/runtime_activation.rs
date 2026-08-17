//! Application service for Runtime instance activation and health.

use std::sync::Arc;

use thiserror::Error;

use crate::{
    runtime_activation_adapter::{
        RuntimeActivationAdapterError, RuntimeLifecycleAdapter, RuntimeLifecycleAdapterRepository,
    },
    runtime_domain::{RuntimeAvailability, RuntimeProbe},
    runtime_instance_domain::{
        RuntimeHealthObservation, RuntimeInstance, RuntimeInstanceDomainError, RuntimeInstanceId,
        RuntimeInstanceLifecycle,
    },
    runtime_instance_repository::{RuntimeInstanceRepository, RuntimeInstanceRepositoryError},
};

#[derive(Debug, Error)]
pub enum RuntimeActivationError {
    #[error(transparent)]
    Domain(#[from] RuntimeInstanceDomainError),
    #[error(transparent)]
    Repository(#[from] RuntimeInstanceRepositoryError),
    #[error(transparent)]
    Adapter(#[from] RuntimeActivationAdapterError),
    #[error("Runtime instance was not found: {0}")]
    InstanceNotFound(RuntimeInstanceId),
    #[error("Runtime instance {0} is not available for execution")]
    InstanceNotReady(RuntimeInstanceId),
}

pub struct RuntimeActivationService<I, A> {
    instances: I,
    adapters: A,
}

impl<I, A> RuntimeActivationService<I, A>
where
    I: RuntimeInstanceRepository,
    A: RuntimeLifecycleAdapterRepository,
{
    pub fn new(instances: I, adapters: A) -> Self {
        Self {
            instances,
            adapters,
        }
    }

    pub fn register_adapter(
        &self,
        adapter: Arc<dyn RuntimeLifecycleAdapter>,
    ) -> Result<(), RuntimeActivationError> {
        self.adapters.register(adapter)?;
        Ok(())
    }

    pub fn register_instance(
        &self,
        instance: RuntimeInstance,
    ) -> Result<RuntimeInstance, RuntimeActivationError> {
        let adapter = self.require_adapter(instance.runtime_id())?;
        if adapter.descriptor().adapter_id() != instance.adapter_id() {
            return Err(RuntimeActivationAdapterError::RuntimeMismatch {
                instance_id: instance.id().clone(),
                runtime_id: instance.runtime_id().clone(),
            }
            .into());
        }
        self.instances.insert(instance.clone())?;
        Ok(instance)
    }

    pub fn get(
        &self,
        instance_id: &RuntimeInstanceId,
    ) -> Result<RuntimeInstance, RuntimeActivationError> {
        self.instances
            .get(instance_id)?
            .ok_or_else(|| RuntimeActivationError::InstanceNotFound(instance_id.clone()))
    }

    pub fn list(&self) -> Result<Vec<RuntimeInstance>, RuntimeActivationError> {
        Ok(self.instances.list()?)
    }

    pub fn activate(
        &self,
        instance_id: &RuntimeInstanceId,
        expected_revision: u64,
        occurred_at: i64,
    ) -> Result<RuntimeInstance, RuntimeActivationError> {
        let current = self.get(instance_id)?;
        let activating = current.transition_to(
            RuntimeInstanceLifecycle::Activating,
            expected_revision,
            occurred_at,
        )?;
        self.instances
            .update(activating.clone(), expected_revision)?;
        let adapter = self.require_adapter(activating.runtime_id())?;
        let probe = match adapter.activate(&activating) {
            Ok(probe) => probe,
            Err(error) => {
                let failed = activating.transition_to(
                    RuntimeInstanceLifecycle::Failed,
                    activating.revision(),
                    occurred_at,
                )?;
                self.instances.update(failed, activating.revision())?;
                return Err(error.into());
            }
        };
        let observed = self.record_probe(&activating, probe, occurred_at)?;
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
        instance_id: &RuntimeInstanceId,
        expected_revision: u64,
        observed_at: i64,
    ) -> Result<RuntimeInstance, RuntimeActivationError> {
        let current = self.get(instance_id)?;
        if current.revision() != expected_revision {
            return Err(RuntimeInstanceDomainError::RevisionConflict {
                expected: expected_revision,
                current: current.revision(),
            }
            .into());
        }
        if !current.lifecycle().accepts_execution() {
            return Err(RuntimeActivationError::InstanceNotReady(
                instance_id.clone(),
            ));
        }
        let adapter = self.require_adapter(current.runtime_id())?;
        let probe = adapter.health(instance_id)?;
        let updated = self.record_probe(&current, probe, observed_at)?;
        self.instances.update(updated.clone(), expected_revision)?;
        Ok(updated)
    }

    pub fn deactivate(
        &self,
        instance_id: &RuntimeInstanceId,
        expected_revision: u64,
        occurred_at: i64,
    ) -> Result<RuntimeInstance, RuntimeActivationError> {
        let current = self.get(instance_id)?;
        let stopping = current.transition_to(
            RuntimeInstanceLifecycle::Stopping,
            expected_revision,
            occurred_at,
        )?;
        self.instances.update(stopping.clone(), expected_revision)?;
        let adapter = self.require_adapter(stopping.runtime_id())?;
        if let Err(error) = adapter.deactivate(instance_id) {
            let failed = stopping.transition_to(
                RuntimeInstanceLifecycle::Failed,
                stopping.revision(),
                occurred_at,
            )?;
            self.instances.update(failed, stopping.revision())?;
            return Err(error.into());
        }
        let stopped = stopping.transition_to(
            RuntimeInstanceLifecycle::Stopped,
            stopping.revision(),
            occurred_at,
        )?;
        self.instances
            .update(stopped.clone(), stopping.revision())?;
        Ok(stopped)
    }

    fn record_probe(
        &self,
        instance: &RuntimeInstance,
        probe: RuntimeProbe,
        observed_at: i64,
    ) -> Result<RuntimeInstance, RuntimeActivationError> {
        if &probe.runtime_id != instance.runtime_id() {
            return Err(RuntimeActivationAdapterError::RuntimeMismatch {
                instance_id: instance.id().clone(),
                runtime_id: probe.runtime_id,
            }
            .into());
        }
        let target = match probe.availability {
            RuntimeAvailability::Ready => RuntimeInstanceLifecycle::Ready,
            RuntimeAvailability::Degraded | RuntimeAvailability::RequiresConfiguration => {
                RuntimeInstanceLifecycle::Degraded
            }
            RuntimeAvailability::Unavailable => RuntimeInstanceLifecycle::Failed,
        };
        let with_health = instance.record_health(
            RuntimeHealthObservation::new(
                probe.availability.into(),
                observed_at,
                probe.diagnostics,
            )?,
            instance.revision(),
        )?;
        Ok(with_health.transition_to(target, with_health.revision(), observed_at)?)
    }

    fn require_adapter(
        &self,
        runtime_id: &crate::runtime_domain::RuntimeId,
    ) -> Result<Arc<dyn RuntimeLifecycleAdapter>, RuntimeActivationError> {
        self.adapters
            .get(runtime_id)?
            .ok_or_else(|| RuntimeActivationAdapterError::NotRegistered(runtime_id.clone()).into())
    }
}
