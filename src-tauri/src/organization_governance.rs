//! Independent organization governance identity, ownership bindings, and
//! immutable boundary evidence.
//!
//! Organization scopes enterprise governance. It does not replace Team, enter
//! Agent identity, or grant Permission. Bindings are contextual ownership
//! evidence only.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    permission_domain::{PermissionPolicyId, PermissionPolicyVersionRef},
    permission_policy_operations::{PermissionPolicyRecordId, PermissionPolicyScopeBindingId},
    team_domain::{TeamId, TeamMembershipId},
};

const MAX_ID_LENGTH: usize = 192;
const MAX_REFERENCE_LENGTH: usize = 256;
const MAX_NAME_LENGTH: usize = 256;
const MAX_PURPOSE_LENGTH: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OrganizationGovernanceDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} is not a valid bounded opaque reference")]
    InvalidReference { field: &'static str },
    #[error("{field} must start with {prefix}")]
    InvalidPrefix {
        field: &'static str,
        prefix: &'static str,
    },
    #[error("Organization governance revision must be positive")]
    InvalidRevision,
    #[error("Organization governance revision is stale: expected {expected}, current {current}")]
    StaleRevision { expected: u64, current: u64 },
    #[error("Organization governance revision is exhausted")]
    RevisionExhausted,
    #[error("Organization lifecycle transition is invalid")]
    InvalidOrganizationLifecycle,
    #[error("Organization binding lifecycle transition is invalid")]
    InvalidBindingLifecycle,
    #[error("Organization governance timestamp ordering is invalid")]
    InvalidTimestamp,
    #[error("Organization binding validity interval is invalid")]
    InvalidValidity,
    #[error("Organization policy target is inconsistent")]
    InvalidPolicyTarget,
    #[error("Organization boundary references are inconsistent")]
    InvalidBoundaryReferences,
    #[error("Accepted Organization boundary evidence is incomplete")]
    IncompleteAcceptedEvidence,
}

macro_rules! typed_id {
    ($name:ident, $field:literal, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(
                value: impl Into<String>,
            ) -> Result<Self, OrganizationGovernanceDomainError> {
                let value = opaque_reference($field, value)?;
                if value.chars().count() > MAX_ID_LENGTH {
                    return Err(OrganizationGovernanceDomainError::TooLong {
                        field: $field,
                        max: MAX_ID_LENGTH,
                    });
                }
                if value.contains('/') {
                    return Err(OrganizationGovernanceDomainError::InvalidReference {
                        field: $field,
                    });
                }
                if !value.starts_with($prefix) {
                    return Err(OrganizationGovernanceDomainError::InvalidPrefix {
                        field: $field,
                        prefix: $prefix,
                    });
                }
                Ok(Self(value))
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

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

typed_id!(OrganizationId, "Organization ID", "organization:");
typed_id!(
    OrganizationTeamBindingId,
    "Organization Team binding ID",
    "organization-team-binding:"
);
typed_id!(
    OrganizationPolicyBindingId,
    "Organization policy binding ID",
    "organization-policy-binding:"
);
typed_id!(
    OrganizationBoundaryEvidenceId,
    "Organization boundary evidence ID",
    "organization-boundary:"
);

fn opaque_reference(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, OrganizationGovernanceDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(OrganizationGovernanceDomainError::Empty { field });
    }
    if value.chars().count() > MAX_REFERENCE_LENGTH {
        return Err(OrganizationGovernanceDomainError::TooLong {
            field,
            max: MAX_REFERENCE_LENGTH,
        });
    }
    let Some((prefix, suffix)) = value.split_once(':') else {
        return Err(OrganizationGovernanceDomainError::InvalidReference { field });
    };
    let normalized = value.to_ascii_lowercase();
    if prefix.is_empty()
        || suffix.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':' | '.' | '/')
        })
        || [
            "apikey",
            "api_key",
            "bearer",
            "credential",
            "filecontent",
            "memorycontent",
            "modeloutput",
            "password",
            "promptcontent",
            "providersecret",
            "refreshtoken",
            "secret",
            "sk-",
        ]
        .iter()
        .any(|forbidden| normalized.contains(forbidden))
    {
        return Err(OrganizationGovernanceDomainError::InvalidReference { field });
    }
    Ok(value.to_string())
}

fn bounded_text(
    field: &'static str,
    value: impl Into<String>,
    max: usize,
) -> Result<String, OrganizationGovernanceDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(OrganizationGovernanceDomainError::Empty { field });
    }
    if value.chars().count() > max {
        return Err(OrganizationGovernanceDomainError::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(OrganizationGovernanceDomainError::InvalidReference { field });
    }
    Ok(value.to_string())
}

