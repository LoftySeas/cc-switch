//! Application service for controlled, non-executable environment preparation.

use thiserror::Error;

use crate::{
    agent_provider_adapter::AgentProviderLifecycleAdapterRepository,
    agent_provider_instance::AgentProviderInstanceId,
    agent_provider_instance_repository::AgentProviderInstanceRepository,
    controlled_execution_environment::{
        ControlledExecutionEnvironment, ControlledExecutionEnvironmentDomainError,
        ControlledExecutionEnvironmentRepository, ControlledExecutionEnvironmentRepositoryError,
        ControlledExecutionPreparationContract, ExecutionEnvironmentPreparationRequest,
        ExecutionIsolationBoundary, ExecutionIsolationBoundaryError,
    },
    model_resolution::{
        ModelResolutionId, ModelResolutionRepository, ModelResolutionRepositoryError,
    },
    runtime_activation_adapter::RuntimeLifecycleAdapterRepository,
    runtime_instance_domain::RuntimeInstanceId,
    runtime_instance_repository::RuntimeInstanceRepository,
};

#[derive(Debug, Error)]
pub enum ControlledExecutionEnvironmentServiceError {
    #[error(transparent)]
    Domain(#[from] ControlledExecutionEnvironmentDomainError),
    #[error(transparent)]
    Isolation(#[from] ExecutionIsolationBoundaryError),
    #[error(transparent)]
    ResolutionRepository(#[from] ModelResolutionRepositoryError),
    #[error(transparent)]
    EnvironmentRepository(#[from] ControlledExecutionEnvironmentRepositoryError),
    #[error("Controlled execution boundary lookup failed: {0}")]
    Boundary(String),
    #[error("Model resolution was not found: {0}")]
    ResolutionNotFound(ModelResolutionId),
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
    #[error("Activated Runtime or Provider identity does not match Model resolution evidence")]
    ActivatedBoundaryMismatch,
}

pub struct ControlledExecutionEnvironmentService<M, RI, RA, PI, PA, I, E> {
    resolutions: M,
    runtime_instances: RI,
    runtime_adapters: RA,
    provider_instances: PI,
    provider_adapters: PA,
    isolation: I,
    environments: E,
}

impl<M, RI, RA, PI, PA, I, E> ControlledExecutionEnvironmentService<M, RI, RA, PI, PA, I, E> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resolutions: M,
        runtime_instances: RI,
        runtime_adapters: RA,
        provider_instances: PI,
        provider_adapters: PA,
        isolation: I,
        environments: E,
    ) -> Self {
        Self {
            resolutions,
            runtime_instances,
            runtime_adapters,
            provider_instances,
            provider_adapters,
            isolation,
            environments,
        }
    }
}

impl<M, RI, RA, PI, PA, I, E> ControlledExecutionPreparationContract
    for ControlledExecutionEnvironmentService<M, RI, RA, PI, PA, I, E>
where
    M: ModelResolutionRepository,
    RI: RuntimeInstanceRepository,
    RA: RuntimeLifecycleAdapterRepository,
    PI: AgentProviderInstanceRepository,
    PA: AgentProviderLifecycleAdapterRepository,
    I: ExecutionIsolationBoundary,
    E: ControlledExecutionEnvironmentRepository,
{
    type Error = ControlledExecutionEnvironmentServiceError;

    fn prepare(
        &self,
        request: ExecutionEnvironmentPreparationRequest,
        prepared_at: i64,
    ) -> Result<ControlledExecutionEnvironment, Self::Error> {
        request.validate()?;
        let resolution = self
            .resolutions
            .get(request.model_resolution_id())?
            .ok_or_else(|| {
                ControlledExecutionEnvironmentServiceError::ResolutionNotFound(
                    request.model_resolution_id().clone(),
                )
            })?;

        let runtime = self
            .runtime_instances
            .get(resolution.runtime_instance_id())
            .map_err(|error| {
                ControlledExecutionEnvironmentServiceError::Boundary(error.to_string())
            })?
            .ok_or_else(|| {
                ControlledExecutionEnvironmentServiceError::RuntimeInstanceNotFound(
                    resolution.runtime_instance_id().clone(),
                )
            })?;
        if !runtime.lifecycle().accepts_execution() {
            return Err(
                ControlledExecutionEnvironmentServiceError::RuntimeInstanceNotReady(
                    runtime.id().clone(),
                ),
            );
        }
        if runtime.runtime_id() != resolution.runtime_id() {
            return Err(ControlledExecutionEnvironmentServiceError::ActivatedBoundaryMismatch);
        }
        let runtime_adapter = self
            .runtime_adapters
            .get(runtime.runtime_id())
            .map_err(|error| {
                ControlledExecutionEnvironmentServiceError::Boundary(error.to_string())
            })?
            .ok_or_else(|| {
                ControlledExecutionEnvironmentServiceError::RuntimeAdapterNotActive(
                    runtime.id().clone(),
                )
            })?;
        if runtime_adapter.descriptor().adapter_id() != runtime.adapter_id() {
            return Err(
                ControlledExecutionEnvironmentServiceError::RuntimeAdapterNotActive(
                    runtime.id().clone(),
                ),
            );
        }

        let provider = self
            .provider_instances
            .get(resolution.provider_instance_id())
            .map_err(|error| {
                ControlledExecutionEnvironmentServiceError::Boundary(error.to_string())
            })?
            .ok_or_else(|| {
                ControlledExecutionEnvironmentServiceError::ProviderInstanceNotFound(
                    resolution.provider_instance_id().clone(),
                )
            })?;
        if !provider.lifecycle().is_available() {
            return Err(
                ControlledExecutionEnvironmentServiceError::ProviderInstanceNotReady(
                    provider.id().clone(),
                ),
            );
        }
        if provider.provider_id() != resolution.provider_id() {
            return Err(ControlledExecutionEnvironmentServiceError::ActivatedBoundaryMismatch);
        }
        let provider_adapter = self
            .provider_adapters
            .get_lifecycle(provider.provider_id())
            .map_err(|error| {
                ControlledExecutionEnvironmentServiceError::Boundary(error.to_string())
            })?
            .ok_or_else(|| {
                ControlledExecutionEnvironmentServiceError::ProviderAdapterNotActive(
                    provider.id().clone(),
                )
            })?;
        if provider_adapter.descriptor().adapter_id() != provider.adapter_id() {
            return Err(
                ControlledExecutionEnvironmentServiceError::ProviderAdapterNotActive(
                    provider.id().clone(),
                ),
            );
        }

        let isolation = self.isolation.prepare_isolation(&request, prepared_at)?;
        let environment =
            ControlledExecutionEnvironment::new(&request, resolution, isolation, prepared_at)?;
        self.environments.insert(environment.clone())?;
        Ok(environment)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

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
        capability_domain::CapabilitySnapshotId,
        controlled_execution_environment::{
            ControlledExecutionEnvironmentId, ExecutionIsolationId,
            InMemoryControlledExecutionEnvironmentRepository, InMemoryPreparationIsolationBoundary,
        },
        execution_domain::{ExecutionGovernanceEvidence, ExecutionModelBinding, ExecutionRequest},
        model_domain::{
            ModelAvailability, ModelAvailabilityId, ModelAvailabilityStatus, ModelDescriptor,
            ModelId, ModelMetadata,
        },
        model_resolution::{
            InMemoryModelResolutionRepository, ModelResolutionRequest, ResolvedModel,
        },
        permission_domain::{AuthorizationDecisionId, PermissionGrantId},
        role_domain::RoleAssignmentId,
        runtime_activation_adapter::{
            InMemoryRuntimeLifecycleAdapterRepository, RuntimeActivationAdapterError,
            RuntimeLifecycleAdapter, RuntimeLifecycleAdapterRepository,
        },
        runtime_adapter::{RuntimeAdapter, RuntimeAdapterError},
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeAdapterId, RuntimeAvailability,
            RuntimeBindingId, RuntimeBindingLifecycle, RuntimeDescriptor, RuntimeExecutionId,
            RuntimeId, RuntimeProbe,
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

    #[derive(Default)]
    struct AdapterCalls {
        runtime_lifecycle: AtomicUsize,
        runtime_invocation: AtomicUsize,
        provider_lifecycle: AtomicUsize,
    }

    struct StubRuntimeAdapter {
        descriptor: RuntimeDescriptor,
        calls: Arc<AdapterCalls>,
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
            self.calls.runtime_invocation.fetch_add(1, Ordering::SeqCst);
            Err(RuntimeExecutionError::InvocationFailed(
                "real execution is outside COD-028".into(),
            ))
        }
    }

    impl RuntimeLifecycleAdapter for StubRuntimeAdapter {
        fn activate(
            &self,
            _instance: &RuntimeInstance,
        ) -> Result<RuntimeProbe, RuntimeActivationAdapterError> {
            self.calls.runtime_lifecycle.fetch_add(1, Ordering::SeqCst);
            Ok(runtime_probe(self.descriptor.runtime_id().clone()))
        }

        fn health(
            &self,
            _instance_id: &RuntimeInstanceId,
        ) -> Result<RuntimeProbe, RuntimeActivationAdapterError> {
            self.calls.runtime_lifecycle.fetch_add(1, Ordering::SeqCst);
            Ok(runtime_probe(self.descriptor.runtime_id().clone()))
        }

        fn deactivate(
            &self,
            _instance_id: &RuntimeInstanceId,
        ) -> Result<(), RuntimeActivationAdapterError> {
            self.calls.runtime_lifecycle.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct StubProviderAdapter {
        descriptor: AgentProviderDescriptor,
        calls: Arc<AdapterCalls>,
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
            self.calls.provider_lifecycle.fetch_add(1, Ordering::SeqCst);
            Ok(provider_probe(self.descriptor.provider_id().clone()))
        }

        fn health(
            &self,
            _instance_id: &AgentProviderInstanceId,
        ) -> Result<ProviderProbe, AgentProviderAdapterError> {
            self.calls.provider_lifecycle.fetch_add(1, Ordering::SeqCst);
            Ok(provider_probe(self.descriptor.provider_id().clone()))
        }

        fn deactivate(
            &self,
            _instance_id: &AgentProviderInstanceId,
        ) -> Result<(), AgentProviderAdapterError> {
            self.calls.provider_lifecycle.fetch_add(1, Ordering::SeqCst);
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

    type TestService = ControlledExecutionEnvironmentService<
        InMemoryModelResolutionRepository,
        InMemoryRuntimeInstanceRepository,
        InMemoryRuntimeLifecycleAdapterRepository,
        InMemoryAgentProviderInstanceRepository,
        InMemoryAgentProviderLifecycleAdapterRepository,
        InMemoryPreparationIsolationBoundary,
        InMemoryControlledExecutionEnvironmentRepository,
    >;

    fn resolved_model() -> ResolvedModel {
        let request = ModelResolutionRequest::new(
            ModelResolutionId::new("resolution:controlled").unwrap(),
            RuntimeInstanceId::new("runtime-instance:controlled").unwrap(),
            AgentProviderInstanceId::new("provider-instance:controlled").unwrap(),
            ModelId::new("model:controlled").unwrap(),
            ModelAvailabilityId::new("availability:controlled").unwrap(),
            vec![],
            14,
        )
        .unwrap();
        ResolvedModel::new(
            &request,
            RuntimeId::new("runtime:controlled").unwrap(),
            AgentProviderId::new("provider:controlled").unwrap(),
            ModelDescriptor::new(
                ModelId::new("model:controlled").unwrap(),
                "Controlled Model",
                ModelMetadata::default(),
                vec![],
            )
            .unwrap(),
            ModelAvailability::new(
                ModelAvailabilityId::new("availability:controlled").unwrap(),
                ModelId::new("model:controlled").unwrap(),
                AgentProviderId::new("provider:controlled").unwrap(),
                "native-controlled",
                ModelAvailabilityStatus::Declared,
                14,
            )
            .unwrap(),
            15,
        )
        .unwrap()
    }

    fn execution_request(runtime_id: &str) -> ExecutionRequest {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding:controlled").unwrap(),
            "agent:controlled",
            RuntimeId::new(runtime_id).unwrap(),
            10,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 11)
        .unwrap();
        ExecutionRequest::new(
            ExecutionContext::new(
                RuntimeExecutionId::new("execution:controlled").unwrap(),
                binding,
                vec!["context:sealed".into()],
                12,
            )
            .unwrap(),
            "Prepare controlled execution",
            ExecutionModelBinding::provider_model(
                ModelId::new("model:controlled").unwrap(),
                AgentProviderId::new("provider:controlled").unwrap(),
                ModelAvailabilityId::new("availability:controlled").unwrap(),
            ),
            ExecutionGovernanceEvidence::new(
                CapabilitySnapshotId::new("capability-snapshot:controlled").unwrap(),
                PermissionGrantId::new("permission-grant:controlled").unwrap(),
                RoleAssignmentId::new("role-assignment:controlled").unwrap(),
                AuthorizationDecisionId::new("authorization:controlled").unwrap(),
            ),
            None,
            13,
        )
        .unwrap()
    }

    fn preparation_request(runtime_id: &str) -> ExecutionEnvironmentPreparationRequest {
        ExecutionEnvironmentPreparationRequest::new(
            ControlledExecutionEnvironmentId::new("environment:controlled").unwrap(),
            execution_request(runtime_id),
            ModelResolutionId::new("resolution:controlled").unwrap(),
            ExecutionIsolationId::new("isolation:controlled").unwrap(),
            16,
        )
        .unwrap()
    }

    fn setup() -> (
        TestService,
        InMemoryControlledExecutionEnvironmentRepository,
        Arc<AdapterCalls>,
    ) {
        let calls = Arc::new(AdapterCalls::default());
        let resolutions = InMemoryModelResolutionRepository::default();
        resolutions.insert(resolved_model()).unwrap();

        let runtime_id = RuntimeId::new("runtime:controlled").unwrap();
        let runtime_adapter_id = RuntimeAdapterId::new("runtime-adapter:controlled").unwrap();
        let runtime_adapters = InMemoryRuntimeLifecycleAdapterRepository::default();
        runtime_adapters
            .register(Arc::new(StubRuntimeAdapter {
                descriptor: RuntimeDescriptor::new(
                    runtime_id.clone(),
                    runtime_adapter_id.clone(),
                    "Controlled Runtime",
                    1,
                    vec![],
                )
                .unwrap(),
                calls: calls.clone(),
            }))
            .unwrap();
        let runtime_instances = InMemoryRuntimeInstanceRepository::default();
        let runtime_instance = RuntimeInstance::new(
            RuntimeInstanceId::new("runtime-instance:controlled").unwrap(),
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
        runtime_instances
            .update(
                observed
                    .transition_to(RuntimeInstanceLifecycle::Ready, observed.revision(), 12)
                    .unwrap(),
                activating.revision(),
            )
            .unwrap();

        let provider_id = AgentProviderId::new("provider:controlled").unwrap();
        let provider_adapter_id =
            AgentProviderAdapterId::new("provider-adapter:controlled").unwrap();
        let provider_adapters = InMemoryAgentProviderLifecycleAdapterRepository::default();
        provider_adapters
            .register_lifecycle(Arc::new(StubProviderAdapter {
                descriptor: AgentProviderDescriptor::new(
                    provider_id.clone(),
                    provider_adapter_id.clone(),
                    "Controlled Provider",
                    1,
                    ProviderMetadata::default(),
                    vec![],
                )
                .unwrap(),
                calls: calls.clone(),
            }))
            .unwrap();
        let provider_instances = InMemoryAgentProviderInstanceRepository::default();
        let provider_instance = AgentProviderInstance::new(
            AgentProviderInstanceId::new("provider-instance:controlled").unwrap(),
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
            .record_probe(provider_probe(provider_id), activating.revision(), 12)
            .unwrap();
        provider_instances
            .update(
                observed
                    .transition_to(
                        AgentProviderInstanceLifecycle::Ready,
                        observed.revision(),
                        12,
                    )
                    .unwrap(),
                activating.revision(),
            )
            .unwrap();

        let environments = InMemoryControlledExecutionEnvironmentRepository::default();
        (
            ControlledExecutionEnvironmentService::new(
                resolutions,
                runtime_instances,
                runtime_adapters,
                provider_instances,
                provider_adapters,
                InMemoryPreparationIsolationBoundary::new("isolation-boundary:memory").unwrap(),
                environments.clone(),
            ),
            environments,
            calls,
        )
    }

    #[test]
    fn prepares_matching_evidence_without_invoking_adapters() {
        let (service, environments, calls) = setup();
        let environment = service
            .prepare(preparation_request("runtime:controlled"), 17)
            .unwrap();

        assert_eq!(
            environment.resolution().model().model_id().as_str(),
            "model:controlled"
        );
        assert_eq!(environments.list().unwrap(), vec![environment]);
        assert_eq!(calls.runtime_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn mismatched_execution_request_fails_without_evidence() {
        let (service, environments, _) = setup();
        assert!(matches!(
            service.prepare(preparation_request("runtime:other"), 17),
            Err(ControlledExecutionEnvironmentServiceError::Domain(
                ControlledExecutionEnvironmentDomainError::ResolutionMismatch
            ))
        ));
        assert!(environments.list().unwrap().is_empty());
    }

    #[test]
    fn preparation_evidence_is_immutable() {
        let (service, environments, _) = setup();
        service
            .prepare(preparation_request("runtime:controlled"), 17)
            .unwrap();
        assert!(matches!(
            service.prepare(preparation_request("runtime:controlled"), 18),
            Err(
                ControlledExecutionEnvironmentServiceError::EnvironmentRepository(
                    ControlledExecutionEnvironmentRepositoryError::AlreadyRecorded(_)
                )
            )
        ));
        assert_eq!(environments.list().unwrap().len(), 1);
    }
}
