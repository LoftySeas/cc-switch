//! Explicit Workflow definitions, runs, steps, and execution-bound task records.
//!
//! Workflow owns orchestration state, not Agent identity or Runtime invocation.
//! A Workflow task references already-resolved governance evidence and one
//! Execution attempt; it never chooses Provider, Model, or Runtime.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    capability_domain::CapabilityRequirement,
    execution_domain::ExecutionGovernanceEvidence,
    role_domain::{RoleAssignmentId, RoleId},
    runtime_domain::RuntimeExecutionId,
    team_domain::{TeamId, TeamMembershipId},
};

const MAX_ID_LENGTH: usize = 160;
const MAX_TEXT_LENGTH: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkflowDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Workflow definition version must be positive")]
    InvalidVersion,
    #[error("Workflow definition must contain at least one step")]
    EmptyWorkflow,
    #[error("Workflow step is duplicated: {0}")]
    DuplicateStep(WorkflowStepId),
    #[error("Workflow step {step_id} references unknown dependency {dependency_id}")]
    UnknownDependency {
        step_id: WorkflowStepId,
        dependency_id: WorkflowStepId,
    },
    #[error("Workflow step cannot depend on itself: {0}")]
    SelfDependency(WorkflowStepId),
    #[error("Workflow definition contains a dependency cycle")]
    DependencyCycle,
    #[error("Workflow Run revision conflict: expected {expected}, current {current}")]
    RunRevisionConflict { expected: u64, current: u64 },
    #[error("Workflow Task revision conflict: expected {expected}, current {current}")]
    TaskRevisionConflict { expected: u64, current: u64 },
    #[error("Invalid Workflow Run lifecycle transition: {from:?} -> {to:?}")]
    InvalidRunTransition {
        from: WorkflowRunLifecycle,
        to: WorkflowRunLifecycle,
    },
    #[error("Invalid Workflow Step state transition: {from:?} -> {to:?}")]
    InvalidStepTransition {
        from: WorkflowStepState,
        to: WorkflowStepState,
    },
    #[error("Invalid Workflow Task lifecycle transition: {from:?} -> {to:?}")]
    InvalidTaskTransition {
        from: WorkflowTaskLifecycle,
        to: WorkflowTaskLifecycle,
    },
    #[error("Workflow step is not part of the definition: {0}")]
    StepNotFound(WorkflowStepId),
    #[error("Workflow step is not ready: {0}")]
    StepNotReady(WorkflowStepId),
    #[error("Workflow timestamp order is invalid")]
    InvalidTimestamp,
    #[error("Workflow task attempt must be positive")]
    InvalidAttempt,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, WorkflowDomainError> {
                Ok(Self(identifier($field, value)?))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

typed_id!(WorkflowId, "Workflow ID");
typed_id!(WorkflowStepId, "Workflow Step ID");
typed_id!(WorkflowRunId, "Workflow Run ID");
typed_id!(WorkflowTaskId, "Workflow Task ID");

fn identifier(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, WorkflowDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(WorkflowDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(WorkflowDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(WorkflowDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn text(field: &'static str, value: impl Into<String>) -> Result<String, WorkflowDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(WorkflowDomainError::Empty { field });
    }
    if value.chars().count() > MAX_TEXT_LENGTH {
        return Err(WorkflowDomainError::TooLong {
            field,
            max: MAX_TEXT_LENGTH,
        });
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStepDefinition {
    id: WorkflowStepId,
    name: String,
    objective: String,
    role_id: RoleId,
    role_version: u16,
    dependencies: Vec<WorkflowStepId>,
    capability_requirements: Vec<CapabilityRequirement>,
    permission_request_refs: Vec<String>,
    acceptance_criteria: Vec<String>,
}

impl WorkflowStepDefinition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: WorkflowStepId,
        name: impl Into<String>,
        objective: impl Into<String>,
        role_id: RoleId,
        role_version: u16,
        dependencies: Vec<WorkflowStepId>,
        capability_requirements: Vec<CapabilityRequirement>,
        permission_request_refs: Vec<String>,
        acceptance_criteria: Vec<String>,
    ) -> Result<Self, WorkflowDomainError> {
        if role_version == 0 {
            return Err(WorkflowDomainError::InvalidVersion);
        }
        let mut unique_dependencies = BTreeSet::new();
        for dependency in &dependencies {
            if dependency == &id {
                return Err(WorkflowDomainError::SelfDependency(id));
            }
            unique_dependencies.insert(dependency.clone());
        }
        Ok(Self {
            id,
            name: text("Workflow Step name", name)?,
            objective: text("Workflow Step objective", objective)?,
            role_id,
            role_version,
            dependencies: unique_dependencies.into_iter().collect(),
            capability_requirements,
            permission_request_refs: permission_request_refs
                .into_iter()
                .map(|value| identifier("Permission Request reference", value))
                .collect::<Result<Vec<_>, _>>()?,
            acceptance_criteria: acceptance_criteria
                .into_iter()
                .map(|value| text("Acceptance criterion", value))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub fn id(&self) -> &WorkflowStepId {
        &self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn objective(&self) -> &str {
        &self.objective
    }
    pub fn role_id(&self) -> &RoleId {
        &self.role_id
    }
    pub fn role_version(&self) -> u16 {
        self.role_version
    }
    pub fn dependencies(&self) -> &[WorkflowStepId] {
        &self.dependencies
    }
    pub fn capability_requirements(&self) -> &[CapabilityRequirement] {
        &self.capability_requirements
    }
    pub fn permission_request_refs(&self) -> &[String] {
        &self.permission_request_refs
    }
    pub fn acceptance_criteria(&self) -> &[String] {
        &self.acceptance_criteria
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    id: WorkflowId,
    version: u16,
    team_id: TeamId,
    name: String,
    purpose: String,
    steps: Vec<WorkflowStepDefinition>,
    created_at: i64,
}

impl WorkflowDefinition {
    pub fn new(
        id: WorkflowId,
        version: u16,
        team_id: TeamId,
        name: impl Into<String>,
        purpose: impl Into<String>,
        steps: Vec<WorkflowStepDefinition>,
        created_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        if version == 0 {
            return Err(WorkflowDomainError::InvalidVersion);
        }
        if steps.is_empty() {
            return Err(WorkflowDomainError::EmptyWorkflow);
        }
        if created_at < 0 {
            return Err(WorkflowDomainError::InvalidTimestamp);
        }
        validate_graph(&steps)?;
        Ok(Self {
            id,
            version,
            team_id,
            name: text("Workflow name", name)?,
            purpose: text("Workflow purpose", purpose)?,
            steps,
            created_at,
        })
    }

    pub fn id(&self) -> &WorkflowId {
        &self.id
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn team_id(&self) -> &TeamId {
        &self.team_id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn purpose(&self) -> &str {
        &self.purpose
    }
    pub fn steps(&self) -> &[WorkflowStepDefinition] {
        &self.steps
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn step(&self, step_id: &WorkflowStepId) -> Option<&WorkflowStepDefinition> {
        self.steps.iter().find(|step| step.id() == step_id)
    }
}

fn validate_graph(steps: &[WorkflowStepDefinition]) -> Result<(), WorkflowDomainError> {
    let mut by_id = BTreeMap::new();
    for step in steps {
        if by_id.insert(step.id().clone(), step).is_some() {
            return Err(WorkflowDomainError::DuplicateStep(step.id().clone()));
        }
    }
    let mut remaining_dependencies = BTreeMap::new();
    let mut dependents: BTreeMap<WorkflowStepId, Vec<WorkflowStepId>> = BTreeMap::new();
    for step in steps {
        remaining_dependencies.insert(step.id().clone(), step.dependencies().len());
        for dependency in step.dependencies() {
            if !by_id.contains_key(dependency) {
                return Err(WorkflowDomainError::UnknownDependency {
                    step_id: step.id().clone(),
                    dependency_id: dependency.clone(),
                });
            }
            dependents
                .entry(dependency.clone())
                .or_default()
                .push(step.id().clone());
        }
    }
    let mut ready = remaining_dependencies
        .iter()
        .filter_map(|(step_id, count)| (*count == 0).then_some(step_id.clone()))
        .collect::<Vec<_>>();
    let mut visited = 0usize;
    while let Some(step_id) = ready.pop() {
        visited += 1;
        if let Some(children) = dependents.get(&step_id) {
            for child in children {
                let count = remaining_dependencies
                    .get_mut(child)
                    .expect("validated Workflow child exists");
                *count -= 1;
                if *count == 0 {
                    ready.push(child.clone());
                }
            }
        }
    }
    if visited != steps.len() {
        return Err(WorkflowDomainError::DependencyCycle);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunLifecycle {
    Draft,
    Ready,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl WorkflowRunLifecycle {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepState {
    Pending,
    Ready,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl WorkflowStepState {
    pub fn can_transition_to(self, target: Self) -> bool {
        use WorkflowStepState::*;
        matches!(
            (self, target),
            (Pending, Ready | Cancelled)
                | (Ready, Running | Cancelled)
                | (Running, Waiting | Succeeded | Failed | Cancelled)
                | (Waiting, Running | Succeeded | Failed | Cancelled)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    id: WorkflowRunId,
    workflow_id: WorkflowId,
    workflow_version: u16,
    team_id: TeamId,
    lifecycle: WorkflowRunLifecycle,
    step_states: BTreeMap<WorkflowStepId, WorkflowStepState>,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl WorkflowRun {
    pub fn new(
        id: WorkflowRunId,
        definition: &WorkflowDefinition,
        created_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        if created_at < definition.created_at() {
            return Err(WorkflowDomainError::InvalidTimestamp);
        }
        Ok(Self {
            id,
            workflow_id: definition.id().clone(),
            workflow_version: definition.version(),
            team_id: definition.team_id().clone(),
            lifecycle: WorkflowRunLifecycle::Draft,
            step_states: definition
                .steps()
                .iter()
                .map(|step| (step.id().clone(), WorkflowStepState::Pending))
                .collect(),
            revision: 1,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn id(&self) -> &WorkflowRunId {
        &self.id
    }
    pub fn workflow_id(&self) -> &WorkflowId {
        &self.workflow_id
    }
    pub fn workflow_version(&self) -> u16 {
        self.workflow_version
    }
    pub fn team_id(&self) -> &TeamId {
        &self.team_id
    }
    pub fn lifecycle(&self) -> WorkflowRunLifecycle {
        self.lifecycle
    }
    pub fn step_states(&self) -> &BTreeMap<WorkflowStepId, WorkflowStepState> {
        &self.step_states
    }
    pub fn step_state(&self, step_id: &WorkflowStepId) -> Option<WorkflowStepState> {
        self.step_states.get(step_id).copied()
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

    pub fn activate(
        &self,
        definition: &WorkflowDefinition,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        self.ensure_revision(expected_revision)?;
        if self.lifecycle != WorkflowRunLifecycle::Draft {
            return Err(WorkflowDomainError::InvalidRunTransition {
                from: self.lifecycle,
                to: WorkflowRunLifecycle::Ready,
            });
        }
        self.ensure_definition(definition)?;
        self.ensure_timestamp(updated_at)?;
        let mut updated = self.clone();
        updated.lifecycle = WorkflowRunLifecycle::Ready;
        for step in definition.steps() {
            if step.dependencies().is_empty() {
                updated
                    .step_states
                    .insert(step.id().clone(), WorkflowStepState::Ready);
            }
        }
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }

    pub fn start_step(
        &self,
        step_id: &WorkflowStepId,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        self.transition_step(
            step_id,
            WorkflowStepState::Running,
            expected_revision,
            updated_at,
        )
    }

    pub fn transition_step(
        &self,
        step_id: &WorkflowStepId,
        target: WorkflowStepState,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        self.ensure_revision(expected_revision)?;
        self.ensure_timestamp(updated_at)?;
        if self.lifecycle.is_terminal() || self.lifecycle == WorkflowRunLifecycle::Draft {
            return Err(WorkflowDomainError::InvalidRunTransition {
                from: self.lifecycle,
                to: self.lifecycle,
            });
        }
        let current = self
            .step_state(step_id)
            .ok_or_else(|| WorkflowDomainError::StepNotFound(step_id.clone()))?;
        if current == target {
            return Ok(self.clone());
        }
        if !current.can_transition_to(target) {
            return Err(WorkflowDomainError::InvalidStepTransition {
                from: current,
                to: target,
            });
        }
        let mut updated = self.clone();
        updated.step_states.insert(step_id.clone(), target);
        updated.lifecycle = match target {
            WorkflowStepState::Running => WorkflowRunLifecycle::Running,
            WorkflowStepState::Waiting => WorkflowRunLifecycle::Waiting,
            WorkflowStepState::Failed => WorkflowRunLifecycle::Failed,
            WorkflowStepState::Cancelled => WorkflowRunLifecycle::Cancelled,
            WorkflowStepState::Succeeded
            | WorkflowStepState::Pending
            | WorkflowStepState::Ready => updated.lifecycle,
        };
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }

    pub fn release_dependents(
        &self,
        definition: &WorkflowDefinition,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        self.ensure_revision(expected_revision)?;
        self.ensure_definition(definition)?;
        self.ensure_timestamp(updated_at)?;
        if self.lifecycle.is_terminal() {
            return Ok(self.clone());
        }
        let mut updated = self.clone();
        for step in definition.steps() {
            if updated.step_state(step.id()) == Some(WorkflowStepState::Pending)
                && step.dependencies().iter().all(|dependency| {
                    updated.step_state(dependency) == Some(WorkflowStepState::Succeeded)
                })
            {
                updated
                    .step_states
                    .insert(step.id().clone(), WorkflowStepState::Ready);
            }
        }
        if updated
            .step_states
            .values()
            .all(|state| *state == WorkflowStepState::Succeeded)
        {
            updated.lifecycle = WorkflowRunLifecycle::Succeeded;
        } else if updated
            .step_states
            .values()
            .any(|state| *state == WorkflowStepState::Running)
        {
            updated.lifecycle = WorkflowRunLifecycle::Running;
        } else if updated
            .step_states
            .values()
            .any(|state| *state == WorkflowStepState::Waiting)
        {
            updated.lifecycle = WorkflowRunLifecycle::Waiting;
        } else {
            updated.lifecycle = WorkflowRunLifecycle::Ready;
        }
        if updated.step_states != self.step_states || updated.lifecycle != self.lifecycle {
            updated.revision += 1;
            updated.updated_at = updated_at;
        }
        Ok(updated)
    }

    pub fn cancel(
        &self,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        self.ensure_revision(expected_revision)?;
        self.ensure_timestamp(updated_at)?;
        if self.lifecycle.is_terminal() {
            return Err(WorkflowDomainError::InvalidRunTransition {
                from: self.lifecycle,
                to: WorkflowRunLifecycle::Cancelled,
            });
        }
        let mut updated = self.clone();
        for state in updated.step_states.values_mut() {
            if !state.is_terminal() {
                *state = WorkflowStepState::Cancelled;
            }
        }
        updated.lifecycle = WorkflowRunLifecycle::Cancelled;
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }

    fn ensure_revision(&self, expected_revision: u64) -> Result<(), WorkflowDomainError> {
        if expected_revision != self.revision {
            return Err(WorkflowDomainError::RunRevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        Ok(())
    }

    fn ensure_timestamp(&self, updated_at: i64) -> Result<(), WorkflowDomainError> {
        if updated_at < self.updated_at {
            return Err(WorkflowDomainError::InvalidTimestamp);
        }
        Ok(())
    }

    fn ensure_definition(
        &self,
        definition: &WorkflowDefinition,
    ) -> Result<(), WorkflowDomainError> {
        if self.workflow_id != *definition.id()
            || self.workflow_version != definition.version()
            || self.team_id != *definition.team_id()
            || self.step_states.len() != definition.steps().len()
        {
            return Err(WorkflowDomainError::InvalidVersion);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTaskLifecycle {
    Assigned,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl WorkflowTaskLifecycle {
    pub fn can_transition_to(self, target: Self) -> bool {
        use WorkflowTaskLifecycle::*;
        matches!(
            (self, target),
            (Assigned, Running | Cancelled)
                | (Running, Waiting | Succeeded | Failed | Cancelled)
                | (Waiting, Running | Succeeded | Failed | Cancelled)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTask {
    id: WorkflowTaskId,
    run_id: WorkflowRunId,
    step_id: WorkflowStepId,
    agent_id: String,
    membership_id: TeamMembershipId,
    role_assignment_id: RoleAssignmentId,
    execution_id: RuntimeExecutionId,
    governance: ExecutionGovernanceEvidence,
    attempt: u16,
    lifecycle: WorkflowTaskLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl WorkflowTask {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: WorkflowTaskId,
        run_id: WorkflowRunId,
        step_id: WorkflowStepId,
        agent_id: impl Into<String>,
        membership_id: TeamMembershipId,
        role_assignment_id: RoleAssignmentId,
        execution_id: RuntimeExecutionId,
        governance: ExecutionGovernanceEvidence,
        attempt: u16,
        created_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        if attempt == 0 {
            return Err(WorkflowDomainError::InvalidAttempt);
        }
        if created_at < 0 {
            return Err(WorkflowDomainError::InvalidTimestamp);
        }
        Ok(Self {
            id,
            run_id,
            step_id,
            agent_id: identifier("Agent ID", agent_id)?,
            membership_id,
            role_assignment_id,
            execution_id,
            governance,
            attempt,
            lifecycle: WorkflowTaskLifecycle::Assigned,
            revision: 1,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn id(&self) -> &WorkflowTaskId {
        &self.id
    }
    pub fn run_id(&self) -> &WorkflowRunId {
        &self.run_id
    }
    pub fn step_id(&self) -> &WorkflowStepId {
        &self.step_id
    }
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn membership_id(&self) -> &TeamMembershipId {
        &self.membership_id
    }
    pub fn role_assignment_id(&self) -> &RoleAssignmentId {
        &self.role_assignment_id
    }
    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }
    pub fn governance(&self) -> &ExecutionGovernanceEvidence {
        &self.governance
    }
    pub fn attempt(&self) -> u16 {
        self.attempt
    }
    pub fn lifecycle(&self) -> WorkflowTaskLifecycle {
        self.lifecycle
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

    pub fn transition_to(
        &self,
        target: WorkflowTaskLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, WorkflowDomainError> {
        if expected_revision != self.revision {
            return Err(WorkflowDomainError::TaskRevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if target == self.lifecycle {
            return Ok(self.clone());
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(WorkflowDomainError::InvalidTaskTransition {
                from: self.lifecycle,
                to: target,
            });
        }
        if updated_at < self.updated_at {
            return Err(WorkflowDomainError::InvalidTimestamp);
        }
        let mut updated = self.clone();
        updated.lifecycle = target;
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability_domain::CapabilitySnapshotId,
        permission_domain::{AuthorizationDecisionId, PermissionGrantId},
    };

    fn step(id: &str, dependencies: Vec<&str>) -> WorkflowStepDefinition {
        WorkflowStepDefinition::new(
            WorkflowStepId::new(id).unwrap(),
            id,
            format!("Complete {id}"),
            RoleId::new("role:worker").unwrap(),
            1,
            dependencies
                .into_iter()
                .map(|dependency| WorkflowStepId::new(dependency).unwrap())
                .collect(),
            Vec::new(),
            vec!["permission:bounded-work".into()],
            vec!["Evidence is recorded".into()],
        )
        .unwrap()
    }

    fn definition() -> WorkflowDefinition {
        WorkflowDefinition::new(
            WorkflowId::new("workflow:delivery").unwrap(),
            1,
            TeamId::new("team:one").unwrap(),
            "Delivery",
            "Deliver explicit governed work",
            vec![
                step("step:build", Vec::new()),
                step("step:review", vec!["step:build"]),
            ],
            1,
        )
        .unwrap()
    }

    #[test]
    fn definition_rejects_dependency_cycles() {
        let result = WorkflowDefinition::new(
            WorkflowId::new("workflow:cycle").unwrap(),
            1,
            TeamId::new("team:one").unwrap(),
            "Cycle",
            "Invalid cyclic workflow",
            vec![
                step("step:a", vec!["step:b"]),
                step("step:b", vec!["step:a"]),
            ],
            1,
        );
        assert!(matches!(result, Err(WorkflowDomainError::DependencyCycle)));
    }

    #[test]
    fn run_releases_only_dependencies_satisfied_by_explicit_success() {
        let definition = definition();
        let run = WorkflowRun::new(WorkflowRunId::new("run:one").unwrap(), &definition, 2)
            .unwrap()
            .activate(&definition, 1, 3)
            .unwrap();
        assert_eq!(
            run.step_state(&WorkflowStepId::new("step:review").unwrap()),
            Some(WorkflowStepState::Pending)
        );
        let running = run
            .start_step(&WorkflowStepId::new("step:build").unwrap(), 2, 4)
            .unwrap();
        let succeeded = running
            .transition_step(
                &WorkflowStepId::new("step:build").unwrap(),
                WorkflowStepState::Succeeded,
                3,
                5,
            )
            .unwrap();
        let released = succeeded.release_dependents(&definition, 4, 5).unwrap();
        assert_eq!(
            released.step_state(&WorkflowStepId::new("step:review").unwrap()),
            Some(WorkflowStepState::Ready)
        );
    }

    #[test]
    fn task_keeps_agent_role_and_governance_as_independent_references() {
        let governance = ExecutionGovernanceEvidence::new(
            CapabilitySnapshotId::new("snapshot:one").unwrap(),
            PermissionGrantId::new("grant:one").unwrap(),
            RoleAssignmentId::new("assignment:one").unwrap(),
            AuthorizationDecisionId::new("decision:one").unwrap(),
        );
        let task = WorkflowTask::new(
            WorkflowTaskId::new("task:one").unwrap(),
            WorkflowRunId::new("run:one").unwrap(),
            WorkflowStepId::new("step:build").unwrap(),
            "agent:one",
            TeamMembershipId::new("membership:one").unwrap(),
            RoleAssignmentId::new("assignment:one").unwrap(),
            RuntimeExecutionId::new("execution:one").unwrap(),
            governance,
            1,
            10,
        )
        .unwrap();

        assert_eq!(task.agent_id(), "agent:one");
        assert_eq!(task.role_assignment_id().as_str(), "assignment:one");
        assert_eq!(
            task.governance().permission_grant_id().as_str(),
            "grant:one"
        );
    }
}
