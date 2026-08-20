//! Audited application and read-model boundary for Organization governance.
//!
//! This service mutates only Organization operational records. Team,
//! Membership, Permission, Role, Capability, Agent, Workflow, and Execution
//! aggregates are read as existing evidence and are never rewritten or treated
//! as authority.

use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    governance_audit::{
        AuditCorrelationReferences, GovernanceAuditDomainError, GovernanceAuditEvent,
        GovernanceAuditEventKind, GovernanceAuditOutcome, GovernanceAuditRecordRequest,
        GovernanceAuditServiceError, GovernanceAuditSink, GovernanceAuditStreamId,
        SanitizedAuditMetadata,
    },
    governance_time::{TrustedClock, TrustedClockError},
    organization_governance::{
        Organization, OrganizationBindingLifecycle, OrganizationBoundaryDenialReason,
        OrganizationBoundaryEvidence, OrganizationBoundaryEvidenceId, OrganizationBoundaryOutcome,
        OrganizationBoundaryReferences, OrganizationGovernanceDomainError, OrganizationId,
        OrganizationLifecycle, OrganizationPolicyBinding, OrganizationPolicyBindingId,
        OrganizationPolicyTarget, OrganizationTeamBinding, OrganizationTeamBindingId,
    },
    organization_governance_repository::{
        OrganizationGovernanceRepository, OrganizationGovernanceRepositoryError,
        MAX_ORGANIZATION_QUERY_LIMIT,
    },
    permission_domain::PermissionPolicyLayer,
    permission_policy_operations::{
        PermissionPolicyRecordLifecycle, PermissionPolicyScopeBindingLifecycle,
    },
    permission_policy_operations_repository::{
        PermissionPolicyOperationsRepository, PermissionPolicyOperationsRepositoryError,
    },
    team_domain::{TeamId, TeamLifecycle},
    team_repository::{TeamRepository, TeamRepositoryError},
};

