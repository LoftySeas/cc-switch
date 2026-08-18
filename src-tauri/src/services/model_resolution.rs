//! Service boundary for explicit Model resolution.

use thiserror::Error;

use crate::{
    agent_provider_adapter::AgentProviderLifecycleAdapterRepository,
    agent_provider_instance::AgentProviderInstanceId,
    agent_provider_instance_repository::AgentProviderInstanceRepository,
    model_domain::{ModelAvailabilityStatus, ModelId},
    model_registry::ModelRegistry,
    model_resolution::{
        ModelResolutionContract, ModelResolutionDomainError, ModelResolutionRepository,
        ModelResolutionRepositoryError, ModelResolutionRequest, ResolvedModel,
    },
    runtime_activation_adapter::RuntimeLifecycleAdapterRepository,
    runtime_instance_domain::RuntimeInstanceId,
    runtime_instance_repository::RuntimeInstanceRepository,
};

#[derive(Debug, Error)]
pub enum ModelResolutionServiceError {
    #[error(transparent)]
    InvalidRequest(#[from] ModelResolutionDomainError),
    #[error(transparent)]
    ResolutionRepository(#[from] ModelResolutionRepositoryError),
    #[error("Model resolution boundary lookup failed: {0}")]
    Boundary(String),
    #[error("Runtime instance was not found: {0}")]
    RuntimeInstanceNotFound(RuntimeInstanceId),
    #[error("Runtime instance is not ready: {0}")]
    RuntimeInstanceNotReady(RuntimeInstanceId),
    #[error("Runtime adapter is not active for instance: {0}")]
    RuntimeAdapterNotActive(RuntimeInstanceId),
    #[error("Provider adapter instance was not found: {0}")]
    ProviderInstanceNotFound(AgentProviderInstanceId),
    #[error("Provider adapter instance is not ready: {0}")]
    ProviderInstanceNotReady(AgentProviderInstanceId),
    #[error("Provider adapter is not active for instance: {0}")]
    ProviderAdapterNotActive(AgentProviderInstanceId),
    #[error("Model was not found: {0}")]
    ModelNotFound(ModelId),
    #[error("Model availability was not found for the explicit request")]
    AvailabilityNotFound,
    #[error("Model availability does not match the explicit Model and Provider identities")]
    AvailabilityMismatch,
    #[error("Explicit Model availability is not declared")]
    AvailabilityNotDeclared,
    #[error("Model capability requirement is not satisfied: {0}")]
    CapabilityNotSatisfied(String),
}

pub struct ModelResolutionService<M, RI, RA, PI, PA, RR> {
    models: M,
    runtime_instances: RI,
    runtime_adapters: RA,
    provider_instances: PI,
    provider_adapters: PA,
    resolutions: RR,
}

impl<M, RI, RA, PI, PA, RR> ModelResolutionService<M, RI, RA, PI, PA, RR> {
    pub fn new(
        models: M,
        runtime_instances: RI,
        runtime_adapters: RA,
        provider_instances: PI,
        provider_adapters: PA,
        resolutions: RR,
    ) -> Self {
        Self {
            models,
            runtime_instances,
            runtime_adapters,
            provider_instances,
            provider_adapters,
            resolutions,
        }
    }
}

impl<M, RI, RA, PI, PA, RR> ModelResolutionContract
    for ModelResolutionService<M, RI, RA, PI, PA, RR>
where
    M: ModelRegistry,
    RI: RuntimeInstanceRepository,
    RA: RuntimeLifecycleAdapterRepository,
    PI: AgentProviderInstanceRepository,
    PA: AgentProviderLifecycleAdapterRepository,
    RR: ModelResolutionRepository,
{
    type Error = ModelResolutionServiceError;

    fn resolve_explicit(
        &self,
        request: ModelResolutionRequest,
        resolved_at: i64,
    ) -> Result<ResolvedModel, Self::Error> {
        request.validate()?;

        let runtime = self
            .runtime_instances
            .get(request.runtime_instance_id())
            .map_err(|error| ModelResolutionServiceError::Boundary(error.to_string()))?
            .ok_or_else(|| {
                ModelResolutionServiceError::RuntimeInstanceNotFound(
                    request.runtime_instance_id().clone(),
                )
            })?;
        if !runtime.lifecycle().accepts_execution() {
            return Err(ModelResolutionServiceError::RuntimeInstanceNotReady(
                runtime.id().clone(),
            ));
        }
        let runtime_adapter = self
            .runtime_adapters
            .get(runtime.runtime_id())
            .map_err(|error| ModelResolutionServiceError::Boundary(error.to_string()))?
            .ok_or_else(|| {
                ModelResolutionServiceError::RuntimeAdapterNotActive(runtime.id().clone())
            })?;
        if runtime_adapter.descriptor().adapter_id() != runtime.adapter_id() {
            return Err(ModelResolutionServiceError::RuntimeAdapterNotActive(
                runtime.id().clone(),
            ));
        }

        let provider = self
            .provider_instances
            .get(request.provider_instance_id())
            .map_err(|error| ModelResolutionServiceError::Boundary(error.to_string()))?
            .ok_or_else(|| {
                ModelResolutionServiceError::ProviderInstanceNotFound(
                    request.provider_instance_id().clone(),
                )
            })?;
        if !provider.lifecycle().is_available() {
            return Err(ModelResolutionServiceError::ProviderInstanceNotReady(
                provider.id().clone(),
            ));
        }
        let provider_adapter = self
            .provider_adapters
            .get_lifecycle(provider.provider_id())
            .map_err(|error| ModelResolutionServiceError::Boundary(error.to_string()))?
            .ok_or_else(|| {
                ModelResolutionServiceError::ProviderAdapterNotActive(provider.id().clone())
            })?;
        if provider_adapter.descriptor().adapter_id() != provider.adapter_id() {
            return Err(ModelResolutionServiceError::ProviderAdapterNotActive(
                provider.id().clone(),
            ));
        }

        let model = self
            .models
            .get_model(request.model_id())
            .map_err(|error| ModelResolutionServiceError::Boundary(error.to_string()))?
            .ok_or_else(|| {
                ModelResolutionServiceError::ModelNotFound(request.model_id().clone())
            })?;
        let availability = self
            .models
            .get_availability(request.availability_id())
            .map_err(|error| ModelResolutionServiceError::Boundary(error.to_string()))?
            .ok_or(ModelResolutionServiceError::AvailabilityNotFound)?;
        if availability.model_id() != request.model_id()
            || availability.provider_id() != provider.provider_id()
        {
            return Err(ModelResolutionServiceError::AvailabilityMismatch);
        }
        if availability.status() != ModelAvailabilityStatus::Declared {
            return Err(ModelResolutionServiceError::AvailabilityNotDeclared);
        }

        for requirement in request.capability_requirements() {
            let satisfied = model.capabilities().iter().any(|capability| {
                capability.name() == requirement.name()
                    && capability.version() >= requirement.minimum_version()
                    && requirement
                        .required_metadata()
                        .iter()
                        .all(|(key, value)| capability.metadata().get(key) == Some(value))
            });
            if !satisfied {
                return Err(ModelResolutionServiceError::CapabilityNotSatisfied(
                    requirement.name().to_string(),
                ));
            }
        }

        let resolution = ResolvedModel::new(
            &request,
            runtime.runtime_id().clone(),
            provider.provider_id().clone(),
            model,
            availability,
            resolved_at,
        )?;
        self.resolutions.insert(resolution.clone())?;
        Ok(resolution)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use super::*;
    use crate::{
        agent_provider_adapter::{
            AgentProviderAdapter, AgentProviderAdapterError, AgentProviderLifecycleAdapter,
            AgentProviderLifecycleAdapterRepository,
            InMemoryAgentProviderLifecycleAdapterRepository,
        },
        agent_provider_domain::{
            AgentProviderAdapterId, AgentProviderDescriptor, AgentProviderId, ProviderAvailability,
            ProviderMetadata, ProviderProbe,
        },
        agent_provider_instance::{AgentProviderInstance, AgentProviderInstanceLifecycle},
        agent_provider_instance_repository::{
            AgentProviderInstanceRepository, InMemoryAgentProviderInstanceRepository,
        },
        model_domain::{
            ModelAvailability, ModelAvailabilityId, ModelCapability, ModelDescriptor, ModelMetadata,
        },
        model_registry::InMemoryModelRegistry,
        model_resolution::{
            InMemoryModelResolutionRepository, ModelResolutionCapabilityRequirement,
            ModelResolutionId, ModelResolutionRepository,
        },
        runtime_activation_adapter::{
            InMemoryRuntimeLifecycleAdapterRepository, RuntimeActivationAdapterError,
            RuntimeLifecycleAdapter, RuntimeLifecycleAdapterRepository,
        },
        runtime_adapter::{RuntimeAdapter, RuntimeAdapterError},
        runtime_domain::{
            RuntimeAdapterId, RuntimeAvailability, RuntimeCapability, RuntimeDescriptor, RuntimeId,
            RuntimeProbe,
        },
        runtime_execution::{
            RuntimeExecutionAdapter, RuntimeExecutionError, RuntimeInvocation,
            RuntimeInvocationOutput,
        },
        runtime_instance_domain::{
            RuntimeHealthObservation, RuntimeHealthStatus, RuntimeInstance,
            RuntimeInstanceLifecycle,
        },
        runtime_instance_repository::{
            InMemoryRuntimeInstanceRepository, RuntimeInstanceRepository,
        },
    };

    struct StubRuntimeAdapter {
        descriptor: RuntimeDescriptor,
    }

    impl RuntimeAdapter for StubRuntimeAdapter {
        fn descriptor(&self) -> &RuntimeDescriptor {
            &self.descriptor
        }

        fn probe(&self) -> Result<RuntimeProbe, RuntimeAdapterError> {
            Ok(runtime_probe(self.descriptor.runtime_id().clone()))
        }
    }

    impl RuntimeExecutionAdapter for StubRuntimeAdapter {
        fn invoke(
            &self,
            _invocation: &RuntimeInvocation,
        ) -> Result<RuntimeInvocationOutput, RuntimeExecutionError> {
            Err(RuntimeExecutionError::InvocationFailed(
                "execution is outside COD-027".into(),
            ))
        }
    }

    impl RuntimeLifecycleAdapter for StubRuntimeAdapter {
        fn activate(
            &self,
            _instance: &RuntimeInstance,
        ) -> Result<RuntimeProbe, RuntimeActivationAdapterError> {
            Ok(runtime_probe(self.descriptor.runtime_id().clone()))
        }

        fn health(
            &self,
            _instance_id: &RuntimeInstanceId,
        ) -> Result<RuntimeProbe, RuntimeActivationAdapterError> {
            Ok(runtime_probe(self.descriptor.runtime_id().clone()))
        }

        fn deactivate(
            &self,
            _instance_id: &RuntimeInstanceId,
        ) -> Result<(), RuntimeActivationAdapterError> {
            Ok(())
        }
    }

    struct StubProviderAdapter {
        descriptor: AgentProviderDescriptor,
    }

    impl AgentProviderAdapter for StubProviderAdapter {
        fn descriptor(&self) -> &AgentProviderDescriptor {
            &self.descriptor
        }

        fn probe(&self) -> Result<ProviderProbe, AgentProviderAdapterError> {
            Ok(provider_probe(self.descriptor.provider_id().clone()))
        }
    }

    impl AgentProviderLifecycleAdapter for StubProviderAdapter {
        fn activate(
            &self,
            _instance: &AgentProviderInstance,
        ) -> Result<ProviderProbe, AgentProviderAdapterError> {
            Ok(provider_probe(self.descriptor.provider_id().clone()))
        }

        fn health(
            &self,
            _instance_id: &AgentProviderInstanceId,
        ) -> Result<ProviderProbe, AgentProviderAdapterError> {
            Ok(provider_probe(self.descriptor.provider_id().clone()))
        }

        fn deactivate(
            &self,
            _instance_id: &AgentProviderInstanceId,
        ) -> Result<(), AgentProviderAdapterError> {
            Ok(())
        }
    }

    fn runtime_probe(runtime_id: RuntimeId) -> RuntimeProbe {
        RuntimeProbe {
            runtime_id,
            availability: RuntimeAvailability::Ready,
            runtime_version: None,
            capabilities: vec![],
            diagnostics: vec![],
        }
    }

    fn provider_probe(provider_id: AgentProviderId) -> ProviderProbe {
        ProviderProbe {
            provider_id,
            availability: ProviderAvailability::Registered,
            diagnostics: vec![],
        }
    }

    type TestService = ModelResolutionService<
        InMemoryModelRegistry,
        InMemoryRuntimeInstanceRepository,
        InMemoryRuntimeLifecycleAdapterRepository,
        InMemoryAgentProviderInstanceRepository,
        InMemoryAgentProviderLifecycleAdapterRepository,
        InMemoryModelResolutionRepository,
    >;

    fn setup() -> (TestService, InMemoryModelResolutionRepository) {
        let runtime_id = RuntimeId::new("runtime:explicit").unwrap();
        let runtime_adapter_id = RuntimeAdapterId::new("runtime-adapter:explicit").unwrap();
        let runtime_adapters = InMemoryRuntimeLifecycleAdapterRepository::default();
        runtime_adapters
            .register(Arc::new(StubRuntimeAdapter {
                descriptor: RuntimeDescriptor::new(
                    runtime_id.clone(),
                    runtime_adapter_id.clone(),
                    "Explicit Runtime",
                    1,
                    vec![RuntimeCapability::new("model:resolution", 1).unwrap()],
                )
                .unwrap(),
            }))
            .unwrap();
        let runtime_instances = InMemoryRuntimeInstanceRepository::default();
        let runtime_instance = RuntimeInstance::new(
            RuntimeInstanceId::new("runtime-instance:explicit").unwrap(),
            runtime_id,
            runtime_adapter_id,
            10,
        )
        .unwrap();
        runtime_instances.insert(runtime_instance.clone()).unwrap();
        let activating = runtime_instance
            .transition_to(RuntimeInstanceLifecycle::Activating, 1, 11)
            .unwrap();
        runtime_instances.update(activating.clone(), 1).unwrap();
        let observed = activating
            .record_health(
                RuntimeHealthObservation::new(RuntimeHealthStatus::Healthy, 12, vec![]).unwrap(),
                activating.revision(),
            )
            .unwrap();
        let ready = observed
            .transition_to(RuntimeInstanceLifecycle::Ready, observed.revision(), 12)
            .unwrap();
        runtime_instances
            .update(ready, activating.revision())
            .unwrap();

        let provider_id = AgentProviderId::new("provider:explicit").unwrap();
        let provider_adapter_id = AgentProviderAdapterId::new("provider-adapter:explicit").unwrap();
        let provider_adapters = InMemoryAgentProviderLifecycleAdapterRepository::default();
        provider_adapters
            .register_lifecycle(Arc::new(StubProviderAdapter {
                descriptor: AgentProviderDescriptor::new(
                    provider_id.clone(),
                    provider_adapter_id.clone(),
                    "Explicit Provider",
                    1,
                    ProviderMetadata::default(),
                    vec![],
                )
                .unwrap(),
            }))
            .unwrap();
        let provider_instances = InMemoryAgentProviderInstanceRepository::default();
        let provider_instance = AgentProviderInstance::new(
            AgentProviderInstanceId::new("provider-instance:explicit").unwrap(),
            provider_id.clone(),
            provider_adapter_id,
            10,
        )
        .unwrap();
        provider_instances
            .insert(provider_instance.clone())
            .unwrap();
        let activating = provider_instance
            .transition_to(AgentProviderInstanceLifecycle::Activating, 1, 11)
            .unwrap();
        provider_instances.update(activating.clone(), 1).unwrap();
        let observed = activating
            .record_probe(
                provider_probe(provider_id.clone()),
                activating.revision(),
                12,
            )
            .unwrap();
        let ready = observed
            .transition_to(
                AgentProviderInstanceLifecycle::Ready,
                observed.revision(),
                12,
            )
            .unwrap();
        provider_instances
            .update(ready, activating.revision())
            .unwrap();

        let models = InMemoryModelRegistry::default();
        for id in ["model:requested", "model:other"] {
            models
                .register_model(
                    ModelDescriptor::new(
                        ModelId::new(id).unwrap(),
                        id,
                        ModelMetadata::default(),
                        vec![ModelCapability::new(
                            "text:generate",
                            2,
                            BTreeMap::from([("mode".into(), "safe".into())]),
                        )
                        .unwrap()],
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        models
            .register_availability(
                ModelAvailability::new(
                    ModelAvailabilityId::new("availability:requested").unwrap(),
                    ModelId::new("model:requested").unwrap(),
                    provider_id,
                    "native-requested",
                    ModelAvailabilityStatus::Declared,
                    12,
                )
                .unwrap(),
            )
            .unwrap();

        let resolutions = InMemoryModelResolutionRepository::default();
        (
            ModelResolutionService::new(
                models,
                runtime_instances,
                runtime_adapters,
                provider_instances,
                provider_adapters,
                resolutions.clone(),
            ),
            resolutions,
        )
    }

    fn request(minimum_version: u16) -> ModelResolutionRequest {
        ModelResolutionRequest::new(
            ModelResolutionId::new("resolution:explicit").unwrap(),
            RuntimeInstanceId::new("runtime-instance:explicit").unwrap(),
            AgentProviderInstanceId::new("provider-instance:explicit").unwrap(),
            ModelId::new("model:requested").unwrap(),
            ModelAvailabilityId::new("availability:requested").unwrap(),
            vec![ModelResolutionCapabilityRequirement::new(
                "text:generate",
                minimum_version,
                BTreeMap::from([("mode".into(), "safe".into())]),
            )
            .unwrap()],
            20,
        )
        .unwrap()
    }

    #[test]
    fn resolves_only_the_explicit_model_across_active_boundaries() {
        let (service, repository) = setup();
        let resolved = service.resolve_explicit(request(2), 21).unwrap();

        assert_eq!(resolved.model().model_id().as_str(), "model:requested");
        assert_eq!(resolved.provider_id().as_str(), "provider:explicit");
        assert_eq!(resolved.runtime_id().as_str(), "runtime:explicit");
        assert_eq!(repository.list().unwrap(), vec![resolved]);
    }

    #[test]
    fn does_not_fallback_when_explicit_capability_is_unsatisfied() {
        let (service, repository) = setup();
        assert!(matches!(
            service.resolve_explicit(request(3), 21),
            Err(ModelResolutionServiceError::CapabilityNotSatisfied(_))
        ));
        assert!(repository.list().unwrap().is_empty());
    }

    #[test]
    fn resolution_evidence_is_immutable() {
        let (service, repository) = setup();
        service.resolve_explicit(request(2), 21).unwrap();
        assert!(matches!(
            service.resolve_explicit(request(2), 22),
            Err(ModelResolutionServiceError::ResolutionRepository(
                ModelResolutionRepositoryError::AlreadyRecorded(_)
            ))
        ));
        assert_eq!(repository.list().unwrap().len(), 1);
    }
}
