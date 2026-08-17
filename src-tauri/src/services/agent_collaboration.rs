//! Permission-bound Agent communication and Handoff coordination service.
//!
//! Messages and Handoffs are immutable evidence. They never mutate Workflow
//! state, assign a Role, or merge Agent identities. A separately governed
//! Workflow task assignment is required after an accepted Handoff.

use thiserror::Error;

use crate::{
    collaboration_domain::{
        CollaborationMessage, CollaborationMessageKind, Handoff, HandoffId, HandoffLifecycle,
    },
    collaboration_repository::{CollaborationRepository, CollaborationRepositoryError},
    permission_domain::AuthorizationDecisionStatus,
    permission_repository::PermissionRepository,
    team_domain::TeamLifecycle,
    team_repository::TeamRepository,
    workflow_domain::{WorkflowStepState, WorkflowTask},
    workflow_repository::WorkflowRepository,
};

#[derive(Debug, Error)]
pub enum AgentCollaborationError {
    #[error(transparent)]
    Domain(#[from] crate::collaboration_domain::CollaborationDomainError),
    #[error(transparent)]
    Repository(#[from] CollaborationRepositoryError),
    #[error("Workflow repository failed: {0}")]
    WorkflowRepository(String),
    #[error("Team repository failed: {0}")]
    TeamRepository(String),
    #[error("Permission repository failed: {0}")]
    PermissionRepository(String),
    #[error("Collaboration context does not match the Team, Workflow Run, or authorizing Task")]
    ContextMismatch,
    #[error(
        "Collaboration Membership is missing, ineffective, or belongs to another Team or Agent"
    )]
    MembershipIneligible,
    #[error("Collaboration Permission Grant is missing, expired, inconsistent, or lacks the required bounded claim")]
    PermissionDenied,
    #[error("Handoff proposal does not match its governed message or target Workflow Step")]
    HandoffMismatch,
    #[error("Handoff resolution actor or message does not match the requested outcome")]
    ResolutionMismatch,
}

pub struct AgentCollaborationService<C, W, T, P> {
    collaboration: C,
    workflows: W,
    teams: T,
    permissions: P,
}

