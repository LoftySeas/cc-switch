//! Role definitions and scoped Role Assignment aggregates.
//!
//! A Role describes responsibility. An assignment records contextual eligibility
//! and narrowing constraints; neither grants Permission or selects infrastructure.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::capability_domain::CapabilityRequirement;

const MAX_ID_LENGTH: usize = 160;
const MAX_TEXT_LENGTH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RoleDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Role definition version must be positive")]
    InvalidVersion,
    #[error("Role Assignment revision must be positive")]
    InvalidRevision,
    #[error("Role Assignment validity interval is invalid")]
    InvalidValidity,
    #[error("Role Assignment identity references must be distinct")]
    IdentityCollision,
    #[error("Invalid Role Assignment lifecycle transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: RoleAssignmentLifecycle,
        to: RoleAssignmentLifecycle,
    },
    #[error("Role Assignment revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, RoleDomainError> {
                Ok(Self(identifier($field, value.into())?))
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

typed_id!(RoleId, "Role ID");
typed_id!(RoleAssignmentId, "Role Assignment ID");

fn identifier(field: &'static str, value: impl Into<String>) -> Result<String, RoleDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(RoleDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(RoleDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(RoleDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn text(field: &'static str, value: impl Into<String>) -> Result<String, RoleDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(RoleDomainError::Empty { field });
    }
    if value.chars().count() > MAX_TEXT_LENGTH {
        return Err(RoleDomainError::TooLong {
            field,
            max: MAX_TEXT_LENGTH,
        });
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDefinition {
    id: RoleId,
    version: u16,
    display_name: String,
    responsibility: String,
    capability_requirements: Vec<CapabilityRequirement>,
    recommended_permission_request_refs: Vec<String>,
}

impl RoleDefinition {
    pub fn new(
        id: RoleId,
        version: u16,
        display_name: impl Into<String>,
        responsibility: impl Into<String>,
        capability_requirements: Vec<CapabilityRequirement>,
        recommended_permission_request_refs: Vec<String>,
    ) -> Result<Self, RoleDomainError> {
        if version == 0 {
            return Err(RoleDomainError::InvalidVersion);
        }
        Ok(Self {
            id,
            version,
            display_name: text("Role display name", display_name)?,
            responsibility: text("Role responsibility", responsibility)?,
            capability_requirements,
            recommended_permission_request_refs: recommended_permission_request_refs
                .into_iter()
                .map(|value| identifier("Recommended Permission Request reference", value))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub fn id(&self) -> &RoleId {
        &self.id
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn responsibility(&self) -> &str {
        &self.responsibility
    }
    pub fn capability_requirements(&self) -> &[CapabilityRequirement] {
        &self.capability_requirements
    }
    pub fn recommended_permission_request_refs(&self) -> &[String] {
        &self.recommended_permission_request_refs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleAssignmentScopeKind {
    Team,
    Repository,
    Workflow,
    WorkflowStep,
    Task,
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignmentScope {
    kind: RoleAssignmentScopeKind,
    reference: String,
}

impl RoleAssignmentScope {
    pub fn new(
        kind: RoleAssignmentScopeKind,
        reference: impl Into<String>,
    ) -> Result<Self, RoleDomainError> {
        Ok(Self {
            kind,
            reference: identifier("Role Assignment scope reference", reference)?,
        })
    }

    pub fn kind(&self) -> RoleAssignmentScopeKind {
        self.kind
    }
    pub fn reference(&self) -> &str {
        &self.reference
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleAssignmentLifecycle {
    Draft,
    Active,
    Suspended,
    Ended,
}

impl RoleAssignmentLifecycle {
    pub fn can_transition_to(self, target: Self) -> bool {
        use RoleAssignmentLifecycle::*;
        matches!(
            (self, target),
            (Draft, Active | Ended) | (Active, Suspended | Ended) | (Suspended, Active | Ended)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignment {
    id: RoleAssignmentId,
    agent_id: String,
    membership_ref: String,
    role_id: RoleId,
    role_version: u16,
    scope: RoleAssignmentScope,
    additional_capability_requirements: Vec<CapabilityRequirement>,
    permission_constraint_policy_refs: Vec<String>,
    provenance_ref: String,
    lifecycle: RoleAssignmentLifecycle,
    valid_from: i64,
    valid_until: Option<i64>,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl RoleAssignment {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: RoleAssignmentId,
        agent_id: impl Into<String>,
        membership_ref: impl Into<String>,
        role_id: RoleId,
        role_version: u16,
        scope: RoleAssignmentScope,
        additional_capability_requirements: Vec<CapabilityRequirement>,
        permission_constraint_policy_refs: Vec<String>,
        provenance_ref: impl Into<String>,
        valid_from: i64,
        valid_until: Option<i64>,
        created_at: i64,
    ) -> Result<Self, RoleDomainError> {
        if role_version == 0 {
            return Err(RoleDomainError::InvalidVersion);
        }
        if valid_from < created_at
            || created_at < 0
            || valid_until.is_some_and(|until| until < valid_from)
        {
            return Err(RoleDomainError::InvalidValidity);
        }
        let assignment = Self {
            id,
            agent_id: identifier("Agent ID", agent_id)?,
            membership_ref: identifier("Team Membership reference", membership_ref)?,
            role_id,
            role_version,
            scope,
            additional_capability_requirements,
            permission_constraint_policy_refs: permission_constraint_policy_refs
                .into_iter()
                .map(|value| identifier("Permission constraint policy reference", value))
                .collect::<Result<Vec<_>, _>>()?,
            provenance_ref: identifier("Role Assignment provenance reference", provenance_ref)?,
            lifecycle: RoleAssignmentLifecycle::Draft,
            valid_from,
            valid_until,
            revision: 1,
            created_at,
            updated_at: created_at,
        };
        assignment.validate()?;
        Ok(assignment)
    }

    pub fn id(&self) -> &RoleAssignmentId {
        &self.id
    }
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn membership_ref(&self) -> &str {
        &self.membership_ref
    }
    pub fn role_id(&self) -> &RoleId {
        &self.role_id
    }
    pub fn role_version(&self) -> u16 {
        self.role_version
    }
    pub fn scope(&self) -> &RoleAssignmentScope {
        &self.scope
    }
    pub fn additional_capability_requirements(&self) -> &[CapabilityRequirement] {
        &self.additional_capability_requirements
    }
    pub fn permission_constraint_policy_refs(&self) -> &[String] {
        &self.permission_constraint_policy_refs
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }
    pub fn lifecycle(&self) -> RoleAssignmentLifecycle {
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
        self.lifecycle == RoleAssignmentLifecycle::Active
            && at >= self.valid_from
            && self.valid_until.is_none_or(|until| at <= until)
    }

    pub fn transition_to(
        &self,
        target: RoleAssignmentLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, RoleDomainError> {
        if self.revision != expected_revision {
            return Err(RoleDomainError::RevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if self.lifecycle == target {
            return Ok(self.clone());
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(RoleDomainError::InvalidTransition {
                from: self.lifecycle,
                to: target,
            });
        }
        if updated_at < self.created_at {
            return Err(RoleDomainError::InvalidValidity);
        }
        let mut updated = self.clone();
        updated.lifecycle = target;
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }

    pub fn validate(&self) -> Result<(), RoleDomainError> {
        if self.id.as_str() == self.agent_id
            || self.id.as_str() == self.membership_ref
            || self.id.as_str() == self.role_id.as_str()
            || self.agent_id == self.membership_ref
            || self.agent_id == self.role_id.as_str()
            || self.membership_ref == self.role_id.as_str()
        {
            return Err(RoleDomainError::IdentityCollision);
        }
        if self.revision == 0 {
            return Err(RoleDomainError::InvalidRevision);
        }
        if self.valid_from < self.created_at
            || self.created_at < 0
            || self.updated_at < self.created_at
            || self
                .valid_until
                .is_some_and(|until| until < self.valid_from)
        {
            return Err(RoleDomainError::InvalidValidity);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment() -> RoleAssignment {
        RoleAssignment::new(
            RoleAssignmentId::new("assignment:one").unwrap(),
            "agent:one",
            "membership:one",
            RoleId::new("role:reviewer").unwrap(),
            1,
            RoleAssignmentScope::new(RoleAssignmentScopeKind::Review, "review:one").unwrap(),
            Vec::new(),
            vec!["policy:review-constraints".into()],
            "provenance:owner",
            10,
            Some(30),
            5,
        )
        .unwrap()
    }

    #[test]
    fn role_assignment_is_not_effective_until_explicitly_activated() {
        let draft = assignment();
        assert!(!draft.is_effective(15));
        let active = draft
            .transition_to(RoleAssignmentLifecycle::Active, 1, 10)
            .unwrap();
        assert!(active.is_effective(15));
        assert!(!active.is_effective(31));
    }

    #[test]
    fn role_definition_recommends_requests_but_contains_no_grant() {
        let role = RoleDefinition::new(
            RoleId::new("role:reviewer").unwrap(),
            1,
            "Reviewer",
            "Review bounded evidence",
            Vec::new(),
            vec!["permission-request:read-repository".into()],
        )
        .unwrap();

        assert_eq!(role.recommended_permission_request_refs().len(), 1);
        assert_eq!(role.responsibility(), "Review bounded evidence");
    }
}
