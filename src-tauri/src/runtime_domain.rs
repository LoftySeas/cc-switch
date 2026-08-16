//! Agent OS Runtime boundary domain types.
//!
//! Runtime identity, adapter identity, and execution identity are intentionally
//! separate from Agent identity. This module contains contracts only; it does
//! not launch or communicate with a concrete runtime.

use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_ID_LENGTH: usize = 128;
const MAX_LABEL_LENGTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Runtime contract version must be positive")]
    InvalidContractVersion,
    #[error("Runtime capability version must be positive")]
    InvalidCapabilityVersion,
    #[error("Duplicate runtime capability: {0}")]
    DuplicateCapability(String),
    #[error("Invalid Runtime execution state transition: {from:?} -> {to:?}")]
    InvalidExecutionTransition {
        from: RuntimeExecutionState,
        to: RuntimeExecutionState,
    },
    #[error("Runtime binding revision must be positive")]
    InvalidBindingRevision,
    #[error("Runtime binding revision conflict: expected {expected}, current {current}")]
    BindingRevisionConflict { expected: i64, current: i64 },
    #[error("Invalid Runtime binding state transition: {from:?} -> {to:?}")]
    InvalidBindingTransition {
        from: RuntimeBindingLifecycle,
        to: RuntimeBindingLifecycle,
    },
    #[error("Runtime binding identities must be distinct")]
    BindingIdentityCollision,
    #[error("Runtime binding update timestamp precedes creation timestamp")]
    InvalidBindingTimestamp,
    #[error("Runtime binding must be active before creating an execution context")]
    InactiveBinding,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, RuntimeDomainError> {
                Ok(Self(validate_identifier($field, value.into())?))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            fn validate(&self) -> Result<(), RuntimeDomainError> {
                validate_identifier($field, self.0.clone()).map(|_| ())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

typed_id!(RuntimeId, "Runtime ID");
typed_id!(RuntimeAdapterId, "Runtime adapter ID");
typed_id!(RuntimeExecutionId, "Runtime execution ID");
typed_id!(RuntimeBindingId, "Runtime binding ID");

fn validate_identifier(field: &'static str, value: String) -> Result<String, RuntimeDomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(RuntimeDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(RuntimeDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(RuntimeDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn validate_text(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, RuntimeDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(RuntimeDomainError::Empty { field });
    }
    if value.chars().count() > MAX_LABEL_LENGTH {
        return Err(RuntimeDomainError::TooLong {
            field,
            max: MAX_LABEL_LENGTH,
        });
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapability {
    name: String,
    version: u16,
}

impl RuntimeCapability {
    pub fn new(name: impl Into<String>, version: u16) -> Result<Self, RuntimeDomainError> {
        if version == 0 {
            return Err(RuntimeDomainError::InvalidCapabilityVersion);
        }
        Ok(Self {
            name: validate_identifier("Runtime capability name", name.into())?,
            version,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    fn validate(&self) -> Result<(), RuntimeDomainError> {
        validate_identifier("Runtime capability name", self.name.clone())?;
        if self.version == 0 {
            return Err(RuntimeDomainError::InvalidCapabilityVersion);
        }
        Ok(())
    }
}

/// Static adapter metadata. It describes a runtime integration without probing
/// or mutating a runtime installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDescriptor {
    runtime_id: RuntimeId,
    adapter_id: RuntimeAdapterId,
    display_name: String,
    contract_version: u16,
    capabilities: Vec<RuntimeCapability>,
}

impl RuntimeDescriptor {
    pub fn new(
        runtime_id: RuntimeId,
        adapter_id: RuntimeAdapterId,
        display_name: impl Into<String>,
        contract_version: u16,
        capabilities: Vec<RuntimeCapability>,
    ) -> Result<Self, RuntimeDomainError> {
        let descriptor = Self {
            runtime_id,
            adapter_id,
            display_name: validate_text("Runtime display name", display_name)?,
            contract_version,
            capabilities,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }

    pub fn adapter_id(&self) -> &RuntimeAdapterId {
        &self.adapter_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn contract_version(&self) -> u16 {
        self.contract_version
    }

    pub fn capabilities(&self) -> &[RuntimeCapability] {
        &self.capabilities
    }

    pub fn validate(&self) -> Result<(), RuntimeDomainError> {
        self.runtime_id.validate()?;
        self.adapter_id.validate()?;
        validate_text("Runtime display name", self.display_name.clone())?;
        if self.contract_version == 0 {
            return Err(RuntimeDomainError::InvalidContractVersion);
        }

        let mut names = std::collections::HashSet::new();
        for capability in &self.capabilities {
            capability.validate()?;
            if !names.insert(capability.name()) {
                return Err(RuntimeDomainError::DuplicateCapability(
                    capability.name().to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Read-only availability observed by an adapter probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAvailability {
    Unavailable,
    RequiresConfiguration,
    Ready,
    Degraded,
}

impl RuntimeAvailability {
    pub fn can_prepare(self) -> bool {
        matches!(self, Self::Ready | Self::Degraded)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    Unknown,
    RequiresConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilityStatus {
    pub capability: RuntimeCapability,
    pub support: CapabilitySupport,
}

/// Non-secret, read-only result of probing one runtime boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProbe {
    pub runtime_id: RuntimeId,
    pub availability: RuntimeAvailability,
    pub runtime_version: Option<String>,
    pub capabilities: Vec<RuntimeCapabilityStatus>,
    pub diagnostics: Vec<String>,
}

impl RuntimeProbe {
    pub fn validate(&self) -> Result<(), RuntimeDomainError> {
        self.runtime_id.validate()?;
        for capability in &self.capabilities {
            capability.capability.validate()?;
        }
        Ok(())
    }
}

/// Normalized lifecycle for one execution attempt. No operation in this module
/// advances a real execution; it only defines legal state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionState {
    Accepted,
    Preparing,
    Running,
    WaitingForInput,
    Cancelling,
    Lost,
    Succeeded,
    Failed,
    Cancelled,
}

impl RuntimeExecutionState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition_to(self, target: Self) -> bool {
        use RuntimeExecutionState::*;
        matches!(
            (self, target),
            (Accepted, Preparing)
                | (Preparing, Running | Failed)
                | (
                    Running,
                    WaitingForInput | Succeeded | Failed | Cancelling | Lost
                )
                | (WaitingForInput, Running | Cancelling | Lost)
                | (Cancelling, Cancelled | Failed)
                | (Lost, Running | Failed)
        )
    }

    pub fn transition_to(self, target: Self) -> Result<Self, RuntimeDomainError> {
        if self.can_transition_to(target) {
            Ok(target)
        } else {
            Err(RuntimeDomainError::InvalidExecutionTransition {
                from: self,
                to: target,
            })
        }
    }
}

/// Lifecycle of the independent relationship between an Agent and a Runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBindingLifecycle {
    Draft,
    Active,
    Suspended,
    Retired,
}

impl RuntimeBindingLifecycle {
    pub fn is_terminal(self) -> bool {
        self == Self::Retired
    }

    pub fn is_eligible(self) -> bool {
        self == Self::Active
    }

    pub fn can_transition_to(self, target: Self) -> bool {
        use RuntimeBindingLifecycle::*;
        matches!(
            (self, target),
            (Draft, Active | Retired)
                | (Active, Suspended | Retired)
                | (Suspended, Active | Retired)
        )
    }
}

/// Independent relationship aggregate between one Agent identity and one
/// Runtime. It lives outside the Agent aggregate so neither identity owns the
/// other, and retirement preserves historical identity without deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeBinding {
    id: RuntimeBindingId,
    agent_id: String,
    runtime_id: RuntimeId,
    lifecycle_state: RuntimeBindingLifecycle,
    revision: i64,
    created_at: i64,
    updated_at: i64,
}

impl AgentRuntimeBinding {
    pub fn new(
        id: RuntimeBindingId,
        agent_id: impl Into<String>,
        runtime_id: RuntimeId,
        created_at: i64,
    ) -> Result<Self, RuntimeDomainError> {
        let binding = Self {
            id,
            agent_id: validate_identifier("Agent ID", agent_id.into())?,
            runtime_id,
            lifecycle_state: RuntimeBindingLifecycle::Draft,
            revision: 1,
            created_at,
            updated_at: created_at,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn id(&self) -> &RuntimeBindingId {
        &self.id
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }

    pub fn lifecycle_state(&self) -> RuntimeBindingLifecycle {
        self.lifecycle_state
    }

    pub fn revision(&self) -> i64 {
        self.revision
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }

    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }

    pub fn transition_to(
        &self,
        target: RuntimeBindingLifecycle,
        expected_revision: i64,
        updated_at: i64,
    ) -> Result<Self, RuntimeDomainError> {
        if expected_revision <= 0 {
            return Err(RuntimeDomainError::InvalidBindingRevision);
        }
        if self.revision != expected_revision {
            return Err(RuntimeDomainError::BindingRevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if self.lifecycle_state == target {
            return Ok(self.clone());
        }
        if !self.lifecycle_state.can_transition_to(target) {
            return Err(RuntimeDomainError::InvalidBindingTransition {
                from: self.lifecycle_state,
                to: target,
            });
        }
        if updated_at < self.created_at {
            return Err(RuntimeDomainError::InvalidBindingTimestamp);
        }

        let mut updated = self.clone();
        updated.lifecycle_state = target;
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }

    pub fn validate(&self) -> Result<(), RuntimeDomainError> {
        self.id.validate()?;
        validate_identifier("Agent ID", self.agent_id.clone())?;
        self.runtime_id.validate()?;
        if self.id.as_str() == self.agent_id
            || self.id.as_str() == self.runtime_id.as_str()
            || self.agent_id == self.runtime_id.as_str()
        {
            return Err(RuntimeDomainError::BindingIdentityCollision);
        }
        if self.revision <= 0 {
            return Err(RuntimeDomainError::InvalidBindingRevision);
        }
        if self.updated_at < self.created_at {
            return Err(RuntimeDomainError::InvalidBindingTimestamp);
        }
        Ok(())
    }
}

/// Bounded data made available while preparing one execution attempt. It holds
/// references only and has no method that launches work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionContext {
    execution_id: RuntimeExecutionId,
    binding: AgentRuntimeBinding,
    context_references: Vec<String>,
    created_at: i64,
}

impl ExecutionContext {
    pub fn new(
        execution_id: RuntimeExecutionId,
        binding: AgentRuntimeBinding,
        context_references: Vec<String>,
        created_at: i64,
    ) -> Result<Self, RuntimeDomainError> {
        let context = Self {
            execution_id,
            binding,
            context_references: context_references
                .into_iter()
                .map(|reference| validate_text("Context reference", reference))
                .collect::<Result<Vec<_>, _>>()?,
            created_at,
        };
        context.validate()?;
        Ok(context)
    }

    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }

    pub fn binding(&self) -> &AgentRuntimeBinding {
        &self.binding
    }

    pub fn context_references(&self) -> &[String] {
        &self.context_references
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }

    pub fn validate(&self) -> Result<(), RuntimeDomainError> {
        self.execution_id.validate()?;
        self.binding.validate()?;
        if !self.binding.lifecycle_state().is_eligible() {
            return Err(RuntimeDomainError::InactiveBinding);
        }
        for reference in &self.context_references {
            validate_text("Context reference", reference.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(name: &str) -> RuntimeCapability {
        RuntimeCapability::new(name, 1).expect("valid capability")
    }

    #[test]
    fn descriptor_validates_identity_version_and_capability_uniqueness() {
        let runtime_id = RuntimeId::new("runtime:test").expect("valid runtime id");
        let adapter_id = RuntimeAdapterId::new("adapter:test").expect("valid adapter id");
        let duplicate = RuntimeDescriptor::new(
            runtime_id.clone(),
            adapter_id.clone(),
            "Test Runtime",
            1,
            vec![capability("execution:test"), capability("execution:test")],
        );
        assert!(matches!(
            duplicate,
            Err(RuntimeDomainError::DuplicateCapability(_))
        ));
        assert!(matches!(
            RuntimeDescriptor::new(runtime_id, adapter_id, "Test Runtime", 0, vec![]),
            Err(RuntimeDomainError::InvalidContractVersion)
        ));
    }

    #[test]
    fn execution_lifecycle_accepts_only_normalized_transitions() {
        assert_eq!(
            RuntimeExecutionState::Accepted
                .transition_to(RuntimeExecutionState::Preparing)
                .expect("accepted can prepare"),
            RuntimeExecutionState::Preparing
        );
        assert!(RuntimeExecutionState::Running
            .can_transition_to(RuntimeExecutionState::WaitingForInput));
        assert!(RuntimeExecutionState::Lost.can_transition_to(RuntimeExecutionState::Running));
        assert!(RuntimeExecutionState::Succeeded.is_terminal());
        assert!(RuntimeExecutionState::Succeeded
            .transition_to(RuntimeExecutionState::Running)
            .is_err());
        assert!(RuntimeExecutionState::Accepted
            .transition_to(RuntimeExecutionState::Succeeded)
            .is_err());
    }

    #[test]
    fn execution_context_keeps_agent_and_runtime_as_distinct_references() {
        let runtime_id = RuntimeId::new("runtime:test").expect("valid runtime id");
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding-1").expect("valid binding id"),
            "agent-1",
            runtime_id.clone(),
            1_000,
        )
        .expect("valid binding")
        .transition_to(RuntimeBindingLifecycle::Active, 1, 1_001)
        .expect("binding activates");
        let context = ExecutionContext::new(
            RuntimeExecutionId::new("execution-1").expect("valid execution id"),
            binding,
            vec!["docs/task.md".to_string()],
            1_000,
        )
        .expect("valid context");

        assert_eq!(context.binding().agent_id(), "agent-1");
        assert_eq!(context.binding().runtime_id(), &runtime_id);
        assert_eq!(context.context_references(), &["docs/task.md"]);
    }

    #[test]
    fn identifiers_and_context_references_fail_closed() {
        assert!(RuntimeId::new("runtime with spaces").is_err());
        assert!(AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding-1").expect("valid binding id"),
            " ",
            RuntimeId::new("runtime:test").expect("valid runtime id"),
            1_000,
        )
        .is_err());
        let active_binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding-1").expect("valid binding id"),
            "agent-1",
            RuntimeId::new("runtime:test").expect("valid runtime id"),
            1_000,
        )
        .expect("valid binding")
        .transition_to(RuntimeBindingLifecycle::Active, 1, 1_001)
        .expect("binding activates");
        assert!(ExecutionContext::new(
            RuntimeExecutionId::new("execution-1").expect("valid execution id"),
            active_binding,
            vec![" ".to_string()],
            1_000,
        )
        .is_err());
    }

    #[test]
    fn runtime_binding_has_independent_identity_and_revisioned_lifecycle() {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding-1").expect("valid binding id"),
            "agent-1",
            RuntimeId::new("runtime:test").expect("valid runtime id"),
            1_000,
        )
        .expect("valid binding");
        assert_eq!(binding.lifecycle_state(), RuntimeBindingLifecycle::Draft);
        assert_eq!(binding.revision(), 1);

        let active = binding
            .transition_to(RuntimeBindingLifecycle::Active, 1, 1_100)
            .expect("draft activates");
        let suspended = active
            .transition_to(RuntimeBindingLifecycle::Suspended, 2, 1_200)
            .expect("active suspends");
        let retired = suspended
            .transition_to(RuntimeBindingLifecycle::Retired, 3, 1_300)
            .expect("suspended retires");

        assert!(retired.lifecycle_state().is_terminal());
        assert_eq!(retired.revision(), 4);
        assert!(retired
            .transition_to(RuntimeBindingLifecycle::Active, 4, 1_400)
            .is_err());
        assert!(active
            .transition_to(RuntimeBindingLifecycle::Retired, 1, 1_300)
            .is_err());
    }
}
