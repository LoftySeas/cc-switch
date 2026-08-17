//! Governance and organization checks for Workflow participation.
//!
//! This gate is read-only. It proves that one explicit Workflow task is backed by
//! an active Agent and Membership, a scoped Role Assignment, a satisfied
//! Capability snapshot, an allowed Decision, a bounded Grant, and an immutable
//! Execution request. It does not invoke a Runtime or advance Workflow state.

use thiserror::Error;

use crate::{
    agent_domain::AgentLifecycle,
    capability_domain::{
        CapabilityRequirement, CapabilityRequirementLevel, CapabilityResolutionStatus,
    },
    capability_registry::CapabilitySnapshotRepository,
    database::Database,
    execution_repository::{ExecutionHistoryRepository, ExecutionRepositoryError},
    governance_admission::GovernedExecutionAdmissionGate,
    permission_repository::PermissionRepository,
    role_domain::RoleAssignmentScopeKind,
    role_repository::RoleRepository,
    runtime_execution::ExecutionAdmissionGate,
    team_domain::{TeamLifecycle, TeamMembershipLifecycle},
    team_repository::TeamRepository,
    workflow_domain::{WorkflowDefinition, WorkflowRun, WorkflowStepDefinition, WorkflowTask},
};

#[derive(Debug, Error)]
pub enum WorkflowGovernanceError {
    #[error("Agent lookup failed: {0}")]
    AgentLookup(String),
    #[error("Agent is missing or inactive: {0}")]
    AgentNotActive(String),
    #[error("Team is missing or inactive")]
    TeamNotActive,
    #[error("Team Membership is missing, inactive, out of scope, or belongs to another Agent")]
    MembershipIneligible,
    #[error("Role Assignment is missing, inactive, out of scope, or does not satisfy the Workflow Step Role")]
    AssignmentIneligible,
    #[error("Workflow Step Capability requirements are not satisfied by the Execution snapshot")]
    CapabilityUnsatisfied,
    #[error("Execution was not found for the Workflow Task")]
    ExecutionNotFound,
    #[error(
        "Execution request does not match the Workflow Task identities or governance evidence"
    )]
    ExecutionMismatch,
    #[error("Execution governance admission was rejected: {0}")]
    AdmissionRejected(String),
    #[error("Workflow governance repository failed: {0}")]
    Repository(String),
}

pub trait WorkflowParticipationGate: Send + Sync {
    fn validate(
        &self,
        db: &Database,
        definition: &WorkflowDefinition,
        run: &WorkflowRun,
        step: &WorkflowStepDefinition,
        task: &WorkflowTask,
        at: i64,
    ) -> Result<(), WorkflowGovernanceError>;
}

pub struct GovernedWorkflowParticipationGate<C, P, R, T, E> {
    capabilities: C,
    permissions: P,
    roles: R,
    teams: T,
    executions: E,
}

impl<C, P, R, T, E> GovernedWorkflowParticipationGate<C, P, R, T, E> {
    pub fn new(capabilities: C, permissions: P, roles: R, teams: T, executions: E) -> Self {
        Self {
            capabilities,
            permissions,
            roles,
            teams,
            executions,
        }
    }
}

