//! Audited application and management boundary for Permission policy operations.
//!
//! This service manages the operational lifecycle around existing immutable
//! `PermissionPolicy` definitions. Policy selection resolves exact versions for
//! an existing evaluator; it never evaluates a request, creates an
//! Authorization Decision, or issues a Permission Grant.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    governance_audit::{
        AuditCorrelationReferences, GovernanceAuditDomainError, GovernanceAuditEventKind,
        GovernanceAuditOutcome, GovernanceAuditRecordRequest, GovernanceAuditServiceError,
        GovernanceAuditSink, GovernanceAuditStreamId, SanitizedAuditMetadata,
    },
    governance_time::{TrustedClock, TrustedClockError},
    permission_domain::{PermissionPolicy, PermissionPolicyLayer, PermissionPolicyVersionRef},
    permission_policy_operations::{
        permission_policy_layer_precedence, PermissionPolicyOperationsDomainError,
        PermissionPolicyRecord, PermissionPolicyRecordId, PermissionPolicyRecordLifecycle,
        PermissionPolicyScopeBinding, PermissionPolicyScopeBindingId,
        PermissionPolicyScopeBindingLifecycle, PermissionPolicyScopeEvidence,
        PermissionPolicyScopeKind, PermissionPolicyScopeSelector,
        PermissionPolicySelectionEvidence, PermissionPolicySelectionEvidenceId,
        PermissionPolicySelectionFailure, PermissionPolicySelectionOutcome,
    },
    permission_policy_operations_repository::{
        PermissionPolicyOperationsRepository, PermissionPolicyOperationsRepositoryError,
        MAX_PERMISSION_POLICY_QUERY_LIMIT,
    },
};

