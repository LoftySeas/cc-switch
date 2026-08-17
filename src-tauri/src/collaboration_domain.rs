//! Immutable Agent communication and explicit Handoff coordination contracts.
//!
//! Communication carries bounded references, never implicit Workflow commands.
//! A Handoff acceptance records intent only; Workflow participation remains an
//! explicit, separately governed assignment.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    permission_domain::{AuthorizationDecisionId, PermissionGrantId},
    team_domain::{TeamId, TeamMembershipId},
    workflow_domain::{WorkflowRunId, WorkflowStepId, WorkflowTaskId},
};

const MAX_ID_LENGTH: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CollaborationDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Collaboration source and target Memberships must be distinct")]
    SelfCommunication,
    #[error("Handoff requires an explicit target Membership")]
    MissingHandoffTarget,
    #[error("Invalid Handoff lifecycle transition: {from:?} -> {to:?}")]
    InvalidHandoffTransition {
        from: HandoffLifecycle,
        to: HandoffLifecycle,
    },
    #[error("Handoff revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("Collaboration timestamp order is invalid")]
    InvalidTimestamp,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, CollaborationDomainError> {
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

typed_id!(CollaborationMessageId, "Collaboration Message ID");
typed_id!(HandoffId, "Handoff ID");

fn identifier(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, CollaborationDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(CollaborationDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(CollaborationDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(CollaborationDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationMessageKind {
    Status,
    Question,
    Response,
    Evidence,
    Review,
    Handoff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationMessage {
    id: CollaborationMessageId,
    team_id: TeamId,
    run_id: WorkflowRunId,
    task_id: WorkflowTaskId,
    source_membership_id: TeamMembershipId,
    target_membership_id: Option<TeamMembershipId>,
    kind: CollaborationMessageKind,
    content_ref: String,
    authorization_decision_id: AuthorizationDecisionId,
    permission_grant_id: PermissionGrantId,
    created_at: i64,
}

impl CollaborationMessage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: CollaborationMessageId,
        team_id: TeamId,
        run_id: WorkflowRunId,
        task_id: WorkflowTaskId,
        source_membership_id: TeamMembershipId,
        target_membership_id: Option<TeamMembershipId>,
        kind: CollaborationMessageKind,
        content_ref: impl Into<String>,
        authorization_decision_id: AuthorizationDecisionId,
        permission_grant_id: PermissionGrantId,
        created_at: i64,
    ) -> Result<Self, CollaborationDomainError> {
        if target_membership_id
            .as_ref()
            .is_some_and(|target| target == &source_membership_id)
        {
            return Err(CollaborationDomainError::SelfCommunication);
        }
        if kind == CollaborationMessageKind::Handoff && target_membership_id.is_none() {
            return Err(CollaborationDomainError::MissingHandoffTarget);
        }
        if created_at < 0 {
            return Err(CollaborationDomainError::InvalidTimestamp);
        }
        Ok(Self {
            id,
            team_id,
            run_id,
            task_id,
            source_membership_id,
            target_membership_id,
            kind,
            content_ref: identifier("Collaboration content reference", content_ref)?,
            authorization_decision_id,
            permission_grant_id,
            created_at,
        })
    }

    pub fn id(&self) -> &CollaborationMessageId {
        &self.id
    }
    pub fn team_id(&self) -> &TeamId {
        &self.team_id
    }
    pub fn run_id(&self) -> &WorkflowRunId {
        &self.run_id
    }
    pub fn task_id(&self) -> &WorkflowTaskId {
        &self.task_id
    }
    pub fn source_membership_id(&self) -> &TeamMembershipId {
        &self.source_membership_id
    }
    pub fn target_membership_id(&self) -> Option<&TeamMembershipId> {
        self.target_membership_id.as_ref()
    }
    pub fn kind(&self) -> CollaborationMessageKind {
        self.kind
    }
    pub fn content_ref(&self) -> &str {
        &self.content_ref
    }
    pub fn authorization_decision_id(&self) -> &AuthorizationDecisionId {
        &self.authorization_decision_id
    }
    pub fn permission_grant_id(&self) -> &PermissionGrantId {
        &self.permission_grant_id
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffLifecycle {
    Proposed,
    Accepted,
    Rejected,
    Cancelled,
}

impl HandoffLifecycle {
    pub fn can_transition_to(self, target: Self) -> bool {
        self == Self::Proposed
            && matches!(target, Self::Accepted | Self::Rejected | Self::Cancelled)
    }

    pub fn is_terminal(self) -> bool {
        self != Self::Proposed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    id: HandoffId,
    team_id: TeamId,
    run_id: WorkflowRunId,
    source_task_id: WorkflowTaskId,
    target_step_id: WorkflowStepId,
    source_membership_id: TeamMembershipId,
    target_membership_id: TeamMembershipId,
    proposal_message_id: CollaborationMessageId,
    resolution_message_id: Option<CollaborationMessageId>,
    lifecycle: HandoffLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl Handoff {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: HandoffId,
        team_id: TeamId,
        run_id: WorkflowRunId,
        source_task_id: WorkflowTaskId,
        target_step_id: WorkflowStepId,
        source_membership_id: TeamMembershipId,
        target_membership_id: TeamMembershipId,
        proposal_message_id: CollaborationMessageId,
        created_at: i64,
    ) -> Result<Self, CollaborationDomainError> {
        if source_membership_id == target_membership_id {
            return Err(CollaborationDomainError::SelfCommunication);
        }
        if created_at < 0 {
            return Err(CollaborationDomainError::InvalidTimestamp);
        }
        Ok(Self {
            id,
            team_id,
            run_id,
            source_task_id,
            target_step_id,
            source_membership_id,
            target_membership_id,
            proposal_message_id,
            resolution_message_id: None,
            lifecycle: HandoffLifecycle::Proposed,
            revision: 1,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn id(&self) -> &HandoffId {
        &self.id
    }
    pub fn team_id(&self) -> &TeamId {
        &self.team_id
    }
    pub fn run_id(&self) -> &WorkflowRunId {
        &self.run_id
    }
    pub fn source_task_id(&self) -> &WorkflowTaskId {
        &self.source_task_id
    }
    pub fn target_step_id(&self) -> &WorkflowStepId {
        &self.target_step_id
    }
    pub fn source_membership_id(&self) -> &TeamMembershipId {
        &self.source_membership_id
    }
    pub fn target_membership_id(&self) -> &TeamMembershipId {
        &self.target_membership_id
    }
    pub fn proposal_message_id(&self) -> &CollaborationMessageId {
        &self.proposal_message_id
    }
    pub fn resolution_message_id(&self) -> Option<&CollaborationMessageId> {
        self.resolution_message_id.as_ref()
    }
    pub fn lifecycle(&self) -> HandoffLifecycle {
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

    pub fn resolve(
        &self,
        target: HandoffLifecycle,
        resolution_message_id: CollaborationMessageId,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, CollaborationDomainError> {
        if expected_revision != self.revision {
            return Err(CollaborationDomainError::RevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(CollaborationDomainError::InvalidHandoffTransition {
                from: self.lifecycle,
                to: target,
            });
        }
        if updated_at < self.updated_at {
            return Err(CollaborationDomainError::InvalidTimestamp);
        }
        let mut updated = self.clone();
        updated.lifecycle = target;
        updated.resolution_message_id = Some(resolution_message_id);
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(
        kind: CollaborationMessageKind,
    ) -> Result<CollaborationMessage, CollaborationDomainError> {
        CollaborationMessage::new(
            CollaborationMessageId::new("message:one")?,
            TeamId::new("team:one").unwrap(),
            WorkflowRunId::new("run:one").unwrap(),
            WorkflowTaskId::new("task:one").unwrap(),
            TeamMembershipId::new("membership:source").unwrap(),
            None,
            kind,
            "artifact:message-one",
            AuthorizationDecisionId::new("decision:one").unwrap(),
            PermissionGrantId::new("grant:one").unwrap(),
            10,
        )
    }

    #[test]
    fn handoff_message_requires_explicit_distinct_target() {
        assert!(matches!(
            message(CollaborationMessageKind::Handoff),
            Err(CollaborationDomainError::MissingHandoffTarget)
        ));
    }

    #[test]
    fn accepted_handoff_does_not_embed_workflow_state_or_agent_identity() {
        let handoff = Handoff::new(
            HandoffId::new("handoff:one").unwrap(),
            TeamId::new("team:one").unwrap(),
            WorkflowRunId::new("run:one").unwrap(),
            WorkflowTaskId::new("task:source").unwrap(),
            WorkflowStepId::new("step:review").unwrap(),
            TeamMembershipId::new("membership:source").unwrap(),
            TeamMembershipId::new("membership:target").unwrap(),
            CollaborationMessageId::new("message:proposal").unwrap(),
            10,
        )
        .unwrap();
        let accepted = handoff
            .resolve(
                HandoffLifecycle::Accepted,
                CollaborationMessageId::new("message:acceptance").unwrap(),
                1,
                11,
            )
            .unwrap();

        assert_eq!(accepted.lifecycle(), HandoffLifecycle::Accepted);
        assert_eq!(accepted.target_step_id().as_str(), "step:review");
    }
}