impl<C, P, R, T, E> WorkflowParticipationGate for GovernedWorkflowParticipationGate<C, P, R, T, E>
where
    C: CapabilitySnapshotRepository + Clone,
    P: PermissionRepository + Clone,
    R: RoleRepository + Clone,
    T: TeamRepository,
    E: ExecutionHistoryRepository,
{
    fn validate(
        &self,
        db: &Database,
        definition: &WorkflowDefinition,
        run: &WorkflowRun,
        step: &WorkflowStepDefinition,
        task: &WorkflowTask,
        at: i64,
    ) -> Result<(), WorkflowGovernanceError> {
        let agent = db
            .get_agent(task.agent_id())
            .map_err(|error| WorkflowGovernanceError::AgentLookup(error.to_string()))?
            .ok_or_else(|| WorkflowGovernanceError::AgentNotActive(task.agent_id().to_string()))?;
        if agent.lifecycle_state != AgentLifecycle::Active {
            return Err(WorkflowGovernanceError::AgentNotActive(
                task.agent_id().to_string(),
            ));
        }

        let team = self
            .teams
            .get_team(run.team_id())
            .map_err(repository)?
            .ok_or(WorkflowGovernanceError::TeamNotActive)?;
        if team.lifecycle() != TeamLifecycle::Active || definition.team_id() != team.id() {
            return Err(WorkflowGovernanceError::TeamNotActive);
        }

        let membership = self
            .teams
            .get_membership(task.membership_id())
            .map_err(repository)?
            .ok_or(WorkflowGovernanceError::MembershipIneligible)?;
        if membership.lifecycle() != TeamMembershipLifecycle::Active
            || !membership.is_effective(at)
            || membership.team_id() != run.team_id()
            || membership.agent_id() != task.agent_id()
        {
            return Err(WorkflowGovernanceError::MembershipIneligible);
        }

        let assignment = self
            .roles
            .get_assignment(task.role_assignment_id())
            .map_err(repository)?
            .ok_or(WorkflowGovernanceError::AssignmentIneligible)?;
        if assignment.id() != task.governance().role_assignment_id()
            || assignment.agent_id() != task.agent_id()
            || assignment.membership_ref() != task.membership_id().as_str()
            || assignment.role_id() != step.role_id()
            || assignment.role_version() != step.role_version()
            || !assignment.is_effective(at)
            || !scope_applies(&assignment, definition, run, step, task)
        {
            return Err(WorkflowGovernanceError::AssignmentIneligible);
        }
        let role = self
            .roles
            .get_definition(assignment.role_id(), assignment.role_version())
            .map_err(repository)?
            .ok_or(WorkflowGovernanceError::AssignmentIneligible)?;

        let execution = self
            .executions
            .get(task.execution_id())
            .map_err(execution_repository)?
            .ok_or(WorkflowGovernanceError::ExecutionNotFound)?;
        let request = execution.request();
        if request.execution_id() != task.execution_id()
            || request.context().binding().agent_id() != task.agent_id()
            || request.governance() != task.governance()
            || request.correlation_ref() != Some(assignment.scope().reference())
            || request.accepted_at() > task.created_at()
        {
            return Err(WorkflowGovernanceError::ExecutionMismatch);
        }

        let snapshot = self
            .capabilities
            .get_snapshot(task.governance().capability_snapshot_id())
            .map_err(repository)?
            .ok_or(WorkflowGovernanceError::CapabilityUnsatisfied)?;
        if snapshot.execution_id() != task.execution_id()
            || !snapshot.is_satisfied()
            || !step
                .capability_requirements()
                .iter()
                .chain(role.capability_requirements())
                .chain(assignment.additional_capability_requirements())
                .all(|requirement| snapshot_satisfies(&snapshot, requirement))
        {
            return Err(WorkflowGovernanceError::CapabilityUnsatisfied);
        }

        GovernedExecutionAdmissionGate::new(
            self.capabilities.clone(),
            self.permissions.clone(),
            self.roles.clone(),
        )
        .admit(request)
        .map_err(|error| WorkflowGovernanceError::AdmissionRejected(error.to_string()))?;
        Ok(())
    }
}

fn scope_applies(
    assignment: &crate::role_domain::RoleAssignment,
    definition: &WorkflowDefinition,
    run: &WorkflowRun,
    step: &WorkflowStepDefinition,
    task: &WorkflowTask,
) -> bool {
    let expected = match assignment.scope().kind() {
        RoleAssignmentScopeKind::Team => run.team_id().as_str(),
        RoleAssignmentScopeKind::Workflow => definition.id().as_str(),
        RoleAssignmentScopeKind::WorkflowStep => step.id().as_str(),
        RoleAssignmentScopeKind::Task => task.id().as_str(),
        RoleAssignmentScopeKind::Repository | RoleAssignmentScopeKind::Review => return false,
    };
    assignment.scope().reference() == expected
}

fn snapshot_satisfies(
    snapshot: &crate::capability_domain::CapabilitySnapshot,
    required: &CapabilityRequirement,
) -> bool {
    snapshot.entries().iter().any(|entry| {
        let resolved = entry.requirement();
        resolved.capability_id() == required.capability_id()
            && resolved.minimum_version() >= required.minimum_version()
            && required
                .required_constraints()
                .iter()
                .all(|(key, value)| resolved.required_constraints().get(key) == Some(value))
            && entry.is_satisfied()
            && !(required.level() == CapabilityRequirementLevel::Required
                && entry.status() == CapabilityResolutionStatus::OptionalFallback)
    })
}

fn repository(error: impl std::fmt::Display) -> WorkflowGovernanceError {
    WorkflowGovernanceError::Repository(error.to_string())
}

fn execution_repository(error: ExecutionRepositoryError) -> WorkflowGovernanceError {
    repository(error)
}
