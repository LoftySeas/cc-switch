//! Non-executable Controlled Execution Environment revalidation service.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    agent_provider_adapter::AgentProviderLifecycleAdapterRepository,
    agent_provider_instance_repository::AgentProviderInstanceRepository,
    controlled_execution_environment::{
        ControlledExecutionEnvironment, ControlledExecutionEnvironmentDomainError,
        ControlledExecutionEnvironmentId, ControlledExecutionEnvironmentRepository,
        ControlledExecutionEnvironmentRepositoryError,
    },
    execution_readiness::{
        ActivationEvidenceAgePolicy, ControlledExecutionEnvironmentReadiness,
        ControlledExecutionEnvironmentRevalidator, EnvironmentStalenessReason,
        ExecutionReadinessDomainError,
    },
    governance_audit::{
        AuditCorrelationReferences, GovernanceAuditDomainError, GovernanceAuditEventKind,
        GovernanceAuditOutcome, GovernanceAuditRecordRequest, GovernanceAuditServiceError,
        GovernanceAuditSink, GovernanceAuditStreamId, SanitizedAuditMetadata,
    },
    governance_time::{TrustedClock, TrustedClockError},
    model_resolution::{ModelResolutionRepository, ModelResolutionRepositoryError},
    runtime_activation_adapter::RuntimeLifecycleAdapterRepository,
    runtime_instance_repository::RuntimeInstanceRepository,
};