impl<C, W, T, P> AgentCollaborationService<C, W, T, P>
where
    C: CollaborationRepository,
    W: WorkflowRepository,
    T: TeamRepository,
    P: PermissionRepository,
{
    pub fn new(collaboration: C, workflows: W, teams: T, permissions: P) -> Self {
        Self {
            collaboration,
            workflows,
            teams,
            permissions,
        }
    }

    pub fn send_message(
        &self,
        message: CollaborationMessage,
    ) -> Result<CollaborationMessage, AgentCollaborationError> {
        self.validate_message(&message)?;
        self.collaboration.record_message(message.clone())?;
        Ok(message)
    }

    pub fn propose_handoff(
        &self,
        proposal_message: CollaborationMessage,
        handoff: Handoff,
    ) -> Result<Handoff, AgentCollaborationError> {
        if proposal_message.kind() != CollaborationMessageKind::Handoff
            || proposal_message.id() != handoff.proposal_message_id()
            || proposal_message.team_id() != handoff.team_id()
            || proposal_message.run_id() != handoff.run_id()
            || proposal_message.task_id() != handoff.source_task_id()
            || proposal_message.source_membership_id() != handoff.source_membership_id()
            || proposal_message.target_membership_id() != Some(handoff.target_membership_id())
            || proposal_message.created_at() != handoff.created_at()
        {
            return Err(AgentCollaborationError::HandoffMismatch);
        }
        let run = self
            .workflows
            .get_run(handoff.run_id())
            .map_err(workflow_repository)?
            .ok_or(AgentCollaborationError::HandoffMismatch)?;
        let definition = self
            .workflows
            .get_definition(run.workflow_id(), run.workflow_version())
            .map_err(workflow_repository)?
            .ok_or(AgentCollaborationError::HandoffMismatch)?;
        if definition.step(handoff.target_step_id()).is_none()
            || matches!(
                run.step_state(handoff.target_step_id()),
                None | Some(WorkflowStepState::Running)
                    | Some(WorkflowStepState::Waiting)
                    | Some(WorkflowStepState::Succeeded)
                    | Some(WorkflowStepState::Failed)
                    | Some(WorkflowStepState::Cancelled)
            )
        {
            return Err(AgentCollaborationError::HandoffMismatch);
        }
        self.send_message(proposal_message)?;
        self.collaboration.insert_handoff(handoff.clone())?;
        Ok(handoff)
    }

    pub fn resolve_handoff(
        &self,
        handoff_id: &HandoffId,
        target: HandoffLifecycle,
        resolution_message: CollaborationMessage,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Handoff, AgentCollaborationError> {
        let handoff = self
            .collaboration
            .get_handoff(handoff_id)?
            .ok_or_else(|| CollaborationRepositoryError::HandoffNotFound(handoff_id.clone()))?;
        let expected_actor = match target {
            HandoffLifecycle::Accepted | HandoffLifecycle::Rejected => {
                handoff.target_membership_id()
            }
            HandoffLifecycle::Cancelled => handoff.source_membership_id(),
            HandoffLifecycle::Proposed => return Err(AgentCollaborationError::ResolutionMismatch),
        };
        if resolution_message.kind() != CollaborationMessageKind::Handoff
            || resolution_message.team_id() != handoff.team_id()
            || resolution_message.run_id() != handoff.run_id()
            || resolution_message.source_membership_id() != expected_actor
            || resolution_message.created_at() != updated_at
        {
            return Err(AgentCollaborationError::ResolutionMismatch);
        }
        self.send_message(resolution_message.clone())?;
        let updated = handoff.resolve(
            target,
            resolution_message.id().clone(),
            expected_revision,
            updated_at,
        )?;
        self.collaboration
            .update_handoff(updated.clone(), expected_revision)?;
        Ok(updated)
    }

    fn validate_message(
        &self,
        message: &CollaborationMessage,
    ) -> Result<(), AgentCollaborationError> {
        let run = self
            .workflows
            .get_run(message.run_id())
            .map_err(workflow_repository)?
            .ok_or(AgentCollaborationError::ContextMismatch)?;
        let task = self
            .workflows
            .get_task(message.task_id())
            .map_err(workflow_repository)?
            .ok_or(AgentCollaborationError::ContextMismatch)?;
        let team = self
            .teams
            .get_team(message.team_id())
            .map_err(team_repository)?
            .ok_or(AgentCollaborationError::ContextMismatch)?;
        if team.lifecycle() != TeamLifecycle::Active
            || run.team_id() != message.team_id()
            || task.run_id() != run.id()
            || message.created_at() < task.created_at()
        {
            return Err(AgentCollaborationError::ContextMismatch);
        }

        self.require_effective_membership(
            message.source_membership_id(),
            message.team_id(),
            message.created_at(),
            Some(task.agent_id()),
        )?;
        if let Some(target) = message.target_membership_id() {
            self.require_effective_membership(
                target,
                message.team_id(),
                message.created_at(),
                None,
            )?;
        }
        self.require_permission(message, &task, &run)
    }

    fn require_effective_membership(
        &self,
        membership_id: &crate::team_domain::TeamMembershipId,
        team_id: &crate::team_domain::TeamId,
        at: i64,
        agent_id: Option<&str>,
    ) -> Result<(), AgentCollaborationError> {
        let membership = self
            .teams
            .get_membership(membership_id)
            .map_err(team_repository)?
            .ok_or(AgentCollaborationError::MembershipIneligible)?;
        if membership.team_id() != team_id
            || !membership.is_effective(at)
            || agent_id.is_some_and(|agent_id| membership.agent_id() != agent_id)
        {
            return Err(AgentCollaborationError::MembershipIneligible);
        }
        Ok(())
    }

    fn require_permission(
        &self,
        message: &CollaborationMessage,
        task: &WorkflowTask,
        run: &crate::workflow_domain::WorkflowRun,
    ) -> Result<(), AgentCollaborationError> {
        let decision = self
            .permissions
            .get_decision(message.authorization_decision_id())
            .map_err(permission_repository)?
            .ok_or(AgentCollaborationError::PermissionDenied)?;
        let grant = self
            .permissions
            .get_grant(message.permission_grant_id())
            .map_err(permission_repository)?
            .ok_or(AgentCollaborationError::PermissionDenied)?;
        if decision.status() != AuthorizationDecisionStatus::Allowed
            || decision.id() != task.governance().authorization_decision_id()
            || decision.grant_id() != Some(grant.id())
            || grant.id() != task.governance().permission_grant_id()
            || grant.execution_id() != task.execution_id()
            || grant.agent_id() != task.agent_id()
            || !grant.is_valid_at(message.created_at())
        {
            return Err(AgentCollaborationError::PermissionDenied);
        }
        let required_action = match message.kind() {
            CollaborationMessageKind::Handoff => "collaboration.handoff",
            CollaborationMessageKind::Status
            | CollaborationMessageKind::Question
            | CollaborationMessageKind::Response
            | CollaborationMessageKind::Evidence
            | CollaborationMessageKind::Review => "collaboration.communicate",
        };
        let permitted = grant.claims().iter().any(|claim| {
            claim.action().as_str() == required_action
                && (claim.resource() == message.team_id().as_str()
                    || claim.resource() == run.id().as_str()
                    || claim.resource() == task.id().as_str())
        });
        if !permitted {
            return Err(AgentCollaborationError::PermissionDenied);
        }
        Ok(())
    }
}

fn workflow_repository(error: impl std::fmt::Display) -> AgentCollaborationError {
    AgentCollaborationError::WorkflowRepository(error.to_string())
}

fn team_repository(error: impl std::fmt::Display) -> AgentCollaborationError {
    AgentCollaborationError::TeamRepository(error.to_string())
}

fn permission_repository(error: impl std::fmt::Display) -> AgentCollaborationError {
    AgentCollaborationError::PermissionRepository(error.to_string())
}
