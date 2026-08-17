//! Team, Team Membership, and Team Relationship domain contracts.
//!
//! Team organization is independent from Agent identity and execution. Membership
//! and relationships describe collaboration context; they never grant Permission,
//! satisfy Capability, or advance Workflow state.

use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_ID_LENGTH: usize = 160;
const MAX_TEXT_LENGTH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TeamDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("{aggregate} revision must be positive")]
    InvalidRevision { aggregate: &'static str },
    #[error("Team Membership validity interval is invalid")]
    InvalidValidity,
    #[error("Team Relationship endpoints must be distinct")]
    RelationshipSelfReference,
    #[error("Invalid Team lifecycle transition: {from:?} -> {to:?}")]
    InvalidTeamTransition {
        from: TeamLifecycle,
        to: TeamLifecycle,
    },
    #[error("Invalid Team Membership lifecycle transition: {from:?} -> {to:?}")]
    InvalidMembershipTransition {
        from: TeamMembershipLifecycle,
        to: TeamMembershipLifecycle,
    },
    #[error("Invalid Team Relationship lifecycle transition: {from:?} -> {to:?}")]
    InvalidRelationshipTransition {
        from: TeamRelationshipLifecycle,
        to: TeamRelationshipLifecycle,
    },
    #[error("{aggregate} revision conflict: expected {expected}, current {current}")]
    RevisionConflict {
        aggregate: &'static str,
        expected: u64,
        current: u64,
    },
    #[error("Update timestamp precedes the aggregate's current timestamp")]
    InvalidTimestamp,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, TeamDomainError> {
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

typed_id!(TeamId, "Team ID");
typed_id!(TeamMembershipId, "Team Membership ID");
typed_id!(TeamRelationshipId, "Team Relationship ID");

fn identifier(field: &'static str, value: impl Into<String>) -> Result<String, TeamDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(TeamDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(TeamDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(TeamDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn text(field: &'static str, value: impl Into<String>) -> Result<String, TeamDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(TeamDomainError::Empty { field });
    }
    if value.chars().count() > MAX_TEXT_LENGTH {
        return Err(TeamDomainError::TooLong {
            field,
            max: MAX_TEXT_LENGTH,
        });
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamLifecycle {
    Draft,
    Active,
    Suspended,
    Archived,
}

impl TeamLifecycle {
    pub fn can_transition_to(self, target: Self) -> bool {
        use TeamLifecycle::*;
        matches!(
            (self, target),
            (Draft, Active | Archived)
                | (Active, Suspended | Archived)
                | (Suspended, Active | Archived)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    id: TeamId,
    name: String,
    purpose: String,
    owner_ref: String,
    policy_refs: Vec<String>,
    compatible_workflow_refs: Vec<String>,
    lifecycle: TeamLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl Team {
    pub fn new(
        id: TeamId,
        name: impl Into<String>,
        purpose: impl Into<String>,
        owner_ref: impl Into<String>,
        policy_refs: Vec<String>,
        compatible_workflow_refs: Vec<String>,
        created_at: i64,
    ) -> Result<Self, TeamDomainError> {
        if created_at < 0 {
            return Err(TeamDomainError::InvalidTimestamp);
        }
        Ok(Self {
            id,
            name: text("Team name", name)?,
            purpose: text("Team purpose", purpose)?,
            owner_ref: identifier("Team owner reference", owner_ref)?,
            policy_refs: policy_refs
                .into_iter()
                .map(|value| identifier("Team policy reference", value))
                .collect::<Result<Vec<_>, _>>()?,
            compatible_workflow_refs: compatible_workflow_refs
                .into_iter()
                .map(|value| identifier("Compatible Workflow reference", value))
                .collect::<Result<Vec<_>, _>>()?,
            lifecycle: TeamLifecycle::Draft,
            revision: 1,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn id(&self) -> &TeamId {
        &self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn purpose(&self) -> &str {
        &self.purpose
    }
    pub fn owner_ref(&self) -> &str {
        &self.owner_ref
    }
    pub fn policy_refs(&self) -> &[String] {
        &self.policy_refs
    }
    pub fn compatible_workflow_refs(&self) -> &[String] {
        &self.compatible_workflow_refs
    }
    pub fn lifecycle(&self) -> TeamLifecycle {
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
        target: TeamLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, TeamDomainError> {
        if expected_revision != self.revision {
            return Err(TeamDomainError::RevisionConflict {
                aggregate: "Team",
                expected: expected_revision,
                current: self.revision,
            });
        }
        if target == self.lifecycle {
            return Ok(self.clone());
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(TeamDomainError::InvalidTeamTransition {
                from: self.lifecycle,
                to: target,
            });
        }
        if updated_at < self.updated_at {
            return Err(TeamDomainError::InvalidTimestamp);
        }
        let mut updated = self.clone();
        updated.lifecycle = target;
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMembershipLifecycle {
    Invited,
    Active,
    Suspended,
    Ended,
}

impl TeamMembershipLifecycle {
    pub fn can_transition_to(self, target: Self) -> bool {
        use TeamMembershipLifecycle::*;
        matches!(
            (self, target),
            (Invited, Active | Ended) | (Active, Suspended | Ended) | (Suspended, Active | Ended)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMembership {
    id: TeamMembershipId,
    team_id: TeamId,
    agent_id: String,
    label: Option<String>,
    policy_refinements: Vec<String>,
    provenance_ref: String,
    lifecycle: TeamMembershipLifecycle,
    valid_from: i64,
    valid_until: Option<i64>,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl TeamMembership {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: TeamMembershipId,
        team_id: TeamId,
        agent_id: impl Into<String>,
        label: Option<String>,
        policy_refinements: Vec<String>,
        provenance_ref: impl Into<String>,
        valid_from: i64,
        valid_until: Option<i64>,
        created_at: i64,
    ) -> Result<Self, TeamDomainError> {
        if created_at < 0
            || valid_from < created_at
            || valid_until.is_some_and(|until| until < valid_from)
        {
            return Err(TeamDomainError::InvalidValidity);
        }
        Ok(Self {
            id,
            team_id,
            agent_id: identifier("Agent ID", agent_id)?,
            label: label
                .map(|value| text("Team Membership label", value))
                .transpose()?,
            policy_refinements: policy_refinements
                .into_iter()
                .map(|value| identifier("Membership policy refinement", value))
                .collect::<Result<Vec<_>, _>>()?,
            provenance_ref: identifier("Membership provenance reference", provenance_ref)?,
            lifecycle: TeamMembershipLifecycle::Invited,
            valid_from,
            valid_until,
            revision: 1,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn id(&self) -> &TeamMembershipId {
        &self.id
    }
    pub fn team_id(&self) -> &TeamId {
        &self.team_id
    }
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
    pub fn policy_refinements(&self) -> &[String] {
        &self.policy_refinements
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }
    pub fn lifecycle(&self) -> TeamMembershipLifecycle {
        self.lifecycle
    }
    pub fn valid_from(&self) -> i64 {
        self.valid_from
    }
    pub fn valid_until(&self) -> Option<i64> {
        self.valid_until
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
    pub fn is_effective(&self, at: i64) -> bool {
        self.lifecycle == TeamMembershipLifecycle::Active
            && at >= self.valid_from
            && self.valid_until.is_none_or(|until| at <= until)
    }

    pub fn transition_to(
        &self,
        target: TeamMembershipLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, TeamDomainError> {
        if expected_revision != self.revision {
            return Err(TeamDomainError::RevisionConflict {
                aggregate: "Team Membership",
                expected: expected_revision,
                current: self.revision,
            });
        }
        if target == self.lifecycle {
            return Ok(self.clone());
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(TeamDomainError::InvalidMembershipTransition {
                from: self.lifecycle,
                to: target,
            });
        }
        if updated_at < self.updated_at {
            return Err(TeamDomainError::InvalidTimestamp);
        }
        let mut updated = self.clone();
        updated.lifecycle = target;
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRelationshipLifecycle {
    Active,
    Ended,
}

impl TeamRelationshipLifecycle {
    pub fn can_transition_to(self, target: Self) -> bool {
        self == Self::Active && target == Self::Ended
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRelationship {
    id: TeamRelationshipId,
    team_id: TeamId,
    source_membership_id: TeamMembershipId,
    target_membership_id: TeamMembershipId,
    relationship_kind: String,
    provenance_ref: String,
    lifecycle: TeamRelationshipLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl TeamRelationship {
    pub fn new(
        id: TeamRelationshipId,
        team_id: TeamId,
        source_membership_id: TeamMembershipId,
        target_membership_id: TeamMembershipId,
        relationship_kind: impl Into<String>,
        provenance_ref: impl Into<String>,
        created_at: i64,
    ) -> Result<Self, TeamDomainError> {
        if source_membership_id == target_membership_id {
            return Err(TeamDomainError::RelationshipSelfReference);
        }
        if created_at < 0 {
            return Err(TeamDomainError::InvalidTimestamp);
        }
        Ok(Self {
            id,
            team_id,
            source_membership_id,
            target_membership_id,
            relationship_kind: identifier("Team Relationship kind", relationship_kind)?,
            provenance_ref: identifier("Team Relationship provenance reference", provenance_ref)?,
            lifecycle: TeamRelationshipLifecycle::Active,
            revision: 1,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn id(&self) -> &TeamRelationshipId {
        &self.id
    }
    pub fn team_id(&self) -> &TeamId {
        &self.team_id
    }
    pub fn source_membership_id(&self) -> &TeamMembershipId {
        &self.source_membership_id
    }
    pub fn target_membership_id(&self) -> &TeamMembershipId {
        &self.target_membership_id
    }
    pub fn relationship_kind(&self) -> &str {
        &self.relationship_kind
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }
    pub fn lifecycle(&self) -> TeamRelationshipLifecycle {
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
        target: TeamRelationshipLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, TeamDomainError> {
        if expected_revision != self.revision {
            return Err(TeamDomainError::RevisionConflict {
                aggregate: "Team Relationship",
                expected: expected_revision,
                current: self.revision,
            });
        }
        if target == self.lifecycle {
            return Ok(self.clone());
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(TeamDomainError::InvalidRelationshipTransition {
                from: self.lifecycle,
                to: target,
            });
        }
        if updated_at < self.updated_at {
            return Err(TeamDomainError::InvalidTimestamp);
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

    #[test]
    fn membership_lifecycle_preserves_agent_identity() {
        let membership = TeamMembership::new(
            TeamMembershipId::new("membership:one").unwrap(),
            TeamId::new("team:one").unwrap(),
            "agent:one",
            None,
            Vec::new(),
            "owner:one",
            10,
            None,
            5,
        )
        .unwrap();
        let active = membership
            .transition_to(TeamMembershipLifecycle::Active, 1, 10)
            .unwrap();
        let ended = active
            .transition_to(TeamMembershipLifecycle::Ended, 2, 20)
            .unwrap();

        assert_eq!(ended.agent_id(), "agent:one");
        assert!(!ended.is_effective(20));
        assert!(matches!(
            ended.transition_to(TeamMembershipLifecycle::Active, 3, 21),
            Err(TeamDomainError::InvalidMembershipTransition { .. })
        ));
    }

    #[test]
    fn relationship_is_collaboration_metadata_not_authority() {
        let relationship = TeamRelationship::new(
            TeamRelationshipId::new("relationship:review").unwrap(),
            TeamId::new("team:one").unwrap(),
            TeamMembershipId::new("membership:author").unwrap(),
            TeamMembershipId::new("membership:reviewer").unwrap(),
            "reviews",
            "owner:one",
            10,
        )
        .unwrap();

        assert_eq!(relationship.relationship_kind(), "reviews");
        assert_eq!(relationship.lifecycle(), TeamRelationshipLifecycle::Active);
    }
}
