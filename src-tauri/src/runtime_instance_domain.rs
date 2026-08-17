//! Runtime activation lifecycle and health domain.
//!
//! A Runtime instance is an operational activation of a Runtime descriptor. It
//! is neither an Agent nor an Execution and it never owns Provider or Model
//! identity.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::runtime_domain::{RuntimeAdapterId, RuntimeAvailability, RuntimeId};

const MAX_ID_LENGTH: usize = 128;
const MAX_DIAGNOSTIC_LENGTH: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeInstanceDomainError {
    #[error("Runtime instance ID is empty")]
    EmptyId,
    #[error("Runtime instance ID exceeds {0} characters")]
    IdTooLong(usize),
    #[error("Runtime instance ID contains whitespace or control characters")]
    InvalidId,
    #[error("Runtime instance identities must remain distinct")]
    IdentityCollision,
    #[error("Runtime instance revision must be positive")]
    InvalidRevision,
    #[error("Runtime instance revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("Invalid Runtime instance transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: RuntimeInstanceLifecycle,
        to: RuntimeInstanceLifecycle,
    },
    #[error("Runtime instance timestamp order is invalid")]
    InvalidTimestamp,
    #[error("Runtime health diagnostic is empty")]
    EmptyDiagnostic,
    #[error("Runtime health diagnostic exceeds {0} characters")]
    DiagnosticTooLong(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeInstanceId(String);

impl RuntimeInstanceId {
    pub fn new(value: impl Into<String>) -> Result<Self, RuntimeInstanceDomainError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(RuntimeInstanceDomainError::EmptyId);
        }
        if value.chars().count() > MAX_ID_LENGTH {
            return Err(RuntimeInstanceDomainError::IdTooLong(MAX_ID_LENGTH));
        }
        if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
            return Err(RuntimeInstanceDomainError::InvalidId);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuntimeInstanceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeInstanceLifecycle {
    Registered,
    Activating,
    Ready,
    Degraded,
    Stopping,
    Stopped,
    Failed,
}

impl RuntimeInstanceLifecycle {
    pub fn can_transition_to(self, target: Self) -> bool {
        use RuntimeInstanceLifecycle::*;
        matches!(
            (self, target),
            (Registered, Activating | Stopped)
                | (Activating, Ready | Degraded | Failed)
                | (Ready, Degraded | Stopping | Failed)
                | (Degraded, Ready | Stopping | Failed)
                | (Stopping, Stopped | Failed)
                | (Stopped, Activating)
                | (Failed, Activating | Stopped)
        )
    }

    pub fn accepts_execution(self) -> bool {
        matches!(self, Self::Ready | Self::Degraded)
    }

    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Activating | Self::Ready | Self::Degraded | Self::Stopping
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHealthStatus {
    Unknown,
    Healthy,
    Degraded,
    Unavailable,
}

impl From<RuntimeAvailability> for RuntimeHealthStatus {
    fn from(value: RuntimeAvailability) -> Self {
        match value {
            RuntimeAvailability::Ready => Self::Healthy,
            RuntimeAvailability::Degraded | RuntimeAvailability::RequiresConfiguration => {
                Self::Degraded
            }
            RuntimeAvailability::Unavailable => Self::Unavailable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealthObservation {
    status: RuntimeHealthStatus,
    observed_at: i64,
    diagnostics: Vec<String>,
}

impl RuntimeHealthObservation {
    pub fn new(
        status: RuntimeHealthStatus,
        observed_at: i64,
        diagnostics: Vec<String>,
    ) -> Result<Self, RuntimeInstanceDomainError> {
        let diagnostics = diagnostics
            .into_iter()
            .map(|diagnostic| {
                let diagnostic = diagnostic.trim();
                if diagnostic.is_empty() {
                    return Err(RuntimeInstanceDomainError::EmptyDiagnostic);
                }
                if diagnostic.chars().count() > MAX_DIAGNOSTIC_LENGTH {
                    return Err(RuntimeInstanceDomainError::DiagnosticTooLong(
                        MAX_DIAGNOSTIC_LENGTH,
                    ));
                }
                Ok(diagnostic.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            status,
            observed_at,
            diagnostics,
        })
    }

    pub fn unknown(observed_at: i64) -> Self {
        Self {
            status: RuntimeHealthStatus::Unknown,
            observed_at,
            diagnostics: Vec::new(),
        }
    }

    pub fn status(&self) -> RuntimeHealthStatus {
        self.status
    }

    pub fn observed_at(&self) -> i64 {
        self.observed_at
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    fn validate(&self) -> Result<(), RuntimeInstanceDomainError> {
        for diagnostic in &self.diagnostics {
            let diagnostic = diagnostic.trim();
            if diagnostic.is_empty() {
                return Err(RuntimeInstanceDomainError::EmptyDiagnostic);
            }
            if diagnostic.chars().count() > MAX_DIAGNOSTIC_LENGTH {
                return Err(RuntimeInstanceDomainError::DiagnosticTooLong(
                    MAX_DIAGNOSTIC_LENGTH,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInstance {
    id: RuntimeInstanceId,
    runtime_id: RuntimeId,
    adapter_id: RuntimeAdapterId,
    lifecycle: RuntimeInstanceLifecycle,
    health: RuntimeHealthObservation,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl RuntimeInstance {
    pub fn new(
        id: RuntimeInstanceId,
        runtime_id: RuntimeId,
        adapter_id: RuntimeAdapterId,
        created_at: i64,
    ) -> Result<Self, RuntimeInstanceDomainError> {
        if id.as_str() == runtime_id.as_str()
            || id.as_str() == adapter_id.as_str()
            || runtime_id.as_str() == adapter_id.as_str()
        {
            return Err(RuntimeInstanceDomainError::IdentityCollision);
        }
        let instance = Self {
            id,
            runtime_id,
            adapter_id,
            lifecycle: RuntimeInstanceLifecycle::Registered,
            health: RuntimeHealthObservation::unknown(created_at),
            revision: 1,
            created_at,
            updated_at: created_at,
        };
        instance.validate()?;
        Ok(instance)
    }

    pub fn id(&self) -> &RuntimeInstanceId {
        &self.id
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

    pub fn health(&self) -> &RuntimeHealthObservation {
        &self.health
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }

    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }

    pub fn validate(&self) -> Result<(), RuntimeInstanceDomainError> {
        RuntimeInstanceId::new(self.id.as_str())?;
        RuntimeId::new(self.runtime_id.as_str())
            .map_err(|_| RuntimeInstanceDomainError::InvalidId)?;
        RuntimeAdapterId::new(self.adapter_id.as_str())
            .map_err(|_| RuntimeInstanceDomainError::InvalidId)?;
        self.health.validate()?;
        if self.id.as_str() == self.runtime_id.as_str()
            || self.id.as_str() == self.adapter_id.as_str()
            || self.runtime_id.as_str() == self.adapter_id.as_str()
        {
            return Err(RuntimeInstanceDomainError::IdentityCollision);
        }
        if self.revision == 0 {
            return Err(RuntimeInstanceDomainError::InvalidRevision);
        }
        if self.updated_at < self.created_at
            || self.health.observed_at() < self.created_at
            || self.health.observed_at() > self.updated_at
        {
            return Err(RuntimeInstanceDomainError::InvalidTimestamp);
        }
        Ok(())
    }

    pub fn transition_to(
        &self,
        target: RuntimeInstanceLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, RuntimeInstanceDomainError> {
        self.ensure_update(expected_revision, updated_at)?;
        if self.lifecycle == target {
            return Ok(self.clone());
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(RuntimeInstanceDomainError::InvalidTransition {
                from: self.lifecycle,
                to: target,
            });
        }
        let mut updated = self.clone();
        updated.lifecycle = target;
        updated.revision += 1;
        updated.updated_at = updated_at;
        updated.validate()?;
        Ok(updated)
    }

    pub fn record_health(
        &self,
        observation: RuntimeHealthObservation,
        expected_revision: u64,
    ) -> Result<Self, RuntimeInstanceDomainError> {
        self.ensure_update(expected_revision, observation.observed_at())?;
        let mut updated = self.clone();
        updated.health = observation;
        updated.revision += 1;
        updated.updated_at = updated.health.observed_at();
        updated.validate()?;
        Ok(updated)
    }

    fn ensure_update(
        &self,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<(), RuntimeInstanceDomainError> {
        if expected_revision == 0 {
            return Err(RuntimeInstanceDomainError::InvalidRevision);
        }
        if self.revision != expected_revision {
            return Err(RuntimeInstanceDomainError::RevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if updated_at < self.updated_at || updated_at < self.created_at {
            return Err(RuntimeInstanceDomainError::InvalidTimestamp);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> RuntimeInstance {
        RuntimeInstance::new(
            RuntimeInstanceId::new("instance:one").unwrap(),
            RuntimeId::new("runtime:one").unwrap(),
            RuntimeAdapterId::new("adapter:one").unwrap(),
            10,
        )
        .unwrap()
    }

    #[test]
    fn lifecycle_is_revisioned_and_does_not_absorb_execution_identity() {
        let activating = instance()
            .transition_to(RuntimeInstanceLifecycle::Activating, 1, 11)
            .unwrap();
        let ready = activating
            .transition_to(RuntimeInstanceLifecycle::Ready, 2, 12)
            .unwrap();

        assert!(ready.lifecycle().accepts_execution());
        assert_eq!(ready.revision(), 3);
        assert_ne!(ready.id().as_str(), ready.runtime_id().as_str());
        assert!(ready
            .transition_to(RuntimeInstanceLifecycle::Registered, 3, 13)
            .is_err());
    }

    #[test]
    fn health_is_explicit_and_timestamp_guarded() {
        let observation = RuntimeHealthObservation::new(
            RuntimeHealthStatus::Healthy,
            11,
            vec!["runtime responded".into()],
        )
        .unwrap();
        let updated = instance().record_health(observation, 1).unwrap();
        assert_eq!(updated.health().status(), RuntimeHealthStatus::Healthy);
        assert!(updated
            .record_health(
                RuntimeHealthObservation::new(RuntimeHealthStatus::Unavailable, 9, vec![]).unwrap(),
                2,
            )
            .is_err());
    }
}
