//! Application service for controlled, non-executable environment preparation.

use std::collections::BTreeMap;

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
    execution_readiness::{
        ActivationEvidenceAgePolicy, ExecutionReadinessDomainError, ProviderActivationSnapshot,
        RuntimeActivationSnapshot,
    },
    governance_audit::{
        AuditCorrelationReferences, GovernanceAuditDomainError, GovernanceAuditEventKind,
        GovernanceAuditOutcome, GovernanceAuditRecordRequest, GovernanceAuditServiceError,
        GovernanceAuditSink, GovernanceAuditStreamId, SanitizedAuditMetadata,
    },
    governance_time::{TrustedClock, TrustedClockError},
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
    #[error(transparent)]
    Readiness(#[from] ExecutionReadinessDomainError),
    #[error(transparent)]
    Clock(#[from] TrustedClockError),
    #[error(transparent)]
    Audit(#[from] GovernanceAuditServiceError),
    #[error(transparent)]
    AuditDomain(#[from] GovernanceAuditDomainError),
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

impl ControlledExecutionEnvironmentServiceError {
    fn reason_code(&self) -> &'static str {
        match self {
            Self::Domain(_) => "domain_validation",
            Self::Isolation(_) => "isolation_rejected",
            Self::ResolutionRepository(_) => "resolution_repository",
            Self::EnvironmentRepository(_) => "environment_repository",
            Self::Readiness(_) => "activation_evidence",
            Self::Clock(_) => "trusted_clock",
            Self::Audit(_) => "audit_failure",
            Self::AuditDomain(_) => "audit_validation",
            Self::Boundary(_) => "boundary_repository",
            Self::ResolutionNotFound(_) => "resolution_missing",
            Self::RuntimeInstanceNotFound(_) => "runtime_instance_missing",
            Self::RuntimeInstanceNotReady(_) => "runtime_unavailable",
            Self::RuntimeAdapterNotActive(_) => "runtime_adapter_mismatch",
            Self::ProviderInstanceNotFound(_) => "provider_instance_missing",
            Self::ProviderInstanceNotReady(_) => "provider_unavailable",
            Self::ProviderAdapterNotActive(_) => "provider_adapter_mismatch",
            Self::ActivatedBoundaryMismatch => "activated_boundary_mismatch",
        }
    }
}

pub struct ControlledExecutionEnvironmentService<M, RI, RA, PI, PA, I, E, C, A> {
    resolutions: M,
    runtime_instances: RI,
    runtime_adapters: RA,
    provider_instances: PI,
    provider_adapters: PA,
    isolation: I,
    environments: E,
    clock: C,
    audit: A,
    evidence_age_policy: ActivationEvidenceAgePolicy,
    audit_actor: String,
}

impl<M, RI, RA, PI, PA, I, E, C, A>
    ControlledExecutionEnvironmentService<M, RI, RA, PI, PA, I, E, C, A>
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resolutions: M,
        runtime_instances: RI,
        runtime_adapters: RA,
        provider_instances: PI,
        provider_adapters: PA,
        isolation: I,
        environments: E,
        clock: C,
        audit: A,
        evidence_age_policy: ActivationEvidenceAgePolicy,
        audit_actor: impl Into<String>,
    ) -> Self {
        Self {
            resolutions,
            runtime_instances,
            runtime_adapters,
            provider_instances,
            provider_adapters,
            isolation,
            environments,
            clock,
            audit,
            evidence_age_policy,
            audit_actor: audit_actor.into(),
        }
    }
}

impl<M, RI, RA, PI, PA, I, E, C, A> ControlledExecutionPreparationContract
    for ControlledExecutionEnvironmentService<M, RI, RA, PI, PA, I, E, C, A>
where
    M: ModelResolutionRepository,
    RI: RuntimeInstanceRepository,
    RA: RuntimeLifecycleAdapterRepository,
    PI: AgentProviderInstanceRepository,
    PA: AgentProviderLifecycleAdapterRepository,
    I: ExecutionIsolationBoundary,
    E: ControlledExecutionEnvironmentRepository,
    C: TrustedClock,
    A: GovernanceAuditSink,
{
    type Error = ControlledExecutionEnvironmentServiceError;

    fn prepare(
        &self,
        request: ExecutionEnvironmentPreparationRequest,
    ) -> Result<ControlledExecutionEnvironment, Self::Error> {
        request.validate()?;
        self.record_event(
            &request,
            GovernanceAuditEventKind::ControlledEnvironmentPreparationRequested,
            GovernanceAuditOutcome::Accepted,
            "controlled_environment",
            request.environment_id().as_str(),
            SanitizedAuditMetadata::empty(),
            request.requested_at(),
        )?;
        let result = self.prepare_validated(&request);
        if let Err(error) = &result {
            let mut metadata = BTreeMap::new();
            metadata.insert("reason_code".into(), error.reason_code().into());
            self.record_event(
                &request,
                GovernanceAuditEventKind::ControlledEnvironmentPreparationRejected,
                GovernanceAuditOutcome::Rejected,
                "controlled_environment",
                request.environment_id().as_str(),
                SanitizedAuditMetadata::new(metadata)?,
                request.requested_at(),
            )?;
        }
        result
    }
}

impl<M, RI, RA, PI, PA, I, E, C, A>
    ControlledExecutionEnvironmentService<M, RI, RA, PI, PA, I, E, C, A>
where
    M: ModelResolutionRepository,
    RI: RuntimeInstanceRepository,
    RA: RuntimeLifecycleAdapterRepository,
    PI: AgentProviderInstanceRepository,
    PA: AgentProviderLifecycleAdapterRepository,
    I: ExecutionIsolationBoundary,
    E: ControlledExecutionEnvironmentRepository,
    C: TrustedClock,
    A: GovernanceAuditSink,
{
    fn prepare_validated(
        &self,
        request: &ExecutionEnvironmentPreparationRequest,
    ) -> Result<ControlledExecutionEnvironment, ControlledExecutionEnvironmentServiceError> {
        let resolution = self
            .resolutions
            .get(request.model_resolution_id())?
            .ok_or_else(|| {
                ControlledExecutionEnvironmentServiceError::ResolutionNotFound(
                    request.model_resolution_id().clone(),
                )
            })?;
        let execution = request.execution_request();
        let model_binding = execution.model_binding();
        if request.model_resolution_id() != resolution.resolution_id()
            || execution.context().binding().runtime_id() != resolution.runtime_id()
            || model_binding.model_id() != resolution.model().model_id()
            || model_binding.provider_id() != Some(resolution.provider_id())
            || model_binding.model_availability_id() != Some(resolution.availability().id())
        {
            return Err(ControlledExecutionEnvironmentServiceError::Domain(
                ControlledExecutionEnvironmentDomainError::ResolutionMismatch,
            ));
        }

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
        let runtime_snapshot_at = self.clock.now()?;
        if runtime_snapshot_at < request.requested_at()
            || runtime_snapshot_at < resolution.resolved_at()
        {
            return Err(ControlledExecutionEnvironmentServiceError::Domain(
                ControlledExecutionEnvironmentDomainError::InvalidTimestamp,
            ));
        }
        let runtime_snapshot = RuntimeActivationSnapshot::capture(
            &runtime,
            runtime_snapshot_at,
            self.evidence_age_policy,
        )?;
        let mut runtime_metadata = BTreeMap::new();
        runtime_metadata.insert("revision".into(), runtime.revision().to_string());
        runtime_metadata.insert("adapter_id".into(), runtime.adapter_id().as_str().into());
        self.record_event(
            request,
            GovernanceAuditEventKind::RuntimeSnapshotCaptured,
            GovernanceAuditOutcome::Accepted,
            "runtime_instance",
            runtime.id().as_str(),
            SanitizedAuditMetadata::new(runtime_metadata)?,
            runtime_snapshot_at,
        )?;

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
        let provider_snapshot_at = self.clock.now()?;
        if provider_snapshot_at < runtime_snapshot_at {
            return Err(ControlledExecutionEnvironmentServiceError::Domain(
                ControlledExecutionEnvironmentDomainError::InvalidTimestamp,
            ));
        }
        let provider_snapshot = ProviderActivationSnapshot::capture(
            &provider,
            provider_snapshot_at,
            self.evidence_age_policy,
        )?;
        let mut provider_metadata = BTreeMap::new();
        provider_metadata.insert("revision".into(), provider.revision().to_string());
        provider_metadata.insert("adapter_id".into(), provider.adapter_id().as_str().into());
        self.record_event(
            request,
            GovernanceAuditEventKind::ProviderSnapshotCaptured,
            GovernanceAuditOutcome::Accepted,
            "provider_instance",
            provider.id().as_str(),
            SanitizedAuditMetadata::new(provider_metadata)?,
            provider_snapshot_at,
        )?;

        let isolation_at = self.clock.now()?;
        if isolation_at < provider_snapshot_at {
            return Err(ControlledExecutionEnvironmentServiceError::Domain(
                ControlledExecutionEnvironmentDomainError::InvalidTimestamp,
            ));
        }
        let isolation = self.isolation.prepare_isolation(request, isolation_at)?;
        let prepared_at = self.clock.now()?;
        let environment = ControlledExecutionEnvironment::new(
            request,
            resolution,
            runtime_snapshot,
            provider_snapshot,
            isolation,
            self.evidence_age_policy,
            prepared_at,
        )?;
        // Record the final acceptance before persistence. This ordering fails
        // closed: an audit failure can never leave a durable environment with
        // no final acceptance evidence. A subsequent persistence failure is
        // recorded by the outer rejection path.
        self.record_event(
            request,
            GovernanceAuditEventKind::ControlledEnvironmentPreparationAccepted,
            GovernanceAuditOutcome::Accepted,
            "controlled_environment",
            request.environment_id().as_str(),
            SanitizedAuditMetadata::empty(),
            prepared_at,
        )?;
        self.environments.insert(environment.clone())?;
        Ok(environment)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_event(
        &self,
        request: &ExecutionEnvironmentPreparationRequest,
        kind: GovernanceAuditEventKind,
        outcome: GovernanceAuditOutcome,
        subject_type: &str,
        subject_reference: &str,
        metadata: SanitizedAuditMetadata,
        not_before: i64,
    ) -> Result<(), ControlledExecutionEnvironmentServiceError> {
        let correlations = AuditCorrelationReferences::for_environment(
            request.execution_request().execution_id().as_str(),
            request.environment_id().as_str(),
            request.model_resolution_id().as_str(),
        )?;
        self.audit.record(GovernanceAuditRecordRequest {
            stream_id: GovernanceAuditStreamId::new(format!(
                "audit-stream:{}",
                request.environment_id().as_str()
            ))?,
            kind,
            outcome,
            actor_reference: self.audit_actor.clone(),
            subject_type: subject_type.into(),
            subject_reference: subject_reference.into(),
            correlations,
            metadata,
            not_before,
        })?;
        Ok(())
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
        execution_readiness::{
            ControlledExecutionEnvironmentReadiness, ControlledExecutionEnvironmentRevalidator,
            EnvironmentStalenessReason,
        },
        governance_audit::{
            GovernanceAuditEvent, GovernanceAuditRecordRequest, GovernanceAuditRepository,
            GovernanceAuditRepositoryError, GovernanceAuditService,
            InMemoryGovernanceAuditRepository,
        },
        governance_time::FixedTrustedClock,
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
        services::execution_readiness::ControlledExecutionEnvironmentRevalidationService,
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

    fn runtime_adapter_repository(
        calls: Arc<AdapterCalls>,
        adapter_id: &str,
    ) -> InMemoryRuntimeLifecycleAdapterRepository {
        let repository = InMemoryRuntimeLifecycleAdapterRepository::default();
        repository
            .register(Arc::new(StubRuntimeAdapter {
                descriptor: RuntimeDescriptor::new(
                    RuntimeId::new("runtime:controlled").unwrap(),
                    RuntimeAdapterId::new(adapter_id).unwrap(),
                    "Controlled Runtime",
                    1,
                    vec![],
                )
                .unwrap(),
                calls,
            }))
            .unwrap();
        repository
    }

    fn provider_adapter_repository(
        calls: Arc<AdapterCalls>,
        adapter_id: &str,
    ) -> InMemoryAgentProviderLifecycleAdapterRepository {
        let repository = InMemoryAgentProviderLifecycleAdapterRepository::default();
        repository
            .register_lifecycle(Arc::new(StubProviderAdapter {
                descriptor: AgentProviderDescriptor::new(
                    AgentProviderId::new("provider:controlled").unwrap(),
                    AgentProviderAdapterId::new(adapter_id).unwrap(),
                    "Controlled Provider",
                    1,
                    ProviderMetadata::default(),
                    vec![],
                )
                .unwrap(),
                calls,
            }))
            .unwrap();
        repository
    }

    #[derive(Clone)]
    struct TestAuditSink {
        delegate: GovernanceAuditService<InMemoryGovernanceAuditRepository, FixedTrustedClock>,
        reject_final_acceptance: bool,
    }

    impl TestAuditSink {
        fn new(
            repository: InMemoryGovernanceAuditRepository,
            trusted_time: i64,
            reject_final_acceptance: bool,
        ) -> Self {
            Self {
                delegate: GovernanceAuditService::new(
                    repository,
                    FixedTrustedClock::new(trusted_time).unwrap(),
                ),
                reject_final_acceptance,
            }
        }
    }

    impl GovernanceAuditSink for TestAuditSink {
        fn record(
            &self,
            request: GovernanceAuditRecordRequest,
        ) -> Result<GovernanceAuditEvent, GovernanceAuditServiceError> {
            if self.reject_final_acceptance
                && request.kind
                    == GovernanceAuditEventKind::ControlledEnvironmentPreparationAccepted
            {
                return Err(GovernanceAuditServiceError::Repository(
                    GovernanceAuditRepositoryError::Persistence(
                        "injected final audit failure".into(),
                    ),
                ));
            }
            self.delegate.record(request)
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
        FixedTrustedClock,
        TestAuditSink,
    >;

    type TestRevalidator = ControlledExecutionEnvironmentRevalidationService<
        InMemoryControlledExecutionEnvironmentRepository,
        InMemoryModelResolutionRepository,
        InMemoryRuntimeInstanceRepository,
        InMemoryRuntimeLifecycleAdapterRepository,
        InMemoryAgentProviderInstanceRepository,
        InMemoryAgentProviderLifecycleAdapterRepository,
        FixedTrustedClock,
        TestAuditSink,
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
        TestRevalidator,
        InMemoryControlledExecutionEnvironmentRepository,
        InMemoryRuntimeInstanceRepository,
        InMemoryAgentProviderInstanceRepository,
        Arc<AdapterCalls>,
        InMemoryGovernanceAuditRepository,
    ) {
        setup_with_policy_and_time(60, 20)
    }

    #[allow(clippy::type_complexity)]
    fn setup_with_policy_and_time(
        max_age_millis: i64,
        trusted_time: i64,
    ) -> (
        TestService,
        TestRevalidator,
        InMemoryControlledExecutionEnvironmentRepository,
        InMemoryRuntimeInstanceRepository,
        InMemoryAgentProviderInstanceRepository,
        Arc<AdapterCalls>,
        InMemoryGovernanceAuditRepository,
    ) {
        setup_with_options(max_age_millis, trusted_time, false)
    }

    #[allow(clippy::type_complexity)]
    fn setup_with_options(
        max_age_millis: i64,
        trusted_time: i64,
        reject_final_acceptance: bool,
    ) -> (
        TestService,
        TestRevalidator,
        InMemoryControlledExecutionEnvironmentRepository,
        InMemoryRuntimeInstanceRepository,
        InMemoryAgentProviderInstanceRepository,
        Arc<AdapterCalls>,
        InMemoryGovernanceAuditRepository,
    ) {
        let calls = Arc::new(AdapterCalls::default());
        let resolutions = InMemoryModelResolutionRepository::default();
        resolutions.insert(resolved_model()).unwrap();

        let runtime_id = RuntimeId::new("runtime:controlled").unwrap();
        let runtime_adapter_id = RuntimeAdapterId::new("runtime-adapter:controlled").unwrap();
        let runtime_adapters =
            runtime_adapter_repository(calls.clone(), "runtime-adapter:controlled");
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
        let provider_adapters =
            provider_adapter_repository(calls.clone(), "provider-adapter:controlled");
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
        let audit_repository = InMemoryGovernanceAuditRepository::default();
        let revalidator = ControlledExecutionEnvironmentRevalidationService::new(
            environments.clone(),
            resolutions.clone(),
            runtime_instances.clone(),
            runtime_adapters.clone(),
            provider_instances.clone(),
            provider_adapters.clone(),
            FixedTrustedClock::new(trusted_time).unwrap(),
            TestAuditSink::new(audit_repository.clone(), trusted_time, false),
            "actor:controlled-test",
        );
        (
            ControlledExecutionEnvironmentService::new(
                resolutions,
                runtime_instances.clone(),
                runtime_adapters,
                provider_instances.clone(),
                provider_adapters,
                InMemoryPreparationIsolationBoundary::new("isolation-boundary:memory").unwrap(),
                environments.clone(),
                FixedTrustedClock::new(trusted_time).unwrap(),
                TestAuditSink::new(
                    audit_repository.clone(),
                    trusted_time,
                    reject_final_acceptance,
                ),
                ActivationEvidenceAgePolicy::new(max_age_millis).unwrap(),
                "actor:controlled-test",
            ),
            revalidator,
            environments,
            runtime_instances,
            provider_instances,
            calls,
            audit_repository,
        )
    }

    #[test]
    fn prepares_matching_evidence_without_invoking_adapters() {
        let (service, revalidator, environments, _, _, calls, audit) = setup();
        let environment = service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();

        assert_eq!(
            environment.resolution().model().model_id().as_str(),
            "model:controlled"
        );
        assert_eq!(environments.list().unwrap(), vec![environment.clone()]);
        assert_eq!(calls.runtime_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
        assert_eq!(environment.runtime_activation().instance_revision(), 4);
        assert_eq!(environment.provider_activation().instance_revision(), 4);
        assert!(revalidator
            .revalidate(environment.environment_id())
            .unwrap()
            .is_ready());
        assert_eq!(calls.runtime_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
        assert_eq!(
            audit
                .list_stream(
                    &GovernanceAuditStreamId::new("audit-stream:environment:controlled").unwrap(),
                    10,
                )
                .unwrap()
                .len(),
            5
        );
    }

    #[test]
    fn mismatched_execution_request_fails_without_evidence() {
        let (service, _, environments, _, _, _, audit) = setup();
        assert!(matches!(
            service.prepare(preparation_request("runtime:other")),
            Err(ControlledExecutionEnvironmentServiceError::Domain(
                ControlledExecutionEnvironmentDomainError::ResolutionMismatch
            ))
        ));
        assert!(environments.list().unwrap().is_empty());
        let events = audit
            .list_stream(
                &GovernanceAuditStreamId::new("audit-stream:environment:controlled").unwrap(),
                10,
            )
            .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events.last().unwrap().kind(),
            GovernanceAuditEventKind::ControlledEnvironmentPreparationRejected
        );
    }

    #[test]
    fn preparation_evidence_is_immutable() {
        let (service, _, environments, _, _, _, _) = setup();
        service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();
        assert!(matches!(
            service.prepare(preparation_request("runtime:controlled")),
            Err(
                ControlledExecutionEnvironmentServiceError::EnvironmentRepository(
                    ControlledExecutionEnvironmentRepositoryError::AlreadyRecorded(_)
                )
            )
        ));
        assert_eq!(environments.list().unwrap().len(), 1);
    }

    #[test]
    fn final_audit_failure_never_persists_an_environment() {
        let (service, _, environments, _, _, calls, audit) = setup_with_options(60, 20, true);
        assert!(matches!(
            service.prepare(preparation_request("runtime:controlled")),
            Err(ControlledExecutionEnvironmentServiceError::Audit(
                GovernanceAuditServiceError::Repository(
                    GovernanceAuditRepositoryError::Persistence(_)
                )
            ))
        ));
        assert!(environments.list().unwrap().is_empty());
        let events = audit
            .list_stream(
                &GovernanceAuditStreamId::new("audit-stream:environment:controlled").unwrap(),
                10,
            )
            .unwrap();
        assert_eq!(
            events.last().unwrap().kind(),
            GovernanceAuditEventKind::ControlledEnvironmentPreparationRejected
        );
        assert_eq!(calls.runtime_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stale_runtime_revision_revalidation_fails_closed_without_invocation() {
        let (service, revalidator, _, runtimes, _, calls, _) = setup();
        let environment = service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();
        let current = runtimes
            .get(environment.runtime_activation().instance_id())
            .unwrap()
            .unwrap();
        let changed = current
            .transition_to(RuntimeInstanceLifecycle::Degraded, current.revision(), 20)
            .unwrap();
        runtimes.update(changed, current.revision()).unwrap();

        let readiness = revalidator
            .revalidate(environment.environment_id())
            .unwrap();
        assert!(matches!(
            readiness,
            ControlledExecutionEnvironmentReadiness::Stale { ref reasons, .. }
                if reasons.contains(&EnvironmentStalenessReason::RuntimeRevisionChanged)
        ));
        assert_eq!(calls.runtime_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unavailable_runtime_revalidation_fails_closed() {
        let (service, revalidator, _, runtimes, _, calls, _) = setup();
        let environment = service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();
        let current = runtimes
            .get(environment.runtime_activation().instance_id())
            .unwrap()
            .unwrap();
        runtimes
            .update(
                current
                    .transition_to(RuntimeInstanceLifecycle::Stopping, current.revision(), 20)
                    .unwrap(),
                current.revision(),
            )
            .unwrap();
        assert!(matches!(
            revalidator.revalidate(environment.environment_id()).unwrap(),
            ControlledExecutionEnvironmentReadiness::Stale { ref reasons, .. }
                if reasons.contains(&EnvironmentStalenessReason::RuntimeUnavailable)
        ));
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unavailable_provider_revalidation_fails_closed() {
        let (service, revalidator, _, _, providers, calls, _) = setup();
        let environment = service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();
        let current = providers
            .get(environment.provider_activation().instance_id())
            .unwrap()
            .unwrap();
        providers
            .update(
                current
                    .transition_to(
                        AgentProviderInstanceLifecycle::Stopping,
                        current.revision(),
                        20,
                    )
                    .unwrap(),
                current.revision(),
            )
            .unwrap();
        assert!(matches!(
            revalidator.revalidate(environment.environment_id()).unwrap(),
            ControlledExecutionEnvironmentReadiness::Stale { ref reasons, .. }
                if reasons.contains(&EnvironmentStalenessReason::ProviderUnavailable)
        ));
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn changed_provider_probe_observation_fails_revalidation_closed() {
        let (service, revalidator, _, _, providers, calls, _) = setup();
        let environment = service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();
        let current = providers
            .get(environment.provider_activation().instance_id())
            .unwrap()
            .unwrap();
        let changed = current
            .record_probe(
                provider_probe(current.provider_id().clone()),
                current.revision(),
                20,
            )
            .unwrap();
        providers.update(changed, current.revision()).unwrap();

        assert!(matches!(
            revalidator.revalidate(environment.environment_id()).unwrap(),
            ControlledExecutionEnvironmentReadiness::Stale { ref reasons, .. }
                if reasons.contains(&EnvironmentStalenessReason::ProviderRevisionChanged)
                    && reasons.contains(&EnvironmentStalenessReason::ProviderObservationMismatch)
        ));
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn adapter_identity_mismatch_revalidation_fails_closed_without_invocation() {
        let (service, _, environments, runtimes, providers, calls, _) = setup();
        let environment = service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();
        let resolutions = InMemoryModelResolutionRepository::default();
        resolutions
            .insert(environment.resolution().clone())
            .unwrap();
        let revalidator = ControlledExecutionEnvironmentRevalidationService::new(
            environments,
            resolutions,
            runtimes,
            runtime_adapter_repository(calls.clone(), "runtime-adapter:mismatch"),
            providers,
            provider_adapter_repository(calls.clone(), "provider-adapter:mismatch"),
            FixedTrustedClock::new(20).unwrap(),
            GovernanceAuditService::new(
                InMemoryGovernanceAuditRepository::default(),
                FixedTrustedClock::new(20).unwrap(),
            ),
            "actor:controlled-test",
        );

        assert!(matches!(
            revalidator.revalidate(environment.environment_id()).unwrap(),
            ControlledExecutionEnvironmentReadiness::Stale { ref reasons, .. }
                if reasons.contains(&EnvironmentStalenessReason::RuntimeAdapterMismatch)
                    && reasons.contains(&EnvironmentStalenessReason::ProviderAdapterMismatch)
        ));
        assert_eq!(calls.runtime_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn missing_resolution_revalidation_fails_closed_without_invocation() {
        let (service, _, environments, runtimes, providers, calls, _) = setup();
        let environment = service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();
        let revalidator = ControlledExecutionEnvironmentRevalidationService::new(
            environments,
            InMemoryModelResolutionRepository::default(),
            runtimes,
            runtime_adapter_repository(calls.clone(), "runtime-adapter:controlled"),
            providers,
            provider_adapter_repository(calls.clone(), "provider-adapter:controlled"),
            FixedTrustedClock::new(20).unwrap(),
            GovernanceAuditService::new(
                InMemoryGovernanceAuditRepository::default(),
                FixedTrustedClock::new(20).unwrap(),
            ),
            "actor:controlled-test",
        );

        assert!(matches!(
            revalidator.revalidate(environment.environment_id()).unwrap(),
            ControlledExecutionEnvironmentReadiness::Stale { ref reasons, .. }
                if reasons.contains(&EnvironmentStalenessReason::MissingModelResolution)
        ));
        assert_eq!(calls.runtime_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unchanged_but_expired_observations_fail_revalidation_closed() {
        let (service, _, environments, runtimes, providers, calls, _) =
            setup_with_policy_and_time(10, 20);
        let environment = service
            .prepare(preparation_request("runtime:controlled"))
            .unwrap();
        let resolutions = InMemoryModelResolutionRepository::default();
        resolutions
            .insert(environment.resolution().clone())
            .unwrap();
        let revalidator = ControlledExecutionEnvironmentRevalidationService::new(
            environments,
            resolutions,
            runtimes,
            runtime_adapter_repository(calls.clone(), "runtime-adapter:controlled"),
            providers,
            provider_adapter_repository(calls.clone(), "provider-adapter:controlled"),
            FixedTrustedClock::new(23).unwrap(),
            TestAuditSink::new(InMemoryGovernanceAuditRepository::default(), 23, false),
            "actor:controlled-test",
        );

        assert!(matches!(
            revalidator.revalidate(environment.environment_id()).unwrap(),
            ControlledExecutionEnvironmentReadiness::Stale { ref reasons, .. }
                if reasons.contains(&EnvironmentStalenessReason::RuntimeEvidenceExpired)
                    && reasons.contains(&EnvironmentStalenessReason::ProviderEvidenceExpired)
        ));
        assert_eq!(calls.runtime_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.provider_lifecycle.load(Ordering::SeqCst), 0);
        assert_eq!(calls.runtime_invocation.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stale_activation_observation_is_rejected_by_policy() {
        let (service, _, environments, _, _, _, _) = setup_with_policy_and_time(5, 20);
        assert!(matches!(
            service.prepare(preparation_request("runtime:controlled")),
            Err(ControlledExecutionEnvironmentServiceError::Readiness(
                ExecutionReadinessDomainError::StaleObservation
            ))
        ));
        assert!(environments.list().unwrap().is_empty());
    }

    #[test]
    fn trusted_time_before_request_is_rejected() {
        let (service, _, environments, _, _, _, _) = setup_with_policy_and_time(60, 15);
        assert!(matches!(
            service.prepare(preparation_request("runtime:controlled")),
            Err(ControlledExecutionEnvironmentServiceError::Audit(
                GovernanceAuditServiceError::TrustedTimeBeforeEvidence
            ))
        ));
        assert!(environments.list().unwrap().is_empty());
    }
}