fn next_revision(current: u64) -> Result<u64, OrganizationGovernanceDomainError> {
    current
        .checked_add(1)
        .ok_or(OrganizationGovernanceDomainError::RevisionExhausted)
}

fn require_revision(current: u64, expected: u64) -> Result<(), OrganizationGovernanceDomainError> {
    if current != expected {
        return Err(OrganizationGovernanceDomainError::StaleRevision { expected, current });
    }
    Ok(())
}

fn validate_team_id(team_id: &TeamId) -> Result<(), OrganizationGovernanceDomainError> {
    let reconstructed = TeamId::new(team_id.as_str())
        .map_err(|_| OrganizationGovernanceDomainError::InvalidReference { field: "Team ID" })?;
    if &reconstructed != team_id {
        return Err(OrganizationGovernanceDomainError::InvalidReference { field: "Team ID" });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationLifecycle {
    Draft,
    Active,
    Suspended,
    Archived,
}

impl OrganizationLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Archived => "archived",
        }
    }

    pub fn can_transition_to(self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Draft, Self::Active | Self::Archived)
                | (Self::Active, Self::Suspended | Self::Archived)
                | (Self::Suspended, Self::Active | Self::Archived)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "OrganizationDto")]
pub struct Organization {
    id: OrganizationId,
    display_name: String,
    purpose: String,
    owner_ref: String,
    lifecycle: OrganizationLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
    archived_at: Option<i64>,
    provenance_ref: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrganizationDto {
    id: OrganizationId,
    display_name: String,
    purpose: String,
    owner_ref: String,
    lifecycle: OrganizationLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
    archived_at: Option<i64>,
    provenance_ref: String,
}

impl TryFrom<OrganizationDto> for Organization {
    type Error = OrganizationGovernanceDomainError;

    fn try_from(value: OrganizationDto) -> Result<Self, Self::Error> {
        let organization = Self {
            id: value.id,
            display_name: value.display_name,
            purpose: value.purpose,
            owner_ref: value.owner_ref,
            lifecycle: value.lifecycle,
            revision: value.revision,
            created_at: value.created_at,
            updated_at: value.updated_at,
            archived_at: value.archived_at,
            provenance_ref: value.provenance_ref,
        };
        organization.validate()?;
        Ok(organization)
    }
}

impl Organization {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: OrganizationId,
        display_name: impl Into<String>,
        purpose: impl Into<String>,
        owner_ref: impl Into<String>,
        provenance_ref: impl Into<String>,
        created_at: i64,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        let organization = Self {
            id,
            display_name: bounded_text("Organization display name", display_name, MAX_NAME_LENGTH)?,
            purpose: bounded_text("Organization purpose", purpose, MAX_PURPOSE_LENGTH)?,
            owner_ref: opaque_reference("Organization owner reference", owner_ref)?,
            lifecycle: OrganizationLifecycle::Draft,
            revision: 1,
            created_at,
            updated_at: created_at,
            archived_at: None,
            provenance_ref: opaque_reference("Organization provenance reference", provenance_ref)?,
        };
        organization.validate()?;
        Ok(organization)
    }

