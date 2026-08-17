//! Runtime-neutral execution request, evidence, lifecycle, and result models.
//!
//! This module records resolved identities and opaque governance evidence. It
//! does not decide capabilities or permissions and does not invoke a Runtime.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    agent_provider_domain::AgentProviderId,
    capability_domain::CapabilitySnapshotId,
    model_domain::{ModelAvailabilityId, ModelId},
    permission_domain::{AuthorizationDecisionId, PermissionGrantId},
    role_domain::RoleAssignmentId,
    runtime_domain::{
        ExecutionContext, RuntimeDomainError, RuntimeExecutionId, RuntimeExecutionState,
    },
};

const MAX_TEXT_LENGTH: usize = 2048;
const MAX_REFERENCE_LENGTH: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecutionDomainError {
    #[error(transparent)]
    InvalidRuntime(#[from] RuntimeDomainError),
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("Provider and Model availability must either both be present or both be absent")]
    IncompleteModelBinding,
    #[error("Execution timestamp order is invalid")]
    InvalidTimestamp,
    #[error("Execution result state must be terminal")]
    NonTerminalResult,
    #[error("Successful execution cannot contain a failure")]
    SuccessWithFailure,
    #[error("Failed or cancelled execution must contain a failure")]
    MissingFailure,
}

fn text(
    field: &'static str,
    value: impl Into<String>,
    max: usize,
) -> Result<String, ExecutionDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(ExecutionDomainError::Empty { field });
    }
    if value.chars().count() > max {
        return Err(ExecutionDomainError::TooLong { field, max });
    }
    Ok(value.to_string())
}

/// Exact Model resolution used by one immutable execution request. Provider
/// identity remains optional for Runtime-local Models, while availability is
/// present exactly when a Provider is present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionModelBinding {
    model_id: ModelId,
    provider_id: Option<AgentProviderId>,
    model_availability_id: Option<ModelAvailabilityId>,
}

impl ExecutionModelBinding {
    pub fn runtime_local(model_id: ModelId) -> Self {
        Self {
            model_id,
            provider_id: None,
            model_availability_id: None,
        }
    }

    pub fn provider_model(
        model_id: ModelId,
        provider_id: AgentProviderId,
        model_availability_id: ModelAvailabilityId,
    ) -> Self {
        Self {
            model_id,
            provider_id: Some(provider_id),
            model_availability_id: Some(model_availability_id),
        }
    }

    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }
    pub fn provider_id(&self) -> Option<&AgentProviderId> {
        self.provider_id.as_ref()
    }
    pub fn model_availability_id(&self) -> Option<&ModelAvailabilityId> {
        self.model_availability_id.as_ref()
    }

    pub fn validate(&self) -> Result<(), ExecutionDomainError> {
        if self.provider_id.is_some() != self.model_availability_id.is_some() {
            return Err(ExecutionDomainError::IncompleteModelBinding);
        }
        Ok(())
    }
}

/// References to evidence produced outside the execution pipeline. Milestone 4
/// consumes these references but does not interpret policy or grant authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionGovernanceEvidence {
    capability_snapshot_id: CapabilitySnapshotId,
    permission_grant_id: PermissionGrantId,
    role_assignment_id: RoleAssignmentId,
    authorization_decision_id: AuthorizationDecisionId,
}

impl ExecutionGovernanceEvidence {
    pub fn new(
        capability_snapshot_id: CapabilitySnapshotId,
        permission_grant_id: PermissionGrantId,
        role_assignment_id: RoleAssignmentId,
        authorization_decision_id: AuthorizationDecisionId,
    ) -> Self {
        Self {
            capability_snapshot_id,
            permission_grant_id,
            role_assignment_id,
            authorization_decision_id,
        }
    }

    pub fn capability_snapshot_id(&self) -> &CapabilitySnapshotId {
        &self.capability_snapshot_id
    }
    pub fn permission_grant_id(&self) -> &PermissionGrantId {
        &self.permission_grant_id
    }
    pub fn role_assignment_id(&self) -> &RoleAssignmentId {
        &self.role_assignment_id
    }
    pub fn authorization_decision_id(&self) -> &AuthorizationDecisionId {
        &self.authorization_decision_id
    }
}

