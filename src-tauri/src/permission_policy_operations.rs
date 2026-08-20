//! Operational lifecycle and explicit scope selection around the existing
//! deny-by-default Permission policy domain.
//!
//! This module does not define authority, Authorization Decisions, or Grants.
//! It only manages immutable `PermissionPolicy` versions and records the exact
//! versions selected for an existing evaluator.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::permission_domain::{
    PermissionAction, PermissionDomainError, PermissionPolicy, PermissionPolicyId,
    PermissionPolicyLayer, PermissionPolicyVersionRef, PermissionRule,
};

const MAX_ID_LENGTH: usize = 192;
const MAX_REFERENCE_LENGTH: usize = 256;
const MAX_SELECTION_SCOPES: usize = 64;
const MAX_SELECTION_POLICIES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PermissionPolicyOperationsDomainError {
    #[error(transparent)]
    Permission(#[from] PermissionDomainError),
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} is not a valid opaque reference")]
    InvalidReference { field: &'static str },
    #[error("{field} must start with {prefix}")]
    InvalidPrefix {
        field: &'static str,
        prefix: &'static str,
    },
    #[error("Operational revision must be positive")]
    InvalidRevision,
    #[error("Operational revision is stale: expected {expected}, current {current}")]
    StaleRevision { expected: u64, current: u64 },
    #[error("Operational revision is exhausted")]
    RevisionExhausted,
    #[error("Policy operational lifecycle transition is invalid")]
    InvalidPolicyLifecycle,
    #[error("Policy scope binding lifecycle transition is invalid")]
    InvalidBindingLifecycle,
    #[error("Policy operational timestamps are invalid")]
    InvalidTimestamp,
    #[error("Policy scope binding validity interval is invalid")]
    InvalidValidity,
    #[error("Replacement policy reference is invalid")]
    InvalidReplacement,
    #[error("Policy version reference does not match the immutable policy definition")]
    PolicyReferenceMismatch,
    #[error("Permission policy definition failed reconstruction validation")]
    InvalidPolicyDefinition,
    #[error("Policy selection must contain at least one explicit scope")]
    EmptySelectionScopes,
    #[error("Policy selection contains duplicate scope evidence")]
    DuplicateSelectionScope,
    #[error("Policy selection exceeds its bounded evidence limit")]
    SelectionLimitExceeded,
    #[error("Accepted selection must contain exact policy versions and denied selection must not")]
    InvalidSelectionOutcome,
}

macro_rules! typed_id {
    ($name:ident, $field:literal, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(
                value: impl Into<String>,
            ) -> Result<Self, PermissionPolicyOperationsDomainError> {
                let value = opaque_reference($field, value)?;
                if value.chars().count() > MAX_ID_LENGTH {
                    return Err(PermissionPolicyOperationsDomainError::TooLong {
                        field: $field,
                        max: MAX_ID_LENGTH,
                    });
                }
                if value.contains('/') {
                    return Err(PermissionPolicyOperationsDomainError::InvalidReference {
                        field: $field,
                    });
                }
                if !value.starts_with($prefix) {
                    return Err(PermissionPolicyOperationsDomainError::InvalidPrefix {
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
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

typed_id!(
    PermissionPolicyRecordId,
    "Permission policy record ID",
    "policy-record:"
);
typed_id!(
    PermissionPolicyScopeBindingId,
    "Permission policy scope binding ID",
    "policy-binding:"
);
typed_id!(
    PermissionPolicySelectionEvidenceId,
    "Permission policy selection evidence ID",
    "policy-selection:"
);

fn opaque_reference(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, PermissionPolicyOperationsDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(PermissionPolicyOperationsDomainError::Empty { field });
    }
    if value.chars().count() > MAX_REFERENCE_LENGTH {
        return Err(PermissionPolicyOperationsDomainError::TooLong {
            field,
            max: MAX_REFERENCE_LENGTH,
        });
    }
    let Some((prefix, suffix)) = value.split_once(':') else {
        return Err(PermissionPolicyOperationsDomainError::InvalidReference { field });
    };
    let allowed = |character: char| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':' | '.' | '/')
    };
    let normalized = value.to_ascii_lowercase();
    if prefix.is_empty()
        || suffix.is_empty()
        || !value.chars().all(allowed)
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
            "refreshtoken",
            "secret",
            "sk-",
        ]
        .iter()
        .any(|forbidden| normalized.contains(forbidden))
    {
        return Err(PermissionPolicyOperationsDomainError::InvalidReference { field });
    }
    Ok(value.to_string())
}

fn validate_policy(policy: &PermissionPolicy) -> Result<(), PermissionPolicyOperationsDomainError> {
    let rules = policy
        .rules()
        .iter()
        .map(|rule| {
            PermissionRule::new(
                rule.effect(),
                PermissionAction::new(rule.action().as_str())?,
                rule.resource_selector(),
                rule.constraints().clone(),
            )
        })
        .collect::<Result<Vec<_>, PermissionDomainError>>()?;
    let reconstructed = PermissionPolicy::new(
        PermissionPolicyId::new(policy.id().as_str())?,
        policy.version(),
        policy.layer(),
        policy.owner_ref(),
        rules,
    )?;
    if &reconstructed != policy {
        return Err(PermissionPolicyOperationsDomainError::InvalidPolicyDefinition);
    }
    Ok(())
}

pub fn permission_policy_layer_precedence(layer: PermissionPolicyLayer) -> u8 {
    match layer {
        PermissionPolicyLayer::Repository => 0,
        PermissionPolicyLayer::HumanOwner => 1,
        PermissionPolicyLayer::Team => 2,
        PermissionPolicyLayer::Workflow => 3,
        PermissionPolicyLayer::RoleAssignment => 4,
        PermissionPolicyLayer::Workspace => 5,
        PermissionPolicyLayer::Environment => 6,
    }
}

fn validate_policy_reference(
    reference: &PermissionPolicyVersionRef,
) -> Result<(), PermissionPolicyOperationsDomainError> {
    let policy_id = PermissionPolicyId::new(reference.policy_id().as_str())?;
    if &policy_id != reference.policy_id() {
        return Err(PermissionPolicyOperationsDomainError::PolicyReferenceMismatch);
    }
    if reference.version() == 0 {
        return Err(PermissionDomainError::InvalidVersion.into());
    }
    Ok(())
}

fn next_revision(current: u64) -> Result<u64, PermissionPolicyOperationsDomainError> {
    current
        .checked_add(1)
        .ok_or(PermissionPolicyOperationsDomainError::RevisionExhausted)
}

fn require_expected_revision(
    current: u64,
    expected: u64,
) -> Result<(), PermissionPolicyOperationsDomainError> {
    if current != expected {
        return Err(PermissionPolicyOperationsDomainError::StaleRevision { expected, current });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicyRecordLifecycle {
    Draft,
    Published,
    Retired,
}

impl PermissionPolicyRecordLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
            Self::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "PermissionPolicyRecordDto")]
pub struct PermissionPolicyRecord {
    id: PermissionPolicyRecordId,
    policy: PermissionPolicy,
    lifecycle: PermissionPolicyRecordLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
    published_at: Option<i64>,
    retired_at: Option<i64>,
    provenance_ref: String,
    replaces: Option<PermissionPolicyVersionRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PermissionPolicyRecordDto {
    id: PermissionPolicyRecordId,
    policy: PermissionPolicy,
    lifecycle: PermissionPolicyRecordLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
    published_at: Option<i64>,
    retired_at: Option<i64>,
    provenance_ref: String,
    replaces: Option<PermissionPolicyVersionRef>,
}

impl TryFrom<PermissionPolicyRecordDto> for PermissionPolicyRecord {
    type Error = PermissionPolicyOperationsDomainError;

    fn try_from(value: PermissionPolicyRecordDto) -> Result<Self, Self::Error> {
        let record = Self {
            id: value.id,
            policy: value.policy,
            lifecycle: value.lifecycle,
            revision: value.revision,
            created_at: value.created_at,
            updated_at: value.updated_at,
            published_at: value.published_at,
            retired_at: value.retired_at,
            provenance_ref: value.provenance_ref,
            replaces: value.replaces,
        };
        record.validate()?;
        Ok(record)
    }
}

impl PermissionPolicyRecord {
    pub fn new_draft(
        id: PermissionPolicyRecordId,
        policy: PermissionPolicy,
        provenance_ref: impl Into<String>,
        replaces: Option<PermissionPolicyVersionRef>,
        created_at: i64,
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        let record = Self {
            id,
            policy,
            lifecycle: PermissionPolicyRecordLifecycle::Draft,
            revision: 1,
            created_at,
            updated_at: created_at,
            published_at: None,
            retired_at: None,
            provenance_ref: opaque_reference("Policy provenance reference", provenance_ref)?,
            replaces,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn publish(
        &self,
        expected_revision: u64,
        published_at: i64,
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        require_expected_revision(self.revision, expected_revision)?;
        if self.lifecycle != PermissionPolicyRecordLifecycle::Draft
            || published_at < self.updated_at
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidPolicyLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = PermissionPolicyRecordLifecycle::Published;
        next.revision = next_revision(self.revision)?;
        next.updated_at = published_at;
        next.published_at = Some(published_at);
        next.validate()?;
        Ok(next)
    }

    pub fn retire(
        &self,
        expected_revision: u64,
        retired_at: i64,
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        require_expected_revision(self.revision, expected_revision)?;
        if self.lifecycle != PermissionPolicyRecordLifecycle::Published
            || retired_at < self.updated_at
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidPolicyLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = PermissionPolicyRecordLifecycle::Retired;
        next.revision = next_revision(self.revision)?;
        next.updated_at = retired_at;
        next.retired_at = Some(retired_at);
        next.validate()?;
        Ok(next)
    }

    pub fn id(&self) -> &PermissionPolicyRecordId {
        &self.id
    }
    pub fn policy(&self) -> &PermissionPolicy {
        &self.policy
    }
    pub fn policy_ref(&self) -> PermissionPolicyVersionRef {
        PermissionPolicyVersionRef::from_policy(&self.policy)
    }
    pub fn lifecycle(&self) -> PermissionPolicyRecordLifecycle {
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
    pub fn published_at(&self) -> Option<i64> {
        self.published_at
    }
    pub fn retired_at(&self) -> Option<i64> {
        self.retired_at
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }
    pub fn replaces(&self) -> Option<&PermissionPolicyVersionRef> {
        self.replaces.as_ref()
    }

    pub fn validate(&self) -> Result<(), PermissionPolicyOperationsDomainError> {
        PermissionPolicyRecordId::new(self.id.as_str())?;
        validate_policy(&self.policy)?;
        if opaque_reference("Policy provenance reference", &self.provenance_ref)?
            != self.provenance_ref
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidReference {
                field: "Policy provenance reference",
            });
        }
        if self.revision == 0 || self.created_at < 0 || self.updated_at < self.created_at {
            return Err(PermissionPolicyOperationsDomainError::InvalidTimestamp);
        }
        if let Some(replaces) = &self.replaces {
            validate_policy_reference(replaces)?;
            if replaces.policy_id() != self.policy.id()
                || replaces.layer() != self.policy.layer()
                || replaces.version() >= self.policy.version()
            {
                return Err(PermissionPolicyOperationsDomainError::InvalidReplacement);
            }
        }
        match self.lifecycle {
            PermissionPolicyRecordLifecycle::Draft => {
                if self.revision != 1
                    || self.updated_at != self.created_at
                    || self.published_at.is_some()
                    || self.retired_at.is_some()
                {
                    return Err(PermissionPolicyOperationsDomainError::InvalidPolicyLifecycle);
                }
            }
            PermissionPolicyRecordLifecycle::Published => {
                if self.revision != 2
                    || self.published_at != Some(self.updated_at)
                    || self.published_at.is_some_and(|at| at < self.created_at)
                    || self.retired_at.is_some()
                {
                    return Err(PermissionPolicyOperationsDomainError::InvalidPolicyLifecycle);
                }
            }
            PermissionPolicyRecordLifecycle::Retired => {
                let Some(published_at) = self.published_at else {
                    return Err(PermissionPolicyOperationsDomainError::InvalidPolicyLifecycle);
                };
                if self.revision != 3
                    || self.retired_at != Some(self.updated_at)
                    || published_at < self.created_at
                    || published_at > self.updated_at
                {
                    return Err(PermissionPolicyOperationsDomainError::InvalidPolicyLifecycle);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicyScopeKind {
    Agent,
    Environment,
    Organization,
    Repository,
    Team,
    Workflow,
    Workspace,
}

impl PermissionPolicyScopeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Environment => "environment",
            Self::Organization => "organization",
            Self::Repository => "repository",
            Self::Team => "team",
            Self::Workflow => "workflow",
            Self::Workspace => "workspace",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    try_from = "PermissionPolicyScopeEvidenceDto"
)]
pub struct PermissionPolicyScopeEvidence {
    scope_kind: PermissionPolicyScopeKind,
    scope_ref: String,
    boundary_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PermissionPolicyScopeEvidenceDto {
    scope_kind: PermissionPolicyScopeKind,
    scope_ref: String,
    boundary_ref: Option<String>,
}

impl TryFrom<PermissionPolicyScopeEvidenceDto> for PermissionPolicyScopeEvidence {
    type Error = PermissionPolicyOperationsDomainError;

    fn try_from(value: PermissionPolicyScopeEvidenceDto) -> Result<Self, Self::Error> {
        Self::new(value.scope_kind, value.scope_ref, value.boundary_ref)
    }
}

impl PermissionPolicyScopeEvidence {
    pub fn new(
        scope_kind: PermissionPolicyScopeKind,
        scope_ref: impl Into<String>,
        boundary_ref: Option<String>,
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        Ok(Self {
            scope_kind,
            scope_ref: opaque_reference("Permission policy scope reference", scope_ref)?,
            boundary_ref: boundary_ref
                .map(|reference| {
                    opaque_reference("Permission policy boundary reference", reference)
                })
                .transpose()?,
        })
    }

    pub fn scope_kind(&self) -> PermissionPolicyScopeKind {
        self.scope_kind
    }
    pub fn scope_ref(&self) -> &str {
        &self.scope_ref
    }
    pub fn boundary_ref(&self) -> Option<&str> {
        self.boundary_ref.as_deref()
    }

    pub fn validate(&self) -> Result<(), PermissionPolicyOperationsDomainError> {
        if opaque_reference("Permission policy scope reference", &self.scope_ref)? != self.scope_ref
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidReference {
                field: "Permission policy scope reference",
            });
        }
        if let Some(reference) = &self.boundary_ref {
            if opaque_reference("Permission policy boundary reference", reference)? != *reference {
                return Err(PermissionPolicyOperationsDomainError::InvalidReference {
                    field: "Permission policy boundary reference",
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    try_from = "PermissionPolicyScopeSelectorDto"
)]
pub struct PermissionPolicyScopeSelector {
    layer: PermissionPolicyLayer,
    scope: PermissionPolicyScopeEvidence,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PermissionPolicyScopeSelectorDto {
    layer: PermissionPolicyLayer,
    scope: PermissionPolicyScopeEvidence,
}

impl TryFrom<PermissionPolicyScopeSelectorDto> for PermissionPolicyScopeSelector {
    type Error = PermissionPolicyOperationsDomainError;

    fn try_from(value: PermissionPolicyScopeSelectorDto) -> Result<Self, Self::Error> {
        Self::new(value.layer, value.scope)
    }
}

impl PermissionPolicyScopeSelector {
    pub fn new(
        layer: PermissionPolicyLayer,
        scope: PermissionPolicyScopeEvidence,
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        scope.validate()?;
        Ok(Self { layer, scope })
    }

    pub fn layer(&self) -> PermissionPolicyLayer {
        self.layer
    }
    pub fn scope(&self) -> &PermissionPolicyScopeEvidence {
        &self.scope
    }
    pub fn scope_kind(&self) -> PermissionPolicyScopeKind {
        self.scope.scope_kind()
    }
    pub fn scope_ref(&self) -> &str {
        self.scope.scope_ref()
    }
    pub fn boundary_ref(&self) -> Option<&str> {
        self.scope.boundary_ref()
    }

    pub fn matches_scope(&self, scope: &PermissionPolicyScopeEvidence) -> bool {
        &self.scope == scope
    }

    pub fn validate(&self) -> Result<(), PermissionPolicyOperationsDomainError> {
        self.scope.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicyScopeBindingLifecycle {
    Draft,
    Active,
    Ended,
}

impl PermissionPolicyScopeBindingLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Ended => "ended",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "PermissionPolicyScopeBindingDto")]
pub struct PermissionPolicyScopeBinding {
    id: PermissionPolicyScopeBindingId,
    record_id: PermissionPolicyRecordId,
    policy_ref: PermissionPolicyVersionRef,
    selector: PermissionPolicyScopeSelector,
    lifecycle: PermissionPolicyScopeBindingLifecycle,
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
struct PermissionPolicyScopeBindingDto {
    id: PermissionPolicyScopeBindingId,
    record_id: PermissionPolicyRecordId,
    policy_ref: PermissionPolicyVersionRef,
    selector: PermissionPolicyScopeSelector,
    lifecycle: PermissionPolicyScopeBindingLifecycle,
    revision: u64,
    valid_from: i64,
    valid_until: Option<i64>,
    created_at: i64,
    updated_at: i64,
    activated_at: Option<i64>,
    ended_at: Option<i64>,
    provenance_ref: String,
}

impl TryFrom<PermissionPolicyScopeBindingDto> for PermissionPolicyScopeBinding {
    type Error = PermissionPolicyOperationsDomainError;

    fn try_from(value: PermissionPolicyScopeBindingDto) -> Result<Self, Self::Error> {
        let binding = Self {
            id: value.id,
            record_id: value.record_id,
            policy_ref: value.policy_ref,
            selector: value.selector,
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

impl PermissionPolicyScopeBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new_draft(
        id: PermissionPolicyScopeBindingId,
        record_id: PermissionPolicyRecordId,
        policy_ref: PermissionPolicyVersionRef,
        selector: PermissionPolicyScopeSelector,
        valid_from: i64,
        valid_until: Option<i64>,
        provenance_ref: impl Into<String>,
        created_at: i64,
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        let binding = Self {
            id,
            record_id,
            policy_ref,
            selector,
            lifecycle: PermissionPolicyScopeBindingLifecycle::Draft,
            revision: 1,
            valid_from,
            valid_until,
            created_at,
            updated_at: created_at,
            activated_at: None,
            ended_at: None,
            provenance_ref: opaque_reference(
                "Policy binding provenance reference",
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
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        require_expected_revision(self.revision, expected_revision)?;
        if self.lifecycle != PermissionPolicyScopeBindingLifecycle::Draft
            || activated_at < self.updated_at
            || self.valid_until.is_some_and(|until| activated_at > until)
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidBindingLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = PermissionPolicyScopeBindingLifecycle::Active;
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
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        require_expected_revision(self.revision, expected_revision)?;
        if self.lifecycle != PermissionPolicyScopeBindingLifecycle::Active
            || ended_at < self.updated_at
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidBindingLifecycle);
        }
        let mut next = self.clone();
        next.lifecycle = PermissionPolicyScopeBindingLifecycle::Ended;
        next.revision = next_revision(self.revision)?;
        next.updated_at = ended_at;
        next.ended_at = Some(ended_at);
        next.validate()?;
        Ok(next)
    }

    pub fn id(&self) -> &PermissionPolicyScopeBindingId {
        &self.id
    }
    pub fn record_id(&self) -> &PermissionPolicyRecordId {
        &self.record_id
    }
    pub fn policy_ref(&self) -> &PermissionPolicyVersionRef {
        &self.policy_ref
    }
    pub fn selector(&self) -> &PermissionPolicyScopeSelector {
        &self.selector
    }
    pub fn lifecycle(&self) -> PermissionPolicyScopeBindingLifecycle {
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
    pub fn is_effective_at(&self, at: i64) -> bool {
        self.lifecycle == PermissionPolicyScopeBindingLifecycle::Active
            && self
                .activated_at
                .is_some_and(|activated_at| at >= activated_at)
            && at >= self.valid_from
            && self.valid_until.is_none_or(|until| at <= until)
    }

    pub fn validate(&self) -> Result<(), PermissionPolicyOperationsDomainError> {
        PermissionPolicyScopeBindingId::new(self.id.as_str())?;
        PermissionPolicyRecordId::new(self.record_id.as_str())?;
        validate_policy_reference(&self.policy_ref)?;
        self.selector.validate()?;
        if opaque_reference("Policy binding provenance reference", &self.provenance_ref)?
            != self.provenance_ref
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidReference {
                field: "Policy binding provenance reference",
            });
        }
        if self.policy_ref.layer() != self.selector.layer() {
            return Err(PermissionPolicyOperationsDomainError::PolicyReferenceMismatch);
        }
        if self.revision == 0 || self.created_at < 0 || self.updated_at < self.created_at {
            return Err(PermissionPolicyOperationsDomainError::InvalidTimestamp);
        }
        if self.valid_from < 0
            || self
                .valid_until
                .is_some_and(|until| until < self.valid_from)
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidValidity);
        }
        match self.lifecycle {
            PermissionPolicyScopeBindingLifecycle::Draft => {
                if self.revision != 1
                    || self.updated_at != self.created_at
                    || self.activated_at.is_some()
                    || self.ended_at.is_some()
                {
                    return Err(PermissionPolicyOperationsDomainError::InvalidBindingLifecycle);
                }
            }
            PermissionPolicyScopeBindingLifecycle::Active => {
                let Some(activated_at) = self.activated_at else {
                    return Err(PermissionPolicyOperationsDomainError::InvalidBindingLifecycle);
                };
                if self.revision != 2
                    || self.ended_at.is_some()
                    || activated_at != self.updated_at
                    || activated_at < self.created_at
                    || self.valid_until.is_some_and(|until| activated_at > until)
                {
                    return Err(PermissionPolicyOperationsDomainError::InvalidBindingLifecycle);
                }
            }
            PermissionPolicyScopeBindingLifecycle::Ended => {
                let (Some(activated_at), Some(ended_at)) = (self.activated_at, self.ended_at)
                else {
                    return Err(PermissionPolicyOperationsDomainError::InvalidBindingLifecycle);
                };
                if self.revision != 3
                    || ended_at != self.updated_at
                    || ended_at < activated_at
                    || self.valid_until.is_some_and(|until| activated_at > until)
                {
                    return Err(PermissionPolicyOperationsDomainError::InvalidBindingLifecycle);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicySelectionFailure {
    NoPolicy,
    AmbiguousPolicy,
    RetiredPolicy,
    OutOfScope,
}

impl PermissionPolicySelectionFailure {
    pub fn reason_code(self) -> &'static str {
        match self {
            Self::NoPolicy => "no_policy",
            Self::AmbiguousPolicy => "ambiguous_policy_selection",
            Self::RetiredPolicy => "retired_policy",
            Self::OutOfScope => "out_of_scope",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicySelectionOutcome {
    Selected,
    Denied(PermissionPolicySelectionFailure),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    try_from = "PermissionPolicySelectionEvidenceDto"
)]
pub struct PermissionPolicySelectionEvidence {
    id: PermissionPolicySelectionEvidenceId,
    scopes: Vec<PermissionPolicyScopeEvidence>,
    selected_policy_versions: Vec<PermissionPolicyVersionRef>,
    outcome: PermissionPolicySelectionOutcome,
    selected_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PermissionPolicySelectionEvidenceDto {
    id: PermissionPolicySelectionEvidenceId,
    scopes: Vec<PermissionPolicyScopeEvidence>,
    selected_policy_versions: Vec<PermissionPolicyVersionRef>,
    outcome: PermissionPolicySelectionOutcome,
    selected_at: i64,
}

impl TryFrom<PermissionPolicySelectionEvidenceDto> for PermissionPolicySelectionEvidence {
    type Error = PermissionPolicyOperationsDomainError;

    fn try_from(value: PermissionPolicySelectionEvidenceDto) -> Result<Self, Self::Error> {
        let evidence = Self {
            id: value.id,
            scopes: value.scopes,
            selected_policy_versions: value.selected_policy_versions,
            outcome: value.outcome,
            selected_at: value.selected_at,
        };
        evidence.validate()?;
        Ok(evidence)
    }
}

impl PermissionPolicySelectionEvidence {
    pub fn new(
        id: PermissionPolicySelectionEvidenceId,
        mut scopes: Vec<PermissionPolicyScopeEvidence>,
        mut selected_policy_versions: Vec<PermissionPolicyVersionRef>,
        outcome: PermissionPolicySelectionOutcome,
        selected_at: i64,
    ) -> Result<Self, PermissionPolicyOperationsDomainError> {
        scopes.sort();
        selected_policy_versions.sort_by(compare_policy_references);
        let evidence = Self {
            id,
            scopes,
            selected_policy_versions,
            outcome,
            selected_at,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn id(&self) -> &PermissionPolicySelectionEvidenceId {
        &self.id
    }
    pub fn scopes(&self) -> &[PermissionPolicyScopeEvidence] {
        &self.scopes
    }
    pub fn selected_policy_versions(&self) -> &[PermissionPolicyVersionRef] {
        &self.selected_policy_versions
    }
    pub fn outcome(&self) -> PermissionPolicySelectionOutcome {
        self.outcome
    }
    pub fn selected_at(&self) -> i64 {
        self.selected_at
    }

    pub fn validate(&self) -> Result<(), PermissionPolicyOperationsDomainError> {
        PermissionPolicySelectionEvidenceId::new(self.id.as_str())?;
        if self.selected_at < 0 {
            return Err(PermissionPolicyOperationsDomainError::InvalidTimestamp);
        }
        if self.scopes.is_empty() {
            return Err(PermissionPolicyOperationsDomainError::EmptySelectionScopes);
        }
        if self.scopes.len() > MAX_SELECTION_SCOPES
            || self.selected_policy_versions.len() > MAX_SELECTION_POLICIES
        {
            return Err(PermissionPolicyOperationsDomainError::SelectionLimitExceeded);
        }
        let mut unique_scopes = HashSet::new();
        for scope in &self.scopes {
            scope.validate()?;
            if !unique_scopes.insert(scope) {
                return Err(PermissionPolicyOperationsDomainError::DuplicateSelectionScope);
            }
        }
        if self.scopes.windows(2).any(|window| window[0] >= window[1]) {
            return Err(PermissionPolicyOperationsDomainError::DuplicateSelectionScope);
        }
        let mut unique_versions = HashSet::new();
        for reference in &self.selected_policy_versions {
            validate_policy_reference(reference)?;
            if !unique_versions.insert((reference.policy_id().clone(), reference.version())) {
                return Err(PermissionPolicyOperationsDomainError::InvalidSelectionOutcome);
            }
        }
        if self.selected_policy_versions.windows(2).any(|window| {
            compare_policy_references(&window[0], &window[1]) != std::cmp::Ordering::Less
        }) {
            return Err(PermissionPolicyOperationsDomainError::InvalidSelectionOutcome);
        }
        if matches!(self.outcome, PermissionPolicySelectionOutcome::Selected)
            != !self.selected_policy_versions.is_empty()
        {
            return Err(PermissionPolicyOperationsDomainError::InvalidSelectionOutcome);
        }
        Ok(())
    }
}

fn compare_policy_references(
    left: &PermissionPolicyVersionRef,
    right: &PermissionPolicyVersionRef,
) -> std::cmp::Ordering {
    permission_policy_layer_precedence(left.layer())
        .cmp(&permission_policy_layer_precedence(right.layer()))
        .then_with(|| left.policy_id().cmp(right.policy_id()))
        .then_with(|| left.version().cmp(&right.version()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::permission_domain::{PermissionRule, PermissionRuleEffect};

    fn policy(version: u16) -> PermissionPolicy {
        PermissionPolicy::new(
            PermissionPolicyId::new("permission-policy:repository").unwrap(),
            version,
            PermissionPolicyLayer::Repository,
            "owner:repository",
            vec![PermissionRule::new(
                PermissionRuleEffect::Deny,
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:repo",
                BTreeMap::new(),
            )
            .unwrap()],
        )
        .unwrap()
    }

    fn selector() -> PermissionPolicyScopeSelector {
        PermissionPolicyScopeSelector::new(
            PermissionPolicyLayer::Repository,
            PermissionPolicyScopeEvidence::new(
                PermissionPolicyScopeKind::Repository,
                "repository:repo",
                None,
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn policy_record_lifecycle_is_revisioned_and_definition_is_stable() {
        let draft = PermissionPolicyRecord::new_draft(
            PermissionPolicyRecordId::new("policy-record:one").unwrap(),
            policy(1),
            "provenance:owner",
            None,
            10,
        )
        .unwrap();
        let published = draft.publish(1, 20).unwrap();
        let retired = published.retire(2, 30).unwrap();

        assert_eq!(retired.revision(), 3);
        assert_eq!(retired.policy(), draft.policy());
        assert_eq!(
            retired.lifecycle(),
            PermissionPolicyRecordLifecycle::Retired
        );
        assert!(matches!(
            draft.publish(2, 20),
            Err(PermissionPolicyOperationsDomainError::StaleRevision { .. })
        ));
    }

    #[test]
    fn policy_binding_is_explicit_and_revisioned() {
        let draft = PermissionPolicyScopeBinding::new_draft(
            PermissionPolicyScopeBindingId::new("policy-binding:one").unwrap(),
            PermissionPolicyRecordId::new("policy-record:one").unwrap(),
            PermissionPolicyVersionRef::from_policy(&policy(1)),
            selector(),
            20,
            Some(40),
            "provenance:owner",
            10,
        )
        .unwrap();
        let active = draft.activate(1, 20).unwrap();
        assert!(active.is_effective_at(30));
        let ended = active.end(2, 35).unwrap();
        assert!(!ended.is_effective_at(35));
        assert_eq!(ended.revision(), 3);
    }

    #[test]
    fn selection_is_deny_by_default_and_bounded() {
        let evidence = PermissionPolicySelectionEvidence::new(
            PermissionPolicySelectionEvidenceId::new("policy-selection:none").unwrap(),
            vec![selector().scope().clone()],
            Vec::new(),
            PermissionPolicySelectionOutcome::Denied(PermissionPolicySelectionFailure::NoPolicy),
            10,
        )
        .unwrap();

        assert!(evidence.selected_policy_versions().is_empty());
        assert!(matches!(
            evidence.outcome(),
            PermissionPolicySelectionOutcome::Denied(PermissionPolicySelectionFailure::NoPolicy)
        ));
    }

    #[test]
    fn persisted_policy_record_is_validated_on_deserialization() {
        let draft = PermissionPolicyRecord::new_draft(
            PermissionPolicyRecordId::new("policy-record:one").unwrap(),
            policy(1),
            "provenance:owner",
            None,
            10,
        )
        .unwrap();
        let mut value = serde_json::to_value(draft).unwrap();
        value["policy"]["version"] = serde_json::json!(0);

        assert!(serde_json::from_value::<PermissionPolicyRecord>(value).is_err());
    }

    #[test]
    fn standalone_operational_ids_validate_during_deserialization() {
        assert!(serde_json::from_str::<PermissionPolicyRecordId>("\"wrong:one\"").is_err());
        assert!(
            serde_json::from_str::<PermissionPolicyRecordId>("\"policy-record:path/segment\"")
                .is_err()
        );
        assert_eq!(
            serde_json::from_str::<PermissionPolicyRecordId>("\" policy-record:one \"")
                .unwrap()
                .as_str(),
            "policy-record:one"
        );
    }

    #[test]
    fn operations_envelope_preserves_legacy_owner_reference_compatibility() {
        let compatible = PermissionPolicy::new(
            PermissionPolicyId::new("permission-policy:legacy").unwrap(),
            1,
            PermissionPolicyLayer::Repository,
            "legacy-owner",
            vec![PermissionRule::new(
                PermissionRuleEffect::Deny,
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:repo",
                BTreeMap::new(),
            )
            .unwrap()],
        )
        .unwrap();

        assert!(PermissionPolicyRecord::new_draft(
            PermissionPolicyRecordId::new("policy-record:legacy").unwrap(),
            compatible,
            "provenance:owner",
            None,
            10,
        )
        .is_ok());
    }

    #[test]
    fn persisted_lifecycle_revision_and_time_are_exact() {
        let published = PermissionPolicyRecord::new_draft(
            PermissionPolicyRecordId::new("policy-record:one").unwrap(),
            policy(1),
            "provenance:owner",
            None,
            10,
        )
        .unwrap()
        .publish(1, 20)
        .unwrap();
        let mut record = serde_json::to_value(published).unwrap();
        record["revision"] = serde_json::json!(99);
        assert!(serde_json::from_value::<PermissionPolicyRecord>(record).is_err());

        let active = PermissionPolicyScopeBinding::new_draft(
            PermissionPolicyScopeBindingId::new("policy-binding:one").unwrap(),
            PermissionPolicyRecordId::new("policy-record:one").unwrap(),
            PermissionPolicyVersionRef::from_policy(&policy(1)),
            selector(),
            10,
            Some(40),
            "provenance:owner",
            10,
        )
        .unwrap()
        .activate(1, 20)
        .unwrap();
        assert!(!active.is_effective_at(15));
        assert!(active.is_effective_at(20));
        let mut binding = serde_json::to_value(active).unwrap();
        binding["activatedAt"] = serde_json::json!(50);
        binding["updatedAt"] = serde_json::json!(50);
        assert!(serde_json::from_value::<PermissionPolicyScopeBinding>(binding).is_err());
    }

    #[test]
    fn selected_policy_identity_cannot_claim_conflicting_layers() {
        let reference = PermissionPolicyVersionRef::from_policy(&policy(1));
        let mut conflicting = serde_json::to_value(&reference).unwrap();
        conflicting["layer"] = serde_json::json!("workspace");
        let conflicting =
            serde_json::from_value::<PermissionPolicyVersionRef>(conflicting).unwrap();
        let value = serde_json::json!({
            "id": "policy-selection:conflict",
            "scopes": [{
                "scopeKind": "repository",
                "scopeRef": "repository:repo",
                "boundaryRef": null
            }],
            "selectedPolicyVersions": [reference, conflicting],
            "outcome": "selected",
            "selectedAt": 10
        });

        assert!(serde_json::from_value::<PermissionPolicySelectionEvidence>(value).is_err());
    }
}
