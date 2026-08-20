//! Immutable activation snapshots and non-executable readiness results.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    agent_provider_domain::{AgentProviderAdapterId, AgentProviderId, ProviderAvailability},
    agent_provider_instance::{
        AgentProviderInstance, AgentProviderInstanceId, AgentProviderInstanceLifecycle,
    },
    controlled_execution_environment::{
        ControlledExecutionEnvironmentDomainError, ControlledExecutionEnvironmentId,
    },
    runtime_domain::{RuntimeAdapterId, RuntimeId},
    runtime_instance_domain::{
        RuntimeHealthStatus, RuntimeInstance, RuntimeInstanceId, RuntimeInstanceLifecycle,
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecutionReadinessDomainError {
    #[error("Activation snapshot revision must be positive")]
    InvalidRevision,
    #[error("Activation snapshot identities do not match the source instance")]
    IdentityMismatch,
    #[error("Activation snapshot lifecycle is unavailable")]
    UnavailableLifecycle,
    #[error("Activation snapshot observation is unavailable")]
    UnavailableObservation,
    #[error("Activation snapshot timestamp order is invalid")]
    InvalidTimestamp,
    #[error("Activation observation exceeds the maximum evidence age")]
    StaleObservation,
    #[error("Evidence age policy must be positive")]
    InvalidEvidenceAge,
    #[error("Readiness result must contain at least one stale reason")]
    MissingStaleReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationEvidenceAgePolicy {
    max_age_millis: i64,
}

impl ActivationEvidenceAgePolicy {
    pub fn new(max_age_millis: i64) -> Result<Self, ExecutionReadinessDomainError> {
        if max_age_millis <= 0 {
            return Err(ExecutionReadinessDomainError::InvalidEvidenceAge);
        }
        Ok(Self { max_age_millis })
    }

    pub fn max_age_millis(self) -> i64 {
        self.max_age_millis
    }

    pub fn validate_observation(
        self,
        observed_at: i64,
        snapshot_at: i64,
    ) -> Result<(), ExecutionReadinessDomainError> {
        if observed_at < 0 || snapshot_at < observed_at {
            return Err(ExecutionReadinessDomainError::InvalidTimestamp);
        }
        if snapshot_at - observed_at > self.max_age_millis {
            return Err(ExecutionReadinessDomainError::StaleObservation);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "RuntimeActivationSnapshotDto")]
pub struct RuntimeActivationSnapshot {
    instance_id: RuntimeInstanceId,
    instance_revision: u64,
    runtime_id: RuntimeId,
    adapter_id: RuntimeAdapterId,
    lifecycle: RuntimeInstanceLifecycle,
    health_status: RuntimeHealthStatus,
    health_observed_at: i64,
    instance_updated_at: i64,
    snapshot_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeActivationSnapshotDto {
    instance_id: RuntimeInstanceId,
    instance_revision: u64,
    runtime_id: RuntimeId,
    adapter_id: RuntimeAdapterId,
    lifecycle: RuntimeInstanceLifecycle,
    health_status: RuntimeHealthStatus,
    health_observed_at: i64,
    instance_updated_at: i64,
    snapshot_at: i64,
}

impl TryFrom<RuntimeActivationSnapshotDto> for RuntimeActivationSnapshot {
    type Error = ExecutionReadinessDomainError;

    fn try_from(value: RuntimeActivationSnapshotDto) -> Result<Self, Self::Error> {
        let snapshot = Self {
            instance_id: value.instance_id,
            instance_revision: value.instance_revision,
            runtime_id: value.runtime_id,
            adapter_id: value.adapter_id,
            lifecycle: value.lifecycle,
            health_status: value.health_status,
            health_observed_at: value.health_observed_at,
            instance_updated_at: value.instance_updated_at,
            snapshot_at: value.snapshot_at,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl RuntimeActivationSnapshot {
    pub fn capture(
        instance: &RuntimeInstance,
        snapshot_at: i64,
        age_policy: ActivationEvidenceAgePolicy,
    ) -> Result<Self, ExecutionReadinessDomainError> {
        instance
            .validate()
            .map_err(|_| ExecutionReadinessDomainError::IdentityMismatch)?;
        let snapshot = Self {
            instance_id: instance.id().clone(),
            instance_revision: instance.revision(),
            runtime_id: instance.runtime_id().clone(),
            adapter_id: instance.adapter_id().clone(),
            lifecycle: instance.lifecycle(),
            health_status: instance.health().status(),
            health_observed_at: instance.health().observed_at(),
            instance_updated_at: instance.updated_at(),
            snapshot_at,
        };
        snapshot.validate()?;
        age_policy.validate_observation(snapshot.health_observed_at, snapshot.snapshot_at)?;
        Ok(snapshot)
    }

    pub fn instance_id(&self) -> &RuntimeInstanceId {
        &self.instance_id
    }
    pub fn instance_revision(&self) -> u64 {
        self.instance_revision
    }
    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }
    pub fn adapter_id(&self) -> &RuntimeAdapterId {
        &self.adapter_id
    }
    pub fn lifecycle(&self) -> RuntimeInstanceLifecycle {
        self.lifecycle
    }
    pub fn health_status(&self) -> RuntimeHealthStatus {
        self.health_status
    }
    pub fn health_observed_at(&self) -> i64 {
        self.health_observed_at
    }
    pub fn instance_updated_at(&self) -> i64 {
        self.instance_updated_at
    }
    pub fn snapshot_at(&self) -> i64 {
        self.snapshot_at
    }

    pub fn validate(&self) -> Result<(), ExecutionReadinessDomainError> {
        RuntimeInstanceId::new(self.instance_id.as_str())
            .map_err(|_| ExecutionReadinessDomainError::IdentityMismatch)?;
        RuntimeId::new(self.runtime_id.as_str())
            .map_err(|_| ExecutionReadinessDomainError::IdentityMismatch)?;
        RuntimeAdapterId::new(self.adapter_id.as_str())
            .map_err(|_| ExecutionReadinessDomainError::IdentityMismatch)?;
        if self.instance_revision == 0 {
            return Err(ExecutionReadinessDomainError::InvalidRevision);
        }
        if !self.lifecycle.accepts_execution() {
            return Err(ExecutionReadinessDomainError::UnavailableLifecycle);
        }
        if !matches!(
            self.health_status,
            RuntimeHealthStatus::Healthy | RuntimeHealthStatus::Degraded
        ) {
            return Err(ExecutionReadinessDomainError::UnavailableObservation);
        }
        if self.instance_id.as_str() == self.runtime_id.as_str()
            || self.instance_id.as_str() == self.adapter_id.as_str()
            || self.runtime_id.as_str() == self.adapter_id.as_str()
        {
            return Err(ExecutionReadinessDomainError::IdentityMismatch);
        }
        if self.health_observed_at < 0
            || self.instance_updated_at < self.health_observed_at
            || self.snapshot_at < self.instance_updated_at
        {
            return Err(ExecutionReadinessDomainError::InvalidTimestamp);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "ProviderActivationSnapshotDto")]
pub struct ProviderActivationSnapshot {
    instance_id: AgentProviderInstanceId,
    instance_revision: u64,
    provider_id: AgentProviderId,
    adapter_id: AgentProviderAdapterId,
    lifecycle: AgentProviderInstanceLifecycle,
    availability: ProviderAvailability,
    observation_at: i64,
    instance_updated_at: i64,
    snapshot_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderActivationSnapshotDto {
    instance_id: AgentProviderInstanceId,
    instance_revision: u64,
    provider_id: AgentProviderId,
    adapter_id: AgentProviderAdapterId,
    lifecycle: AgentProviderInstanceLifecycle,
    availability: ProviderAvailability,
    observation_at: i64,
    instance_updated_at: i64,
    snapshot_at: i64,
}

impl TryFrom<ProviderActivationSnapshotDto> for ProviderActivationSnapshot {
    type Error = ExecutionReadinessDomainError;

    fn try_from(value: ProviderActivationSnapshotDto) -> Result<Self, Self::Error> {
        let snapshot = Self {
            instance_id: value.instance_id,
            instance_revision: value.instance_revision,
            provider_id: value.provider_id,
            adapter_id: value.adapter_id,
            lifecycle: value.lifecycle,
            availability: value.availability,
            observation_at: value.observation_at,
            instance_updated_at: value.instance_updated_at,
            snapshot_at: value.snapshot_at,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl ProviderActivationSnapshot {
    pub fn capture(
        instance: &AgentProviderInstance,
        snapshot_at: i64,
        age_policy: ActivationEvidenceAgePolicy,
    ) -> Result<Self, ExecutionReadinessDomainError> {
        instance
            .validate()
            .map_err(|_| ExecutionReadinessDomainError::IdentityMismatch)?;
        let probe = instance
            .last_probe()
            .ok_or(ExecutionReadinessDomainError::UnavailableObservation)?;
        let observation_at = instance
            .last_probe_observed_at()
            .ok_or(ExecutionReadinessDomainError::UnavailableObservation)?;
        let snapshot = Self {
            instance_id: instance.id().clone(),
            instance_revision: instance.revision(),
            provider_id: instance.provider_id().clone(),
            adapter_id: instance.adapter_id().clone(),
            lifecycle: instance.lifecycle(),
            availability: probe.availability,
            observation_at,
            instance_updated_at: instance.updated_at(),
            snapshot_at,
        };
        snapshot.validate()?;
        age_policy.validate_observation(snapshot.observation_at, snapshot.snapshot_at)?;
        Ok(snapshot)
    }

    pub fn instance_id(&self) -> &AgentProviderInstanceId {
        &self.instance_id
    }
    pub fn instance_revision(&self) -> u64 {
        self.instance_revision
    }
    pub fn provider_id(&self) -> &AgentProviderId {
        &self.provider_id
    }
    pub fn adapter_id(&self) -> &AgentProviderAdapterId {
        &self.adapter_id
    }
    pub fn lifecycle(&self) -> AgentProviderInstanceLifecycle {
        self.lifecycle
    }
    pub fn availability(&self) -> ProviderAvailability {
        self.availability
    }
    pub fn observation_at(&self) -> i64 {
        self.observation_at
    }
    pub fn instance_updated_at(&self) -> i64 {
        self.instance_updated_at
    }
    pub fn snapshot_at(&self) -> i64 {
        self.snapshot_at
    }

    pub fn validate(&self) -> Result<(), ExecutionReadinessDomainError> {
        AgentProviderInstanceId::new(self.instance_id.as_str())
            .map_err(|_| ExecutionReadinessDomainError::IdentityMismatch)?;
        AgentProviderId::new(self.provider_id.as_str())
            .map_err(|_| ExecutionReadinessDomainError::IdentityMismatch)?;
        AgentProviderAdapterId::new(self.adapter_id.as_str())
            .map_err(|_| ExecutionReadinessDomainError::IdentityMismatch)?;
        if self.instance_revision == 0 {
            return Err(ExecutionReadinessDomainError::InvalidRevision);
        }
        if !self.lifecycle.is_available() {
            return Err(ExecutionReadinessDomainError::UnavailableLifecycle);
        }
        if self.availability != ProviderAvailability::Registered {
            return Err(ExecutionReadinessDomainError::UnavailableObservation);
        }
        if self.instance_id.as_str() == self.provider_id.as_str()
            || self.instance_id.as_str() == self.adapter_id.as_str()
            || self.provider_id.as_str() == self.adapter_id.as_str()
        {
            return Err(ExecutionReadinessDomainError::IdentityMismatch);
        }
        if self.observation_at < 0
            || self.instance_updated_at < self.observation_at
            || self.snapshot_at < self.instance_updated_at
        {
            return Err(ExecutionReadinessDomainError::InvalidTimestamp);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentStalenessReason {
    MissingRuntimeInstance,
    RuntimeRevisionChanged,
    RuntimeIdentityMismatch,
    RuntimeAdapterMismatch,
    RuntimeLifecycleMismatch,
    RuntimeHealthMismatch,
    RuntimeEvidenceExpired,
    RuntimeUnavailable,
    MissingProviderInstance,
    ProviderRevisionChanged,
    ProviderIdentityMismatch,
    ProviderAdapterMismatch,
    ProviderLifecycleMismatch,
    ProviderAvailabilityMismatch,
    ProviderObservationMismatch,
    ProviderEvidenceExpired,
    ProviderUnavailable,
    MissingModelResolution,
    ModelResolutionMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlledExecutionEnvironmentReadiness {
    Ready {
        environment_id: ControlledExecutionEnvironmentId,
        revalidated_at: i64,
    },
    Stale {
        environment_id: ControlledExecutionEnvironmentId,
        reasons: Vec<EnvironmentStalenessReason>,
        revalidated_at: i64,
    },
}

impl ControlledExecutionEnvironmentReadiness {
    pub fn ready(
        environment_id: ControlledExecutionEnvironmentId,
        revalidated_at: i64,
    ) -> Result<Self, ExecutionReadinessDomainError> {
        if revalidated_at < 0 {
            return Err(ExecutionReadinessDomainError::InvalidTimestamp);
        }
        Ok(Self::Ready {
            environment_id,
            revalidated_at,
        })
    }

    pub fn stale(
        environment_id: ControlledExecutionEnvironmentId,
        reasons: Vec<EnvironmentStalenessReason>,
        revalidated_at: i64,
    ) -> Result<Self, ExecutionReadinessDomainError> {
        if reasons.is_empty() {
            return Err(ExecutionReadinessDomainError::MissingStaleReason);
        }
        if revalidated_at < 0 {
            return Err(ExecutionReadinessDomainError::InvalidTimestamp);
        }
        Ok(Self::Stale {
            environment_id,
            reasons,
            revalidated_at,
        })
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

pub trait ControlledExecutionEnvironmentRevalidator {
    type Error;

    fn revalidate(
        &self,
        environment_id: &ControlledExecutionEnvironmentId,
    ) -> Result<ControlledExecutionEnvironmentReadiness, Self::Error>;
}

impl From<ExecutionReadinessDomainError> for ControlledExecutionEnvironmentDomainError {
    fn from(_: ExecutionReadinessDomainError) -> Self {
        ControlledExecutionEnvironmentDomainError::ActivationSnapshotMismatch
    }
}