#[derive(Debug, Error)]
pub enum PermissionPolicyOperationsServiceError {
    #[error(transparent)]
    Domain(#[from] PermissionPolicyOperationsDomainError),
    #[error(transparent)]
    Repository(#[from] PermissionPolicyOperationsRepositoryError),
    #[error(transparent)]
    Clock(#[from] TrustedClockError),
    #[error(transparent)]
    Audit(#[from] GovernanceAuditServiceError),
    #[error(transparent)]
    AuditDomain(#[from] GovernanceAuditDomainError),
    #[error("Permission policy record was not found: {0}")]
    PolicyRecordNotFound(PermissionPolicyRecordId),
    #[error("Permission policy scope binding was not found: {0}")]
    ScopeBindingNotFound(PermissionPolicyScopeBindingId),
    #[error("Permission policy record is not Published: {0}")]
    PolicyNotPublished(PermissionPolicyRecordId),
    #[error("Retired Permission policy cannot become active: {0}")]
    RetiredPolicy(PermissionPolicyRecordId),
    #[error("Permission policy still has an Active scope binding: {0}")]
    PolicyHasActiveBinding(PermissionPolicyRecordId),
    #[error("Replacement binding does not explicitly replace the active policy version")]
    ReplacementLineageMismatch,
}

impl PermissionPolicyOperationsServiceError {
    fn reason_code(&self) -> &'static str {
        match self {
            Self::Domain(PermissionPolicyOperationsDomainError::StaleRevision { .. }) => {
                "stale_revision"
            }
            Self::Domain(
                PermissionPolicyOperationsDomainError::InvalidPolicyLifecycle
                | PermissionPolicyOperationsDomainError::InvalidBindingLifecycle,
            ) => "invalid_lifecycle",
            Self::Domain(
                PermissionPolicyOperationsDomainError::PolicyReferenceMismatch
                | PermissionPolicyOperationsDomainError::InvalidReplacement,
            ) => "scope_mismatch",
            Self::Domain(_) => "domain_validation",
            Self::Repository(PermissionPolicyOperationsRepositoryError::RevisionConflict {
                ..
            }) => "stale_revision",
            Self::Repository(
                PermissionPolicyOperationsRepositoryError::ActiveBindingConflict
                | PermissionPolicyOperationsRepositoryError::PolicyHasActiveBinding(_),
            ) => "active_binding_exists",
            Self::Repository(PermissionPolicyOperationsRepositoryError::PolicyNotPublished(_)) => {
                "policy_not_published"
            }
            Self::Repository(
                PermissionPolicyOperationsRepositoryError::PolicyRecordMismatch(_)
                | PermissionPolicyOperationsRepositoryError::ReplacementSelectorMismatch,
            ) => "scope_mismatch",
            Self::Repository(PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                ..
            }) => "invalid_lifecycle",
            Self::Repository(PermissionPolicyOperationsRepositoryError::NotFound { .. }) => {
                "policy_not_found"
            }
            Self::Repository(_) => "boundary_repository",
            Self::Clock(_) => "trusted_clock",
            Self::Audit(_) => "audit_failure",
            Self::AuditDomain(_) => "audit_validation",
            Self::PolicyRecordNotFound(_) => "policy_not_found",
            Self::ScopeBindingNotFound(_) | Self::ReplacementLineageMismatch => "scope_mismatch",
            Self::PolicyNotPublished(_) => "policy_not_published",
            Self::RetiredPolicy(_) => "retired_policy",
            Self::PolicyHasActiveBinding(_) => "active_binding_exists",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPolicyRecordManagementView {
    pub record_id: String,
    pub policy_id: String,
    pub policy_version: u16,
    pub policy_layer: PermissionPolicyLayer,
    pub owner_ref: String,
    pub rule_count: usize,
    pub lifecycle: PermissionPolicyRecordLifecycle,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub published_at: Option<i64>,
    pub retired_at: Option<i64>,
    pub provenance_ref: String,
    pub replaces: Option<PermissionPolicyVersionRef>,
}

impl From<&PermissionPolicyRecord> for PermissionPolicyRecordManagementView {
    fn from(record: &PermissionPolicyRecord) -> Self {
        Self {
            record_id: record.id().to_string(),
            policy_id: record.policy().id().to_string(),
            policy_version: record.policy().version(),
            policy_layer: record.policy().layer(),
            owner_ref: record.policy().owner_ref().to_string(),
            rule_count: record.policy().rules().len(),
            lifecycle: record.lifecycle(),
            revision: record.revision(),
            created_at: record.created_at(),
            updated_at: record.updated_at(),
            published_at: record.published_at(),
            retired_at: record.retired_at(),
            provenance_ref: record.provenance_ref().to_string(),
            replaces: record.replaces().cloned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPolicyRecordInspectionView {
    pub summary: PermissionPolicyRecordManagementView,
    pub definition: PermissionPolicy,
}

impl From<&PermissionPolicyRecord> for PermissionPolicyRecordInspectionView {
    fn from(record: &PermissionPolicyRecord) -> Self {
        Self {
            summary: record.into(),
            definition: record.policy().clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPolicyScopeBindingManagementView {
    pub binding_id: String,
    pub policy_record_id: String,
    pub policy: PermissionPolicyVersionRef,
    pub scope_kind: PermissionPolicyScopeKind,
    pub scope_ref: String,
    pub boundary_ref: Option<String>,
    pub lifecycle: PermissionPolicyScopeBindingLifecycle,
    pub revision: u64,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub activated_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub provenance_ref: String,
}

impl From<&PermissionPolicyScopeBinding> for PermissionPolicyScopeBindingManagementView {
    fn from(binding: &PermissionPolicyScopeBinding) -> Self {
        Self {
            binding_id: binding.id().to_string(),
            policy_record_id: binding.record_id().to_string(),
            policy: binding.policy_ref().clone(),
            scope_kind: binding.selector().scope_kind(),
            scope_ref: binding.selector().scope_ref().to_string(),
            boundary_ref: binding.selector().boundary_ref().map(ToString::to_string),
            lifecycle: binding.lifecycle(),
            revision: binding.revision(),
            valid_from: binding.valid_from(),
            valid_until: binding.valid_until(),
            created_at: binding.created_at(),
            updated_at: binding.updated_at(),
            activated_at: binding.activated_at(),
            ended_at: binding.ended_at(),
            provenance_ref: binding.provenance_ref().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPolicySelectionManagementView {
    pub selection_evidence_id: String,
    pub scopes: Vec<PermissionPolicyScopeEvidence>,
    pub selected_policy_versions: Vec<PermissionPolicyVersionRef>,
    pub outcome: PermissionPolicySelectionOutcome,
    pub selected_at: i64,
}

impl From<&PermissionPolicySelectionEvidence> for PermissionPolicySelectionManagementView {
    fn from(evidence: &PermissionPolicySelectionEvidence) -> Self {
        Self {
            selection_evidence_id: evidence.id().to_string(),
            scopes: evidence.scopes().to_vec(),
            selected_policy_versions: evidence.selected_policy_versions().to_vec(),
            outcome: evidence.outcome(),
            selected_at: evidence.selected_at(),
        }
    }
}

pub struct PermissionPolicyOperationsService<R, C, A> {
    repository: R,
    clock: C,
    audit: A,
    audit_actor: String,
}

impl<R, C, A> PermissionPolicyOperationsService<R, C, A> {
    pub fn new(repository: R, clock: C, audit: A, audit_actor: impl Into<String>) -> Self {
        Self {
            repository,
            clock,
            audit,
            audit_actor: audit_actor.into(),
        }
    }
}

impl<R, C, A> PermissionPolicyOperationsService<R, C, A>
where
    R: PermissionPolicyOperationsRepository,
    C: TrustedClock,
    A: GovernanceAuditSink,
{
    pub fn create_draft_policy_record(
        &self,
        record_id: PermissionPolicyRecordId,
        policy: PermissionPolicy,
        provenance_ref: impl Into<String>,
        replaces: Option<PermissionPolicyVersionRef>,
    ) -> Result<PermissionPolicyRecord, PermissionPolicyOperationsServiceError> {
        let subject = record_id.to_string();
        let at = self.trusted_now("permission_policy_record", &subject, Some(&record_id))?;
        let record = match PermissionPolicyRecord::new_draft(
            record_id.clone(),
            policy,
            provenance_ref,
            replaces,
            at,
        ) {
            Ok(record) => record,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_record",
                    &subject,
                    Some(&record_id),
                    at,
                    None,
                )
            }
        };
        let metadata = self.policy_metadata(&record, None)?;
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicyDraftCreated,
            GovernanceAuditOutcome::Created,
            "permission_policy_record",
            &subject,
            Some(&record_id),
            metadata,
            at,
        )?;
        if let Err(error) = self.repository.insert_policy_record(record.clone()) {
            return self.reject_operation(
                error.into(),
                "permission_policy_record",
                &subject,
                Some(&record_id),
                at,
                None,
            );
        }
        Ok(record)
    }

    pub fn publish_policy_record(
        &self,
        record_id: &PermissionPolicyRecordId,
        expected_revision: u64,
    ) -> Result<PermissionPolicyRecord, PermissionPolicyOperationsServiceError> {
        let subject = record_id.to_string();
        let at = self.trusted_now("permission_policy_record", &subject, Some(record_id))?;
        let current = match self.require_policy_record(record_id) {
            Ok(record) => record,
            Err(error) => {
                return self.reject_operation(
                    error,
                    "permission_policy_record",
                    &subject,
                    Some(record_id),
                    at,
                    Some(expected_revision),
                )
            }
        };
        let published = match current.publish(expected_revision, at) {
            Ok(record) => record,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_record",
                    &subject,
                    Some(record_id),
                    at,
                    Some(expected_revision),
                )
            }
        };
        let metadata = self.policy_metadata(&published, Some(expected_revision))?;
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicyPublished,
            GovernanceAuditOutcome::Updated,
            "permission_policy_record",
            &subject,
            Some(record_id),
            metadata,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_policy_record(published.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                "permission_policy_record",
                &subject,
                Some(record_id),
                at,
                Some(expected_revision),
            );
        }
        Ok(published)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_scope_binding_draft(
        &self,
        binding_id: PermissionPolicyScopeBindingId,
        record_id: &PermissionPolicyRecordId,
        scope: PermissionPolicyScopeEvidence,
        valid_from: i64,
        valid_until: Option<i64>,
        provenance_ref: impl Into<String>,
    ) -> Result<PermissionPolicyScopeBinding, PermissionPolicyOperationsServiceError> {
        let subject = binding_id.to_string();
        let at = self.trusted_now("permission_policy_scope_binding", &subject, Some(record_id))?;
        let record = match self.require_published_policy_record(record_id) {
            Ok(record) => record,
            Err(error) => {
                return self.reject_operation(
                    error,
                    "permission_policy_scope_binding",
                    &subject,
                    Some(record_id),
                    at,
                    None,
                )
            }
        };
        let selector = match PermissionPolicyScopeSelector::new(record.policy().layer(), scope) {
            Ok(selector) => selector,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_scope_binding",
                    &subject,
                    Some(record_id),
                    at,
                    None,
                )
            }
        };
        let binding = match PermissionPolicyScopeBinding::new_draft(
            binding_id,
            record.id().clone(),
            record.policy_ref(),
            selector,
            valid_from,
            valid_until,
            provenance_ref,
            at,
        ) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_scope_binding",
                    &subject,
                    Some(record_id),
                    at,
                    None,
                )
            }
        };
        let metadata = self.binding_metadata(&binding, None)?;
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicyScopeBindingCreated,
            GovernanceAuditOutcome::Created,
            "permission_policy_scope_binding",
            &subject,
            Some(record_id),
            metadata,
            at,
        )?;
        if let Err(error) = self.repository.insert_scope_binding(binding.clone()) {
            return self.reject_operation(
                error.into(),
                "permission_policy_scope_binding",
                &subject,
                Some(record_id),
                at,
                None,
            );
        }
        Ok(binding)
    }

    pub fn activate_scope_binding(
        &self,
        binding_id: &PermissionPolicyScopeBindingId,
        expected_revision: u64,
    ) -> Result<PermissionPolicyScopeBinding, PermissionPolicyOperationsServiceError> {
        let subject = binding_id.to_string();
        let at = self.trusted_now("permission_policy_scope_binding", &subject, None)?;
        let current = match self.require_scope_binding(binding_id) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error,
                    "permission_policy_scope_binding",
                    &subject,
                    None,
                    at,
                    Some(expected_revision),
                )
            }
        };
        let record_id = current.record_id().clone();
        if let Err(error) = self.require_published_policy_record(&record_id) {
            return self.reject_operation(
                error,
                "permission_policy_scope_binding",
                &subject,
                Some(&record_id),
                at,
                Some(expected_revision),
            );
        }
        let active = match current.activate(expected_revision, at) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_scope_binding",
                    &subject,
                    Some(&record_id),
                    at,
                    Some(expected_revision),
                )
            }
        };
        let metadata = self.binding_metadata(&active, Some(expected_revision))?;
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicyScopeBindingActivated,
            GovernanceAuditOutcome::Updated,
            "permission_policy_scope_binding",
            &subject,
            Some(&record_id),
            metadata,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_scope_binding(active.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                "permission_policy_scope_binding",
                &subject,
                Some(&record_id),
                at,
                Some(expected_revision),
            );
        }
        Ok(active)
    }

    pub fn replace_active_policy_version(
        &self,
        active_binding_id: &PermissionPolicyScopeBindingId,
        active_expected_revision: u64,
        replacement_binding_id: &PermissionPolicyScopeBindingId,
        replacement_expected_revision: u64,
    ) -> Result<
        (PermissionPolicyScopeBinding, PermissionPolicyScopeBinding),
        PermissionPolicyOperationsServiceError,
    > {
        let subject = replacement_binding_id.to_string();
        let at = self.trusted_now("permission_policy_scope_binding", &subject, None)?;
        let active = match self.require_scope_binding(active_binding_id) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error,
                    "permission_policy_scope_binding",
                    &subject,
                    None,
                    at,
                    Some(active_expected_revision),
                )
            }
        };
        let replacement = match self.require_scope_binding(replacement_binding_id) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error,
                    "permission_policy_scope_binding",
                    &subject,
                    None,
                    at,
                    Some(replacement_expected_revision),
                )
            }
        };
        let replacement_record_id = replacement.record_id().clone();
        let replacement_record = match self.require_published_policy_record(&replacement_record_id)
        {
            Ok(record) => record,
            Err(error) => {
                return self.reject_operation(
                    error,
                    "permission_policy_scope_binding",
                    &subject,
                    Some(&replacement_record_id),
                    at,
                    Some(replacement_expected_revision),
                )
            }
        };
        if replacement_record.replaces() != Some(active.policy_ref())
            || active.selector() != replacement.selector()
        {
            return self.reject_operation(
                PermissionPolicyOperationsServiceError::ReplacementLineageMismatch,
                "permission_policy_scope_binding",
                &subject,
                Some(&replacement_record_id),
                at,
                Some(replacement_expected_revision),
            );
        }
        let ended = match active.end(active_expected_revision, at) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_scope_binding",
                    &subject,
                    Some(&replacement_record_id),
                    at,
                    Some(active_expected_revision),
                )
            }
        };
        let activated = match replacement.activate(replacement_expected_revision, at) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_scope_binding",
                    &subject,
                    Some(&replacement_record_id),
                    at,
                    Some(replacement_expected_revision),
                )
            }
        };
        let metadata = self.binding_metadata(&activated, Some(replacement_expected_revision))?;
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicyVersionReplaced,
            GovernanceAuditOutcome::Updated,
            "permission_policy_scope_binding",
            &subject,
            Some(&replacement_record_id),
            metadata,
            at,
        )?;
        if let Err(error) = self.repository.replace_active_binding(
            ended.clone(),
            active_expected_revision,
            activated.clone(),
            replacement_expected_revision,
        ) {
            return self.reject_operation(
                error.into(),
                "permission_policy_scope_binding",
                &subject,
                Some(&replacement_record_id),
                at,
                Some(replacement_expected_revision),
            );
        }
        Ok((ended, activated))
    }

    pub fn end_scope_binding(
        &self,
        binding_id: &PermissionPolicyScopeBindingId,
        expected_revision: u64,
    ) -> Result<PermissionPolicyScopeBinding, PermissionPolicyOperationsServiceError> {
        let subject = binding_id.to_string();
        let at = self.trusted_now("permission_policy_scope_binding", &subject, None)?;
        let current = match self.require_scope_binding(binding_id) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error,
                    "permission_policy_scope_binding",
                    &subject,
                    None,
                    at,
                    Some(expected_revision),
                )
            }
        };
        let record_id = current.record_id().clone();
        let ended = match current.end(expected_revision, at) {
            Ok(binding) => binding,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_scope_binding",
                    &subject,
                    Some(&record_id),
                    at,
                    Some(expected_revision),
                )
            }
        };
        let metadata = self.binding_metadata(&ended, Some(expected_revision))?;
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicyScopeBindingEnded,
            GovernanceAuditOutcome::Updated,
            "permission_policy_scope_binding",
            &subject,
            Some(&record_id),
            metadata,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_scope_binding(ended.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                "permission_policy_scope_binding",
                &subject,
                Some(&record_id),
                at,
                Some(expected_revision),
            );
        }
        Ok(ended)
    }

    pub fn retire_policy_record(
        &self,
        record_id: &PermissionPolicyRecordId,
        expected_revision: u64,
    ) -> Result<PermissionPolicyRecord, PermissionPolicyOperationsServiceError> {
        let subject = record_id.to_string();
        let at = self.trusted_now("permission_policy_record", &subject, Some(record_id))?;
        let current = match self.require_policy_record(record_id) {
            Ok(record) => record,
            Err(error) => {
                return self.reject_operation(
                    error,
                    "permission_policy_record",
                    &subject,
                    Some(record_id),
                    at,
                    Some(expected_revision),
                )
            }
        };
        let bindings = match self
            .repository
            .list_scope_bindings_for_record(record_id, MAX_PERMISSION_POLICY_QUERY_LIMIT)
        {
            Ok(bindings) => bindings,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_record",
                    &subject,
                    Some(record_id),
                    at,
                    Some(expected_revision),
                )
            }
        };
        if bindings
            .iter()
            .any(|binding| binding.lifecycle() == PermissionPolicyScopeBindingLifecycle::Active)
        {
            return self.reject_operation(
                PermissionPolicyOperationsServiceError::PolicyHasActiveBinding(record_id.clone()),
                "permission_policy_record",
                &subject,
                Some(record_id),
                at,
                Some(expected_revision),
            );
        }
        let retired = match current.retire(expected_revision, at) {
            Ok(record) => record,
            Err(error) => {
                return self.reject_operation(
                    error.into(),
                    "permission_policy_record",
                    &subject,
                    Some(record_id),
                    at,
                    Some(expected_revision),
                )
            }
        };
        let metadata = self.policy_metadata(&retired, Some(expected_revision))?;
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicyRetired,
            GovernanceAuditOutcome::Updated,
            "permission_policy_record",
            &subject,
            Some(record_id),
            metadata,
            at,
        )?;
        if let Err(error) = self
            .repository
            .update_policy_record(retired.clone(), expected_revision)
        {
            return self.reject_operation(
                error.into(),
                "permission_policy_record",
                &subject,
                Some(record_id),
                at,
                Some(expected_revision),
            );
        }
        Ok(retired)
    }

    pub fn select_policies(
        &self,
        scopes: Vec<PermissionPolicyScopeEvidence>,
    ) -> Result<PermissionPolicySelectionEvidence, PermissionPolicyOperationsServiceError> {
        let evidence_id = PermissionPolicySelectionEvidenceId::new(format!(
            "policy-selection:{}",
            uuid::Uuid::new_v4()
        ))?;
        let subject = evidence_id.to_string();
        let at = self.trusted_now("permission_policy_selection", &subject, None)?;
        let evidence = match self.select_at(evidence_id, scopes, at) {
            Ok(evidence) => evidence,
            Err(error) => {
                self.record_selection_error(&subject, &error, at)?;
                return Err(error);
            }
        };
        let (kind, outcome) = match evidence.outcome() {
            PermissionPolicySelectionOutcome::Selected => (
                GovernanceAuditEventKind::PermissionPolicySelectionAccepted,
                GovernanceAuditOutcome::Accepted,
            ),
            PermissionPolicySelectionOutcome::Denied(
                PermissionPolicySelectionFailure::NoPolicy,
            ) => (
                GovernanceAuditEventKind::PermissionPolicySelectionRejected,
                GovernanceAuditOutcome::NoPolicy,
            ),
            PermissionPolicySelectionOutcome::Denied(_) => (
                GovernanceAuditEventKind::PermissionPolicySelectionRejected,
                GovernanceAuditOutcome::Denied,
            ),
        };
        let metadata = self.selection_metadata(&evidence)?;
        self.record_event(
            kind,
            outcome,
            "permission_policy_selection",
            &subject,
            None,
            metadata,
            at,
        )?;
        if let Err(error) = self.repository.append_selection_evidence(evidence.clone()) {
            let error = PermissionPolicyOperationsServiceError::Repository(error);
            self.record_selection_error(&subject, &error, at)?;
            return Err(error);
        }
        Ok(evidence)
    }

    pub fn list_policy_record_views(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyRecordManagementView>, PermissionPolicyOperationsServiceError>
    {
        Ok(self
            .repository
            .list_policy_records(limit)?
            .iter()
            .map(Into::into)
            .collect())
    }

    pub fn inspect_policy_record(
        &self,
        record_id: &PermissionPolicyRecordId,
    ) -> Result<Option<PermissionPolicyRecordInspectionView>, PermissionPolicyOperationsServiceError>
    {
        Ok(self
            .repository
            .get_policy_record(record_id)?
            .as_ref()
            .map(Into::into))
    }

    pub fn list_scope_binding_views(
        &self,
        limit: usize,
    ) -> Result<
        Vec<PermissionPolicyScopeBindingManagementView>,
        PermissionPolicyOperationsServiceError,
    > {
        Ok(self
            .repository
            .list_scope_bindings(limit)?
            .iter()
            .map(Into::into)
            .collect())
    }

    pub fn get_scope_binding_view(
        &self,
        binding_id: &PermissionPolicyScopeBindingId,
    ) -> Result<
        Option<PermissionPolicyScopeBindingManagementView>,
        PermissionPolicyOperationsServiceError,
    > {
        Ok(self
            .repository
            .get_scope_binding(binding_id)?
            .as_ref()
            .map(Into::into))
    }

    pub fn list_selection_evidence_views(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicySelectionManagementView>, PermissionPolicyOperationsServiceError>
    {
        Ok(self
            .repository
            .list_selection_evidence(limit)?
            .iter()
            .map(Into::into)
            .collect())
    }

    pub fn get_selection_evidence_view(
        &self,
        evidence_id: &PermissionPolicySelectionEvidenceId,
    ) -> Result<
        Option<PermissionPolicySelectionManagementView>,
        PermissionPolicyOperationsServiceError,
    > {
        Ok(self
            .repository
            .get_selection_evidence(evidence_id)?
            .as_ref()
            .map(Into::into))
    }

    fn select_at(
        &self,
        evidence_id: PermissionPolicySelectionEvidenceId,
        scopes: Vec<PermissionPolicyScopeEvidence>,
        selected_at: i64,
    ) -> Result<PermissionPolicySelectionEvidence, PermissionPolicyOperationsServiceError> {
        PermissionPolicySelectionEvidence::new(
            evidence_id.clone(),
            scopes.clone(),
            Vec::new(),
            PermissionPolicySelectionOutcome::Denied(PermissionPolicySelectionFailure::NoPolicy),
            selected_at,
        )?;

        let effective = self.repository.list_effective_bindings(
            &scopes,
            selected_at,
            MAX_PERMISSION_POLICY_QUERY_LIMIT,
        )?;
        if effective.is_empty() {
            let failure = self.classify_empty_selection(&scopes)?;
            return Ok(PermissionPolicySelectionEvidence::new(
                evidence_id,
                scopes,
                Vec::new(),
                PermissionPolicySelectionOutcome::Denied(failure),
                selected_at,
            )?);
        }

        let mut selector_counts = HashMap::<PermissionPolicyScopeSelector, usize>::new();
        for binding in &effective {
            *selector_counts
                .entry(binding.selector().clone())
                .or_default() += 1;
        }
        if selector_counts.values().any(|count| *count > 1) {
            return Ok(PermissionPolicySelectionEvidence::new(
                evidence_id,
                scopes,
                Vec::new(),
                PermissionPolicySelectionOutcome::Denied(
                    PermissionPolicySelectionFailure::AmbiguousPolicy,
                ),
                selected_at,
            )?);
        }

        let mut selected = BTreeMap::<(u8, String, u16), PermissionPolicyVersionRef>::new();
        for binding in effective {
            let Some(record) = self.repository.get_policy_record(binding.record_id())? else {
                return Ok(Self::denied_selection(
                    evidence_id,
                    scopes,
                    PermissionPolicySelectionFailure::OutOfScope,
                    selected_at,
                )?);
            };
            if record.lifecycle() == PermissionPolicyRecordLifecycle::Retired {
                return Ok(Self::denied_selection(
                    evidence_id,
                    scopes,
                    PermissionPolicySelectionFailure::RetiredPolicy,
                    selected_at,
                )?);
            }
            if record.lifecycle() != PermissionPolicyRecordLifecycle::Published
                || record.policy_ref() != *binding.policy_ref()
            {
                return Ok(Self::denied_selection(
                    evidence_id,
                    scopes,
                    PermissionPolicySelectionFailure::OutOfScope,
                    selected_at,
                )?);
            }
            let reference = binding.policy_ref().clone();
            selected.insert(
                (
                    permission_policy_layer_precedence(reference.layer()),
                    reference.policy_id().to_string(),
                    reference.version(),
                ),
                reference,
            );
        }
        Ok(PermissionPolicySelectionEvidence::new(
            evidence_id,
            scopes,
            selected.into_values().collect(),
            PermissionPolicySelectionOutcome::Selected,
            selected_at,
        )?)
    }

    fn classify_empty_selection(
        &self,
        scopes: &[PermissionPolicyScopeEvidence],
    ) -> Result<PermissionPolicySelectionFailure, PermissionPolicyOperationsServiceError> {
        let bindings = self
            .repository
            .list_scope_bindings(MAX_PERMISSION_POLICY_QUERY_LIMIT)?;
        if bindings.is_empty() {
            return Ok(PermissionPolicySelectionFailure::NoPolicy);
        }
        for binding in &bindings {
            if scopes
                .iter()
                .any(|scope| binding.selector().matches_scope(scope))
            {
                if let Some(record) = self.repository.get_policy_record(binding.record_id())? {
                    if record.lifecycle() == PermissionPolicyRecordLifecycle::Retired {
                        return Ok(PermissionPolicySelectionFailure::RetiredPolicy);
                    }
                }
            }
        }
        Ok(PermissionPolicySelectionFailure::OutOfScope)
    }

    fn denied_selection(
        evidence_id: PermissionPolicySelectionEvidenceId,
        scopes: Vec<PermissionPolicyScopeEvidence>,
        failure: PermissionPolicySelectionFailure,
        selected_at: i64,
    ) -> Result<PermissionPolicySelectionEvidence, PermissionPolicyOperationsDomainError> {
        PermissionPolicySelectionEvidence::new(
            evidence_id,
            scopes,
            Vec::new(),
            PermissionPolicySelectionOutcome::Denied(failure),
            selected_at,
        )
    }

    fn require_policy_record(
        &self,
        record_id: &PermissionPolicyRecordId,
    ) -> Result<PermissionPolicyRecord, PermissionPolicyOperationsServiceError> {
        self.repository
            .get_policy_record(record_id)?
            .ok_or_else(|| {
                PermissionPolicyOperationsServiceError::PolicyRecordNotFound(record_id.clone())
            })
    }

    fn require_published_policy_record(
        &self,
        record_id: &PermissionPolicyRecordId,
    ) -> Result<PermissionPolicyRecord, PermissionPolicyOperationsServiceError> {
        let record = self.require_policy_record(record_id)?;
        match record.lifecycle() {
            PermissionPolicyRecordLifecycle::Published => Ok(record),
            PermissionPolicyRecordLifecycle::Retired => Err(
                PermissionPolicyOperationsServiceError::RetiredPolicy(record_id.clone()),
            ),
            PermissionPolicyRecordLifecycle::Draft => Err(
                PermissionPolicyOperationsServiceError::PolicyNotPublished(record_id.clone()),
            ),
        }
    }

    fn require_scope_binding(
        &self,
        binding_id: &PermissionPolicyScopeBindingId,
    ) -> Result<PermissionPolicyScopeBinding, PermissionPolicyOperationsServiceError> {
        self.repository
            .get_scope_binding(binding_id)?
            .ok_or_else(|| {
                PermissionPolicyOperationsServiceError::ScopeBindingNotFound(binding_id.clone())
            })
    }

    fn trusted_now(
        &self,
        subject_type: &str,
        subject_reference: &str,
        policy_record_id: Option<&PermissionPolicyRecordId>,
    ) -> Result<i64, PermissionPolicyOperationsServiceError> {
        match self.clock.now() {
            Ok(at) => Ok(at),
            Err(error) => {
                let error = PermissionPolicyOperationsServiceError::Clock(error);
                self.record_operation_rejection(
                    &error,
                    subject_type,
                    subject_reference,
                    policy_record_id,
                    0,
                    None,
                )?;
                Err(error)
            }
        }
    }

    fn reject_operation<T>(
        &self,
        error: PermissionPolicyOperationsServiceError,
        subject_type: &str,
        subject_reference: &str,
        policy_record_id: Option<&PermissionPolicyRecordId>,
        not_before: i64,
        expected_revision: Option<u64>,
    ) -> Result<T, PermissionPolicyOperationsServiceError> {
        self.record_operation_rejection(
            &error,
            subject_type,
            subject_reference,
            policy_record_id,
            not_before,
            expected_revision,
        )?;
        Err(error)
    }

    fn record_operation_rejection(
        &self,
        error: &PermissionPolicyOperationsServiceError,
        subject_type: &str,
        subject_reference: &str,
        policy_record_id: Option<&PermissionPolicyRecordId>,
        not_before: i64,
        expected_revision: Option<u64>,
    ) -> Result<(), PermissionPolicyOperationsServiceError> {
        let mut values = BTreeMap::from([("reason_code".into(), error.reason_code().into())]);
        if let Some(expected) = expected_revision.filter(|revision| *revision > 0) {
            values.insert("expected_revision".into(), expected.to_string());
        }
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicyOperationRejected,
            GovernanceAuditOutcome::Rejected,
            subject_type,
            subject_reference,
            policy_record_id,
            SanitizedAuditMetadata::new(values)?,
            not_before,
        )?;
        Ok(())
    }

    fn record_selection_error(
        &self,
        subject_reference: &str,
        error: &PermissionPolicyOperationsServiceError,
        not_before: i64,
    ) -> Result<(), PermissionPolicyOperationsServiceError> {
        let metadata = SanitizedAuditMetadata::new(BTreeMap::from([
            ("reason_code".into(), error.reason_code().into()),
            ("selection_count".into(), "0".into()),
        ]))?;
        self.record_event(
            GovernanceAuditEventKind::PermissionPolicySelectionRejected,
            GovernanceAuditOutcome::Rejected,
            "permission_policy_selection",
            subject_reference,
            None,
            metadata,
            not_before,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn record_event(
        &self,
        kind: GovernanceAuditEventKind,
        outcome: GovernanceAuditOutcome,
        subject_type: &str,
        subject_reference: &str,
        policy_record_id: Option<&PermissionPolicyRecordId>,
        metadata: SanitizedAuditMetadata,
        not_before: i64,
    ) -> Result<(), PermissionPolicyOperationsServiceError> {
        let correlations = AuditCorrelationReferences::new(
            None,
            None,
            None,
            None,
            None,
            policy_record_id.map(ToString::to_string),
            None,
        )?;
        self.audit.record(GovernanceAuditRecordRequest {
            stream_id: Self::audit_stream(subject_reference)?,
            kind,
            outcome,
            actor_reference: self.audit_actor.clone(),
            subject_type: subject_type.to_string(),
            subject_reference: subject_reference.to_string(),
            correlations,
            metadata,
            not_before,
        })?;
        Ok(())
    }

    fn audit_stream(
        subject_reference: &str,
    ) -> Result<GovernanceAuditStreamId, GovernanceAuditDomainError> {
        let digest = format!("{:x}", Sha256::digest(subject_reference.as_bytes()));
        GovernanceAuditStreamId::new(format!("audit-stream:permission-policy:{}", &digest[..32]))
    }

    fn policy_metadata(
        &self,
        record: &PermissionPolicyRecord,
        expected_revision: Option<u64>,
    ) -> Result<SanitizedAuditMetadata, PermissionPolicyOperationsServiceError> {
        let mut values = BTreeMap::from([
            ("lifecycle".into(), record.lifecycle().as_str().into()),
            (
                "policy_layer".into(),
                policy_layer(record.policy().layer()).into(),
            ),
            ("policy_revision".into(), record.revision().to_string()),
            (
                "policy_version".into(),
                record.policy().version().to_string(),
            ),
        ]);
        if let Some(expected) = expected_revision.filter(|revision| *revision > 0) {
            values.insert("expected_revision".into(), expected.to_string());
        }
        Ok(SanitizedAuditMetadata::new(values)?)
    }

    fn binding_metadata(
        &self,
        binding: &PermissionPolicyScopeBinding,
        expected_revision: Option<u64>,
    ) -> Result<SanitizedAuditMetadata, PermissionPolicyOperationsServiceError> {
        let mut values = BTreeMap::from([
            ("binding_revision".into(), binding.revision().to_string()),
            ("lifecycle".into(), binding.lifecycle().as_str().into()),
            (
                "policy_layer".into(),
                policy_layer(binding.policy_ref().layer()).into(),
            ),
            (
                "policy_version".into(),
                binding.policy_ref().version().to_string(),
            ),
            (
                "scope_kind".into(),
                binding.selector().scope_kind().as_str().into(),
            ),
        ]);
        if let Some(expected) = expected_revision.filter(|revision| *revision > 0) {
            values.insert("expected_revision".into(), expected.to_string());
        }
        Ok(SanitizedAuditMetadata::new(values)?)
    }

    fn selection_metadata(
        &self,
        evidence: &PermissionPolicySelectionEvidence,
    ) -> Result<SanitizedAuditMetadata, PermissionPolicyOperationsServiceError> {
        let mut values = BTreeMap::from([(
            "selection_count".into(),
            evidence.selected_policy_versions().len().to_string(),
        )]);
        if let PermissionPolicySelectionOutcome::Denied(reason) = evidence.outcome() {
            values.insert("reason_code".into(), reason.reason_code().into());
        }
        Ok(SanitizedAuditMetadata::new(values)?)
    }
}

fn policy_layer(layer: PermissionPolicyLayer) -> &'static str {
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
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        capability_domain::{CapabilityId, CapabilitySnapshotId},
        governance_audit::{
            GovernanceAuditEvent, GovernanceAuditRepository, GovernanceAuditRepositoryError,
            GovernanceAuditService, InMemoryGovernanceAuditRepository,
        },
        governance_time::FixedTrustedClock,
        permission_domain::{
            AuthorizationDecision, AuthorizationDecisionId, AuthorizationDecisionStatus,
            PermissionAction, PermissionCeilingId, PermissionClaim, PermissionGrant,
            PermissionGrantId, PermissionPolicyId, PermissionRequest, PermissionRequestId,
            PermissionRule, PermissionRuleEffect,
        },
        permission_policy_operations_repository::{
            InMemoryPermissionPolicyOperationsRepository, PermissionPolicyOperationsRepository,
        },
        permission_repository::{InMemoryPermissionRepository, PermissionRepository},
        role_domain::RoleAssignmentId,
        runtime_domain::RuntimeExecutionId,
    };

    type TestAudit = GovernanceAuditService<InMemoryGovernanceAuditRepository, FixedTrustedClock>;
    type TestService = PermissionPolicyOperationsService<
        InMemoryPermissionPolicyOperationsRepository,
        FixedTrustedClock,
        TestAudit,
    >;

    fn policy(id: &str, version: u16, layer: PermissionPolicyLayer) -> PermissionPolicy {
        PermissionPolicy::new(
            PermissionPolicyId::new(id).unwrap(),
            version,
            layer,
            "owner:test",
            vec![PermissionRule::new(
                PermissionRuleEffect::Deny,
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:test",
                BTreeMap::new(),
            )
            .unwrap()],
        )
        .unwrap()
    }

    fn repository_scope() -> PermissionPolicyScopeEvidence {
        PermissionPolicyScopeEvidence::new(
            PermissionPolicyScopeKind::Repository,
            "repository:test",
            Some("organization:test".into()),
        )
        .unwrap()
    }

    fn test_service(
        repository: InMemoryPermissionPolicyOperationsRepository,
        audit_repository: InMemoryGovernanceAuditRepository,
    ) -> TestService {
        PermissionPolicyOperationsService::new(
            repository,
            FixedTrustedClock::new(50).unwrap(),
            GovernanceAuditService::new(audit_repository, FixedTrustedClock::new(50).unwrap()),
            "actor:permission-policy-test",
        )
    }

    fn stream_events(
        audit: &InMemoryGovernanceAuditRepository,
        subject: &str,
    ) -> Vec<GovernanceAuditEvent> {
        audit
            .list_stream(
                &PermissionPolicyOperationsService::<
                    InMemoryPermissionPolicyOperationsRepository,
                    FixedTrustedClock,
                    TestAudit,
                >::audit_stream(subject)
                .unwrap(),
                100,
            )
            .unwrap()
    }

    fn create_published(
        service: &TestService,
        record_id: &str,
        policy_id: &str,
        version: u16,
        replaces: Option<PermissionPolicyVersionRef>,
    ) -> PermissionPolicyRecord {
        let draft = service
            .create_draft_policy_record(
                PermissionPolicyRecordId::new(record_id).unwrap(),
                policy(policy_id, version, PermissionPolicyLayer::Repository),
                "provenance:test",
                replaces,
            )
            .unwrap();
        service.publish_policy_record(draft.id(), 1).unwrap()
    }

    fn create_active_binding(
        service: &TestService,
        binding_id: &str,
        record: &PermissionPolicyRecord,
    ) -> PermissionPolicyScopeBinding {
        let draft = service
            .create_scope_binding_draft(
                PermissionPolicyScopeBindingId::new(binding_id).unwrap(),
                record.id(),
                repository_scope(),
                0,
                Some(100),
                "provenance:test",
            )
            .unwrap();
        service.activate_scope_binding(draft.id(), 1).unwrap()
    }

    #[test]
    fn lifecycle_selection_management_and_audit_remain_non_authoritative() {
        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let audit = InMemoryGovernanceAuditRepository::default();
        let service = test_service(repository.clone(), audit.clone());
        let record = create_published(
            &service,
            "policy-record:lifecycle",
            "permission-policy:lifecycle",
            1,
            None,
        );
        let active = create_active_binding(&service, "policy-binding:lifecycle", &record);

        let selection = service.select_policies(vec![repository_scope()]).unwrap();
        assert_eq!(
            selection.outcome(),
            PermissionPolicySelectionOutcome::Selected
        );
        assert_eq!(selection.selected_policy_versions(), &[record.policy_ref()]);
        assert_eq!(selection.selected_at(), 50);

        let legacy_permissions = InMemoryPermissionRepository::default();
        assert!(legacy_permissions
            .get_decision(&AuthorizationDecisionId::new("authorization:not-created").unwrap())
            .unwrap()
            .is_none());
        assert!(legacy_permissions
            .get_grant(&PermissionGrantId::new("permission-grant:not-created").unwrap())
            .unwrap()
            .is_none());

        let policy_views = service.list_policy_record_views(10).unwrap();
        assert_eq!(policy_views.len(), 1);
        assert_eq!(policy_views[0].rule_count, 1);
        assert_eq!(
            service
                .inspect_policy_record(record.id())
                .unwrap()
                .unwrap()
                .definition,
            *record.policy()
        );
        assert_eq!(service.list_scope_binding_views(10).unwrap().len(), 1);
        assert_eq!(service.list_selection_evidence_views(10).unwrap().len(), 1);
        assert!(matches!(
            service.list_policy_record_views(0),
            Err(PermissionPolicyOperationsServiceError::Repository(
                PermissionPolicyOperationsRepositoryError::InvalidQueryLimit
            ))
        ));

        let ended = service.end_scope_binding(active.id(), 2).unwrap();
        assert_eq!(
            ended.lifecycle(),
            PermissionPolicyScopeBindingLifecycle::Ended
        );
        let retired = service.retire_policy_record(record.id(), 2).unwrap();
        assert_eq!(
            retired.lifecycle(),
            PermissionPolicyRecordLifecycle::Retired
        );

        assert_eq!(
            stream_events(&audit, record.id().as_str())
                .iter()
                .map(GovernanceAuditEvent::kind)
                .collect::<Vec<_>>(),
            vec![
                GovernanceAuditEventKind::PermissionPolicyDraftCreated,
                GovernanceAuditEventKind::PermissionPolicyPublished,
                GovernanceAuditEventKind::PermissionPolicyRetired,
            ]
        );
        assert_eq!(
            stream_events(&audit, active.id().as_str())
                .iter()
                .map(GovernanceAuditEvent::kind)
                .collect::<Vec<_>>(),
            vec![
                GovernanceAuditEventKind::PermissionPolicyScopeBindingCreated,
                GovernanceAuditEventKind::PermissionPolicyScopeBindingActivated,
                GovernanceAuditEventKind::PermissionPolicyScopeBindingEnded,
            ]
        );
        assert_eq!(
            stream_events(&audit, selection.id().as_str())[0].kind(),
            GovernanceAuditEventKind::PermissionPolicySelectionAccepted
        );
    }

    #[test]
    fn explicit_replacement_is_atomic_and_selects_only_the_new_exact_version() {
        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let audit = InMemoryGovernanceAuditRepository::default();
        let service = test_service(repository.clone(), audit.clone());
        let version_one = create_published(
            &service,
            "policy-record:version-one",
            "permission-policy:versioned",
            1,
            None,
        );
        let active_one =
            create_active_binding(&service, "policy-binding:version-one", &version_one);
        let version_two = create_published(
            &service,
            "policy-record:version-two",
            "permission-policy:versioned",
            2,
            Some(version_one.policy_ref()),
        );
        let draft_two = service
            .create_scope_binding_draft(
                PermissionPolicyScopeBindingId::new("policy-binding:version-two").unwrap(),
                version_two.id(),
                repository_scope(),
                0,
                Some(100),
                "provenance:test",
            )
            .unwrap();

        let (ended_one, active_two) = service
            .replace_active_policy_version(active_one.id(), 2, draft_two.id(), 1)
            .unwrap();
        assert_eq!(
            ended_one.lifecycle(),
            PermissionPolicyScopeBindingLifecycle::Ended
        );
        assert_eq!(
            active_two.lifecycle(),
            PermissionPolicyScopeBindingLifecycle::Active
        );
        let selection = service.select_policies(vec![repository_scope()]).unwrap();
        assert_eq!(
            selection.selected_policy_versions(),
            &[version_two.policy_ref()]
        );
        assert_eq!(
            stream_events(&audit, active_two.id().as_str())
                .iter()
                .map(GovernanceAuditEvent::kind)
                .collect::<Vec<_>>(),
            vec![
                GovernanceAuditEventKind::PermissionPolicyScopeBindingCreated,
                GovernanceAuditEventKind::PermissionPolicyVersionReplaced,
            ]
        );
    }

    #[test]
    fn stale_revision_fails_closed_and_is_audited_without_mutation() {
        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let audit = InMemoryGovernanceAuditRepository::default();
        let service = test_service(repository.clone(), audit.clone());
        let draft = service
            .create_draft_policy_record(
                PermissionPolicyRecordId::new("policy-record:stale").unwrap(),
                policy(
                    "permission-policy:stale",
                    1,
                    PermissionPolicyLayer::Repository,
                ),
                "provenance:test",
                None,
            )
            .unwrap();

        assert!(matches!(
            service.publish_policy_record(draft.id(), 9),
            Err(PermissionPolicyOperationsServiceError::Domain(
                PermissionPolicyOperationsDomainError::StaleRevision { .. }
            ))
        ));
        assert_eq!(
            repository.get_policy_record(draft.id()).unwrap(),
            Some(draft.clone())
        );
        let events = stream_events(&audit, draft.id().as_str());
        assert_eq!(
            events.last().unwrap().kind(),
            GovernanceAuditEventKind::PermissionPolicyOperationRejected
        );
        assert_eq!(
            events.last().unwrap().metadata().values()["reason_code"],
            "stale_revision"
        );
    }

    #[test]
    fn no_policy_out_of_scope_and_retired_policy_are_immutable_denials() {
        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let audit = InMemoryGovernanceAuditRepository::default();
        let service = test_service(repository, audit.clone());
        let none = service.select_policies(vec![repository_scope()]).unwrap();
        assert_eq!(
            none.outcome(),
            PermissionPolicySelectionOutcome::Denied(PermissionPolicySelectionFailure::NoPolicy)
        );
        assert_eq!(
            stream_events(&audit, none.id().as_str())[0].outcome(),
            GovernanceAuditOutcome::NoPolicy
        );

        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let audit = InMemoryGovernanceAuditRepository::default();
        let service = test_service(repository, audit);
        let record = create_published(
            &service,
            "policy-record:retired-selection",
            "permission-policy:retired-selection",
            1,
            None,
        );
        let active = create_active_binding(&service, "policy-binding:retired-selection", &record);
        service.end_scope_binding(active.id(), 2).unwrap();
        service.retire_policy_record(record.id(), 2).unwrap();
        let retired = service.select_policies(vec![repository_scope()]).unwrap();
        assert_eq!(
            retired.outcome(),
            PermissionPolicySelectionOutcome::Denied(
                PermissionPolicySelectionFailure::RetiredPolicy
            )
        );
        assert!(retired.selected_policy_versions().is_empty());

        let other_scope = PermissionPolicyScopeEvidence::new(
            PermissionPolicyScopeKind::Repository,
            "repository:other",
            Some("organization:test".into()),
        )
        .unwrap();
        let out_of_scope = service.select_policies(vec![other_scope]).unwrap();
        assert_eq!(
            out_of_scope.outcome(),
            PermissionPolicySelectionOutcome::Denied(PermissionPolicySelectionFailure::OutOfScope)
        );
    }

    #[derive(Clone)]
    struct AmbiguousSelectionRepository {
        delegate: InMemoryPermissionPolicyOperationsRepository,
    }

    impl PermissionPolicyOperationsRepository for AmbiguousSelectionRepository {
        fn insert_policy_record(
            &self,
            record: PermissionPolicyRecord,
        ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
            self.delegate.insert_policy_record(record)
        }

        fn get_policy_record(
            &self,
            id: &PermissionPolicyRecordId,
        ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError>
        {
            self.delegate.get_policy_record(id)
        }

        fn get_policy_record_by_version(
            &self,
            policy_id: &PermissionPolicyId,
            version: u16,
        ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError>
        {
            self.delegate
                .get_policy_record_by_version(policy_id, version)
        }

        fn list_policy_records(
            &self,
            limit: usize,
        ) -> Result<Vec<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError>
        {
            self.delegate.list_policy_records(limit)
        }

        fn update_policy_record(
            &self,
            record: PermissionPolicyRecord,
            expected_revision: u64,
        ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
            self.delegate
                .update_policy_record(record, expected_revision)
        }

        fn insert_scope_binding(
            &self,
            binding: PermissionPolicyScopeBinding,
        ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
            self.delegate.insert_scope_binding(binding)
        }

        fn get_scope_binding(
            &self,
            id: &PermissionPolicyScopeBindingId,
        ) -> Result<Option<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>
        {
            self.delegate.get_scope_binding(id)
        }

        fn list_scope_bindings(
            &self,
            limit: usize,
        ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>
        {
            self.delegate.list_scope_bindings(limit)
        }

        fn list_scope_bindings_for_record(
            &self,
            record_id: &PermissionPolicyRecordId,
            limit: usize,
        ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>
        {
            self.delegate
                .list_scope_bindings_for_record(record_id, limit)
        }

        fn update_scope_binding(
            &self,
            binding: PermissionPolicyScopeBinding,
            expected_revision: u64,
        ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
            self.delegate
                .update_scope_binding(binding, expected_revision)
        }

        fn replace_active_binding(
            &self,
            ended_binding: PermissionPolicyScopeBinding,
            ended_expected_revision: u64,
            activated_binding: PermissionPolicyScopeBinding,
            activated_expected_revision: u64,
        ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
            self.delegate.replace_active_binding(
                ended_binding,
                ended_expected_revision,
                activated_binding,
                activated_expected_revision,
            )
        }

        fn list_effective_bindings(
            &self,
            scopes: &[PermissionPolicyScopeEvidence],
            at: i64,
            limit: usize,
        ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>
        {
            let mut bindings = self.delegate.list_effective_bindings(scopes, at, limit)?;
            if let Some(binding) = bindings.first().cloned() {
                bindings.push(binding);
            }
            Ok(bindings)
        }

        fn append_selection_evidence(
            &self,
            evidence: PermissionPolicySelectionEvidence,
        ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
            self.delegate.append_selection_evidence(evidence)
        }

        fn get_selection_evidence(
            &self,
            id: &PermissionPolicySelectionEvidenceId,
        ) -> Result<
            Option<PermissionPolicySelectionEvidence>,
            PermissionPolicyOperationsRepositoryError,
        > {
            self.delegate.get_selection_evidence(id)
        }

        fn list_selection_evidence(
            &self,
            limit: usize,
        ) -> Result<Vec<PermissionPolicySelectionEvidence>, PermissionPolicyOperationsRepositoryError>
        {
            self.delegate.list_selection_evidence(limit)
        }
    }

    #[test]
    fn ambiguous_repository_result_fails_closed_even_behind_repository_guards() {
        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let setup = test_service(
            repository.clone(),
            InMemoryGovernanceAuditRepository::default(),
        );
        let record = create_published(
            &setup,
            "policy-record:ambiguous",
            "permission-policy:ambiguous",
            1,
            None,
        );
        create_active_binding(&setup, "policy-binding:ambiguous", &record);

        let audit = InMemoryGovernanceAuditRepository::default();
        let service = PermissionPolicyOperationsService::new(
            AmbiguousSelectionRepository {
                delegate: repository,
            },
            FixedTrustedClock::new(50).unwrap(),
            GovernanceAuditService::new(audit.clone(), FixedTrustedClock::new(50).unwrap()),
            "actor:permission-policy-test",
        );
        let evidence = service.select_policies(vec![repository_scope()]).unwrap();
        assert_eq!(
            evidence.outcome(),
            PermissionPolicySelectionOutcome::Denied(
                PermissionPolicySelectionFailure::AmbiguousPolicy
            )
        );
        assert!(evidence.selected_policy_versions().is_empty());
        let event = &stream_events(&audit, evidence.id().as_str())[0];
        assert_eq!(
            event.kind(),
            GovernanceAuditEventKind::PermissionPolicySelectionRejected
        );
        assert_eq!(
            event.metadata().values()["reason_code"],
            "ambiguous_policy_selection"
        );
    }

    #[test]
    fn historical_authorization_decision_and_grant_are_unchanged_by_operations() {
        let legacy = InMemoryPermissionRepository::default();
        let legacy_policy = policy(
            "permission-policy:historical",
            1,
            PermissionPolicyLayer::Repository,
        );
        let request = PermissionRequest::new(
            PermissionRequestId::new("permission-request:historical").unwrap(),
            RuntimeExecutionId::new("execution:historical").unwrap(),
            "agent:historical",
            RoleAssignmentId::new("role-assignment:historical").unwrap(),
            "repository:test",
            CapabilitySnapshotId::new("capability-snapshot:historical").unwrap(),
            PermissionCeilingId::new("permission-ceiling:historical").unwrap(),
            1,
            vec![legacy_policy.id().clone()],
            vec![PermissionClaim::new(
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:test",
                BTreeMap::new(),
                CapabilityId::new("capability:write-enforcement").unwrap(),
            )
            .unwrap()],
            Vec::new(),
            10,
            100,
        )
        .unwrap();
        let decision_id = AuthorizationDecisionId::new("authorization:historical").unwrap();
        let grant_id = PermissionGrantId::new("permission-grant:historical").unwrap();
        let grant =
            PermissionGrant::new(grant_id.clone(), decision_id.clone(), &request, 50).unwrap();
        let decision = AuthorizationDecision::new(
            decision_id.clone(),
            &request,
            AuthorizationDecisionStatus::Allowed,
            vec![PermissionPolicyVersionRef::from_policy(&legacy_policy)],
            vec!["historical authorization".into()],
            Some(grant_id.clone()),
            50,
        )
        .unwrap();
        legacy
            .record_evaluation(request, decision.clone(), Some(grant.clone()))
            .unwrap();

        let service = test_service(
            InMemoryPermissionPolicyOperationsRepository::default(),
            InMemoryGovernanceAuditRepository::default(),
        );
        let operational_v1 = create_published(
            &service,
            "policy-record:historical-v1",
            "permission-policy:historical",
            1,
            None,
        );
        let operational_v2 = create_published(
            &service,
            "policy-record:historical-v2",
            "permission-policy:historical",
            2,
            Some(operational_v1.policy_ref()),
        );
        service
            .retire_policy_record(operational_v2.id(), operational_v2.revision())
            .unwrap();

        assert_eq!(legacy.get_decision(&decision_id).unwrap(), Some(decision));
        assert_eq!(legacy.get_grant(&grant_id).unwrap(), Some(grant));
    }

    #[derive(Clone, Copy)]
    struct RejectingAuditSink;

    impl GovernanceAuditSink for RejectingAuditSink {
        fn record(
            &self,
            _request: GovernanceAuditRecordRequest,
        ) -> Result<GovernanceAuditEvent, GovernanceAuditServiceError> {
            Err(GovernanceAuditServiceError::Repository(
                GovernanceAuditRepositoryError::Persistence("injected audit failure".into()),
            ))
        }
    }

    #[test]
    fn final_audit_failure_prevents_operational_persistence() {
        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let service = PermissionPolicyOperationsService::new(
            repository.clone(),
            FixedTrustedClock::new(50).unwrap(),
            RejectingAuditSink,
            "actor:permission-policy-test",
        );
        let id = PermissionPolicyRecordId::new("policy-record:audit-failure").unwrap();
        assert!(matches!(
            service.create_draft_policy_record(
                id.clone(),
                policy(
                    "permission-policy:audit-failure",
                    1,
                    PermissionPolicyLayer::Repository,
                ),
                "provenance:test",
                None,
            ),
            Err(PermissionPolicyOperationsServiceError::Audit(_))
        ));
        assert!(repository.get_policy_record(&id).unwrap().is_none());
    }
}
