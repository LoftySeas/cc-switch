//! Replaceable persistence boundary for Permission policy operations.
//!
//! Operational records and scope bindings use optimistic concurrency. Policy
//! definitions and aggregate identities remain immutable, while selection
//! evidence is append-only. Loading from persistence always reconstructs and
//! validates the domain aggregate before returning it.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, RwLock},
};

use rusqlite::{params, OptionalExtension, Transaction};
use thiserror::Error;

use crate::{
    database::{lock_conn, Database},
    error::AppError,
    permission_domain::{PermissionPolicyId, PermissionPolicyLayer},
    permission_policy_operations::{
        permission_policy_layer_precedence, PermissionPolicyOperationsDomainError,
        PermissionPolicyRecord, PermissionPolicyRecordId, PermissionPolicyRecordLifecycle,
        PermissionPolicyScopeBinding, PermissionPolicyScopeBindingId,
        PermissionPolicyScopeBindingLifecycle, PermissionPolicyScopeEvidence,
        PermissionPolicySelectionEvidence, PermissionPolicySelectionEvidenceId,
        PermissionPolicySelectionOutcome,
    },
};

pub const MAX_PERMISSION_POLICY_QUERY_LIMIT: usize = 256;

#[derive(Debug, Error)]
pub enum PermissionPolicyOperationsRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] PermissionPolicyOperationsDomainError),
    #[error("{record} already exists: {id}")]
    AlreadyExists { record: &'static str, id: String },
    #[error("Permission policy version already exists: {policy_id} v{version}")]
    PolicyVersionAlreadyExists {
        policy_id: PermissionPolicyId,
        version: u16,
    },
    #[error("{record} was not found: {id}")]
    NotFound { record: &'static str, id: String },
    #[error("{record} revision conflict for {id}: expected {expected}, current {current}")]
    RevisionConflict {
        record: &'static str,
        id: String,
        expected: u64,
        current: u64,
    },
    #[error("{record} immutable identity or definition changed: {id}")]
    ImmutableRecord { record: &'static str, id: String },
    #[error("{record} lifecycle update is invalid: {id}")]
    InvalidLifecycle { record: &'static str, id: String },
    #[error("Permission policy scope binding does not match its exact policy record: {0}")]
    PolicyRecordMismatch(PermissionPolicyScopeBindingId),
    #[error("Permission policy is not Published: {0}")]
    PolicyNotPublished(PermissionPolicyRecordId),
    #[error("Permission policy still has an Active scope binding: {0}")]
    PolicyHasActiveBinding(PermissionPolicyRecordId),
    #[error("An Active policy binding already exists for this exact layer and selector")]
    ActiveBindingConflict,
    #[error("Active policy replacement must retain the exact selector")]
    ReplacementSelectorMismatch,
    #[error("Permission policy repository query limit must be between 1 and {MAX_PERMISSION_POLICY_QUERY_LIMIT}")]
    InvalidQueryLimit,
    #[error("Permission policy repository result exceeds its bounded query limit")]
    ResultLimitExceeded,
    #[error("Permission policy operations repository lock failed: {0}")]
    RegistryLock(String),
    #[error("Permission policy operations persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for PermissionPolicyOperationsRepositoryError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait PermissionPolicyOperationsRepository: Send + Sync {
    fn insert_policy_record(
        &self,
        record: PermissionPolicyRecord,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError>;
    fn get_policy_record(
        &self,
        id: &PermissionPolicyRecordId,
    ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError>;
    fn get_policy_record_by_version(
        &self,
        policy_id: &PermissionPolicyId,
        version: u16,
    ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError>;
    fn list_policy_records(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError>;
    fn update_policy_record(
        &self,
        record: PermissionPolicyRecord,
        expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError>;

    fn insert_scope_binding(
        &self,
        binding: PermissionPolicyScopeBinding,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError>;
    fn get_scope_binding(
        &self,
        id: &PermissionPolicyScopeBindingId,
    ) -> Result<Option<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>;
    fn list_scope_bindings(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>;
    fn list_scope_bindings_for_record(
        &self,
        record_id: &PermissionPolicyRecordId,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>;
    fn update_scope_binding(
        &self,
        binding: PermissionPolicyScopeBinding,
        expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError>;
    fn replace_active_binding(
        &self,
        ended_binding: PermissionPolicyScopeBinding,
        ended_expected_revision: u64,
        activated_binding: PermissionPolicyScopeBinding,
        activated_expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError>;
    fn list_effective_bindings(
        &self,
        scopes: &[PermissionPolicyScopeEvidence],
        at: i64,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>;

    fn append_selection_evidence(
        &self,
        evidence: PermissionPolicySelectionEvidence,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError>;
    fn get_selection_evidence(
        &self,
        id: &PermissionPolicySelectionEvidenceId,
    ) -> Result<Option<PermissionPolicySelectionEvidence>, PermissionPolicyOperationsRepositoryError>;
    fn list_selection_evidence(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicySelectionEvidence>, PermissionPolicyOperationsRepositoryError>;
}

fn validate_limit(limit: usize) -> Result<(), PermissionPolicyOperationsRepositoryError> {
    if limit == 0 || limit > MAX_PERMISSION_POLICY_QUERY_LIMIT {
        return Err(PermissionPolicyOperationsRepositoryError::InvalidQueryLimit);
    }
    Ok(())
}

fn policy_layer(value: PermissionPolicyLayer) -> &'static str {
    match value {
        PermissionPolicyLayer::Repository => "repository",
        PermissionPolicyLayer::HumanOwner => "human_owner",
        PermissionPolicyLayer::Team => "team",
        PermissionPolicyLayer::Workflow => "workflow",
        PermissionPolicyLayer::RoleAssignment => "role_assignment",
        PermissionPolicyLayer::Workspace => "workspace",
        PermissionPolicyLayer::Environment => "environment",
    }
}

fn selection_outcome(value: PermissionPolicySelectionOutcome) -> &'static str {
    match value {
        PermissionPolicySelectionOutcome::Selected => "selected",
        PermissionPolicySelectionOutcome::Denied(reason) => reason.reason_code(),
    }
}

fn exact_policy_match(
    record: &PermissionPolicyRecord,
    binding: &PermissionPolicyScopeBinding,
) -> bool {
    record.id() == binding.record_id() && record.policy_ref() == *binding.policy_ref()
}

fn validate_record_update(
    current: &PermissionPolicyRecord,
    next: &PermissionPolicyRecord,
    expected_revision: u64,
) -> Result<(), PermissionPolicyOperationsRepositoryError> {
    next.validate()?;
    if current.revision() != expected_revision {
        return Err(
            PermissionPolicyOperationsRepositoryError::RevisionConflict {
                record: "Permission policy record",
                id: current.id().to_string(),
                expected: expected_revision,
                current: current.revision(),
            },
        );
    }
    if next.revision() != expected_revision.saturating_add(1) {
        return Err(
            PermissionPolicyOperationsRepositoryError::RevisionConflict {
                record: "Permission policy record",
                id: current.id().to_string(),
                expected: expected_revision.saturating_add(1),
                current: next.revision(),
            },
        );
    }
    if current.id() != next.id()
        || current.policy() != next.policy()
        || current.created_at() != next.created_at()
        || current.provenance_ref() != next.provenance_ref()
        || current.replaces() != next.replaces()
        || (current.lifecycle() == PermissionPolicyRecordLifecycle::Published
            && current.published_at() != next.published_at())
    {
        return Err(PermissionPolicyOperationsRepositoryError::ImmutableRecord {
            record: "Permission policy record",
            id: current.id().to_string(),
        });
    }
    let legal = matches!(
        (current.lifecycle(), next.lifecycle()),
        (
            PermissionPolicyRecordLifecycle::Draft,
            PermissionPolicyRecordLifecycle::Published
        ) | (
            PermissionPolicyRecordLifecycle::Published,
            PermissionPolicyRecordLifecycle::Retired
        )
    );
    if !legal {
        return Err(
            PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                record: "Permission policy record",
                id: current.id().to_string(),
            },
        );
    }
    Ok(())
}

fn validate_binding_update(
    current: &PermissionPolicyScopeBinding,
    next: &PermissionPolicyScopeBinding,
    expected_revision: u64,
) -> Result<(), PermissionPolicyOperationsRepositoryError> {
    next.validate()?;
    if current.revision() != expected_revision {
        return Err(
            PermissionPolicyOperationsRepositoryError::RevisionConflict {
                record: "Permission policy scope binding",
                id: current.id().to_string(),
                expected: expected_revision,
                current: current.revision(),
            },
        );
    }
    if next.revision() != expected_revision.saturating_add(1) {
        return Err(
            PermissionPolicyOperationsRepositoryError::RevisionConflict {
                record: "Permission policy scope binding",
                id: current.id().to_string(),
                expected: expected_revision.saturating_add(1),
                current: next.revision(),
            },
        );
    }
    if current.id() != next.id()
        || current.record_id() != next.record_id()
        || current.policy_ref() != next.policy_ref()
        || current.selector() != next.selector()
        || current.valid_from() != next.valid_from()
        || current.valid_until() != next.valid_until()
        || current.created_at() != next.created_at()
        || current.provenance_ref() != next.provenance_ref()
        || (current.lifecycle() == PermissionPolicyScopeBindingLifecycle::Active
            && current.activated_at() != next.activated_at())
    {
        return Err(PermissionPolicyOperationsRepositoryError::ImmutableRecord {
            record: "Permission policy scope binding",
            id: current.id().to_string(),
        });
    }
    let legal = matches!(
        (current.lifecycle(), next.lifecycle()),
        (
            PermissionPolicyScopeBindingLifecycle::Draft,
            PermissionPolicyScopeBindingLifecycle::Active
        ) | (
            PermissionPolicyScopeBindingLifecycle::Active,
            PermissionPolicyScopeBindingLifecycle::Ended
        )
    );
    if !legal {
        return Err(
            PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                record: "Permission policy scope binding",
                id: current.id().to_string(),
            },
        );
    }
    Ok(())
}

#[derive(Default)]
struct InMemoryPermissionPolicyOperationsState {
    records: BTreeMap<PermissionPolicyRecordId, PermissionPolicyRecord>,
    policy_versions: BTreeMap<(PermissionPolicyId, u16), PermissionPolicyRecordId>,
    bindings: BTreeMap<PermissionPolicyScopeBindingId, PermissionPolicyScopeBinding>,
    selection_evidence:
        BTreeMap<PermissionPolicySelectionEvidenceId, PermissionPolicySelectionEvidence>,
}

#[derive(Clone, Default)]
pub struct InMemoryPermissionPolicyOperationsRepository {
    state: Arc<RwLock<InMemoryPermissionPolicyOperationsState>>,
}

impl InMemoryPermissionPolicyOperationsRepository {
    fn active_binding_conflicts(
        state: &InMemoryPermissionPolicyOperationsState,
        candidate: &PermissionPolicyScopeBinding,
        ignored_ids: &[&PermissionPolicyScopeBindingId],
    ) -> bool {
        state.bindings.values().any(|binding| {
            binding.lifecycle() == PermissionPolicyScopeBindingLifecycle::Active
                && binding.selector() == candidate.selector()
                && !ignored_ids.iter().any(|id| binding.id() == *id)
        })
    }

    fn require_exact_published_record(
        state: &InMemoryPermissionPolicyOperationsState,
        binding: &PermissionPolicyScopeBinding,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let record = state.records.get(binding.record_id()).ok_or_else(|| {
            PermissionPolicyOperationsRepositoryError::NotFound {
                record: "Permission policy record",
                id: binding.record_id().to_string(),
            }
        })?;
        if !exact_policy_match(record, binding) {
            return Err(
                PermissionPolicyOperationsRepositoryError::PolicyRecordMismatch(
                    binding.id().clone(),
                ),
            );
        }
        if record.lifecycle() != PermissionPolicyRecordLifecycle::Published {
            return Err(
                PermissionPolicyOperationsRepositoryError::PolicyNotPublished(record.id().clone()),
            );
        }
        Ok(())
    }
}

impl PermissionPolicyOperationsRepository for InMemoryPermissionPolicyOperationsRepository {
    fn insert_policy_record(
        &self,
        record: PermissionPolicyRecord,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        record.validate()?;
        if record.lifecycle() != PermissionPolicyRecordLifecycle::Draft || record.revision() != 1 {
            return Err(
                PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                    record: "Permission policy record",
                    id: record.id().to_string(),
                },
            );
        }
        let mut state = self.state.write().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        if state.records.contains_key(record.id()) {
            return Err(PermissionPolicyOperationsRepositoryError::AlreadyExists {
                record: "Permission policy record",
                id: record.id().to_string(),
            });
        }
        let version_key = (record.policy().id().clone(), record.policy().version());
        if state.policy_versions.contains_key(&version_key) {
            return Err(
                PermissionPolicyOperationsRepositoryError::PolicyVersionAlreadyExists {
                    policy_id: version_key.0,
                    version: version_key.1,
                },
            );
        }
        if let Some(replaces) = record.replaces() {
            let replacement_key = (replaces.policy_id().clone(), replaces.version());
            if !state.policy_versions.contains_key(&replacement_key) {
                return Err(PermissionPolicyOperationsRepositoryError::NotFound {
                    record: "Replaced Permission policy version",
                    id: format!("{} v{}", replaces.policy_id(), replaces.version()),
                });
            }
        }
        state
            .policy_versions
            .insert(version_key, record.id().clone());
        state.records.insert(record.id().clone(), record);
        Ok(())
    }

    fn get_policy_record(
        &self,
        id: &PermissionPolicyRecordId,
    ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError> {
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        Ok(state.records.get(id).cloned())
    }

    fn get_policy_record_by_version(
        &self,
        policy_id: &PermissionPolicyId,
        version: u16,
    ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError> {
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        Ok(state
            .policy_versions
            .get(&(policy_id.clone(), version))
            .and_then(|id| state.records.get(id))
            .cloned())
    }

    fn list_policy_records(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError> {
        validate_limit(limit)?;
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        let mut records = state.records.values().cloned().collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.policy()
                .id()
                .cmp(right.policy().id())
                .then_with(|| left.policy().version().cmp(&right.policy().version()))
                .then_with(|| left.id().cmp(right.id()))
        });
        records.truncate(limit);
        Ok(records)
    }

    fn update_policy_record(
        &self,
        record: PermissionPolicyRecord,
        expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let mut state = self.state.write().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        let current = state.records.get(record.id()).cloned().ok_or_else(|| {
            PermissionPolicyOperationsRepositoryError::NotFound {
                record: "Permission policy record",
                id: record.id().to_string(),
            }
        })?;
        validate_record_update(&current, &record, expected_revision)?;
        if record.lifecycle() == PermissionPolicyRecordLifecycle::Retired
            && state.bindings.values().any(|binding| {
                binding.record_id() == record.id()
                    && binding.lifecycle() == PermissionPolicyScopeBindingLifecycle::Active
            })
        {
            return Err(
                PermissionPolicyOperationsRepositoryError::PolicyHasActiveBinding(
                    record.id().clone(),
                ),
            );
        }
        state.records.insert(record.id().clone(), record);
        Ok(())
    }

    fn insert_scope_binding(
        &self,
        binding: PermissionPolicyScopeBinding,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        binding.validate()?;
        if binding.lifecycle() != PermissionPolicyScopeBindingLifecycle::Draft
            || binding.revision() != 1
        {
            return Err(
                PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                    record: "Permission policy scope binding",
                    id: binding.id().to_string(),
                },
            );
        }
        let mut state = self.state.write().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        if state.bindings.contains_key(binding.id()) {
            return Err(PermissionPolicyOperationsRepositoryError::AlreadyExists {
                record: "Permission policy scope binding",
                id: binding.id().to_string(),
            });
        }
        Self::require_exact_published_record(&state, &binding)?;
        state.bindings.insert(binding.id().clone(), binding);
        Ok(())
    }

    fn get_scope_binding(
        &self,
        id: &PermissionPolicyScopeBindingId,
    ) -> Result<Option<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>
    {
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        Ok(state.bindings.get(id).cloned())
    }

    fn list_scope_bindings(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError> {
        validate_limit(limit)?;
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        Ok(state.bindings.values().take(limit).cloned().collect())
    }

    fn list_scope_bindings_for_record(
        &self,
        record_id: &PermissionPolicyRecordId,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError> {
        validate_limit(limit)?;
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        Ok(state
            .bindings
            .values()
            .filter(|binding| binding.record_id() == record_id)
            .take(limit)
            .cloned()
            .collect())
    }

    fn update_scope_binding(
        &self,
        binding: PermissionPolicyScopeBinding,
        expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let mut state = self.state.write().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        let current = state.bindings.get(binding.id()).cloned().ok_or_else(|| {
            PermissionPolicyOperationsRepositoryError::NotFound {
                record: "Permission policy scope binding",
                id: binding.id().to_string(),
            }
        })?;
        validate_binding_update(&current, &binding, expected_revision)?;
        if binding.lifecycle() == PermissionPolicyScopeBindingLifecycle::Active {
            Self::require_exact_published_record(&state, &binding)?;
            if Self::active_binding_conflicts(&state, &binding, &[binding.id()]) {
                return Err(PermissionPolicyOperationsRepositoryError::ActiveBindingConflict);
            }
        }
        state.bindings.insert(binding.id().clone(), binding);
        Ok(())
    }

    fn replace_active_binding(
        &self,
        ended_binding: PermissionPolicyScopeBinding,
        ended_expected_revision: u64,
        activated_binding: PermissionPolicyScopeBinding,
        activated_expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let mut state = self.state.write().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        if ended_binding.id() == activated_binding.id()
            || ended_binding.selector() != activated_binding.selector()
        {
            return Err(PermissionPolicyOperationsRepositoryError::ReplacementSelectorMismatch);
        }
        let current_ended = state
            .bindings
            .get(ended_binding.id())
            .cloned()
            .ok_or_else(|| PermissionPolicyOperationsRepositoryError::NotFound {
                record: "Permission policy scope binding",
                id: ended_binding.id().to_string(),
            })?;
        let current_activated = state
            .bindings
            .get(activated_binding.id())
            .cloned()
            .ok_or_else(|| PermissionPolicyOperationsRepositoryError::NotFound {
                record: "Permission policy scope binding",
                id: activated_binding.id().to_string(),
            })?;
        validate_binding_update(&current_ended, &ended_binding, ended_expected_revision)?;
        validate_binding_update(
            &current_activated,
            &activated_binding,
            activated_expected_revision,
        )?;
        if ended_binding.lifecycle() != PermissionPolicyScopeBindingLifecycle::Ended
            || activated_binding.lifecycle() != PermissionPolicyScopeBindingLifecycle::Active
        {
            return Err(
                PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                    record: "Permission policy scope binding replacement",
                    id: activated_binding.id().to_string(),
                },
            );
        }
        Self::require_exact_published_record(&state, &activated_binding)?;
        if Self::active_binding_conflicts(
            &state,
            &activated_binding,
            &[ended_binding.id(), activated_binding.id()],
        ) {
            return Err(PermissionPolicyOperationsRepositoryError::ActiveBindingConflict);
        }
        state
            .bindings
            .insert(ended_binding.id().clone(), ended_binding);
        state
            .bindings
            .insert(activated_binding.id().clone(), activated_binding);
        Ok(())
    }

    fn list_effective_bindings(
        &self,
        scopes: &[PermissionPolicyScopeEvidence],
        at: i64,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError> {
        validate_limit(limit)?;
        for scope in scopes {
            scope.validate()?;
        }
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        let mut bindings = state
            .bindings
            .values()
            .filter(|binding| {
                binding.is_effective_at(at)
                    && scopes
                        .iter()
                        .any(|scope| binding.selector().matches_scope(scope))
            })
            .cloned()
            .collect::<Vec<_>>();
        bindings.sort_by(|left, right| {
            permission_policy_layer_precedence(left.selector().layer())
                .cmp(&permission_policy_layer_precedence(
                    right.selector().layer(),
                ))
                .then_with(|| {
                    left.policy_ref()
                        .policy_id()
                        .cmp(right.policy_ref().policy_id())
                })
                .then_with(|| {
                    left.policy_ref()
                        .version()
                        .cmp(&right.policy_ref().version())
                })
                .then_with(|| left.id().cmp(right.id()))
        });
        if bindings.len() > limit {
            return Err(PermissionPolicyOperationsRepositoryError::ResultLimitExceeded);
        }
        Ok(bindings)
    }

    fn append_selection_evidence(
        &self,
        evidence: PermissionPolicySelectionEvidence,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        evidence.validate()?;
        let mut state = self.state.write().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        if state.selection_evidence.contains_key(evidence.id()) {
            return Err(PermissionPolicyOperationsRepositoryError::AlreadyExists {
                record: "Permission policy selection evidence",
                id: evidence.id().to_string(),
            });
        }
        state
            .selection_evidence
            .insert(evidence.id().clone(), evidence);
        Ok(())
    }

    fn get_selection_evidence(
        &self,
        id: &PermissionPolicySelectionEvidenceId,
    ) -> Result<Option<PermissionPolicySelectionEvidence>, PermissionPolicyOperationsRepositoryError>
    {
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        Ok(state.selection_evidence.get(id).cloned())
    }

    fn list_selection_evidence(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicySelectionEvidence>, PermissionPolicyOperationsRepositoryError>
    {
        validate_limit(limit)?;
        let state = self.state.read().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::RegistryLock(error.to_string())
        })?;
        let mut evidence = state
            .selection_evidence
            .values()
            .cloned()
            .collect::<Vec<_>>();
        evidence.sort_by(|left, right| {
            left.selected_at()
                .cmp(&right.selected_at())
                .then_with(|| left.id().cmp(right.id()))
        });
        evidence.truncate(limit);
        Ok(evidence)
    }
}

const POLICY_RECORD_COLUMNS: &str = "policy_record_id,policy_id,policy_version,policy_layer,record_json,lifecycle_state,revision,created_at,updated_at";
const SCOPE_BINDING_COLUMNS: &str = "binding_id,policy_record_id,policy_id,policy_version,policy_layer,scope_kind,scope_ref,boundary_ref,binding_json,lifecycle_state,revision,valid_from,valid_until,created_at,updated_at";
const SELECTION_EVIDENCE_COLUMNS: &str = "selection_evidence_id,evidence_json,outcome,selected_at";

#[derive(Debug)]
struct StoredPolicyRecord {
    id: String,
    policy_id: String,
    policy_version: i64,
    policy_layer: String,
    record_json: String,
    lifecycle: String,
    revision: i64,
    created_at: i64,
    updated_at: i64,
}

impl StoredPolicyRecord {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            policy_id: row.get(1)?,
            policy_version: row.get(2)?,
            policy_layer: row.get(3)?,
            record_json: row.get(4)?,
            lifecycle: row.get(5)?,
            revision: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }
}

#[derive(Debug)]
struct StoredScopeBinding {
    id: String,
    record_id: String,
    policy_id: String,
    policy_version: i64,
    policy_layer: String,
    scope_kind: String,
    scope_ref: String,
    boundary_ref: Option<String>,
    binding_json: String,
    lifecycle: String,
    revision: i64,
    valid_from: i64,
    valid_until: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

impl StoredScopeBinding {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            record_id: row.get(1)?,
            policy_id: row.get(2)?,
            policy_version: row.get(3)?,
            policy_layer: row.get(4)?,
            scope_kind: row.get(5)?,
            scope_ref: row.get(6)?,
            boundary_ref: row.get(7)?,
            binding_json: row.get(8)?,
            lifecycle: row.get(9)?,
            revision: row.get(10)?,
            valid_from: row.get(11)?,
            valid_until: row.get(12)?,
            created_at: row.get(13)?,
            updated_at: row.get(14)?,
        })
    }
}

#[derive(Debug)]
struct StoredSelectionEvidence {
    id: String,
    evidence_json: String,
    outcome: String,
    selected_at: i64,
}

impl StoredSelectionEvidence {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            evidence_json: row.get(1)?,
            outcome: row.get(2)?,
            selected_at: row.get(3)?,
        })
    }
}

#[derive(Clone)]
pub struct SqlitePermissionPolicyOperationsRepository {
    database: Arc<Database>,
}

impl SqlitePermissionPolicyOperationsRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    fn encode<T: serde::Serialize>(
        value: &T,
    ) -> Result<String, PermissionPolicyOperationsRepositoryError> {
        serde_json::to_string(value).map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })
    }

    fn decode_policy_record(
        stored: StoredPolicyRecord,
    ) -> Result<PermissionPolicyRecord, PermissionPolicyOperationsRepositoryError> {
        let record = serde_json::from_str::<PermissionPolicyRecord>(&stored.record_json).map_err(
            |error| PermissionPolicyOperationsRepositoryError::Persistence(error.to_string()),
        )?;
        record.validate()?;
        if Self::encode(&record)? != stored.record_json {
            return Err(PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy record JSON is not its canonical validated representation"
                    .into(),
            ));
        }
        let stored_revision = u64::try_from(stored.revision).map_err(|_| {
            PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy record revision is invalid".into(),
            )
        })?;
        if record.id().as_str() != stored.id
            || record.policy().id().as_str() != stored.policy_id
            || i64::from(record.policy().version()) != stored.policy_version
            || policy_layer(record.policy().layer()) != stored.policy_layer
            || record.lifecycle().as_str() != stored.lifecycle
            || record.revision() != stored_revision
            || record.created_at() != stored.created_at
            || record.updated_at() != stored.updated_at
        {
            return Err(PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy record indexed columns do not match validated JSON".into(),
            ));
        }
        Ok(record)
    }

    fn decode_scope_binding(
        stored: StoredScopeBinding,
    ) -> Result<PermissionPolicyScopeBinding, PermissionPolicyOperationsRepositoryError> {
        let binding = serde_json::from_str::<PermissionPolicyScopeBinding>(&stored.binding_json)
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        binding.validate()?;
        if Self::encode(&binding)? != stored.binding_json {
            return Err(PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy scope binding JSON is not its canonical validated representation"
                    .into(),
            ));
        }
        let stored_revision = u64::try_from(stored.revision).map_err(|_| {
            PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy scope binding revision is invalid".into(),
            )
        })?;
        if binding.id().as_str() != stored.id
            || binding.record_id().as_str() != stored.record_id
            || binding.policy_ref().policy_id().as_str() != stored.policy_id
            || i64::from(binding.policy_ref().version()) != stored.policy_version
            || policy_layer(binding.policy_ref().layer()) != stored.policy_layer
            || binding.selector().scope_kind().as_str() != stored.scope_kind
            || binding.selector().scope_ref() != stored.scope_ref
            || binding.selector().boundary_ref() != stored.boundary_ref.as_deref()
            || binding.lifecycle().as_str() != stored.lifecycle
            || binding.revision() != stored_revision
            || binding.valid_from() != stored.valid_from
            || binding.valid_until() != stored.valid_until
            || binding.created_at() != stored.created_at
            || binding.updated_at() != stored.updated_at
        {
            return Err(PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy scope binding indexed columns do not match validated JSON"
                    .into(),
            ));
        }
        Ok(binding)
    }

    fn decode_selection_evidence(
        stored: StoredSelectionEvidence,
    ) -> Result<PermissionPolicySelectionEvidence, PermissionPolicyOperationsRepositoryError> {
        let evidence =
            serde_json::from_str::<PermissionPolicySelectionEvidence>(&stored.evidence_json)
                .map_err(|error| {
                    PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
                })?;
        evidence.validate()?;
        if Self::encode(&evidence)? != stored.evidence_json {
            return Err(PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy selection JSON is not its canonical validated representation"
                    .into(),
            ));
        }
        if evidence.id().as_str() != stored.id
            || selection_outcome(evidence.outcome()) != stored.outcome
            || evidence.selected_at() != stored.selected_at
        {
            return Err(PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy selection indexed columns do not match validated JSON".into(),
            ));
        }
        Ok(evidence)
    }

    fn load_policy_record_on_conn(
        conn: &rusqlite::Connection,
        id: &PermissionPolicyRecordId,
    ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError> {
        let sql = format!(
            "SELECT {POLICY_RECORD_COLUMNS} FROM agent_os_permission_policy_records WHERE policy_record_id=?1"
        );
        let stored = conn
            .query_row(&sql, [id.as_str()], StoredPolicyRecord::from_row)
            .optional()
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        stored.map(Self::decode_policy_record).transpose()
    }

    fn load_policy_record_by_version_on_conn(
        conn: &rusqlite::Connection,
        policy_id: &PermissionPolicyId,
        version: u16,
    ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError> {
        let sql = format!(
            "SELECT {POLICY_RECORD_COLUMNS} FROM agent_os_permission_policy_records WHERE policy_id=?1 AND policy_version=?2"
        );
        let stored = conn
            .query_row(
                &sql,
                params![policy_id.as_str(), version],
                StoredPolicyRecord::from_row,
            )
            .optional()
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        stored.map(Self::decode_policy_record).transpose()
    }

    fn load_scope_binding_on_conn(
        conn: &rusqlite::Connection,
        id: &PermissionPolicyScopeBindingId,
    ) -> Result<Option<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>
    {
        let sql = format!(
            "SELECT {SCOPE_BINDING_COLUMNS} FROM agent_os_permission_policy_scope_bindings WHERE binding_id=?1"
        );
        let stored = conn
            .query_row(&sql, [id.as_str()], StoredScopeBinding::from_row)
            .optional()
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        let binding = stored.map(Self::decode_scope_binding).transpose()?;
        if let Some(binding) = &binding {
            Self::validate_loaded_binding_reference(conn, binding)?;
        }
        Ok(binding)
    }

    fn validate_loaded_binding_reference(
        conn: &rusqlite::Connection,
        binding: &PermissionPolicyScopeBinding,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let record =
            Self::load_policy_record_on_conn(conn, binding.record_id())?.ok_or_else(|| {
                PermissionPolicyOperationsRepositoryError::Persistence(
                    "Permission policy scope binding references a missing policy record".into(),
                )
            })?;
        if !exact_policy_match(&record, binding) {
            return Err(PermissionPolicyOperationsRepositoryError::Persistence(
                "Permission policy scope binding does not match its exact persisted policy record"
                    .into(),
            ));
        }
        Ok(())
    }

    fn require_exact_published_record_on_conn(
        conn: &rusqlite::Connection,
        binding: &PermissionPolicyScopeBinding,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let record =
            Self::load_policy_record_on_conn(conn, binding.record_id())?.ok_or_else(|| {
                PermissionPolicyOperationsRepositoryError::NotFound {
                    record: "Permission policy record",
                    id: binding.record_id().to_string(),
                }
            })?;
        if !exact_policy_match(&record, binding) {
            return Err(
                PermissionPolicyOperationsRepositoryError::PolicyRecordMismatch(
                    binding.id().clone(),
                ),
            );
        }
        if record.lifecycle() != PermissionPolicyRecordLifecycle::Published {
            return Err(
                PermissionPolicyOperationsRepositoryError::PolicyNotPublished(record.id().clone()),
            );
        }
        Ok(())
    }

    fn map_policy_insert_error(
        error: rusqlite::Error,
        record: &PermissionPolicyRecord,
    ) -> PermissionPolicyOperationsRepositoryError {
        let message = error.to_string();
        if message.contains("policy_id") && message.contains("policy_version") {
            PermissionPolicyOperationsRepositoryError::PolicyVersionAlreadyExists {
                policy_id: record.policy().id().clone(),
                version: record.policy().version(),
            }
        } else if message.contains("UNIQUE constraint failed") {
            PermissionPolicyOperationsRepositoryError::AlreadyExists {
                record: "Permission policy record",
                id: record.id().to_string(),
            }
        } else {
            PermissionPolicyOperationsRepositoryError::Persistence(message)
        }
    }

    fn map_binding_write_error(
        error: rusqlite::Error,
        binding: &PermissionPolicyScopeBinding,
    ) -> PermissionPolicyOperationsRepositoryError {
        let message = error.to_string();
        if message.contains("idx_agent_os_permission_binding_active_unique")
            || (message.contains("UNIQUE constraint failed")
                && message.contains("policy_layer")
                && message.contains("scope_kind"))
        {
            PermissionPolicyOperationsRepositoryError::ActiveBindingConflict
        } else if message.contains("UNIQUE constraint failed") {
            PermissionPolicyOperationsRepositoryError::AlreadyExists {
                record: "Permission policy scope binding",
                id: binding.id().to_string(),
            }
        } else {
            PermissionPolicyOperationsRepositoryError::Persistence(message)
        }
    }

    fn update_binding_in_transaction(
        transaction: &Transaction<'_>,
        binding: &PermissionPolicyScopeBinding,
        expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let changed = transaction
            .execute(
                "UPDATE agent_os_permission_policy_scope_bindings
                 SET binding_json=?1,lifecycle_state=?2,revision=?3,updated_at=?4
                 WHERE binding_id=?5 AND revision=?6",
                params![
                    Self::encode(binding)?,
                    binding.lifecycle().as_str(),
                    binding.revision() as i64,
                    binding.updated_at(),
                    binding.id().as_str(),
                    expected_revision as i64,
                ],
            )
            .map_err(|error| Self::map_binding_write_error(error, binding))?;
        if changed != 1 {
            return Err(
                PermissionPolicyOperationsRepositoryError::RevisionConflict {
                    record: "Permission policy scope binding",
                    id: binding.id().to_string(),
                    expected: expected_revision,
                    current: expected_revision.saturating_add(1),
                },
            );
        }
        Ok(())
    }
}

impl PermissionPolicyOperationsRepository for SqlitePermissionPolicyOperationsRepository {
    fn insert_policy_record(
        &self,
        record: PermissionPolicyRecord,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        record.validate()?;
        if record.lifecycle() != PermissionPolicyRecordLifecycle::Draft || record.revision() != 1 {
            return Err(
                PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                    record: "Permission policy record",
                    id: record.id().to_string(),
                },
            );
        }
        let conn = lock_conn!(self.database.conn);
        let transaction = conn.unchecked_transaction().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        if let Some(replaces) = record.replaces() {
            if Self::load_policy_record_by_version_on_conn(
                &transaction,
                replaces.policy_id(),
                replaces.version(),
            )?
            .is_none()
            {
                return Err(PermissionPolicyOperationsRepositoryError::NotFound {
                    record: "Replaced Permission policy version",
                    id: format!("{} v{}", replaces.policy_id(), replaces.version()),
                });
            }
        }
        transaction
            .execute(
                "INSERT INTO agent_os_permission_policy_records
                 (policy_record_id,policy_id,policy_version,policy_layer,record_json,
                  lifecycle_state,revision,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    record.id().as_str(),
                    record.policy().id().as_str(),
                    record.policy().version(),
                    policy_layer(record.policy().layer()),
                    Self::encode(&record)?,
                    record.lifecycle().as_str(),
                    record.revision() as i64,
                    record.created_at(),
                    record.updated_at(),
                ],
            )
            .map_err(|error| Self::map_policy_insert_error(error, &record))?;
        transaction.commit().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        Ok(())
    }

    fn get_policy_record(
        &self,
        id: &PermissionPolicyRecordId,
    ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        Self::load_policy_record_on_conn(&conn, id)
    }

    fn get_policy_record_by_version(
        &self,
        policy_id: &PermissionPolicyId,
        version: u16,
    ) -> Result<Option<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        Self::load_policy_record_by_version_on_conn(&conn, policy_id, version)
    }

    fn list_policy_records(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyRecord>, PermissionPolicyOperationsRepositoryError> {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let sql = format!(
            "SELECT {POLICY_RECORD_COLUMNS} FROM agent_os_permission_policy_records
             ORDER BY policy_id,policy_version,policy_record_id LIMIT ?1"
        );
        let mut statement = conn.prepare(&sql).map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        let rows = statement
            .query_map([limit as i64], StoredPolicyRecord::from_row)
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        rows.map(|row| {
            row.map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })
            .and_then(Self::decode_policy_record)
        })
        .collect()
    }

    fn update_policy_record(
        &self,
        record: PermissionPolicyRecord,
        expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let transaction = conn.unchecked_transaction().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        let current =
            Self::load_policy_record_on_conn(&transaction, record.id())?.ok_or_else(|| {
                PermissionPolicyOperationsRepositoryError::NotFound {
                    record: "Permission policy record",
                    id: record.id().to_string(),
                }
            })?;
        validate_record_update(&current, &record, expected_revision)?;
        if record.lifecycle() == PermissionPolicyRecordLifecycle::Retired {
            let active_count: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM agent_os_permission_policy_scope_bindings
                     WHERE policy_record_id=?1 AND lifecycle_state='active'",
                    [record.id().as_str()],
                    |row| row.get(0),
                )
                .map_err(|error| {
                    PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
                })?;
            if active_count != 0 {
                return Err(
                    PermissionPolicyOperationsRepositoryError::PolicyHasActiveBinding(
                        record.id().clone(),
                    ),
                );
            }
        }
        let changed = transaction
            .execute(
                "UPDATE agent_os_permission_policy_records
                 SET record_json=?1,lifecycle_state=?2,revision=?3,updated_at=?4
                 WHERE policy_record_id=?5 AND revision=?6",
                params![
                    Self::encode(&record)?,
                    record.lifecycle().as_str(),
                    record.revision() as i64,
                    record.updated_at(),
                    record.id().as_str(),
                    expected_revision as i64,
                ],
            )
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        if changed != 1 {
            return Err(
                PermissionPolicyOperationsRepositoryError::RevisionConflict {
                    record: "Permission policy record",
                    id: record.id().to_string(),
                    expected: expected_revision,
                    current: expected_revision.saturating_add(1),
                },
            );
        }
        transaction.commit().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        Ok(())
    }

    fn insert_scope_binding(
        &self,
        binding: PermissionPolicyScopeBinding,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        binding.validate()?;
        if binding.lifecycle() != PermissionPolicyScopeBindingLifecycle::Draft
            || binding.revision() != 1
        {
            return Err(
                PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                    record: "Permission policy scope binding",
                    id: binding.id().to_string(),
                },
            );
        }
        let conn = lock_conn!(self.database.conn);
        let transaction = conn.unchecked_transaction().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        Self::require_exact_published_record_on_conn(&transaction, &binding)?;
        transaction
            .execute(
                "INSERT INTO agent_os_permission_policy_scope_bindings
                 (binding_id,policy_record_id,policy_id,policy_version,policy_layer,
                  scope_kind,scope_ref,boundary_ref,binding_json,lifecycle_state,
                  revision,valid_from,valid_until,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![
                    binding.id().as_str(),
                    binding.record_id().as_str(),
                    binding.policy_ref().policy_id().as_str(),
                    binding.policy_ref().version(),
                    policy_layer(binding.policy_ref().layer()),
                    binding.selector().scope_kind().as_str(),
                    binding.selector().scope_ref(),
                    binding.selector().boundary_ref(),
                    Self::encode(&binding)?,
                    binding.lifecycle().as_str(),
                    binding.revision() as i64,
                    binding.valid_from(),
                    binding.valid_until(),
                    binding.created_at(),
                    binding.updated_at(),
                ],
            )
            .map_err(|error| Self::map_binding_write_error(error, &binding))?;
        transaction.commit().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        Ok(())
    }

    fn get_scope_binding(
        &self,
        id: &PermissionPolicyScopeBindingId,
    ) -> Result<Option<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError>
    {
        let conn = lock_conn!(self.database.conn);
        Self::load_scope_binding_on_conn(&conn, id)
    }

    fn list_scope_bindings(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError> {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let sql = format!(
            "SELECT {SCOPE_BINDING_COLUMNS} FROM agent_os_permission_policy_scope_bindings
             ORDER BY binding_id LIMIT ?1"
        );
        let mut statement = conn.prepare(&sql).map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        let rows = statement
            .query_map([limit as i64], StoredScopeBinding::from_row)
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        let mut bindings = Vec::new();
        for row in rows {
            let binding = Self::decode_scope_binding(row.map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?)?;
            Self::validate_loaded_binding_reference(&conn, &binding)?;
            bindings.push(binding);
        }
        Ok(bindings)
    }

    fn list_scope_bindings_for_record(
        &self,
        record_id: &PermissionPolicyRecordId,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError> {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let sql = format!(
            "SELECT {SCOPE_BINDING_COLUMNS} FROM agent_os_permission_policy_scope_bindings
             WHERE policy_record_id=?1 ORDER BY binding_id LIMIT ?2"
        );
        let mut statement = conn.prepare(&sql).map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        let rows = statement
            .query_map(
                params![record_id.as_str(), limit as i64],
                StoredScopeBinding::from_row,
            )
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        let mut bindings = Vec::new();
        for row in rows {
            let binding = Self::decode_scope_binding(row.map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?)?;
            Self::validate_loaded_binding_reference(&conn, &binding)?;
            bindings.push(binding);
        }
        Ok(bindings)
    }

    fn update_scope_binding(
        &self,
        binding: PermissionPolicyScopeBinding,
        expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let transaction = conn.unchecked_transaction().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        let current =
            Self::load_scope_binding_on_conn(&transaction, binding.id())?.ok_or_else(|| {
                PermissionPolicyOperationsRepositoryError::NotFound {
                    record: "Permission policy scope binding",
                    id: binding.id().to_string(),
                }
            })?;
        validate_binding_update(&current, &binding, expected_revision)?;
        if binding.lifecycle() == PermissionPolicyScopeBindingLifecycle::Active {
            Self::require_exact_published_record_on_conn(&transaction, &binding)?;
        }
        Self::update_binding_in_transaction(&transaction, &binding, expected_revision)?;
        transaction.commit().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        Ok(())
    }

    fn replace_active_binding(
        &self,
        ended_binding: PermissionPolicyScopeBinding,
        ended_expected_revision: u64,
        activated_binding: PermissionPolicyScopeBinding,
        activated_expected_revision: u64,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        if ended_binding.id() == activated_binding.id()
            || ended_binding.selector() != activated_binding.selector()
        {
            return Err(PermissionPolicyOperationsRepositoryError::ReplacementSelectorMismatch);
        }
        let conn = lock_conn!(self.database.conn);
        let transaction = conn.unchecked_transaction().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        let current_ended = Self::load_scope_binding_on_conn(&transaction, ended_binding.id())?
            .ok_or_else(|| PermissionPolicyOperationsRepositoryError::NotFound {
                record: "Permission policy scope binding",
                id: ended_binding.id().to_string(),
            })?;
        let current_activated =
            Self::load_scope_binding_on_conn(&transaction, activated_binding.id())?.ok_or_else(
                || PermissionPolicyOperationsRepositoryError::NotFound {
                    record: "Permission policy scope binding",
                    id: activated_binding.id().to_string(),
                },
            )?;
        validate_binding_update(&current_ended, &ended_binding, ended_expected_revision)?;
        validate_binding_update(
            &current_activated,
            &activated_binding,
            activated_expected_revision,
        )?;
        if ended_binding.lifecycle() != PermissionPolicyScopeBindingLifecycle::Ended
            || activated_binding.lifecycle() != PermissionPolicyScopeBindingLifecycle::Active
        {
            return Err(
                PermissionPolicyOperationsRepositoryError::InvalidLifecycle {
                    record: "Permission policy scope binding replacement",
                    id: activated_binding.id().to_string(),
                },
            );
        }
        Self::require_exact_published_record_on_conn(&transaction, &activated_binding)?;
        Self::update_binding_in_transaction(&transaction, &ended_binding, ended_expected_revision)?;
        Self::update_binding_in_transaction(
            &transaction,
            &activated_binding,
            activated_expected_revision,
        )?;
        transaction.commit().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        Ok(())
    }

    fn list_effective_bindings(
        &self,
        scopes: &[PermissionPolicyScopeEvidence],
        at: i64,
        limit: usize,
    ) -> Result<Vec<PermissionPolicyScopeBinding>, PermissionPolicyOperationsRepositoryError> {
        validate_limit(limit)?;
        if scopes.len() > MAX_PERMISSION_POLICY_QUERY_LIMIT {
            return Err(PermissionPolicyOperationsRepositoryError::InvalidQueryLimit);
        }
        if at < 0 {
            return Err(PermissionPolicyOperationsDomainError::InvalidTimestamp.into());
        }
        let mut unique_scopes = BTreeSet::new();
        for scope in scopes {
            scope.validate()?;
            unique_scopes.insert(scope.clone());
        }
        let conn = lock_conn!(self.database.conn);
        let transaction = conn.unchecked_transaction().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        let sql = format!(
            "SELECT {SCOPE_BINDING_COLUMNS} FROM agent_os_permission_policy_scope_bindings
             WHERE lifecycle_state='active' AND scope_kind=?1 AND scope_ref=?2
               AND boundary_ref IS ?3 AND valid_from<=?4
               AND (valid_until IS NULL OR valid_until>=?4)
             ORDER BY policy_layer,policy_id,policy_version,binding_id"
        );
        let mut bindings = BTreeMap::new();
        for scope in unique_scopes {
            let mut statement = transaction.prepare(&sql).map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
            let rows = statement
                .query_map(
                    params![
                        scope.scope_kind().as_str(),
                        scope.scope_ref(),
                        scope.boundary_ref(),
                        at,
                    ],
                    StoredScopeBinding::from_row,
                )
                .map_err(|error| {
                    PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
                })?;
            for row in rows {
                let binding = Self::decode_scope_binding(row.map_err(|error| {
                    PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
                })?)?;
                Self::validate_loaded_binding_reference(&transaction, &binding)?;
                if !binding.is_effective_at(at) || !binding.selector().matches_scope(&scope) {
                    return Err(PermissionPolicyOperationsRepositoryError::Persistence(
                        "Effective policy binding query returned inconsistent scope evidence"
                            .into(),
                    ));
                }
                bindings.insert(binding.id().clone(), binding);
                if bindings.len() > limit {
                    return Err(PermissionPolicyOperationsRepositoryError::ResultLimitExceeded);
                }
            }
        }
        let mut bindings = bindings.into_values().collect::<Vec<_>>();
        bindings.sort_by(|left, right| {
            permission_policy_layer_precedence(left.selector().layer())
                .cmp(&permission_policy_layer_precedence(
                    right.selector().layer(),
                ))
                .then_with(|| {
                    left.policy_ref()
                        .policy_id()
                        .cmp(right.policy_ref().policy_id())
                })
                .then_with(|| {
                    left.policy_ref()
                        .version()
                        .cmp(&right.policy_ref().version())
                })
                .then_with(|| left.id().cmp(right.id()))
        });
        transaction.commit().map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        Ok(bindings)
    }

    fn append_selection_evidence(
        &self,
        evidence: PermissionPolicySelectionEvidence,
    ) -> Result<(), PermissionPolicyOperationsRepositoryError> {
        evidence.validate()?;
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_permission_policy_selection_evidence
             (selection_evidence_id,evidence_json,outcome,selected_at)
             VALUES (?1,?2,?3,?4)",
            params![
                evidence.id().as_str(),
                Self::encode(&evidence)?,
                selection_outcome(evidence.outcome()),
                evidence.selected_at(),
            ],
        )
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("UNIQUE constraint failed") {
                PermissionPolicyOperationsRepositoryError::AlreadyExists {
                    record: "Permission policy selection evidence",
                    id: evidence.id().to_string(),
                }
            } else {
                PermissionPolicyOperationsRepositoryError::Persistence(message)
            }
        })?;
        Ok(())
    }

    fn get_selection_evidence(
        &self,
        id: &PermissionPolicySelectionEvidenceId,
    ) -> Result<Option<PermissionPolicySelectionEvidence>, PermissionPolicyOperationsRepositoryError>
    {
        let conn = lock_conn!(self.database.conn);
        let sql = format!(
            "SELECT {SELECTION_EVIDENCE_COLUMNS} FROM agent_os_permission_policy_selection_evidence
             WHERE selection_evidence_id=?1"
        );
        let stored = conn
            .query_row(&sql, [id.as_str()], StoredSelectionEvidence::from_row)
            .optional()
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        stored.map(Self::decode_selection_evidence).transpose()
    }

    fn list_selection_evidence(
        &self,
        limit: usize,
    ) -> Result<Vec<PermissionPolicySelectionEvidence>, PermissionPolicyOperationsRepositoryError>
    {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let sql = format!(
            "SELECT {SELECTION_EVIDENCE_COLUMNS} FROM agent_os_permission_policy_selection_evidence
             ORDER BY selected_at,selection_evidence_id LIMIT ?1"
        );
        let mut statement = conn.prepare(&sql).map_err(|error| {
            PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
        })?;
        let rows = statement
            .query_map([limit as i64], StoredSelectionEvidence::from_row)
            .map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })?;
        rows.map(|row| {
            row.map_err(|error| {
                PermissionPolicyOperationsRepositoryError::Persistence(error.to_string())
            })
            .and_then(Self::decode_selection_evidence)
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        permission_domain::{
            PermissionAction, PermissionPolicy, PermissionPolicyId, PermissionRule,
            PermissionRuleEffect,
        },
        permission_policy_operations::{
            PermissionPolicyScopeKind, PermissionPolicyScopeSelector,
            PermissionPolicySelectionFailure,
        },
    };

    fn policy(id: &str, version: u16, layer: PermissionPolicyLayer) -> PermissionPolicy {
        PermissionPolicy::new(
            PermissionPolicyId::new(id).unwrap(),
            version,
            layer,
            "owner:repository",
            vec![PermissionRule::new(
                PermissionRuleEffect::Deny,
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:repository",
                BTreeMap::new(),
            )
            .unwrap()],
        )
        .unwrap()
    }

    fn published_record(
        record_id: &str,
        policy_id: &str,
        version: u16,
        layer: PermissionPolicyLayer,
        replaces: Option<&PermissionPolicyRecord>,
    ) -> (PermissionPolicyRecord, PermissionPolicyRecord) {
        let draft = PermissionPolicyRecord::new_draft(
            PermissionPolicyRecordId::new(record_id).unwrap(),
            policy(policy_id, version, layer),
            "provenance:test",
            replaces.map(PermissionPolicyRecord::policy_ref),
            i64::from(version),
        )
        .unwrap();
        let published = draft.publish(1, i64::from(version) + 10).unwrap();
        (draft, published)
    }

    fn scope() -> PermissionPolicyScopeEvidence {
        PermissionPolicyScopeEvidence::new(
            PermissionPolicyScopeKind::Repository,
            "repository:test",
            Some("organization:test".into()),
        )
        .unwrap()
    }

    fn draft_binding(id: &str, record: &PermissionPolicyRecord) -> PermissionPolicyScopeBinding {
        PermissionPolicyScopeBinding::new_draft(
            PermissionPolicyScopeBindingId::new(id).unwrap(),
            record.id().clone(),
            record.policy_ref(),
            PermissionPolicyScopeSelector::new(record.policy().layer(), scope()).unwrap(),
            0,
            Some(100),
            "provenance:test",
            20,
        )
        .unwrap()
    }

    fn persist_published<R: PermissionPolicyOperationsRepository>(
        repository: &R,
        draft: PermissionPolicyRecord,
        published: PermissionPolicyRecord,
    ) {
        repository.insert_policy_record(draft).unwrap();
        repository.update_policy_record(published, 1).unwrap();
    }

    #[test]
    fn in_memory_rejects_stale_revision_and_keeps_policy_definition_immutable() {
        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let (draft, published) = published_record(
            "policy-record:one",
            "permission-policy:one",
            1,
            PermissionPolicyLayer::Repository,
            None,
        );
        repository.insert_policy_record(draft.clone()).unwrap();

        assert!(matches!(
            repository.update_policy_record(published.clone(), 2),
            Err(PermissionPolicyOperationsRepositoryError::RevisionConflict { .. })
        ));
        assert_eq!(
            repository.get_policy_record(draft.id()).unwrap(),
            Some(draft.clone())
        );

        repository
            .update_policy_record(published.clone(), 1)
            .unwrap();
        assert_eq!(
            repository
                .get_policy_record_by_version(published.policy().id(), 1)
                .unwrap(),
            Some(published.clone())
        );

        let retired = published.retire(2, 20).unwrap();
        let mut forged_json = serde_json::to_value(retired).unwrap();
        forged_json["publishedAt"] = serde_json::json!(12);
        let forged: PermissionPolicyRecord = serde_json::from_value(forged_json).unwrap();
        assert!(matches!(
            repository.update_policy_record(forged, 2),
            Err(PermissionPolicyOperationsRepositoryError::ImmutableRecord { .. })
        ));
    }

    #[test]
    fn sqlite_persists_exact_records_and_rejects_tampered_indexed_columns() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqlitePermissionPolicyOperationsRepository::new(database.clone());
        let (draft, published) = published_record(
            "policy-record:sqlite",
            "permission-policy:sqlite",
            1,
            PermissionPolicyLayer::Repository,
            None,
        );
        persist_published(&repository, draft, published.clone());

        let recreated = SqlitePermissionPolicyOperationsRepository::new(database.clone());
        assert_eq!(
            recreated.get_policy_record(published.id()).unwrap(),
            Some(published.clone())
        );

        let conn = database.conn.lock().unwrap();
        conn.execute_batch(
            "DROP TRIGGER trg_agent_os_permission_policy_record_update_consistent;
             DROP TRIGGER trg_agent_os_permission_policy_record_update_guard;
             UPDATE agent_os_permission_policy_records
             SET policy_id='permission-policy:tampered'
             WHERE policy_record_id='policy-record:sqlite';",
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            recreated.get_policy_record(published.id()),
            Err(PermissionPolicyOperationsRepositoryError::Persistence(_))
        ));
    }

    #[test]
    fn sqlite_active_uniqueness_and_atomic_replacement_preserve_one_binding() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqlitePermissionPolicyOperationsRepository::new(database);
        let (draft_one, published_one) = published_record(
            "policy-record:version-one",
            "permission-policy:versions",
            1,
            PermissionPolicyLayer::Repository,
            None,
        );
        persist_published(&repository, draft_one, published_one.clone());
        let (draft_two, published_two) = published_record(
            "policy-record:version-two",
            "permission-policy:versions",
            2,
            PermissionPolicyLayer::Repository,
            Some(&published_one),
        );
        persist_published(&repository, draft_two, published_two.clone());

        let binding_one = draft_binding("policy-binding:one", &published_one);
        let binding_two = draft_binding("policy-binding:two", &published_two);
        repository
            .insert_scope_binding(binding_one.clone())
            .unwrap();
        repository
            .insert_scope_binding(binding_two.clone())
            .unwrap();
        let active_one = binding_one.activate(1, 30).unwrap();
        repository
            .update_scope_binding(active_one.clone(), 1)
            .unwrap();

        let active_two = binding_two.activate(1, 31).unwrap();
        assert!(matches!(
            repository.update_scope_binding(active_two.clone(), 1),
            Err(PermissionPolicyOperationsRepositoryError::ActiveBindingConflict)
        ));
        assert_eq!(
            repository.get_scope_binding(binding_two.id()).unwrap(),
            Some(binding_two.clone())
        );

        let ended_one = active_one.end(2, 32).unwrap();
        let mut forged_json = serde_json::to_value(&ended_one).unwrap();
        forged_json["activatedAt"] = serde_json::json!(31);
        let forged: PermissionPolicyScopeBinding = serde_json::from_value(forged_json).unwrap();
        assert!(matches!(
            repository.replace_active_binding(forged, 2, active_two.clone(), 1),
            Err(PermissionPolicyOperationsRepositoryError::ImmutableRecord { .. })
        ));
        assert_eq!(
            repository.get_scope_binding(active_one.id()).unwrap(),
            Some(active_one.clone())
        );
        repository
            .replace_active_binding(ended_one.clone(), 2, active_two.clone(), 1)
            .unwrap();
        assert_eq!(
            repository.get_scope_binding(active_one.id()).unwrap(),
            Some(ended_one)
        );
        assert_eq!(
            repository.get_scope_binding(active_two.id()).unwrap(),
            Some(active_two)
        );
    }

    #[test]
    fn effective_selection_scope_discovers_all_policy_layers_without_caller_choice() {
        let repository = InMemoryPermissionPolicyOperationsRepository::default();
        let (repository_draft, repository_record) = published_record(
            "policy-record:repository-layer",
            "permission-policy:repository-layer",
            1,
            PermissionPolicyLayer::Repository,
            None,
        );
        let (workspace_draft, workspace_record) = published_record(
            "policy-record:workspace-layer",
            "permission-policy:workspace-layer",
            1,
            PermissionPolicyLayer::Workspace,
            None,
        );
        persist_published(&repository, repository_draft, repository_record.clone());
        persist_published(&repository, workspace_draft, workspace_record.clone());
        for (id, record) in [
            ("policy-binding:repository-layer", &repository_record),
            ("policy-binding:workspace-layer", &workspace_record),
        ] {
            let draft = draft_binding(id, record);
            let active = draft.activate(1, 30).unwrap();
            repository.insert_scope_binding(draft).unwrap();
            repository.update_scope_binding(active, 1).unwrap();
        }

        let selected = repository
            .list_effective_bindings(&[scope()], 40, 10)
            .unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(
            selected
                .iter()
                .map(|binding| binding.selector().layer())
                .collect::<Vec<_>>(),
            vec![
                PermissionPolicyLayer::Repository,
                PermissionPolicyLayer::Workspace
            ]
        );
    }

    #[test]
    fn sqlite_selection_evidence_is_bounded_validated_and_append_only() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqlitePermissionPolicyOperationsRepository::new(database.clone());
        let evidence = PermissionPolicySelectionEvidence::new(
            PermissionPolicySelectionEvidenceId::new("policy-selection:denied").unwrap(),
            vec![scope()],
            Vec::new(),
            PermissionPolicySelectionOutcome::Denied(PermissionPolicySelectionFailure::NoPolicy),
            50,
        )
        .unwrap();
        repository
            .append_selection_evidence(evidence.clone())
            .unwrap();
        assert_eq!(
            repository.get_selection_evidence(evidence.id()).unwrap(),
            Some(evidence.clone())
        );
        assert!(matches!(
            repository.list_selection_evidence(0),
            Err(PermissionPolicyOperationsRepositoryError::InvalidQueryLimit)
        ));

        let conn = database.conn.lock().unwrap();
        assert!(conn
            .execute(
                "UPDATE agent_os_permission_policy_selection_evidence SET selected_at=51",
                [],
            )
            .is_err());
        assert!(conn
            .execute(
                "DELETE FROM agent_os_permission_policy_selection_evidence",
                [],
            )
            .is_err());
    }

    #[test]
    fn sqlite_load_rejects_noncanonical_nested_policy_json() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqlitePermissionPolicyOperationsRepository::new(database.clone());
        let (draft, published) = published_record(
            "policy-record:noncanonical",
            "permission-policy:noncanonical",
            1,
            PermissionPolicyLayer::Repository,
            None,
        );
        persist_published(&repository, draft, published.clone());

        let conn = database.conn.lock().unwrap();
        conn.execute_batch(
            "DROP TRIGGER trg_agent_os_permission_policy_record_update_consistent;
             DROP TRIGGER trg_agent_os_permission_policy_record_update_guard;
             UPDATE agent_os_permission_policy_records
             SET record_json=json_set(record_json,'$.policy.unexpected','ignored')
             WHERE policy_record_id='policy-record:noncanonical';",
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            repository.get_policy_record(published.id()),
            Err(PermissionPolicyOperationsRepositoryError::Persistence(_))
        ));
    }
}