#[derive(Debug, Error)]
pub enum OrganizationGovernanceServiceError {
    #[error(transparent)]
    Domain(#[from] OrganizationGovernanceDomainError),
    #[error(transparent)]
    Repository(#[from] OrganizationGovernanceRepositoryError),
    #[error(transparent)]
    TeamRepository(#[from] TeamRepositoryError),
    #[error(transparent)]
    PolicyRepository(#[from] PermissionPolicyOperationsRepositoryError),
    #[error(transparent)]
    Clock(#[from] TrustedClockError),
    #[error(transparent)]
    Audit(#[from] GovernanceAuditServiceError),
    #[error(transparent)]
    AuditDomain(#[from] GovernanceAuditDomainError),
    #[error("Organization was not found: {0}")]
    OrganizationNotFound(OrganizationId),
    #[error("Organization is not Active: {0}")]
    OrganizationNotActive(OrganizationId),
    #[error("Organization Team binding was not found: {0}")]
    TeamBindingNotFound(OrganizationTeamBindingId),
    #[error("Organization policy binding was not found: {0}")]
    PolicyBindingNotFound(OrganizationPolicyBindingId),
    #[error("Team was not found: {0}")]
    TeamNotFound(TeamId),
    #[error("Team is not Active: {0}")]
    TeamNotActive(TeamId),
    #[error("Permission policy target is missing or not in the required exact lifecycle")]
    PolicyTargetUnavailable,
    #[error("Cross-organization ownership or reference is forbidden")]
    CrossOrganizationReference,
}

impl OrganizationGovernanceServiceError {
    fn reason_code(&self) -> &'static str {
        match self {
            Self::Domain(OrganizationGovernanceDomainError::StaleRevision { .. })
            | Self::Repository(OrganizationGovernanceRepositoryError::RevisionConflict {
                ..
            }) => "stale_revision",
            Self::Domain(
                OrganizationGovernanceDomainError::InvalidOrganizationLifecycle
                | OrganizationGovernanceDomainError::InvalidBindingLifecycle,
            )
            | Self::Repository(OrganizationGovernanceRepositoryError::InvalidLifecycle {
                ..
            }) => "invalid_lifecycle",
            Self::Repository(OrganizationGovernanceRepositoryError::ArchivedReadOnly(_)) => {
                "archived_read_only"
            }
            Self::Repository(
                OrganizationGovernanceRepositoryError::ActiveTeamOwnerConflict(_)
                | OrganizationGovernanceRepositoryError::ActivePolicyOwnerConflict
                | OrganizationGovernanceRepositoryError::ActiveBindingsRemain(_),
            ) => "active_binding_exists",
            Self::Repository(_) | Self::TeamRepository(_) | Self::PolicyRepository(_) => {
                "boundary_repository"
            }
            Self::Clock(_) => "trusted_clock",
            Self::Audit(_) => "audit_failure",
            Self::AuditDomain(_) => "audit_validation",
            Self::OrganizationNotFound(_) => "organization_not_found",
            Self::OrganizationNotActive(_) => "inactive_organization",
            Self::TeamBindingNotFound(_) => "team_binding_not_found",
            Self::PolicyBindingNotFound(_) => "policy_binding_not_found",
            Self::TeamNotFound(_) => "team_not_found",
            Self::TeamNotActive(_) => "team_binding_inactive",
            Self::PolicyTargetUnavailable => "policy_binding_inactive",
            Self::CrossOrganizationReference => "cross_organization_reference",
            Self::Domain(_) => "domain_validation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationBoundaryResolutionRequest {
    evidence_id: OrganizationBoundaryEvidenceId,
    declared_scope_organization_id: OrganizationId,
    references: OrganizationBoundaryReferences,
    provenance_ref: String,
}

impl OrganizationBoundaryResolutionRequest {
    pub fn new(
        evidence_id: OrganizationBoundaryEvidenceId,
        declared_scope_organization_id: OrganizationId,
        references: OrganizationBoundaryReferences,
        provenance_ref: impl Into<String>,
    ) -> Result<Self, OrganizationGovernanceDomainError> {
        references.validate()?;
        let provenance_ref = provenance_ref.into();
        // Reuse immutable evidence construction validation for bounded refs only
        // after audit identity exists; here a temporary valid audit ref checks the
        // request provenance without accepting caller-forged final evidence time.
        OrganizationBoundaryEvidence::new(
            evidence_id.clone(),
            references.clone(),
            OrganizationBoundaryOutcome::Denied(
                OrganizationBoundaryDenialReason::InactiveOrganization,
            ),
            0,
            provenance_ref.clone(),
            "audit:request-validation",
        )?;
        Ok(Self {
            evidence_id,
            declared_scope_organization_id,
            references,
            provenance_ref,
        })
    }

    pub fn evidence_id(&self) -> &OrganizationBoundaryEvidenceId {
        &self.evidence_id
    }
    pub fn declared_scope_organization_id(&self) -> &OrganizationId {
        &self.declared_scope_organization_id
    }
    pub fn references(&self) -> &OrganizationBoundaryReferences {
        &self.references
    }
    pub fn provenance_ref(&self) -> &str {
        &self.provenance_ref
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationManagementSummary {
    pub organization_id: String,
    pub display_name: String,
    pub purpose: String,
    pub owner_ref: String,
    pub lifecycle: OrganizationLifecycle,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived_at: Option<i64>,
    pub provenance_ref: String,
}

impl From<&Organization> for OrganizationManagementSummary {
    fn from(value: &Organization) -> Self {
        Self {
            organization_id: value.id().to_string(),
            display_name: value.display_name().to_string(),
            purpose: value.purpose().to_string(),
            owner_ref: value.owner_ref().to_string(),
            lifecycle: value.lifecycle(),
            revision: value.revision(),
            created_at: value.created_at(),
            updated_at: value.updated_at(),
            archived_at: value.archived_at(),
            provenance_ref: value.provenance_ref().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationTeamBindingManagementView {
    pub binding_id: String,
    pub team_id: String,
    pub lifecycle: OrganizationBindingLifecycle,
    pub revision: u64,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub provenance_ref: String,
}

impl From<&OrganizationTeamBinding> for OrganizationTeamBindingManagementView {
    fn from(value: &OrganizationTeamBinding) -> Self {
        Self {
            binding_id: value.id().to_string(),
            team_id: value.team_id().to_string(),
            lifecycle: value.lifecycle(),
            revision: value.revision(),
            valid_from: value.valid_from(),
            valid_until: value.valid_until(),
            created_at: value.created_at(),
            updated_at: value.updated_at(),
            provenance_ref: value.provenance_ref().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationPolicyBindingManagementView {
    pub binding_id: String,
    pub target_kind: String,
    pub policy_record_id: String,
    pub policy_id: String,
    pub policy_version: u16,
    pub policy_layer: String,
    pub policy_scope_binding_id: Option<String>,
    pub lifecycle: OrganizationBindingLifecycle,
    pub revision: u64,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub provenance_ref: String,
}

impl From<&OrganizationPolicyBinding> for OrganizationPolicyBindingManagementView {
    fn from(value: &OrganizationPolicyBinding) -> Self {
        Self {
            binding_id: value.id().to_string(),
            target_kind: value.target().kind().to_string(),
            policy_record_id: value.target().record_id().to_string(),
            policy_id: value.target().policy_ref().policy_id().to_string(),
            policy_version: value.target().policy_ref().version(),
            policy_layer: policy_layer_name(value.target().policy_ref().layer()).to_string(),
            policy_scope_binding_id: value.target().scope_binding_id().map(ToString::to_string),
            lifecycle: value.lifecycle(),
            revision: value.revision(),
            valid_from: value.valid_from(),
            valid_until: value.valid_until(),
            created_at: value.created_at(),
            updated_at: value.updated_at(),
            provenance_ref: value.provenance_ref().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationBoundaryEvidenceManagementView {
    pub evidence_id: String,
    pub outcome: OrganizationBoundaryOutcome,
    pub resolved_at: i64,
    pub team_id: Option<String>,
    pub agent_ref: Option<String>,
    pub workflow_ref: Option<String>,
    pub execution_ref: Option<String>,
    pub resource_ref: Option<String>,
    pub audit_ref: String,
}

impl From<&OrganizationBoundaryEvidence> for OrganizationBoundaryEvidenceManagementView {
    fn from(value: &OrganizationBoundaryEvidence) -> Self {
        Self {
            evidence_id: value.id().to_string(),
            outcome: value.outcome(),
            resolved_at: value.resolved_at(),
            team_id: value.references().team_id().map(ToString::to_string),
            agent_ref: value.references().agent_ref().map(ToString::to_string),
            workflow_ref: value.references().workflow_ref().map(ToString::to_string),
            execution_ref: value.references().execution_ref().map(ToString::to_string),
            resource_ref: value.references().resource_ref().map(ToString::to_string),
            audit_ref: value.audit_ref().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationGovernanceManagementView {
    pub organization: OrganizationManagementSummary,
    pub team_bindings: Vec<OrganizationTeamBindingManagementView>,
    pub policy_bindings: Vec<OrganizationPolicyBindingManagementView>,
    pub boundary_evidence: Vec<OrganizationBoundaryEvidenceManagementView>,
}

pub struct OrganizationGovernanceService<OR, TR, PR, C, A> {
    repository: OR,
    teams: TR,
    policies: PR,
    clock: C,
    audit: A,
    audit_actor: String,
}

impl<OR, TR, PR, C, A> OrganizationGovernanceService<OR, TR, PR, C, A> {
    pub fn new(
        repository: OR,
        teams: TR,
        policies: PR,
        clock: C,
        audit: A,
        audit_actor: impl Into<String>,
    ) -> Self {
        Self {
            repository,
            teams,
            policies,
            clock,
            audit,
            audit_actor: audit_actor.into(),
        }
    }
}

impl<OR, TR, PR, C, A> OrganizationGovernanceService<OR, TR, PR, C, A>
where
    OR: OrganizationGovernanceRepository,
    TR: TeamRepository,
    PR: PermissionPolicyOperationsRepository,
    C: TrustedClock,
    A: GovernanceAuditSink,
{
    #[allow(clippy::too_many_arguments)]
    pub fn create_organization(
        &self,
        organization_id: OrganizationId,
        display_name: impl Into<String>,
        purpose: impl Into<String>,
        owner_ref: impl Into<String>,
        provenance_ref: impl Into<String>,
    ) -> Result<Organization, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let organization = match Organization::new(
            organization_id.clone(),
            display_name,
            purpose,
            owner_ref,
            provenance_ref,
            at,
        ) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationLifecycleChangeRejected,
                    "organization",
                    organization_id.as_str(),
                    &organization_id,
                    at,
                    None,
                )
            }
        };
        self.record_event(
            GovernanceAuditEventKind::OrganizationCreated,
            GovernanceAuditOutcome::Created,
            "organization",
            organization.id().as_str(),
            organization.id(),
            lifecycle_metadata(organization.lifecycle(), organization.revision(), None)?,
            at,
        )?;
        if let Err(error) = self.repository.insert_organization(organization.clone()) {
            return self.reject_operation(
                error.into(),
                GovernanceAuditEventKind::OrganizationLifecycleChangeRejected,
                "organization",
                organization.id().as_str(),
                organization.id(),
                at,
                None,
            );
        }
        Ok(organization)
    }

    pub fn activate_organization(
        &self,
        organization_id: &OrganizationId,
        expected_revision: u64,
    ) -> Result<Organization, OrganizationGovernanceServiceError> {
        self.transition_organization(
            organization_id,
            OrganizationLifecycle::Active,
            expected_revision,
        )
    }

    pub fn suspend_organization(
        &self,
        organization_id: &OrganizationId,
        expected_revision: u64,
    ) -> Result<Organization, OrganizationGovernanceServiceError> {
        self.transition_organization(
            organization_id,
            OrganizationLifecycle::Suspended,
            expected_revision,
        )
    }

    pub fn archive_organization(
        &self,
        organization_id: &OrganizationId,
        expected_revision: u64,
    ) -> Result<Organization, OrganizationGovernanceServiceError> {
        self.transition_organization(
            organization_id,
            OrganizationLifecycle::Archived,
            expected_revision,
        )
    }

    fn transition_organization(
        &self,
        organization_id: &OrganizationId,
        target: OrganizationLifecycle,
        expected_revision: u64,
    ) -> Result<Organization, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let current = match self.require_organization(organization_id) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error,
                    GovernanceAuditEventKind::OrganizationLifecycleChangeRejected,
                    "organization",
                    organization_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        let next = match current.transition_to(target, expected_revision, at) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationLifecycleChangeRejected,
                    "organization",
                    organization_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        self.record_event(
            GovernanceAuditEventKind::OrganizationLifecycleChanged,
            GovernanceAuditOutcome::Updated,
            "organization",
            organization_id.as_str(),
            organization_id,
            lifecycle_metadata(next.lifecycle(), next.revision(), Some(expected_revision))?,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_organization(next.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                GovernanceAuditEventKind::OrganizationLifecycleChangeRejected,
                "organization",
                organization_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        Ok(next)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_team_binding_draft(
        &self,
        binding_id: OrganizationTeamBindingId,
        organization_id: &OrganizationId,
        team_id: TeamId,
        valid_from: i64,
        valid_until: Option<i64>,
        provenance_ref: impl Into<String>,
    ) -> Result<OrganizationTeamBinding, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let subject = binding_id.to_string();
        if let Err(error) = self.require_mutable_organization(organization_id) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                "organization_team_binding",
                &subject,
                organization_id,
                at,
                None,
            );
        }
        if let Err(error) = self.require_team(&team_id, false) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                "organization_team_binding",
                &subject,
                organization_id,
                at,
                None,
            );
        }
        let binding = match OrganizationTeamBinding::new_draft(
            binding_id,
            organization_id.clone(),
            team_id,
            valid_from,
            valid_until,
            provenance_ref,
            at,
        ) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                    "organization_team_binding",
                    &subject,
                    organization_id,
                    at,
                    None,
                )
            }
        };
        self.record_event(
            GovernanceAuditEventKind::OrganizationTeamBindingCreated,
            GovernanceAuditOutcome::Created,
            "organization_team_binding",
            &subject,
            organization_id,
            binding_metadata(binding.lifecycle(), binding.revision(), None)?,
            at,
        )?;
        if let Err(error) = self.repository.insert_team_binding(binding.clone()) {
            return self.reject_operation(
                error.into(),
                GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                "organization_team_binding",
                &subject,
                organization_id,
                at,
                None,
            );
        }
        Ok(binding)
    }

    pub fn activate_team_binding(
        &self,
        organization_id: &OrganizationId,
        binding_id: &OrganizationTeamBindingId,
        expected_revision: u64,
    ) -> Result<OrganizationTeamBinding, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let current = match self.require_team_binding(binding_id) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error,
                    GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                    "organization_team_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        if current.organization_id() != organization_id {
            return self.reject_operation(
                OrganizationGovernanceServiceError::CrossOrganizationReference,
                GovernanceAuditEventKind::CrossOrganizationAccessDenied,
                "cross_organization_request",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        if let Err(error) = self.require_active_organization(organization_id) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                "organization_team_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        if let Err(error) = self.require_team(current.team_id(), true) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                "organization_team_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        let existing = match self
            .repository
            .get_active_team_binding(current.team_id(), at)
        {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                    "organization_team_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        if let Some(existing) = existing {
            if existing.id() != current.id()
                && existing.organization_id() != current.organization_id()
            {
                return self.reject_operation(
                    OrganizationGovernanceServiceError::CrossOrganizationReference,
                    GovernanceAuditEventKind::CrossOrganizationAccessDenied,
                    "cross_organization_request",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                );
            }
        }
        let next = match current.activate(expected_revision, at) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                    "organization_team_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        self.record_event(
            GovernanceAuditEventKind::OrganizationTeamBindingActivated,
            GovernanceAuditOutcome::Updated,
            "organization_team_binding",
            binding_id.as_str(),
            organization_id,
            binding_metadata(next.lifecycle(), next.revision(), Some(expected_revision))?,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_team_binding(next.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                "organization_team_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        Ok(next)
    }

    pub fn end_team_binding(
        &self,
        organization_id: &OrganizationId,
        binding_id: &OrganizationTeamBindingId,
        expected_revision: u64,
    ) -> Result<OrganizationTeamBinding, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let current = match self.require_team_binding(binding_id) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error,
                    GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                    "organization_team_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        if current.organization_id() != organization_id {
            return self.reject_operation(
                OrganizationGovernanceServiceError::CrossOrganizationReference,
                GovernanceAuditEventKind::CrossOrganizationAccessDenied,
                "cross_organization_request",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        if let Err(error) = self.require_mutable_organization(organization_id) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                "organization_team_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        let next = match current.end(expected_revision, at) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                    "organization_team_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        self.record_event(
            GovernanceAuditEventKind::OrganizationTeamBindingEnded,
            GovernanceAuditOutcome::Updated,
            "organization_team_binding",
            binding_id.as_str(),
            organization_id,
            binding_metadata(next.lifecycle(), next.revision(), Some(expected_revision))?,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_team_binding(next.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                GovernanceAuditEventKind::OrganizationTeamBindingRejected,
                "organization_team_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        Ok(next)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_policy_binding_draft(
        &self,
        binding_id: OrganizationPolicyBindingId,
        organization_id: &OrganizationId,
        target: OrganizationPolicyTarget,
        valid_from: i64,
        valid_until: Option<i64>,
        provenance_ref: impl Into<String>,
    ) -> Result<OrganizationPolicyBinding, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let subject = binding_id.to_string();
        if let Err(error) = self.require_mutable_organization(organization_id) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                "organization_policy_binding",
                &subject,
                organization_id,
                at,
                None,
            );
        }
        if let Err(error) = self.require_exact_policy_target(&target, false) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                "organization_policy_binding",
                &subject,
                organization_id,
                at,
                None,
            );
        }
        let binding = match OrganizationPolicyBinding::new_draft(
            binding_id,
            organization_id.clone(),
            target,
            valid_from,
            valid_until,
            provenance_ref,
            at,
        ) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                    "organization_policy_binding",
                    &subject,
                    organization_id,
                    at,
                    None,
                )
            }
        };
        self.record_event(
            GovernanceAuditEventKind::OrganizationPolicyBindingCreated,
            GovernanceAuditOutcome::Created,
            "organization_policy_binding",
            &subject,
            organization_id,
            binding_metadata(binding.lifecycle(), binding.revision(), None)?,
            at,
        )?;
        if let Err(error) = self.repository.insert_policy_binding(binding.clone()) {
            return self.reject_operation(
                error.into(),
                GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                "organization_policy_binding",
                &subject,
                organization_id,
                at,
                None,
            );
        }
        Ok(binding)
    }

    pub fn activate_policy_binding(
        &self,
        organization_id: &OrganizationId,
        binding_id: &OrganizationPolicyBindingId,
        expected_revision: u64,
    ) -> Result<OrganizationPolicyBinding, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let current = match self.require_policy_binding(binding_id) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error,
                    GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                    "organization_policy_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        if current.organization_id() != organization_id {
            return self.reject_operation(
                OrganizationGovernanceServiceError::CrossOrganizationReference,
                GovernanceAuditEventKind::CrossOrganizationAccessDenied,
                "cross_organization_request",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        if let Err(error) = self.require_active_organization(organization_id) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                "organization_policy_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        if let Err(error) = self.require_exact_policy_target(current.target(), true) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                "organization_policy_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        let existing = match self
            .repository
            .get_active_policy_binding(current.target(), at)
        {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                    "organization_policy_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        if let Some(existing) = existing {
            if existing.id() != current.id()
                && existing.organization_id() != current.organization_id()
            {
                return self.reject_operation(
                    OrganizationGovernanceServiceError::CrossOrganizationReference,
                    GovernanceAuditEventKind::CrossOrganizationAccessDenied,
                    "cross_organization_request",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                );
            }
        }
        let next = match current.activate(expected_revision, at) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                    "organization_policy_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        self.record_event(
            GovernanceAuditEventKind::OrganizationPolicyBindingActivated,
            GovernanceAuditOutcome::Updated,
            "organization_policy_binding",
            binding_id.as_str(),
            organization_id,
            binding_metadata(next.lifecycle(), next.revision(), Some(expected_revision))?,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_policy_binding(next.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                "organization_policy_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        Ok(next)
    }

    pub fn end_policy_binding(
        &self,
        organization_id: &OrganizationId,
        binding_id: &OrganizationPolicyBindingId,
        expected_revision: u64,
    ) -> Result<OrganizationPolicyBinding, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let current = match self.require_policy_binding(binding_id) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error,
                    GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                    "organization_policy_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        if current.organization_id() != organization_id {
            return self.reject_operation(
                OrganizationGovernanceServiceError::CrossOrganizationReference,
                GovernanceAuditEventKind::CrossOrganizationAccessDenied,
                "cross_organization_request",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        if let Err(error) = self.require_mutable_organization(organization_id) {
            return self.reject_operation(
                error,
                GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                "organization_policy_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        let next = match current.end(expected_revision, at) {
            Ok(value) => value,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                    "organization_policy_binding",
                    binding_id.as_str(),
                    organization_id,
                    at,
                    Some(expected_revision),
                )
            }
        };
        self.record_event(
            GovernanceAuditEventKind::OrganizationPolicyBindingEnded,
            GovernanceAuditOutcome::Updated,
            "organization_policy_binding",
            binding_id.as_str(),
            organization_id,
            binding_metadata(next.lifecycle(), next.revision(), Some(expected_revision))?,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_policy_binding(next.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                GovernanceAuditEventKind::OrganizationPolicyBindingRejected,
                "organization_policy_binding",
                binding_id.as_str(),
                organization_id,
                at,
                Some(expected_revision),
            );
        }
        Ok(next)
    }

    pub fn resolve_boundary(
        &self,
        request: OrganizationBoundaryResolutionRequest,
    ) -> Result<OrganizationBoundaryEvidence, OrganizationGovernanceServiceError> {
        let at = self.clock.now()?;
        let reason = self.boundary_denial_reason(&request, at)?;
        let outcome = reason.map_or(
            OrganizationBoundaryOutcome::Accepted,
            OrganizationBoundaryOutcome::Denied,
        );
        let is_cross = reason.is_some_and(|value| {
            matches!(
                value,
                OrganizationBoundaryDenialReason::CrossOrganizationReference
                    | OrganizationBoundaryDenialReason::TeamOwnedByAnotherOrganization
                    | OrganizationBoundaryDenialReason::QueryScopeMismatch
            )
        });
        let mut audit_metadata = BTreeMap::from([(
            "reason_code".into(),
            reason
                .map_or("activation_evidence", |value| value.as_str())
                .into(),
        )]);
        if let Some(revision) = request.references().organization_revision() {
            audit_metadata.insert("organization_revision".into(), revision.to_string());
        }
        let event = self.record_event(
            if is_cross {
                GovernanceAuditEventKind::CrossOrganizationAccessDenied
            } else if reason.is_some() {
                GovernanceAuditEventKind::OrganizationBoundaryResolutionRejected
            } else {
                GovernanceAuditEventKind::OrganizationBoundaryResolutionAccepted
            },
            if reason.is_some() {
                GovernanceAuditOutcome::Denied
            } else {
                GovernanceAuditOutcome::Accepted
            },
            if is_cross {
                "cross_organization_request"
            } else {
                "organization_boundary"
            },
            request.evidence_id().as_str(),
            request.references().organization_id(),
            SanitizedAuditMetadata::new(audit_metadata)?,
            at,
        )?;
        let evidence = OrganizationBoundaryEvidence::new(
            request.evidence_id().clone(),
            request.references().clone(),
            outcome,
            at,
            request.provenance_ref(),
            event.event_id().to_string(),
        )?;
        self.repository.append_boundary_evidence(evidence.clone())?;
        Ok(evidence)
    }

    pub fn management_view(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<OrganizationGovernanceManagementView, OrganizationGovernanceServiceError> {
        if !(1..=MAX_ORGANIZATION_QUERY_LIMIT).contains(&limit) {
            return Err(OrganizationGovernanceRepositoryError::InvalidQueryLimit.into());
        }
        let organization = self.require_organization(organization_id)?;
        let team_bindings = self
            .repository
            .list_team_bindings(organization_id, limit)?
            .iter()
            .map(OrganizationTeamBindingManagementView::from)
            .collect();
        let policy_bindings = self
            .repository
            .list_policy_bindings(organization_id, limit)?
            .iter()
            .map(OrganizationPolicyBindingManagementView::from)
            .collect();
        let boundary_evidence = self
            .repository
            .list_boundary_evidence(organization_id, limit)?
            .iter()
            .map(OrganizationBoundaryEvidenceManagementView::from)
            .collect();
        Ok(OrganizationGovernanceManagementView {
            organization: (&organization).into(),
            team_bindings,
            policy_bindings,
            boundary_evidence,
        })
    }

    fn boundary_denial_reason(
        &self,
        request: &OrganizationBoundaryResolutionRequest,
        at: i64,
    ) -> Result<Option<OrganizationBoundaryDenialReason>, OrganizationGovernanceServiceError> {
        let references = request.references();
        if request.declared_scope_organization_id() != references.organization_id() {
            return Ok(Some(
                OrganizationBoundaryDenialReason::CrossOrganizationReference,
            ));
        }
        let Some(organization) = self
            .repository
            .get_organization(references.organization_id())?
        else {
            return Ok(Some(OrganizationBoundaryDenialReason::OrganizationNotFound));
        };
        if organization.lifecycle() != OrganizationLifecycle::Active {
            return Ok(Some(OrganizationBoundaryDenialReason::InactiveOrganization));
        }
        if references.organization_revision() != Some(organization.revision()) {
            return Ok(Some(OrganizationBoundaryDenialReason::StaleRevision));
        }
        if let (Some(team_id), Some(binding_id), Some(expected_revision)) = (
            references.team_id(),
            references.team_binding_id(),
            references.team_binding_revision(),
        ) {
            let Some(binding) = self.repository.get_team_binding(binding_id)? else {
                return Ok(Some(OrganizationBoundaryDenialReason::TeamBindingNotFound));
            };
            if binding.organization_id() != references.organization_id()
                || binding.team_id() != team_id
            {
                return Ok(Some(
                    OrganizationBoundaryDenialReason::CrossOrganizationReference,
                ));
            }
            if binding.revision() != expected_revision {
                return Ok(Some(OrganizationBoundaryDenialReason::StaleRevision));
            }
            if !binding.is_effective_at(at) {
                return Ok(Some(OrganizationBoundaryDenialReason::TeamBindingInactive));
            }
            let Some(active_owner) = self.repository.get_active_team_binding(team_id, at)? else {
                return Ok(Some(OrganizationBoundaryDenialReason::TeamBindingInactive));
            };
            if active_owner.id() != binding.id()
                || active_owner.organization_id() != references.organization_id()
            {
                return Ok(Some(
                    OrganizationBoundaryDenialReason::TeamOwnedByAnotherOrganization,
                ));
            }
        }
        if let (Some(binding_id), Some(expected_revision)) = (
            references.policy_binding_id(),
            references.policy_binding_revision(),
        ) {
            let Some(binding) = self.repository.get_policy_binding(binding_id)? else {
                return Ok(Some(
                    OrganizationBoundaryDenialReason::PolicyBindingNotFound,
                ));
            };
            if binding.organization_id() != references.organization_id() {
                return Ok(Some(
                    OrganizationBoundaryDenialReason::CrossOrganizationReference,
                ));
            }
            if binding.revision() != expected_revision {
                return Ok(Some(OrganizationBoundaryDenialReason::StaleRevision));
            }
            if !binding.is_effective_at(at) {
                return Ok(Some(
                    OrganizationBoundaryDenialReason::PolicyBindingInactive,
                ));
            }
        }
        if let (Some(membership_id), Some(expected_revision)) =
            (references.membership_id(), references.membership_revision())
        {
            let Some(membership) = self.teams.get_membership(membership_id)? else {
                return Ok(Some(
                    OrganizationBoundaryDenialReason::MembershipNotEffective,
                ));
            };
            if membership.revision() != expected_revision
                || references.team_id() != Some(membership.team_id())
                || references.agent_ref() != Some(membership.agent_id())
                || !membership.is_effective(at)
            {
                return Ok(Some(
                    OrganizationBoundaryDenialReason::MembershipNotEffective,
                ));
            }
        }
        Ok(None)
    }

    fn require_organization(
        &self,
        organization_id: &OrganizationId,
    ) -> Result<Organization, OrganizationGovernanceServiceError> {
        self.repository
            .get_organization(organization_id)?
            .ok_or_else(|| {
                OrganizationGovernanceServiceError::OrganizationNotFound(organization_id.clone())
            })
    }

    fn require_mutable_organization(
        &self,
        organization_id: &OrganizationId,
    ) -> Result<Organization, OrganizationGovernanceServiceError> {
        let organization = self.require_organization(organization_id)?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            )
            .into());
        }
        Ok(organization)
    }

    fn require_active_organization(
        &self,
        organization_id: &OrganizationId,
    ) -> Result<Organization, OrganizationGovernanceServiceError> {
        let organization = self.require_organization(organization_id)?;
        if organization.lifecycle() != OrganizationLifecycle::Active {
            return Err(OrganizationGovernanceServiceError::OrganizationNotActive(
                organization.id().clone(),
            ));
        }
        Ok(organization)
    }

    fn require_team(
        &self,
        team_id: &TeamId,
        require_active: bool,
    ) -> Result<(), OrganizationGovernanceServiceError> {
        let team = self
            .teams
            .get_team(team_id)?
            .ok_or_else(|| OrganizationGovernanceServiceError::TeamNotFound(team_id.clone()))?;
        if require_active && team.lifecycle() != TeamLifecycle::Active {
            return Err(OrganizationGovernanceServiceError::TeamNotActive(
                team_id.clone(),
            ));
        }
        Ok(())
    }

    fn require_team_binding(
        &self,
        binding_id: &OrganizationTeamBindingId,
    ) -> Result<OrganizationTeamBinding, OrganizationGovernanceServiceError> {
        self.repository
            .get_team_binding(binding_id)?
            .ok_or_else(|| {
                OrganizationGovernanceServiceError::TeamBindingNotFound(binding_id.clone())
            })
    }

    fn require_policy_binding(
        &self,
        binding_id: &OrganizationPolicyBindingId,
    ) -> Result<OrganizationPolicyBinding, OrganizationGovernanceServiceError> {
        self.repository
            .get_policy_binding(binding_id)?
            .ok_or_else(|| {
                OrganizationGovernanceServiceError::PolicyBindingNotFound(binding_id.clone())
            })
    }

    fn require_exact_policy_target(
        &self,
        target: &OrganizationPolicyTarget,
        require_active: bool,
    ) -> Result<(), OrganizationGovernanceServiceError> {
        let record = self
            .policies
            .get_policy_record(target.record_id())?
            .ok_or(OrganizationGovernanceServiceError::PolicyTargetUnavailable)?;
        if &record.policy_ref() != target.policy_ref()
            || require_active && record.lifecycle() != PermissionPolicyRecordLifecycle::Published
        {
            return Err(OrganizationGovernanceServiceError::PolicyTargetUnavailable);
        }
        if let Some(scope_binding_id) = target.scope_binding_id() {
            let binding = self
                .policies
                .get_scope_binding(scope_binding_id)?
                .ok_or(OrganizationGovernanceServiceError::PolicyTargetUnavailable)?;
            if binding.record_id() != target.record_id()
                || binding.policy_ref() != target.policy_ref()
                || require_active
                    && binding.lifecycle() != PermissionPolicyScopeBindingLifecycle::Active
            {
                return Err(OrganizationGovernanceServiceError::PolicyTargetUnavailable);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn reject_operation<T>(
        &self,
        error: OrganizationGovernanceServiceError,
        kind: GovernanceAuditEventKind,
        subject_type: &str,
        subject_reference: &str,
        organization_id: &OrganizationId,
        not_before: i64,
        expected_revision: Option<u64>,
    ) -> Result<T, OrganizationGovernanceServiceError> {
        let mut values = BTreeMap::from([("reason_code".into(), error.reason_code().into())]);
        if let Some(revision) = expected_revision.filter(|value| *value > 0) {
            values.insert("expected_revision".into(), revision.to_string());
        }
        self.record_event(
            kind,
            GovernanceAuditOutcome::Rejected,
            subject_type,
            subject_reference,
            organization_id,
            SanitizedAuditMetadata::new(values)?,
            not_before,
        )?;
        Err(error)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_event(
        &self,
        kind: GovernanceAuditEventKind,
        outcome: GovernanceAuditOutcome,
        subject_type: &str,
        subject_reference: &str,
        organization_id: &OrganizationId,
        metadata: SanitizedAuditMetadata,
        not_before: i64,
    ) -> Result<GovernanceAuditEvent, OrganizationGovernanceServiceError> {
        let digest = format!("{:x}", Sha256::digest(subject_reference.as_bytes()));
        Ok(self.audit.record(GovernanceAuditRecordRequest {
            stream_id: GovernanceAuditStreamId::new(format!(
                "audit-stream:organization:{}",
                &digest[..32]
            ))?,
            kind,
            outcome,
            actor_reference: self.audit_actor.clone(),
            subject_type: subject_type.to_string(),
            subject_reference: subject_reference.to_string(),
            correlations: AuditCorrelationReferences::new(
                None,
                None,
                None,
                None,
                None,
                None,
                Some(organization_id.to_string()),
            )?,
            metadata,
            not_before,
        })?)
    }
}

fn lifecycle_metadata(
    lifecycle: OrganizationLifecycle,
    revision: u64,
    expected_revision: Option<u64>,
) -> Result<SanitizedAuditMetadata, GovernanceAuditDomainError> {
    let mut values = BTreeMap::from([
        ("lifecycle".into(), lifecycle.as_str().into()),
        ("organization_revision".into(), revision.to_string()),
    ]);
    if let Some(value) = expected_revision {
        values.insert("expected_revision".into(), value.to_string());
    }
    SanitizedAuditMetadata::new(values)
}

fn binding_metadata(
    lifecycle: OrganizationBindingLifecycle,
    revision: u64,
    expected_revision: Option<u64>,
) -> Result<SanitizedAuditMetadata, GovernanceAuditDomainError> {
    let mut values = BTreeMap::from([
        ("lifecycle".into(), lifecycle.as_str().into()),
        ("binding_revision".into(), revision.to_string()),
    ]);
    if let Some(value) = expected_revision {
        values.insert("expected_revision".into(), value.to_string());
    }
    SanitizedAuditMetadata::new(values)
}

fn policy_layer_name(layer: PermissionPolicyLayer) -> &'static str {
    match layer {
        PermissionPolicyLayer::Repository => "repository",
        PermissionPolicyLayer::HumanOwner => "human_owner",
        PermissionPolicyLayer::Team => "team",
        PermissionPolicyLayer::Workflow => "workflow",
        PermissionPolicyLayer::RoleAssignment => "role_assignment",
        PermissionPolicyLayer::Workspace => "workspace",
        PermissionPolicyLayer::Environment => "environment",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        governance_audit::{
            GovernanceAuditRepository, GovernanceAuditService, InMemoryGovernanceAuditRepository,
        },
        governance_time::FixedTrustedClock,
        organization_governance_repository::InMemoryOrganizationGovernanceRepository,
        permission_domain::{
            PermissionAction, PermissionPolicy, PermissionPolicyId, PermissionPolicyLayer,
            PermissionRule, PermissionRuleEffect,
        },
        permission_policy_operations::{PermissionPolicyRecord, PermissionPolicyRecordId},
        permission_policy_operations_repository::InMemoryPermissionPolicyOperationsRepository,
        team_domain::{Team, TeamMembership, TeamMembershipId, TeamMembershipLifecycle},
        team_repository::InMemoryTeamRepository,
    };

    type TestService = OrganizationGovernanceService<
        InMemoryOrganizationGovernanceRepository,
        InMemoryTeamRepository,
        InMemoryPermissionPolicyOperationsRepository,
        FixedTrustedClock,
        GovernanceAuditService<InMemoryGovernanceAuditRepository, FixedTrustedClock>,
    >;

    fn service(at: i64) -> (TestService, InMemoryGovernanceAuditRepository) {
        let audit_repository = InMemoryGovernanceAuditRepository::default();
        let clock = FixedTrustedClock::new(at).unwrap();
        (
            OrganizationGovernanceService::new(
                InMemoryOrganizationGovernanceRepository::default(),
                InMemoryTeamRepository::default(),
                InMemoryPermissionPolicyOperationsRepository::default(),
                clock,
                GovernanceAuditService::new(audit_repository.clone(), clock),
                "actor:codex",
            ),
            audit_repository,
        )
    }

    fn active_team(teams: &InMemoryTeamRepository, id: &str, at: i64) -> Team {
        let draft = Team::new(
            TeamId::new(id).unwrap(),
            id,
            "Governed Team",
            "owner:test",
            Vec::new(),
            Vec::new(),
            at,
        )
        .unwrap();
        teams.insert_team(draft.clone()).unwrap();
        let active = draft.transition_to(TeamLifecycle::Active, 1, at).unwrap();
        teams.update_team(active.clone(), 1).unwrap();
        active
    }

    #[test]
    fn lifecycle_and_team_binding_operations_are_audited() {
        let audit_repository = InMemoryGovernanceAuditRepository::default();
        let teams = InMemoryTeamRepository::default();
        active_team(&teams, "team:one", 10);
        let clock = FixedTrustedClock::new(10).unwrap();
        let service = OrganizationGovernanceService::new(
            InMemoryOrganizationGovernanceRepository::default(),
            teams,
            InMemoryPermissionPolicyOperationsRepository::default(),
            clock,
            GovernanceAuditService::new(audit_repository.clone(), clock),
            "actor:codex",
        );
        let organization = service
            .create_organization(
                OrganizationId::new("organization:one").unwrap(),
                "One",
                "Govern One",
                "owner:test",
                "provenance:cod-031",
            )
            .unwrap();
        service.activate_organization(organization.id(), 1).unwrap();
        let draft = service
            .create_team_binding_draft(
                OrganizationTeamBindingId::new("organization-team-binding:one").unwrap(),
                organization.id(),
                TeamId::new("team:one").unwrap(),
                10,
                None,
                "provenance:cod-031",
            )
            .unwrap();
        service
            .activate_team_binding(organization.id(), draft.id(), 1)
            .unwrap();

        let digest = format!("{:x}", Sha256::digest(draft.id().as_str().as_bytes()));
        let stream =
            GovernanceAuditStreamId::new(format!("audit-stream:organization:{}", &digest[..32]))
                .unwrap();
        assert_eq!(audit_repository.list_stream(&stream, 10).unwrap().len(), 2);
    }

    #[test]
    fn cross_organization_team_ownership_fails_closed_and_is_audited() {
        let audit_repository = InMemoryGovernanceAuditRepository::default();
        let teams = InMemoryTeamRepository::default();
        active_team(&teams, "team:shared", 10);
        let clock = FixedTrustedClock::new(10).unwrap();
        let service = OrganizationGovernanceService::new(
            InMemoryOrganizationGovernanceRepository::default(),
            teams,
            InMemoryPermissionPolicyOperationsRepository::default(),
            clock,
            GovernanceAuditService::new(audit_repository, clock),
            "actor:codex",
        );
        let mut organizations = Vec::new();
        for id in ["organization:one", "organization:two"] {
            let draft = service
                .create_organization(
                    OrganizationId::new(id).unwrap(),
                    id,
                    "Govern bounded work",
                    "owner:test",
                    "provenance:cod-031",
                )
                .unwrap();
            organizations.push(service.activate_organization(draft.id(), 1).unwrap());
        }
        let first = service
            .create_team_binding_draft(
                OrganizationTeamBindingId::new("organization-team-binding:first").unwrap(),
                organizations[0].id(),
                TeamId::new("team:shared").unwrap(),
                10,
                None,
                "provenance:cod-031",
            )
            .unwrap();
        service
            .activate_team_binding(organizations[0].id(), first.id(), 1)
            .unwrap();
        let second = service
            .create_team_binding_draft(
                OrganizationTeamBindingId::new("organization-team-binding:second").unwrap(),
                organizations[1].id(),
                TeamId::new("team:shared").unwrap(),
                10,
                None,
                "provenance:cod-031",
            )
            .unwrap();
        assert!(matches!(
            service.activate_team_binding(organizations[1].id(), second.id(), 1),
            Err(OrganizationGovernanceServiceError::CrossOrganizationReference)
        ));
    }

    #[test]
    fn boundary_resolution_denies_cross_scope_and_scoped_views_do_not_leak() {
        let (service, _) = service(10);
        let first = service
            .create_organization(
                OrganizationId::new("organization:one").unwrap(),
                "One",
                "Govern One",
                "owner:test",
                "provenance:cod-031",
            )
            .unwrap();
        let first = service.activate_organization(first.id(), 1).unwrap();
        let second = service
            .create_organization(
                OrganizationId::new("organization:two").unwrap(),
                "Two",
                "Govern Two",
                "owner:test",
                "provenance:cod-031",
            )
            .unwrap();
        service.activate_organization(second.id(), 1).unwrap();
        let references = OrganizationBoundaryReferences::new(
            first.id().clone(),
            Some(first.revision()),
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
        let evidence = service
            .resolve_boundary(
                OrganizationBoundaryResolutionRequest::new(
                    OrganizationBoundaryEvidenceId::new("organization-boundary:cross").unwrap(),
                    OrganizationId::new("organization:two").unwrap(),
                    references,
                    "provenance:cod-031",
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            evidence.outcome(),
            OrganizationBoundaryOutcome::Denied(
                OrganizationBoundaryDenialReason::CrossOrganizationReference
            )
        );
        let first_view = service.management_view(first.id(), 10).unwrap();
        assert_eq!(first_view.boundary_evidence.len(), 1);
        let second_view = service
            .management_view(&OrganizationId::new("organization:two").unwrap(), 10)
            .unwrap();
        assert!(second_view.boundary_evidence.is_empty());
    }

    #[test]
    fn missing_organization_returns_immutable_deny_evidence() {
        let (service, _) = service(10);
        let organization_id = OrganizationId::new("organization:missing").unwrap();
        let references = OrganizationBoundaryReferences::new(
            organization_id.clone(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("workflow:missing".into()),
            None,
            None,
        )
        .unwrap();
        let evidence = service
            .resolve_boundary(
                OrganizationBoundaryResolutionRequest::new(
                    OrganizationBoundaryEvidenceId::new("organization-boundary:missing").unwrap(),
                    organization_id,
                    references,
                    "provenance:cod-031",
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            evidence.outcome(),
            OrganizationBoundaryOutcome::Denied(
                OrganizationBoundaryDenialReason::OrganizationNotFound
            )
        );
    }

    #[test]
    fn management_and_binding_operations_do_not_mutate_team_identity() {
        let audit_repository = InMemoryGovernanceAuditRepository::default();
        let teams = InMemoryTeamRepository::default();
        let original = active_team(&teams, "team:immutable", 10);
        let clock = FixedTrustedClock::new(10).unwrap();
        let service = OrganizationGovernanceService::new(
            InMemoryOrganizationGovernanceRepository::default(),
            teams.clone(),
            InMemoryPermissionPolicyOperationsRepository::default(),
            clock,
            GovernanceAuditService::new(audit_repository, clock),
            "actor:codex",
        );
        let organization = service
            .create_organization(
                OrganizationId::new("organization:one").unwrap(),
                "One",
                "Govern One",
                "owner:test",
                "provenance:cod-031",
            )
            .unwrap();
        service.activate_organization(organization.id(), 1).unwrap();
        let draft = service
            .create_team_binding_draft(
                OrganizationTeamBindingId::new("organization-team-binding:immutable").unwrap(),
                organization.id(),
                original.id().clone(),
                10,
                None,
                "provenance:cod-031",
            )
            .unwrap();
        service
            .activate_team_binding(organization.id(), draft.id(), 1)
            .unwrap();
        assert_eq!(teams.get_team(original.id()).unwrap(), Some(original));
    }

    #[test]
    fn exact_policy_binding_creates_scope_without_decision_or_grant() {
        let audit_repository = InMemoryGovernanceAuditRepository::default();
        let policies = InMemoryPermissionPolicyOperationsRepository::default();
        let policy = PermissionPolicy::new(
            PermissionPolicyId::new("permission-policy:organization").unwrap(),
            1,
            PermissionPolicyLayer::Team,
            "owner:test",
            vec![PermissionRule::new(
                PermissionRuleEffect::Deny,
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:repository",
                BTreeMap::new(),
            )
            .unwrap()],
        )
        .unwrap();
        let draft_record = PermissionPolicyRecord::new_draft(
            PermissionPolicyRecordId::new("policy-record:organization").unwrap(),
            policy,
            "provenance:cod-031",
            None,
            10,
        )
        .unwrap();
        policies.insert_policy_record(draft_record.clone()).unwrap();
        let record = draft_record.publish(1, 10).unwrap();
        policies.update_policy_record(record.clone(), 1).unwrap();
        let clock = FixedTrustedClock::new(10).unwrap();
        let service = OrganizationGovernanceService::new(
            InMemoryOrganizationGovernanceRepository::default(),
            InMemoryTeamRepository::default(),
            policies.clone(),
            clock,
            GovernanceAuditService::new(audit_repository, clock),
            "actor:codex",
        );
        let organization = service
            .create_organization(
                OrganizationId::new("organization:policy").unwrap(),
                "Policy Organization",
                "Scope exact policy versions",
                "owner:test",
                "provenance:cod-031",
            )
            .unwrap();
        let organization = service.activate_organization(organization.id(), 1).unwrap();
        let target = OrganizationPolicyTarget::PolicyRecord {
            record_id: record.id().clone(),
            policy_ref: record.policy_ref(),
        };
        let draft = service
            .create_policy_binding_draft(
                OrganizationPolicyBindingId::new("organization-policy-binding:one").unwrap(),
                organization.id(),
                target.clone(),
                10,
                None,
                "provenance:cod-031",
            )
            .unwrap();
        let active = service
            .activate_policy_binding(organization.id(), draft.id(), 1)
            .unwrap();
        assert_eq!(active.lifecycle(), OrganizationBindingLifecycle::Active);
        let second = service
            .create_organization(
                OrganizationId::new("organization:policy-two").unwrap(),
                "Policy Organization Two",
                "Reject cross-organization policy ownership",
                "owner:test",
                "provenance:cod-031",
            )
            .unwrap();
        let second = service.activate_organization(second.id(), 1).unwrap();
        let second_binding = service
            .create_policy_binding_draft(
                OrganizationPolicyBindingId::new("organization-policy-binding:two").unwrap(),
                second.id(),
                target,
                10,
                None,
                "provenance:cod-031",
            )
            .unwrap();
        assert!(matches!(
            service.activate_policy_binding(second.id(), second_binding.id(), 1),
            Err(OrganizationGovernanceServiceError::CrossOrganizationReference)
        ));
        assert!(policies.list_selection_evidence(10).unwrap().is_empty());
        assert_eq!(
            policies.get_policy_record(record.id()).unwrap(),
            Some(record)
        );
    }

    #[test]
    fn stale_lifecycle_rejection_is_audited_without_state_change() {
        let (service, audit_repository) = service(10);
        let draft = service
            .create_organization(
                OrganizationId::new("organization:stale").unwrap(),
                "Stale",
                "Verify optimistic concurrency",
                "owner:test",
                "provenance:cod-031",
            )
            .unwrap();
        let active = service.activate_organization(draft.id(), 1).unwrap();
        assert!(matches!(
            service.suspend_organization(active.id(), 1),
            Err(OrganizationGovernanceServiceError::Domain(
                OrganizationGovernanceDomainError::StaleRevision { .. }
            ))
        ));
        assert_eq!(
            service
                .management_view(active.id(), 10)
                .unwrap()
                .organization
                .revision,
            2
        );
        let digest = format!("{:x}", Sha256::digest(active.id().as_str().as_bytes()));
        let stream =
            GovernanceAuditStreamId::new(format!("audit-stream:organization:{}", &digest[..32]))
                .unwrap();
        assert_eq!(audit_repository.list_stream(&stream, 10).unwrap().len(), 3);
    }

    #[test]
    fn derived_agent_scope_uses_membership_evidence_without_mutating_membership() {
        let audit_repository = InMemoryGovernanceAuditRepository::default();
        let teams = InMemoryTeamRepository::default();
        let team = active_team(&teams, "team:derived", 10);
        let invited = TeamMembership::new(
            TeamMembershipId::new("membership:derived").unwrap(),
            team.id().clone(),
            "agent:derived",
            None,
            Vec::new(),
            "provenance:membership",
            10,
            None,
            10,
        )
        .unwrap();
        teams.insert_membership(invited.clone()).unwrap();
        let membership = invited
            .transition_to(TeamMembershipLifecycle::Active, 1, 10)
            .unwrap();
        teams.update_membership(membership.clone(), 1).unwrap();
        let clock = FixedTrustedClock::new(10).unwrap();
        let service = OrganizationGovernanceService::new(
            InMemoryOrganizationGovernanceRepository::default(),
            teams.clone(),
            InMemoryPermissionPolicyOperationsRepository::default(),
            clock,
            GovernanceAuditService::new(audit_repository, clock),
            "actor:codex",
        );
        let organization = service
            .create_organization(
                OrganizationId::new("organization:derived").unwrap(),
                "Derived",
                "Derive temporary Agent scope",
                "owner:test",
                "provenance:cod-031",
            )
            .unwrap();
        let organization = service.activate_organization(organization.id(), 1).unwrap();
        let team_binding = service
            .create_team_binding_draft(
                OrganizationTeamBindingId::new("organization-team-binding:derived").unwrap(),
                organization.id(),
                team.id().clone(),
                10,
                None,
                "provenance:cod-031",
            )
            .unwrap();
        let team_binding = service
            .activate_team_binding(organization.id(), team_binding.id(), 1)
            .unwrap();
        let references = OrganizationBoundaryReferences::new(
            organization.id().clone(),
            Some(organization.revision()),
            Some(team.id().clone()),
            Some(team_binding.id().clone()),
            Some(team_binding.revision()),
            None,
            None,
            Some(membership.id().clone()),
            Some(membership.revision()),
            Some(membership.agent_id().to_string()),
            None,
            None,
            None,
        )
        .unwrap();
        let evidence = service
            .resolve_boundary(
                OrganizationBoundaryResolutionRequest::new(
                    OrganizationBoundaryEvidenceId::new("organization-boundary:derived").unwrap(),
                    organization.id().clone(),
                    references,
                    "provenance:cod-031",
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(evidence.outcome(), OrganizationBoundaryOutcome::Accepted);
        assert_eq!(evidence.references().agent_ref(), Some("agent:derived"));
        assert_eq!(
            teams.get_membership(membership.id()).unwrap(),
            Some(membership)
        );
    }
}