/// Immutable input accepted for exactly one execution attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRequest {
    context: ExecutionContext,
    objective: String,
    model_binding: ExecutionModelBinding,
    governance: ExecutionGovernanceEvidence,
    correlation_ref: Option<String>,
    accepted_at: i64,
}

impl ExecutionRequest {
    pub fn new(
        context: ExecutionContext,
        objective: impl Into<String>,
        model_binding: ExecutionModelBinding,
        governance: ExecutionGovernanceEvidence,
        correlation_ref: Option<String>,
        accepted_at: i64,
    ) -> Result<Self, ExecutionDomainError> {
        let request = Self {
            context,
            objective: text("Execution objective", objective, MAX_TEXT_LENGTH)?,
            model_binding,
            governance,
            correlation_ref: correlation_ref
                .map(|value| text("Correlation reference", value, MAX_REFERENCE_LENGTH))
                .transpose()?,
            accepted_at,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn execution_id(&self) -> &RuntimeExecutionId {
        self.context.execution_id()
    }
    pub fn context(&self) -> &ExecutionContext {
        &self.context
    }
    pub fn objective(&self) -> &str {
        &self.objective
    }
    pub fn model_binding(&self) -> &ExecutionModelBinding {
        &self.model_binding
    }
    pub fn governance(&self) -> &ExecutionGovernanceEvidence {
        &self.governance
    }
    pub fn correlation_ref(&self) -> Option<&str> {
        self.correlation_ref.as_deref()
    }
    pub fn accepted_at(&self) -> i64 {
        self.accepted_at
    }

    pub fn validate(&self) -> Result<(), ExecutionDomainError> {
        self.context.validate()?;
        self.model_binding.validate()?;
        text(
            "Execution objective",
            self.objective.clone(),
            MAX_TEXT_LENGTH,
        )?;
        if self.accepted_at < self.context.created_at() {
            return Err(ExecutionDomainError::InvalidTimestamp);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTransition {
    sequence: u64,
    from: RuntimeExecutionState,
    to: RuntimeExecutionState,
    occurred_at: i64,
    reason: String,
}

impl ExecutionTransition {
    pub(crate) fn new(
        sequence: u64,
        from: RuntimeExecutionState,
        to: RuntimeExecutionState,
        occurred_at: i64,
        reason: impl Into<String>,
    ) -> Result<Self, ExecutionDomainError> {
        from.transition_to(to)?;
        Ok(Self {
            sequence,
            from,
            to,
            occurred_at,
            reason: text("Transition reason", reason, MAX_REFERENCE_LENGTH)?,
        })
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn from(&self) -> RuntimeExecutionState {
        self.from
    }
    pub fn to(&self) -> RuntimeExecutionState {
        self.to
    }
    pub fn occurred_at(&self) -> i64 {
        self.occurred_at
    }
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFailureKind {
    AdmissionRejected,
    RuntimeUnavailable,
    ContextRejected,
    InvocationFailed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionFailure {
    kind: ExecutionFailureKind,
    code: String,
    message: String,
    retry_safe: bool,
}

impl ExecutionFailure {
    pub fn new(
        kind: ExecutionFailureKind,
        code: impl Into<String>,
        message: impl Into<String>,
        retry_safe: bool,
    ) -> Result<Self, ExecutionDomainError> {
        Ok(Self {
            kind,
            code: text("Failure code", code, MAX_REFERENCE_LENGTH)?,
            message: text("Failure message", message, MAX_TEXT_LENGTH)?,
            retry_safe,
        })
    }

    pub fn kind(&self) -> &ExecutionFailureKind {
        &self.kind
    }
    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn retry_safe(&self) -> bool {
        self.retry_safe
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    execution_id: RuntimeExecutionId,
    state: RuntimeExecutionState,
    summary: String,
    artifact_references: Vec<String>,
    failure: Option<ExecutionFailure>,
    completed_at: i64,
}

impl ExecutionResult {
    pub fn new(
        execution_id: RuntimeExecutionId,
        state: RuntimeExecutionState,
        summary: impl Into<String>,
        artifact_references: Vec<String>,
        failure: Option<ExecutionFailure>,
        completed_at: i64,
    ) -> Result<Self, ExecutionDomainError> {
        if !state.is_terminal() {
            return Err(ExecutionDomainError::NonTerminalResult);
        }
        if state == RuntimeExecutionState::Succeeded && failure.is_some() {
            return Err(ExecutionDomainError::SuccessWithFailure);
        }
        if state != RuntimeExecutionState::Succeeded && failure.is_none() {
            return Err(ExecutionDomainError::MissingFailure);
        }
        Ok(Self {
            execution_id,
            state,
            summary: text("Execution result summary", summary, MAX_TEXT_LENGTH)?,
            artifact_references: artifact_references
                .into_iter()
                .map(|value| text("Artifact reference", value, MAX_REFERENCE_LENGTH))
                .collect::<Result<Vec<_>, _>>()?,
            failure,
            completed_at,
        })
    }

    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }
    pub fn state(&self) -> RuntimeExecutionState {
        self.state
    }
    pub fn summary(&self) -> &str {
        &self.summary
    }
    pub fn artifact_references(&self) -> &[String] {
        &self.artifact_references
    }
    pub fn failure(&self) -> Option<&ExecutionFailure> {
        self.failure.as_ref()
    }
    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_domain::{
        AgentRuntimeBinding, RuntimeBindingId, RuntimeBindingLifecycle, RuntimeId,
    };

    fn context() -> ExecutionContext {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding:one").unwrap(),
            "agent:one",
            RuntimeId::new("runtime:one").unwrap(),
            10,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 11)
        .unwrap();
        ExecutionContext::new(
            RuntimeExecutionId::new("execution:one").unwrap(),
            binding,
            vec!["context:one".into()],
            12,
        )
        .unwrap()
    }

    #[test]
    fn request_preserves_distinct_agent_runtime_provider_and_model_identities() {
        let request = ExecutionRequest::new(
            context(),
            "Perform the bounded task",
            ExecutionModelBinding::provider_model(
                ModelId::new("model:one").unwrap(),
                AgentProviderId::new("provider:one").unwrap(),
                ModelAvailabilityId::new("availability:one").unwrap(),
            ),
            ExecutionGovernanceEvidence::new(
                CapabilitySnapshotId::new("capability:snapshot").unwrap(),
                PermissionGrantId::new("permission:grant").unwrap(),
                RoleAssignmentId::new("assignment:one").unwrap(),
                AuthorizationDecisionId::new("decision:one").unwrap(),
            ),
            Some("task:one".into()),
            13,
        )
        .unwrap();

        assert_eq!(request.context().binding().agent_id(), "agent:one");
        assert_eq!(
            request.context().binding().runtime_id().as_str(),
            "runtime:one"
        );
        assert_eq!(request.model_binding().model_id().as_str(), "model:one");
        assert_eq!(
            request.model_binding().provider_id().unwrap().as_str(),
            "provider:one"
        );
        assert_eq!(
            request.governance().permission_grant_id().as_str(),
            "permission:grant"
        );
    }

    #[test]
    fn result_requires_terminal_state_and_failure_consistency() {
        assert!(matches!(
            ExecutionResult::new(
                RuntimeExecutionId::new("execution:one").unwrap(),
                RuntimeExecutionState::Running,
                "not done",
                Vec::new(),
                None,
                20,
            ),
            Err(ExecutionDomainError::NonTerminalResult)
        ));
        assert!(matches!(
            ExecutionResult::new(
                RuntimeExecutionId::new("execution:one").unwrap(),
                RuntimeExecutionState::Failed,
                "failed",
                Vec::new(),
                None,
                20,
            ),
            Err(ExecutionDomainError::MissingFailure)
        ));
    }
}