#[derive(Debug, Error)]
pub enum ControlledExecutionEnvironmentRevalidationError {
    #[error(transparent)]
    EnvironmentRepository(#[from] ControlledExecutionEnvironmentRepositoryError),
    #[error(transparent)]
    ResolutionRepository(#[from] ModelResolutionRepositoryError),
    #[error(transparent)]
    Domain(#[from] ExecutionReadinessDomainError),
    #[error(transparent)]
    EnvironmentDomain(#[from] ControlledExecutionEnvironmentDomainError),
    #[error(transparent)]
    AuditDomain(#[from] GovernanceAuditDomainError),
    #[error(transparent)]
    Audit(#[from] GovernanceAuditServiceError),
    #[error(transparent)]
    Clock(#[from] TrustedClockError),
    #[error("Controlled execution environment was not found: {0}")]
    EnvironmentNotFound(ControlledExecutionEnvironmentId),
    #[error("Controlled execution readiness boundary lookup failed: {0}")]
    Boundary(String),
    #[error("Trusted revalidation time precedes environment preparation")]
    TrustedTimeBeforeEnvironment,
    #[error("Trusted revalidation time precedes the current operational evidence")]
    TrustedTimeBeforeOperationalEvidence,
}

pub struct ControlledExecutionEnvironmentRevalidationService<E, M, RI, RA, PI, PA, C, A> {
    environments: E,
    resolutions: M,
    runtime_instances: RI,
    runtime_adapters: RA,
    provider_instances: PI,
    provider_adapters: PA,
    clock: C,
    audit: A,
    audit_actor: String,
}

impl<E, M, RI, RA, PI, PA, C, A>
    ControlledExecutionEnvironmentRevalidationService<E, M, RI, RA, PI, PA, C, A>
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        environments: E,
        resolutions: M,
        runtime_instances: RI,
        runtime_adapters: RA,
        provider_instances: PI,
        provider_adapters: PA,
        clock: C,
        audit: A,
        audit_actor: impl Into<String>,
    ) -> Self {
        Self {
            environments,
            resolutions,
            runtime_instances,
            runtime_adapters,
            provider_instances,
            provider_adapters,
            clock,
            audit,
            audit_actor: audit_actor.into(),
        }
    }
}

impl<E, M, RI, RA, PI, PA, C, A> ControlledExecutionEnvironmentRevalidator
    for ControlledExecutionEnvironmentRevalidationService<E, M, RI, RA, PI, PA, C, A>
where
    E: ControlledExecutionEnvironmentRepository,
    M: ModelResolutionRepository,
    RI: RuntimeInstanceRepository,
    RA: RuntimeLifecycleAdapterRepository,
    PI: AgentProviderInstanceRepository,
    PA: AgentProviderLifecycleAdapterRepository,
    C: TrustedClock,
    A: GovernanceAuditSink,
{
    type Error = ControlledExecutionEnvironmentRevalidationError;

    fn revalidate(
        &self,
        environment_id: &ControlledExecutionEnvironmentId,
    ) -> Result<ControlledExecutionEnvironmentReadiness, Self::Error> {
        let environment = self
            .environments
            .get(environment_id)?
            .ok_or_else(|| Self::Error::EnvironmentNotFound(environment_id.clone()))?;
        environment.validate()?;
        let revalidated_at = self.clock.now()?;
        if revalidated_at < environment.prepared_at() {
            return Err(Self::Error::TrustedTimeBeforeEnvironment);
        }

        let mut reasons = Vec::new();
        self.validate_runtime(&environment, revalidated_at, &mut reasons)?;
        self.validate_provider(&environment, revalidated_at, &mut reasons)?;
        self.validate_resolution(&environment, &mut reasons)?;

        let readiness = if reasons.is_empty() {
            ControlledExecutionEnvironmentReadiness::ready(
                environment.environment_id().clone(),
                revalidated_at,
            )?
        } else {
            ControlledExecutionEnvironmentReadiness::stale(
                environment.environment_id().clone(),
                reasons,
                revalidated_at,
            )?
        };
        self.record(&environment, &readiness, revalidated_at)?;
        Ok(readiness)
    }
}

impl<E, M, RI, RA, PI, PA, C, A>
    ControlledExecutionEnvironmentRevalidationService<E, M, RI, RA, PI, PA, C, A>
where
    E: ControlledExecutionEnvironmentRepository,
    M: ModelResolutionRepository,
    RI: RuntimeInstanceRepository,
    RA: RuntimeLifecycleAdapterRepository,
    PI: AgentProviderInstanceRepository,
    PA: AgentProviderLifecycleAdapterRepository,
    C: TrustedClock,
    A: GovernanceAuditSink,
{
    fn validate_runtime(
        &self,
        environment: &ControlledExecutionEnvironment,
        revalidated_at: i64,
        reasons: &mut Vec<EnvironmentStalenessReason>,
    ) -> Result<(), ControlledExecutionEnvironmentRevalidationError> {
        let snapshot = environment.runtime_activation();
        let instance = self
            .runtime_instances
            .get(snapshot.instance_id())
            .map_err(|error| {
                ControlledExecutionEnvironmentRevalidationError::Boundary(error.to_string())
            })?;
        let Some(instance) = instance else {
            reasons.push(EnvironmentStalenessReason::MissingRuntimeInstance);
            return Ok(());
        };
        instance.validate().map_err(|error| {
            ControlledExecutionEnvironmentRevalidationError::Boundary(error.to_string())
        })?;
        if instance.updated_at() > revalidated_at {
            return Err(
                ControlledExecutionEnvironmentRevalidationError::TrustedTimeBeforeOperationalEvidence,
            );
        }
        if instance.revision() != snapshot.instance_revision() {
            reasons.push(EnvironmentStalenessReason::RuntimeRevisionChanged);
        }
        if instance.runtime_id() != snapshot.runtime_id() || instance.id() != snapshot.instance_id()
        {
            reasons.push(EnvironmentStalenessReason::RuntimeIdentityMismatch);
        }
        if instance.adapter_id() != snapshot.adapter_id() {
            reasons.push(EnvironmentStalenessReason::RuntimeAdapterMismatch);
        }
        if instance.lifecycle() != snapshot.lifecycle() {
            reasons.push(EnvironmentStalenessReason::RuntimeLifecycleMismatch);
        }
        if instance.health().status() != snapshot.health_status()
            || instance.health().observed_at() != snapshot.health_observed_at()
        {
            reasons.push(EnvironmentStalenessReason::RuntimeHealthMismatch);
        }
        match ActivationEvidenceAgePolicy::new(environment.evidence_max_age_millis())?
            .validate_observation(instance.health().observed_at(), revalidated_at)
        {
            Ok(()) => {}
            Err(ExecutionReadinessDomainError::StaleObservation) => {
                reasons.push(EnvironmentStalenessReason::RuntimeEvidenceExpired);
            }
            Err(error) => return Err(error.into()),
        }
        if !instance.lifecycle().accepts_execution()
            || !matches!(
                instance.health().status(),
                crate::runtime_instance_domain::RuntimeHealthStatus::Healthy
                    | crate::runtime_instance_domain::RuntimeHealthStatus::Degraded
            )
        {
            reasons.push(EnvironmentStalenessReason::RuntimeUnavailable);
        }
        let adapter = self
            .runtime_adapters
            .get(snapshot.runtime_id())
            .map_err(|error| {
                ControlledExecutionEnvironmentRevalidationError::Boundary(error.to_string())
            })?;
        if adapter.is_none_or(|adapter| adapter.descriptor().adapter_id() != snapshot.adapter_id())
        {
            reasons.push(EnvironmentStalenessReason::RuntimeAdapterMismatch);
        }
        Ok(())
    }

    fn validate_provider(
        &self,
        environment: &ControlledExecutionEnvironment,
        revalidated_at: i64,
        reasons: &mut Vec<EnvironmentStalenessReason>,
    ) -> Result<(), ControlledExecutionEnvironmentRevalidationError> {
        let snapshot = environment.provider_activation();
        let instance = self
            .provider_instances
            .get(snapshot.instance_id())
            .map_err(|error| {
                ControlledExecutionEnvironmentRevalidationError::Boundary(error.to_string())
            })?;
        let Some(instance) = instance else {
            reasons.push(EnvironmentStalenessReason::MissingProviderInstance);
            return Ok(());
        };
        instance.validate().map_err(|error| {
            ControlledExecutionEnvironmentRevalidationError::Boundary(error.to_string())
        })?;
        if instance.updated_at() > revalidated_at {
            return Err(
                ControlledExecutionEnvironmentRevalidationError::TrustedTimeBeforeOperationalEvidence,
            );
        }
        if instance.revision() != snapshot.instance_revision() {
            reasons.push(EnvironmentStalenessReason::ProviderRevisionChanged);
        }
        if instance.provider_id() != snapshot.provider_id()
            || instance.id() != snapshot.instance_id()
        {
            reasons.push(EnvironmentStalenessReason::ProviderIdentityMismatch);
        }
        if instance.adapter_id() != snapshot.adapter_id() {
            reasons.push(EnvironmentStalenessReason::ProviderAdapterMismatch);
        }
        if instance.lifecycle() != snapshot.lifecycle() {
            reasons.push(EnvironmentStalenessReason::ProviderLifecycleMismatch);
        }
        if instance
            .last_probe()
            .is_none_or(|probe| probe.availability != snapshot.availability())
        {
            reasons.push(EnvironmentStalenessReason::ProviderAvailabilityMismatch);
        }
        let current_observation_at = instance.last_probe_observed_at().ok_or_else(|| {
            ControlledExecutionEnvironmentRevalidationError::Boundary(
                "Provider observation timestamp is unavailable".into(),
            )
        })?;
        if current_observation_at != snapshot.observation_at() {
            reasons.push(EnvironmentStalenessReason::ProviderObservationMismatch);
        }
        match ActivationEvidenceAgePolicy::new(environment.evidence_max_age_millis())?
            .validate_observation(current_observation_at, revalidated_at)
        {
            Ok(()) => {}
            Err(ExecutionReadinessDomainError::StaleObservation) => {
                reasons.push(EnvironmentStalenessReason::ProviderEvidenceExpired);
            }
            Err(error) => return Err(error.into()),
        }
        if !instance.lifecycle().is_available()
            || instance
                .last_probe()
                .is_none_or(|probe| probe.availability != snapshot.availability())
        {
            reasons.push(EnvironmentStalenessReason::ProviderUnavailable);
        }
        let adapter = self
            .provider_adapters
            .get_lifecycle(snapshot.provider_id())
            .map_err(|error| {
                ControlledExecutionEnvironmentRevalidationError::Boundary(error.to_string())
            })?;
        if adapter.is_none_or(|adapter| adapter.descriptor().adapter_id() != snapshot.adapter_id())
        {
            reasons.push(EnvironmentStalenessReason::ProviderAdapterMismatch);
        }
        Ok(())
    }

    fn validate_resolution(
        &self,
        environment: &ControlledExecutionEnvironment,
        reasons: &mut Vec<EnvironmentStalenessReason>,
    ) -> Result<(), ControlledExecutionEnvironmentRevalidationError> {
        match self
            .resolutions
            .get(environment.resolution().resolution_id())?
        {
            None => reasons.push(EnvironmentStalenessReason::MissingModelResolution),
            Some(resolution) if &resolution != environment.resolution() => {
                reasons.push(EnvironmentStalenessReason::ModelResolutionMismatch);
            }
            Some(_) => {}
        }
        Ok(())
    }

    fn record(
        &self,
        environment: &ControlledExecutionEnvironment,
        readiness: &ControlledExecutionEnvironmentReadiness,
        revalidated_at: i64,
    ) -> Result<(), ControlledExecutionEnvironmentRevalidationError> {
        let (kind, outcome, reason_count) = match readiness {
            ControlledExecutionEnvironmentReadiness::Ready { .. } => (
                GovernanceAuditEventKind::ControlledEnvironmentRevalidationAccepted,
                GovernanceAuditOutcome::Accepted,
                0,
            ),
            ControlledExecutionEnvironmentReadiness::Stale { reasons, .. } => (
                GovernanceAuditEventKind::ControlledEnvironmentRevalidationRejected,
                GovernanceAuditOutcome::Stale,
                reasons.len(),
            ),
        };
        let mut metadata = BTreeMap::new();
        metadata.insert("reason_count".into(), reason_count.to_string());
        self.audit.record(GovernanceAuditRecordRequest {
            stream_id: GovernanceAuditStreamId::new(format!(
                "audit-stream:{}",
                environment.environment_id().as_str()
            ))?,
            kind,
            outcome,
            actor_reference: self.audit_actor.clone(),
            subject_type: "controlled_environment".into(),
            subject_reference: environment.environment_id().as_str().into(),
            correlations: AuditCorrelationReferences::for_environment(
                environment.execution_request().execution_id().as_str(),
                environment.environment_id().as_str(),
                environment.resolution().resolution_id().as_str(),
            )?,
            metadata: SanitizedAuditMetadata::new(metadata)?,
            not_before: revalidated_at,
        })?;
        Ok(())
    }
}
