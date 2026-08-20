//! Operational lifecycle for an activated Agent OS Provider adapter.
//!
//! A Provider adapter instance references the Agent OS Provider descriptor and
//! its adapter. It does not copy the existing CC Switch Provider record, own
//! credentials, select a Model, or represent a Runtime or Agent.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::agent_provider_domain::{
    AgentProviderAdapterId, AgentProviderId, ProviderAvailability, ProviderProbe,
};

const MAX_ID_LENGTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentProviderInstanceDomainError {
    #[error("Provider adapter instance ID is empty")]
    EmptyId,
    #[error("Provider adapter instance ID exceeds {0} characters")]
    IdTooLong(usize),
    #[error("Provider adapter instance ID contains whitespace or control characters")]
    InvalidId,
    #[error("Provider adapter instance identities must remain distinct")]
    IdentityCollision,
    #[error("Provider or adapter identity is invalid")]
    InvalidBoundaryIdentity,
    #[error("Provider adapter instance revision must be positive")]
    InvalidRevision,
    #[error("Provider adapter instance revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("Invalid Provider adapter instance transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: AgentProviderInstanceLifecycle,
        to: AgentProviderInstanceLifecycle,
    },
    #[error("Provider adapter instance timestamp order is invalid")]
    InvalidTimestamp,
    #[error("Provider probe identity does not match the activated Provider")]
    ProbeIdentityMismatch,
    #[error("Provider probe is invalid: {0}")]
    InvalidProbe(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentProviderInstanceId(String);

impl AgentProviderInstanceId {
    pub fn new(value: impl Into<String>) -> Result<Self, AgentProviderInstanceDomainError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(AgentProviderInstanceDomainError::EmptyId);
        }
        if value.chars().count() > MAX_ID_LENGTH {
            return Err(AgentProviderInstanceDomainError::IdTooLong(MAX_ID_LENGTH));
        }
        if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
            return Err(AgentProviderInstanceDomainError::InvalidId);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentProviderInstanceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderInstanceLifecycle {
    Registered,
    Activating,
    Ready,
    Degraded,
    Stopping,
    Stopped,
    Failed,
}

impl AgentProviderInstanceLifecycle {
    pub fn can_transition_to(self, target: Self) -> bool {
        use AgentProviderInstanceLifecycle::*;
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

    pub fn is_available(self) -> bool {
        matches!(self, Self::Ready | Self::Degraded)
    }

    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Activating | Self::Ready | Self::Degraded | Self::Stopping
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "AgentProviderInstanceDto")]
pub struct AgentProviderInstance {
    id: AgentProviderInstanceId,
    provider_id: AgentProviderId,
    adapter_id: AgentProviderAdapterId,
    lifecycle: AgentProviderInstanceLifecycle,
    last_probe: Option<ProviderProbe>,
    last_probe_observed_at: Option<i64>,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentProviderInstanceDto {
    id: AgentProviderInstanceId,
    provider_id: AgentProviderId,
    adapter_id: AgentProviderAdapterId,
    lifecycle: AgentProviderInstanceLifecycle,
    last_probe: Option<ProviderProbe>,
    #[serde(default)]
    last_probe_observed_at: Option<i64>,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl TryFrom<AgentProviderInstanceDto> for AgentProviderInstance {
    type Error = AgentProviderInstanceDomainError;

    fn try_from(value: AgentProviderInstanceDto) -> Result<Self, Self::Error> {
        let instance = Self {
            id: value.id,
            provider_id: value.provider_id,
            adapter_id: value.adapter_id,
            lifecycle: value.lifecycle,
            last_probe: value.last_probe,
            last_probe_observed_at: value.last_probe_observed_at,
            revision: value.revision,
            created_at: value.created_at,
            updated_at: value.updated_at,
        };
        instance.validate()?;
        Ok(instance)
    }
}

impl AgentProviderInstance {
    pub fn new(
        id: AgentProviderInstanceId,
        provider_id: AgentProviderId,
        adapter_id: AgentProviderAdapterId,
        created_at: i64,
    ) -> Result<Self, AgentProviderInstanceDomainError> {
        if id.as_str() == provider_id.as_str()
            || id.as_str() == adapter_id.as_str()
            || provider_id.as_str() == adapter_id.as_str()
        {
            return Err(AgentProviderInstanceDomainError::IdentityCollision);
        }
        if created_at < 0 {
            return Err(AgentProviderInstanceDomainError::InvalidTimestamp);
        }
        let instance = Self {
            id,
            provider_id,
            adapter_id,
            lifecycle: AgentProviderInstanceLifecycle::Registered,
            last_probe: None,
            last_probe_observed_at: None,
            revision: 1,
            created_at,
            updated_at: created_at,
        };
        instance.validate()?;
        Ok(instance)
    }

    pub fn id(&self) -> &AgentProviderInstanceId {
        &self.id
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

    pub fn last_probe(&self) -> Option<&ProviderProbe> {
        self.last_probe.as_ref()
    }

    pub fn last_probe_observed_at(&self) -> Option<i64> {
        self.last_probe_observed_at
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

    pub fn validate(&self) -> Result<(), AgentProviderInstanceDomainError> {
        AgentProviderInstanceId::new(self.id.as_str())?;
        AgentProviderId::new(self.provider_id.as_str())
            .map_err(|_| AgentProviderInstanceDomainError::InvalidBoundaryIdentity)?;
        AgentProviderAdapterId::new(self.adapter_id.as_str())
            .map_err(|_| AgentProviderInstanceDomainError::InvalidBoundaryIdentity)?;
        if self.id.as_str() == self.provider_id.as_str()
            || self.id.as_str() == self.adapter_id.as_str()
            || self.provider_id.as_str() == self.adapter_id.as_str()
        {
            return Err(AgentProviderInstanceDomainError::IdentityCollision);
        }
        if self.revision == 0 {
            return Err(AgentProviderInstanceDomainError::InvalidRevision);
        }
        if self.created_at < 0 || self.updated_at < self.created_at {
            return Err(AgentProviderInstanceDomainError::InvalidTimestamp);
        }
        if let Some(probe) = self.last_probe.as_ref() {
            if &probe.provider_id != self.provider_id() {
                return Err(AgentProviderInstanceDomainError::ProbeIdentityMismatch);
            }
            probe.validate().map_err(|error| {
                AgentProviderInstanceDomainError::InvalidProbe(error.to_string())
            })?;
        }
        if self.last_probe.is_some() != self.last_probe_observed_at.is_some() {
            return Err(AgentProviderInstanceDomainError::InvalidTimestamp);
        }
        if self.last_probe_observed_at.is_some_and(|observed_at| {
            observed_at < self.created_at || observed_at > self.updated_at
        }) {
            return Err(AgentProviderInstanceDomainError::InvalidTimestamp);
        }
        if self.lifecycle.is_available() && self.last_probe.is_none() {
            return Err(AgentProviderInstanceDomainError::InvalidProbe(
                "available instance requires a Provider probe".into(),
            ));
        }
        Ok(())
    }

    pub fn transition_to(
        &self,
        target: AgentProviderInstanceLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, AgentProviderInstanceDomainError> {
        self.ensure_update(expected_revision, updated_at)?;
        if self.lifecycle == target {
            return Ok(self.clone());
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(AgentProviderInstanceDomainError::InvalidTransition {
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

    pub fn record_probe(
        &self,
        probe: ProviderProbe,
        expected_revision: u64,
        observed_at: i64,
    ) -> Result<Self, AgentProviderInstanceDomainError> {
        self.ensure_update(expected_revision, observed_at)?;
        if probe.provider_id != self.provider_id {
            return Err(AgentProviderInstanceDomainError::ProbeIdentityMismatch);
        }
        probe
            .validate()
            .map_err(|error| AgentProviderInstanceDomainError::InvalidProbe(error.to_string()))?;
        let mut updated = self.clone();
        updated.last_probe = Some(probe);
        updated.last_probe_observed_at = Some(observed_at);
        updated.revision += 1;
        updated.updated_at = observed_at;
        updated.validate()?;
        Ok(updated)
    }

    fn ensure_update(
        &self,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<(), AgentProviderInstanceDomainError> {
        if expected_revision == 0 {
            return Err(AgentProviderInstanceDomainError::InvalidRevision);
        }
        if self.revision != expected_revision {
            return Err(AgentProviderInstanceDomainError::RevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if updated_at < self.updated_at || updated_at < self.created_at {
            return Err(AgentProviderInstanceDomainError::InvalidTimestamp);
        }
        Ok(())
    }
}

pub(crate) fn lifecycle_for_probe(probe: &ProviderProbe) -> AgentProviderInstanceLifecycle {
    match probe.availability {
        ProviderAvailability::Registered => AgentProviderInstanceLifecycle::Ready,
        ProviderAvailability::Missing => AgentProviderInstanceLifecycle::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> AgentProviderInstance {
        AgentProviderInstance::new(
            AgentProviderInstanceId::new("provider-instance:one").unwrap(),
            AgentProviderId::new("provider:one").unwrap(),
            AgentProviderAdapterId::new("adapter:one").unwrap(),
            10,
        )
        .unwrap()
    }

    #[test]
    fn lifecycle_is_revisioned_and_requires_explicit_probe_before_ready() {
        let activating = instance()
            .transition_to(AgentProviderInstanceLifecycle::Activating, 1, 11)
            .unwrap();
        assert!(activating
            .transition_to(AgentProviderInstanceLifecycle::Ready, 2, 12)
            .is_err());
        let observed = activating
            .record_probe(
                ProviderProbe {
                    provider_id: AgentProviderId::new("provider:one").unwrap(),
                    availability: ProviderAvailability::Registered,
                    diagnostics: vec!["legacy source registered".into()],
                },
                2,
                12,
            )
            .unwrap();
        let ready = observed
            .transition_to(AgentProviderInstanceLifecycle::Ready, 3, 12)
            .unwrap();
        assert!(ready.lifecycle().is_available());
        assert_eq!(ready.revision(), 4);
        assert_eq!(ready.last_probe_observed_at(), Some(12));

        let degraded = ready
            .transition_to(AgentProviderInstanceLifecycle::Degraded, 4, 20)
            .unwrap();
        assert_eq!(degraded.updated_at(), 20);
        assert_eq!(degraded.last_probe_observed_at(), Some(12));

        let mut invalid = serde_json::to_value(degraded).unwrap();
        invalid
            .as_object_mut()
            .unwrap()
            .remove("lastProbeObservedAt");
        assert!(serde_json::from_value::<AgentProviderInstance>(invalid).is_err());
    }

    #[test]
    fn provider_instance_identity_is_not_legacy_provider_or_adapter_identity() {
        assert!(AgentProviderInstance::new(
            AgentProviderInstanceId::new("provider:same").unwrap(),
            AgentProviderId::new("provider:same").unwrap(),
            AgentProviderAdapterId::new("adapter:other").unwrap(),
            10,
        )
        .is_err());

        let instance = instance();
        let mut invalid_provider = serde_json::to_value(&instance).unwrap();
        invalid_provider["providerId"] = serde_json::json!("invalid provider");
        assert!(serde_json::from_value::<AgentProviderInstance>(invalid_provider).is_err());

        let mut invalid_adapter = serde_json::to_value(instance).unwrap();
        invalid_adapter["adapterId"] = serde_json::json!("invalid adapter");
        assert!(serde_json::from_value::<AgentProviderInstance>(invalid_adapter).is_err());
    }
}