    pub fn transition_to(
        &self,
        target: OrganizationLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        require_revision(self.revision, expected_revision)?;
        if self.lifecycle == OrganizationLifecycle::Archived
            || !self.lifecycle.can_transition_to(target)
            || updated_at < self.updated_at
        {
            return Err(OrganizationGovernanceDomainError::InvalidOrganizationLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = target;
        next.revision = next_revision(self.revision)?;
        next.updated_at = updated_at;
        next.archived_at = (target == OrganizationLifecycle::Archived).then_some(updated_at);
        next.validate()?;
        Ok(next)
    }

    pub fn id(&self) -> &OrganizationId {
        &self.id
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn purpose(&self) -> &str {
        &self.purpose
    }
    pub fn owner_ref(&self) -> &str {
        &self.owner_ref
    }
    pub fn lifecycle(&self) -> OrganizationLifecycle {
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
    pub fn archived_at(&self) -> Option<i64> {
        self.archived_at
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }

    pub fn validate(&self) -> Result<(), OrganizationGovernanceDomainError> {
        OrganizationId::new(self.id.as_str())?;
        bounded_text(
            "Organization display name",
            self.display_name.clone(),
            MAX_NAME_LENGTH,
        )?;
        bounded_text(
            "Organization purpose",
            self.purpose.clone(),
            MAX_PURPOSE_LENGTH,
        )?;
        opaque_reference("Organization owner reference", self.owner_ref.clone())?;
        opaque_reference(
            "Organization provenance reference",
            self.provenance_ref.clone(),
        )?;
        if self.revision == 0
            || self.created_at < 0
            || self.updated_at < self.created_at
            || (self.lifecycle == OrganizationLifecycle::Draft
                && (self.revision != 1 || self.updated_at != self.created_at))
            || (self.lifecycle == OrganizationLifecycle::Archived
                && self.archived_at != Some(self.updated_at))
            || (self.lifecycle != OrganizationLifecycle::Archived && self.archived_at.is_some())
        {
            return Err(OrganizationGovernanceDomainError::InvalidTimestamp);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationBindingLifecycle {
    Draft,
    Active,
    Ended,
}

impl OrganizationBindingLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Ended => "ended",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "OrganizationTeamBindingDto")]
pub struct OrganizationTeamBinding {
    id: OrganizationTeamBindingId,
    organization_id: OrganizationId,
    team_id: TeamId,
    lifecycle: OrganizationBindingLifecycle,
    revision: u64,
    valid_from: i64,
    valid_until: Option<i64>,
    created_at: i64,
    updated_at: i64,
    activated_at: Option<i64>,
    ended_at: Option<i64>,
    provenance_ref: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrganizationTeamBindingDto {
    id: OrganizationTeamBindingId,
    organization_id: OrganizationId,
    team_id: TeamId,
    lifecycle: OrganizationBindingLifecycle,
    revision: u64,
    valid_from: i64,
    valid_until: Option<i64>,
    created_at: i64,
    updated_at: i64,
    activated_at: Option<i64>,
    ended_at: Option<i64>,
    provenance_ref: String,
}

impl TryFrom<OrganizationTeamBindingDto> for OrganizationTeamBinding {
    type Error = OrganizationGovernanceDomainError;
    fn try_from(value: OrganizationTeamBindingDto) -> Result<Self, Self::Error> {
        let binding = Self {
            id: value.id,
            organization_id: value.organization_id,
            team_id: value.team_id,
            lifecycle: value.lifecycle,
            revision: value.revision,
            valid_from: value.valid_from,
            valid_until: value.valid_until,
            created_at: value.created_at,
            updated_at: value.updated_at,
            activated_at: value.activated_at,
            ended_at: value.ended_at,
            provenance_ref: value.provenance_ref,
        };
        binding.validate()?;
        Ok(binding)
    }
}

impl OrganizationTeamBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new_draft(
        id: OrganizationTeamBindingId,
        organization_id: OrganizationId,
        team_id: TeamId,
        valid_from: i64,
        valid_until: Option<i64>,
        provenance_ref: impl Into<String>,
        created_at: i64,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        let binding = Self {
            id,
            organization_id,
            team_id,
            lifecycle: OrganizationBindingLifecycle::Draft,
            revision: 1,
            valid_from,
            valid_until,
            created_at,
            updated_at: created_at,
            activated_at: None,
            ended_at: None,
            provenance_ref: opaque_reference("Team binding provenance reference", provenance_ref)?,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn activate(
        &self,
        expected_revision: u64,
        activated_at: i64,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        require_revision(self.revision, expected_revision)?;
        if self.lifecycle != OrganizationBindingLifecycle::Draft
            || activated_at < self.updated_at
            || activated_at < self.valid_from
            || self.valid_until.is_some_and(|until| activated_at > until)
        {
            return Err(OrganizationGovernanceDomainError::InvalidBindingLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = OrganizationBindingLifecycle::Active;
        next.revision = next_revision(self.revision)?;
        next.updated_at = activated_at;
        next.activated_at = Some(activated_at);
        next.validate()?;
        Ok(next)
    }

    pub fn end(
        &self,
        expected_revision: u64,
        ended_at: i64,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        require_revision(self.revision, expected_revision)?;
        if self.lifecycle != OrganizationBindingLifecycle::Active || ended_at < self.updated_at {
            return Err(OrganizationGovernanceDomainError::InvalidBindingLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = OrganizationBindingLifecycle::Ended;
        next.revision = next_revision(self.revision)?;
        next.updated_at = ended_at;
        next.ended_at = Some(ended_at);
        next.validate()?;
        Ok(next)
    }

    pub fn is_effective_at(&self, at: i64) -> bool {
        self.lifecycle == OrganizationBindingLifecycle::Active
            && at >= self.valid_from
            && self.valid_until.is_none_or(|until| at <= until)
    }

    pub fn id(&self) -> &OrganizationTeamBindingId {
        &self.id
    }
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }
    pub fn team_id(&self) -> &TeamId {
        &self.team_id
    }
    pub fn lifecycle(&self) -> OrganizationBindingLifecycle {
        self.lifecycle
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn valid_from(&self) -> i64 {
        self.valid_from
    }
    pub fn valid_until(&self) -> Option<i64> {
        self.valid_until
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }
    pub fn activated_at(&self) -> Option<i64> {
        self.activated_at
    }
    pub fn ended_at(&self) -> Option<i64> {
        self.ended_at
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }

    pub fn validate(&self) -> Result<(), OrganizationGovernanceDomainError> {
        OrganizationTeamBindingId::new(self.id.as_str())?;
        OrganizationId::new(self.organization_id.as_str())?;
        validate_team_id(&self.team_id)?;
        opaque_reference(
            "Team binding provenance reference",
            self.provenance_ref.clone(),
        )?;
        validate_binding_shape(
            self.lifecycle,
            self.revision,
            self.valid_from,
            self.valid_until,
            self.created_at,
            self.updated_at,
            self.activated_at,
            self.ended_at,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OrganizationPolicyTarget {
    PolicyRecord {
        record_id: PermissionPolicyRecordId,
        policy_ref: PermissionPolicyVersionRef,
    },
    PolicyScopeBinding {
        scope_binding_id: PermissionPolicyScopeBindingId,
        record_id: PermissionPolicyRecordId,
        policy_ref: PermissionPolicyVersionRef,
    },
}

impl OrganizationPolicyTarget {
    pub fn record_id(&self) -> &PermissionPolicyRecordId {
        match self {
            Self::PolicyRecord { record_id, .. } | Self::PolicyScopeBinding { record_id, .. } => {
                record_id
            }
        }
    }
    pub fn policy_ref(&self) -> &PermissionPolicyVersionRef {
        match self {
            Self::PolicyRecord { policy_ref, .. } | Self::PolicyScopeBinding { policy_ref, .. } => {
                policy_ref
            }
        }
    }
    pub fn scope_binding_id(&self) -> Option<&PermissionPolicyScopeBindingId> {
        match self {
            Self::PolicyRecord { .. } => None,
            Self::PolicyScopeBinding {
                scope_binding_id, ..
            } => Some(scope_binding_id),
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::PolicyRecord { .. } => "policy_record",
            Self::PolicyScopeBinding { .. } => "policy_scope_binding",
        }
    }

    pub fn validate(&self) -> Result<(), OrganizationGovernanceDomainError> {
        PermissionPolicyRecordId::new(self.record_id().as_str())
            .map_err(|_| OrganizationGovernanceDomainError::InvalidPolicyTarget)?;
        PermissionPolicyId::new(self.policy_ref().policy_id().as_str())
            .map_err(|_| OrganizationGovernanceDomainError::InvalidPolicyTarget)?;
        if self.policy_ref().version() == 0 {
            return Err(OrganizationGovernanceDomainError::InvalidPolicyTarget);
        }
        if let Some(scope_binding_id) = self.scope_binding_id() {
            PermissionPolicyScopeBindingId::new(scope_binding_id.as_str())
                .map_err(|_| OrganizationGovernanceDomainError::InvalidPolicyTarget)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "OrganizationPolicyBindingDto")]
pub struct OrganizationPolicyBinding {
    id: OrganizationPolicyBindingId,
    organization_id: OrganizationId,
    target: OrganizationPolicyTarget,
    lifecycle: OrganizationBindingLifecycle,
    revision: u64,
    valid_from: i64,
    valid_until: Option<i64>,
    created_at: i64,
    updated_at: i64,
    activated_at: Option<i64>,
    ended_at: Option<i64>,
    provenance_ref: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrganizationPolicyBindingDto {
    id: OrganizationPolicyBindingId,
    organization_id: OrganizationId,
    target: OrganizationPolicyTarget,
    lifecycle: OrganizationBindingLifecycle,
    revision: u64,
    valid_from: i64,
    valid_until: Option<i64>,
    created_at: i64,
    updated_at: i64,
    activated_at: Option<i64>,
    ended_at: Option<i64>,
    provenance_ref: String,
}

impl TryFrom<OrganizationPolicyBindingDto> for OrganizationPolicyBinding {
    type Error = OrganizationGovernanceDomainError;
    fn try_from(value: OrganizationPolicyBindingDto) -> Result<Self, Self::Error> {
        let binding = Self {
            id: value.id,
            organization_id: value.organization_id,
            target: value.target,
            lifecycle: value.lifecycle,
            revision: value.revision,
            valid_from: value.valid_from,
            valid_until: value.valid_until,
            created_at: value.created_at,
            updated_at: value.updated_at,
            activated_at: value.activated_at,
            ended_at: value.ended_at,
            provenance_ref: value.provenance_ref,
        };
        binding.validate()?;
        Ok(binding)
    }
}

impl OrganizationPolicyBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new_draft(
        id: OrganizationPolicyBindingId,
        organization_id: OrganizationId,
        target: OrganizationPolicyTarget,
        valid_from: i64,
        valid_until: Option<i64>,
        provenance_ref: impl Into<String>,
        created_at: i64,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        let binding = Self {
            id,
            organization_id,
            target,
            lifecycle: OrganizationBindingLifecycle::Draft,
            revision: 1,
            valid_from,
            valid_until,
            created_at,
            updated_at: created_at,
            activated_at: None,
            ended_at: None,
            provenance_ref: opaque_reference(
                "Organization policy binding provenance reference",
                provenance_ref,
            )?,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn activate(
        &self,
        expected_revision: u64,
        activated_at: i64,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        require_revision(self.revision, expected_revision)?;
        if self.lifecycle != OrganizationBindingLifecycle::Draft
            || activated_at < self.updated_at
            || activated_at < self.valid_from
            || self.valid_until.is_some_and(|until| activated_at > until)
        {
            return Err(OrganizationGovernanceDomainError::InvalidBindingLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = OrganizationBindingLifecycle::Active;
        next.revision = next_revision(self.revision)?;
        next.updated_at = activated_at;
        next.activated_at = Some(activated_at);
        next.validate()?;
        Ok(next)
    }

    pub fn end(
        &self,
        expected_revision: u64,
        ended_at: i64,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        require_revision(self.revision, expected_revision)?;
        if self.lifecycle != OrganizationBindingLifecycle::Active || ended_at < self.updated_at {
            return Err(OrganizationGovernanceDomainError::InvalidBindingLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = OrganizationBindingLifecycle::Ended;
        next.revision = next_revision(self.revision)?;
        next.updated_at = ended_at;
        next.ended_at = Some(ended_at);
        next.validate()?;
        Ok(next)
    }

    pub fn is_effective_at(&self, at: i64) -> bool {
        self.lifecycle == OrganizationBindingLifecycle::Active
            && at >= self.valid_from
            && self.valid_until.is_none_or(|until| at <= until)
    }

    pub fn id(&self) -> &OrganizationPolicyBindingId {
        &self.id
    }
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }
    pub fn target(&self) -> &OrganizationPolicyTarget {
        &self.target
    }
    pub fn lifecycle(&self) -> OrganizationBindingLifecycle {
        self.lifecycle
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn valid_from(&self) -> i64 {
        self.valid_from
    }
    pub fn valid_until(&self) -> Option<i64> {
        self.valid_until
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }
    pub fn activated_at(&self) -> Option<i64> {
        self.activated_at
    }
    pub fn ended_at(&self) -> Option<i64> {
        self.ended_at
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }

    pub fn validate(&self) -> Result<(), OrganizationGovernanceDomainError> {
        OrganizationPolicyBindingId::new(self.id.as_str())?;
        OrganizationId::new(self.organization_id.as_str())?;
        self.target.validate()?;
        opaque_reference(
            "Organization policy binding provenance reference",
            self.provenance_ref.clone(),
        )?;
        validate_binding_shape(
            self.lifecycle,
            self.revision,
            self.valid_from,
            self.valid_until,
            self.created_at,
            self.updated_at,
            self.activated_at,
            self.ended_at,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_binding_shape(
    lifecycle: OrganizationBindingLifecycle,
    revision: u64,
    valid_from: i64,
    valid_until: Option<i64>,
    created_at: i64,
    updated_at: i64,
    activated_at: Option<i64>,
    ended_at: Option<i64>,
) -> Result<(), OrganizationGovernanceDomainError> {
    if revision == 0
        || created_at < 0
        || valid_from < created_at
        || updated_at < created_at
        || valid_until.is_some_and(|until| until < valid_from)
    {
        return Err(OrganizationGovernanceDomainError::InvalidValidity);
    }
    let valid = match lifecycle {
        OrganizationBindingLifecycle::Draft => {
            revision == 1
                && updated_at == created_at
                && activated_at.is_none()
                && ended_at.is_none()
        }
        OrganizationBindingLifecycle::Active => {
            revision == 2
                && activated_at == Some(updated_at)
                && ended_at.is_none()
                && updated_at >= valid_from
                && valid_until.is_none_or(|until| updated_at <= until)
        }
        OrganizationBindingLifecycle::Ended => {
            revision == 3
                && activated_at.is_some_and(|activated| activated <= updated_at)
                && ended_at == Some(updated_at)
        }
    };
    if !valid {
        return Err(OrganizationGovernanceDomainError::InvalidBindingLifecycle);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    try_from = "OrganizationBoundaryReferencesDto"
)]
pub struct OrganizationBoundaryReferences {
    organization_id: OrganizationId,
    organization_revision: Option<u64>,
    team_id: Option<TeamId>,
    team_binding_id: Option<OrganizationTeamBindingId>,
    team_binding_revision: Option<u64>,
    policy_binding_id: Option<OrganizationPolicyBindingId>,
    policy_binding_revision: Option<u64>,
    membership_id: Option<TeamMembershipId>,
    membership_revision: Option<u64>,
    agent_ref: Option<String>,
    workflow_ref: Option<String>,
    execution_ref: Option<String>,
    resource_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrganizationBoundaryReferencesDto {
    organization_id: OrganizationId,
    organization_revision: Option<u64>,
    team_id: Option<TeamId>,
    team_binding_id: Option<OrganizationTeamBindingId>,
    team_binding_revision: Option<u64>,
    policy_binding_id: Option<OrganizationPolicyBindingId>,
    policy_binding_revision: Option<u64>,
    membership_id: Option<TeamMembershipId>,
    membership_revision: Option<u64>,
    agent_ref: Option<String>,
    workflow_ref: Option<String>,
    execution_ref: Option<String>,
    resource_ref: Option<String>,
}

impl TryFrom<OrganizationBoundaryReferencesDto> for OrganizationBoundaryReferences {
    type Error = OrganizationGovernanceDomainError;
    fn try_from(value: OrganizationBoundaryReferencesDto) -> Result<Self, Self::Error> {
        Self::new(
            value.organization_id,
            value.organization_revision,
            value.team_id,
            value.team_binding_id,
            value.team_binding_revision,
            value.policy_binding_id,
            value.policy_binding_revision,
            value.membership_id,
            value.membership_revision,
            value.agent_ref,
            value.workflow_ref,
            value.execution_ref,
            value.resource_ref,
        )
    }
}

impl OrganizationBoundaryReferences {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        organization_revision: Option<u64>,
        team_id: Option<TeamId>,
        team_binding_id: Option<OrganizationTeamBindingId>,
        team_binding_revision: Option<u64>,
        policy_binding_id: Option<OrganizationPolicyBindingId>,
        policy_binding_revision: Option<u64>,
        membership_id: Option<TeamMembershipId>,
        membership_revision: Option<u64>,
        agent_ref: Option<String>,
        workflow_ref: Option<String>,
        execution_ref: Option<String>,
        resource_ref: Option<String>,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        let references = Self {
            organization_id,
            organization_revision,
            team_id,
            team_binding_id,
            team_binding_revision,
            policy_binding_id,
            policy_binding_revision,
            membership_id,
            membership_revision,
            agent_ref,
            workflow_ref,
            execution_ref,
            resource_ref,
        };
        references.validate()?;
        Ok(references)
    }

    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }
    pub fn organization_revision(&self) -> Option<u64> {
        self.organization_revision
    }
    pub fn team_id(&self) -> Option<&TeamId> {
        self.team_id.as_ref()
    }
    pub fn team_binding_id(&self) -> Option<&OrganizationTeamBindingId> {
        self.team_binding_id.as_ref()
    }
    pub fn team_binding_revision(&self) -> Option<u64> {
        self.team_binding_revision
    }
    pub fn policy_binding_id(&self) -> Option<&OrganizationPolicyBindingId> {
        self.policy_binding_id.as_ref()
    }
    pub fn policy_binding_revision(&self) -> Option<u64> {
        self.policy_binding_revision
    }
    pub fn membership_id(&self) -> Option<&TeamMembershipId> {
        self.membership_id.as_ref()
    }
    pub fn membership_revision(&self) -> Option<u64> {
        self.membership_revision
    }
    pub fn agent_ref(&self) -> Option<&str> {
        self.agent_ref.as_deref()
    }
    pub fn workflow_ref(&self) -> Option<&str> {
        self.workflow_ref.as_deref()
    }
    pub fn execution_ref(&self) -> Option<&str> {
        self.execution_ref.as_deref()
    }
    pub fn resource_ref(&self) -> Option<&str> {
        self.resource_ref.as_deref()
    }

    pub fn validate(&self) -> Result<(), OrganizationGovernanceDomainError> {
        OrganizationId::new(self.organization_id.as_str())?;
        if self.organization_revision == Some(0)
            || self.team_binding_revision == Some(0)
            || self.policy_binding_revision == Some(0)
            || self.membership_revision == Some(0)
            || self.team_binding_id.is_some() != self.team_binding_revision.is_some()
            || self.policy_binding_id.is_some() != self.policy_binding_revision.is_some()
            || self.membership_id.is_some() != self.membership_revision.is_some()
            || self.team_binding_id.is_some() != self.team_id.is_some()
            || self.membership_id.is_some() && self.team_id.is_none()
            || self.membership_id.is_some() && self.agent_ref.is_none()
        {
            return Err(OrganizationGovernanceDomainError::InvalidBoundaryReferences);
        }
        if let Some(team_id) = &self.team_id {
            validate_team_id(team_id)?;
        }
        if let Some(id) = &self.team_binding_id {
            OrganizationTeamBindingId::new(id.as_str())?;
        }
        if let Some(id) = &self.policy_binding_id {
            OrganizationPolicyBindingId::new(id.as_str())?;
        }
        if let Some(id) = &self.membership_id {
            let rebuilt = TeamMembershipId::new(id.as_str()).map_err(|_| {
                OrganizationGovernanceDomainError::InvalidReference {
                    field: "Team Membership ID",
                }
            })?;
            if &rebuilt != id {
                return Err(OrganizationGovernanceDomainError::InvalidBoundaryReferences);
            }
        }
        for (field, reference) in [
            ("Agent reference", &self.agent_ref),
            ("Workflow reference", &self.workflow_ref),
            ("Execution reference", &self.execution_ref),
            ("Resource reference", &self.resource_ref),
        ] {
            if let Some(reference) = reference {
                opaque_reference(field, reference.clone())?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationBoundaryDenialReason {
    OrganizationNotFound,
    InactiveOrganization,
    TeamBindingNotFound,
    TeamBindingInactive,
    TeamOwnedByAnotherOrganization,
    PolicyBindingNotFound,
    PolicyBindingInactive,
    CrossOrganizationReference,
    MembershipNotEffective,
    StaleRevision,
    QueryScopeMismatch,
}

impl OrganizationBoundaryDenialReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OrganizationNotFound => "organization_not_found",
            Self::InactiveOrganization => "inactive_organization",
            Self::TeamBindingNotFound => "team_binding_not_found",
            Self::TeamBindingInactive => "team_binding_inactive",
            Self::TeamOwnedByAnotherOrganization => "team_owned_by_another_organization",
            Self::PolicyBindingNotFound => "policy_binding_not_found",
            Self::PolicyBindingInactive => "policy_binding_inactive",
            Self::CrossOrganizationReference => "cross_organization_reference",
            Self::MembershipNotEffective => "membership_not_effective",
            Self::StaleRevision => "stale_revision",
            Self::QueryScopeMismatch => "query_scope_mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationBoundaryOutcome {
    Accepted,
    Denied(OrganizationBoundaryDenialReason),
}

impl OrganizationBoundaryOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Denied(reason) => reason.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "OrganizationBoundaryEvidenceDto")]
pub struct OrganizationBoundaryEvidence {
    id: OrganizationBoundaryEvidenceId,
    references: OrganizationBoundaryReferences,
    outcome: OrganizationBoundaryOutcome,
    resolved_at: i64,
    provenance_ref: String,
    audit_ref: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrganizationBoundaryEvidenceDto {
    id: OrganizationBoundaryEvidenceId,
    references: OrganizationBoundaryReferences,
    outcome: OrganizationBoundaryOutcome,
    resolved_at: i64,
    provenance_ref: String,
    audit_ref: String,
}

impl TryFrom<OrganizationBoundaryEvidenceDto> for OrganizationBoundaryEvidence {
    type Error = OrganizationGovernanceDomainError;
    fn try_from(value: OrganizationBoundaryEvidenceDto) -> Result<Self, Self::Error> {
        Self::new(
            value.id,
            value.references,
            value.outcome,
            value.resolved_at,
            value.provenance_ref,
            value.audit_ref,
        )
    }
}

impl OrganizationBoundaryEvidence {
    pub fn new(
        id: OrganizationBoundaryEvidenceId,
        references: OrganizationBoundaryReferences,
        outcome: OrganizationBoundaryOutcome,
        resolved_at: i64,
        provenance_ref: impl Into<String>,
        audit_ref: impl Into<String>,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        let evidence = Self {
            id,
            references,
            outcome,
            resolved_at,
            provenance_ref: opaque_reference(
                "Organization boundary provenance reference",
                provenance_ref,
            )?,
            audit_ref: opaque_reference("Organization boundary audit reference", audit_ref)?,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn id(&self) -> &OrganizationBoundaryEvidenceId {
        &self.id
    }
    pub fn references(&self) -> &OrganizationBoundaryReferences {
        &self.references
    }
    pub fn outcome(&self) -> OrganizationBoundaryOutcome {
        self.outcome
    }
    pub fn resolved_at(&self) -> i64 {
        self.resolved_at
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }
    pub fn audit_ref(&self) -> &str {
        &self.audit_ref
    }

    pub fn validate(&self) -> Result<(), OrganizationGovernanceDomainError> {
        OrganizationBoundaryEvidenceId::new(self.id.as_str())?;
        self.references.validate()?;
        opaque_reference(
            "Organization boundary provenance reference",
            self.provenance_ref.clone(),
        )?;
        let audit_ref = opaque_reference(
            "Organization boundary audit reference",
            self.audit_ref.clone(),
        )?;
        if !audit_ref.starts_with("audit:") || self.resolved_at < 0 {
            return Err(OrganizationGovernanceDomainError::InvalidReference {
                field: "Organization boundary audit reference",
            });
        }
        if self.outcome == OrganizationBoundaryOutcome::Accepted
            && self.references.organization_revision.is_none()
        {
            return Err(OrganizationGovernanceDomainError::IncompleteAcceptedEvidence);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn organization() -> Organization {
        Organization::new(
            OrganizationId::new("organization:alpha").unwrap(),
            "Alpha",
            "Govern alpha delivery",
            "owner:alpha",
            "provenance:cod-031",
            10,
        )
        .unwrap()
    }

    #[test]
    fn organization_lifecycle_uses_expected_revision_and_archive_is_terminal() {
        let draft = organization();
        assert!(matches!(
            draft.transition_to(OrganizationLifecycle::Active, 2, 11),
            Err(OrganizationGovernanceDomainError::StaleRevision { .. })
        ));
        let active = draft
            .transition_to(OrganizationLifecycle::Active, 1, 11)
            .unwrap();
        let suspended = active
            .transition_to(OrganizationLifecycle::Suspended, 2, 12)
            .unwrap();
        let archived = suspended
            .transition_to(OrganizationLifecycle::Archived, 3, 13)
            .unwrap();
        assert_eq!(archived.archived_at(), Some(13));
        assert_eq!(archived.revision(), 4);
        assert!(archived
            .transition_to(OrganizationLifecycle::Active, 4, 14)
            .is_err());
    }

    #[test]
    fn binding_lifecycles_are_exact_and_time_ordered() {
        let draft = OrganizationTeamBinding::new_draft(
            OrganizationTeamBindingId::new("organization-team-binding:one").unwrap(),
            OrganizationId::new("organization:alpha").unwrap(),
            TeamId::new("team:one").unwrap(),
            11,
            Some(20),
            "provenance:cod-031",
            10,
        )
        .unwrap();
        assert!(draft.activate(1, 9).is_err());
        let active = draft.activate(1, 11).unwrap();
        assert!(active.is_effective_at(15));
        let ended = active.end(2, 16).unwrap();
        assert_eq!(ended.revision(), 3);
        assert!(!ended.is_effective_at(16));
    }

    #[test]
    fn exact_policy_target_round_trips_without_creating_authority() {
        let target = OrganizationPolicyTarget::PolicyRecord {
            record_id: PermissionPolicyRecordId::new("policy-record:one").unwrap(),
            policy_ref: serde_json::from_value(serde_json::json!({
                "policyId": "permission-policy:one",
                "version": 2,
                "layer": "team"
            }))
            .unwrap(),
        };
        let binding = OrganizationPolicyBinding::new_draft(
            OrganizationPolicyBindingId::new("organization-policy-binding:one").unwrap(),
            OrganizationId::new("organization:alpha").unwrap(),
            target,
            10,
            None,
            "provenance:cod-031",
            10,
        )
        .unwrap();
        let encoded = serde_json::to_string(&binding).unwrap();
        let decoded: OrganizationPolicyBinding = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, binding);
        assert_eq!(binding.target().kind(), "policy_record");
    }

    #[test]
    fn persisted_shapes_reject_unknown_fields_and_invalid_ids() {
        let value = serde_json::to_value(organization()).unwrap();
        let mut unknown = value.clone();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("credential".into(), serde_json::json!("never"));
        assert!(serde_json::from_value::<Organization>(unknown).is_err());
        let mut invalid_id = value;
        invalid_id["id"] = serde_json::json!("team:not-an-organization");
        assert!(serde_json::from_value::<Organization>(invalid_id).is_err());
    }

    #[test]
    fn accepted_boundary_requires_exact_organization_revision() {
        let references = OrganizationBoundaryReferences::new(
            OrganizationId::new("organization:alpha").unwrap(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("workflow:one".into()),
            None,
            None,
        )
        .unwrap();
        assert!(OrganizationBoundaryEvidence::new(
            OrganizationBoundaryEvidenceId::new("organization-boundary:one").unwrap(),
            references,
            OrganizationBoundaryOutcome::Accepted,
            20,
            "provenance:cod-031",
            "audit:event-one",
        )
        .is_err());
    }
}
