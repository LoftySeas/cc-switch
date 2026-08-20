//! Replaceable persistence boundary for Organization governance.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use crate::{
    database::{lock_conn, Database},
    error::AppError,
    organization_governance::{
        Organization, OrganizationBindingLifecycle, OrganizationBoundaryEvidence,
        OrganizationBoundaryEvidenceId, OrganizationGovernanceDomainError, OrganizationId,
        OrganizationLifecycle, OrganizationPolicyBinding, OrganizationPolicyBindingId,
        OrganizationPolicyTarget, OrganizationTeamBinding, OrganizationTeamBindingId,
    },
    permission_domain::PermissionPolicyLayer,
    team_domain::TeamId,
};

pub const MAX_ORGANIZATION_QUERY_LIMIT: usize = 1_000;

#[derive(Debug, Error)]
pub enum OrganizationGovernanceRepositoryError {
    #[error(transparent)]
    Domain(#[from] OrganizationGovernanceDomainError),
    #[error("{aggregate} is already registered: {id}")]
    AlreadyExists { aggregate: &'static str, id: String },
    #[error("{aggregate} was not found: {id}")]
    NotFound { aggregate: &'static str, id: String },
    #[error("{aggregate} revision conflict: expected {expected}, current {current}")]
    RevisionConflict {
        aggregate: &'static str,
        expected: u64,
        current: u64,
    },
    #[error("{aggregate} identity or immutable fields changed")]
    ImmutableFieldChanged { aggregate: &'static str },
    #[error("{aggregate} lifecycle update is invalid")]
    InvalidLifecycle { aggregate: &'static str },
    #[error("Archived Organization is read-only: {0}")]
    ArchivedReadOnly(OrganizationId),
    #[error("Organization must be Active for this operation: {0}")]
    OrganizationNotActive(OrganizationId),
    #[error("Team already has an Active owning Organization: {0}")]
    ActiveTeamOwnerConflict(TeamId),
    #[error("Policy target already has an Active owning Organization binding")]
    ActivePolicyOwnerConflict,
    #[error("Organization still has Active Team or policy bindings: {0}")]
    ActiveBindingsRemain(OrganizationId),
    #[error(
        "Organization-scoped query limit must be between 1 and {MAX_ORGANIZATION_QUERY_LIMIT}"
    )]
    InvalidQueryLimit,
    #[error("Persisted {aggregate} columns do not match validated canonical JSON")]
    PersistedStateMismatch { aggregate: &'static str },
    #[error("Organization governance repository lock failed: {0}")]
    RegistryLock(String),
    #[error("Organization governance persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for OrganizationGovernanceRepositoryError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait OrganizationGovernanceRepository: Send + Sync {
    fn insert_organization(
        &self,
        organization: Organization,
    ) -> Result<(), OrganizationGovernanceRepositoryError>;
    fn get_organization(
        &self,
        organization_id: &OrganizationId,
    ) -> Result<Option<Organization>, OrganizationGovernanceRepositoryError>;
    fn list_organizations(
        &self,
        limit: usize,
    ) -> Result<Vec<Organization>, OrganizationGovernanceRepositoryError>;
    fn update_organization(
        &self,
        organization: Organization,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError>;

    fn insert_team_binding(
        &self,
        binding: OrganizationTeamBinding,
    ) -> Result<(), OrganizationGovernanceRepositoryError>;
    fn get_team_binding(
        &self,
        binding_id: &OrganizationTeamBindingId,
    ) -> Result<Option<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError>;
    fn get_active_team_binding(
        &self,
        team_id: &TeamId,
        at: i64,
    ) -> Result<Option<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError>;
    fn list_team_bindings(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError>;
    fn update_team_binding(
        &self,
        binding: OrganizationTeamBinding,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError>;

    fn insert_policy_binding(
        &self,
        binding: OrganizationPolicyBinding,
    ) -> Result<(), OrganizationGovernanceRepositoryError>;
    fn get_policy_binding(
        &self,
        binding_id: &OrganizationPolicyBindingId,
    ) -> Result<Option<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError>;
    fn get_active_policy_binding(
        &self,
        target: &OrganizationPolicyTarget,
        at: i64,
    ) -> Result<Option<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError>;
    fn list_policy_bindings(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError>;
    fn update_policy_binding(
        &self,
        binding: OrganizationPolicyBinding,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError>;

    fn append_boundary_evidence(
        &self,
        evidence: OrganizationBoundaryEvidence,
    ) -> Result<(), OrganizationGovernanceRepositoryError>;
    fn get_boundary_evidence(
        &self,
        evidence_id: &OrganizationBoundaryEvidenceId,
    ) -> Result<Option<OrganizationBoundaryEvidence>, OrganizationGovernanceRepositoryError>;
    fn list_boundary_evidence(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationBoundaryEvidence>, OrganizationGovernanceRepositoryError>;
}

#[derive(Default)]
struct InMemoryState {
    organizations: HashMap<OrganizationId, Organization>,
    team_bindings: HashMap<OrganizationTeamBindingId, OrganizationTeamBinding>,
    policy_bindings: HashMap<OrganizationPolicyBindingId, OrganizationPolicyBinding>,
    evidence: HashMap<OrganizationBoundaryEvidenceId, OrganizationBoundaryEvidence>,
}

#[derive(Clone, Default)]
pub struct InMemoryOrganizationGovernanceRepository {
    state: Arc<RwLock<InMemoryState>>,
}

impl InMemoryOrganizationGovernanceRepository {
    fn read(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, InMemoryState>, OrganizationGovernanceRepositoryError>
    {
        self.state
            .read()
            .map_err(|error| OrganizationGovernanceRepositoryError::RegistryLock(error.to_string()))
    }

    fn write(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, InMemoryState>, OrganizationGovernanceRepositoryError>
    {
        self.state
            .write()
            .map_err(|error| OrganizationGovernanceRepositoryError::RegistryLock(error.to_string()))
    }
}

fn validate_limit(limit: usize) -> Result<(), OrganizationGovernanceRepositoryError> {
    if !(1..=MAX_ORGANIZATION_QUERY_LIMIT).contains(&limit) {
        return Err(OrganizationGovernanceRepositoryError::InvalidQueryLimit);
    }
    Ok(())
}

fn validate_organization_update(
    current: &Organization,
    next: &Organization,
    expected_revision: u64,
) -> Result<(), OrganizationGovernanceRepositoryError> {
    current.validate()?;
    next.validate()?;
    if current.revision() != expected_revision {
        return Err(OrganizationGovernanceRepositoryError::RevisionConflict {
            aggregate: "Organization",
            expected: expected_revision,
            current: current.revision(),
        });
    }
    if current.lifecycle() == OrganizationLifecycle::Archived {
        return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
            current.id().clone(),
        ));
    }
    if current.id() != next.id()
        || current.display_name() != next.display_name()
        || current.purpose() != next.purpose()
        || current.owner_ref() != next.owner_ref()
        || current.provenance_ref() != next.provenance_ref()
        || current.created_at() != next.created_at()
    {
        return Err(
            OrganizationGovernanceRepositoryError::ImmutableFieldChanged {
                aggregate: "Organization",
            },
        );
    }
    if next.revision() != expected_revision + 1
        || !current.lifecycle().can_transition_to(next.lifecycle())
        || next.updated_at() < current.updated_at()
    {
        return Err(OrganizationGovernanceRepositoryError::InvalidLifecycle {
            aggregate: "Organization",
        });
    }
    Ok(())
}

fn validate_team_binding_update(
    current: &OrganizationTeamBinding,
    next: &OrganizationTeamBinding,
    expected_revision: u64,
) -> Result<(), OrganizationGovernanceRepositoryError> {
    current.validate()?;
    next.validate()?;
    if current.revision() != expected_revision {
        return Err(OrganizationGovernanceRepositoryError::RevisionConflict {
            aggregate: "Organization Team binding",
            expected: expected_revision,
            current: current.revision(),
        });
    }
    if current.id() != next.id()
        || current.organization_id() != next.organization_id()
        || current.team_id() != next.team_id()
        || current.valid_from() != next.valid_from()
        || current.valid_until() != next.valid_until()
        || current.created_at() != next.created_at()
        || current.provenance_ref() != next.provenance_ref()
    {
        return Err(
            OrganizationGovernanceRepositoryError::ImmutableFieldChanged {
                aggregate: "Organization Team binding",
            },
        );
    }
    let legal = matches!(
        (current.lifecycle(), next.lifecycle()),
        (
            OrganizationBindingLifecycle::Draft,
            OrganizationBindingLifecycle::Active
        ) | (
            OrganizationBindingLifecycle::Active,
            OrganizationBindingLifecycle::Ended
        )
    );
    if next.revision() != expected_revision + 1 || !legal {
        return Err(OrganizationGovernanceRepositoryError::InvalidLifecycle {
            aggregate: "Organization Team binding",
        });
    }
    Ok(())
}

fn validate_policy_binding_update(
    current: &OrganizationPolicyBinding,
    next: &OrganizationPolicyBinding,
    expected_revision: u64,
) -> Result<(), OrganizationGovernanceRepositoryError> {
    current.validate()?;
    next.validate()?;
    if current.revision() != expected_revision {
        return Err(OrganizationGovernanceRepositoryError::RevisionConflict {
            aggregate: "Organization policy binding",
            expected: expected_revision,
            current: current.revision(),
        });
    }
    if current.id() != next.id()
        || current.organization_id() != next.organization_id()
        || current.target() != next.target()
        || current.valid_from() != next.valid_from()
        || current.valid_until() != next.valid_until()
        || current.created_at() != next.created_at()
        || current.provenance_ref() != next.provenance_ref()
    {
        return Err(
            OrganizationGovernanceRepositoryError::ImmutableFieldChanged {
                aggregate: "Organization policy binding",
            },
        );
    }
    let legal = matches!(
        (current.lifecycle(), next.lifecycle()),
        (
            OrganizationBindingLifecycle::Draft,
            OrganizationBindingLifecycle::Active
        ) | (
            OrganizationBindingLifecycle::Active,
            OrganizationBindingLifecycle::Ended
        )
    );
    if next.revision() != expected_revision + 1 || !legal {
        return Err(OrganizationGovernanceRepositoryError::InvalidLifecycle {
            aggregate: "Organization policy binding",
        });
    }
    Ok(())
}

impl OrganizationGovernanceRepository for InMemoryOrganizationGovernanceRepository {
    fn insert_organization(
        &self,
        organization: Organization,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        organization.validate()?;
        if organization.lifecycle() != OrganizationLifecycle::Draft || organization.revision() != 1
        {
            return Err(OrganizationGovernanceRepositoryError::InvalidLifecycle {
                aggregate: "Organization",
            });
        }
        let mut state = self.write()?;
        if state.organizations.contains_key(organization.id()) {
            return Err(OrganizationGovernanceRepositoryError::AlreadyExists {
                aggregate: "Organization",
                id: organization.id().to_string(),
            });
        }
        state
            .organizations
            .insert(organization.id().clone(), organization);
        Ok(())
    }

    fn get_organization(
        &self,
        organization_id: &OrganizationId,
    ) -> Result<Option<Organization>, OrganizationGovernanceRepositoryError> {
        Ok(self.read()?.organizations.get(organization_id).cloned())
    }

    fn list_organizations(
        &self,
        limit: usize,
    ) -> Result<Vec<Organization>, OrganizationGovernanceRepositoryError> {
        validate_limit(limit)?;
        let mut values = self
            .read()?
            .organizations
            .values()
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.id().cmp(right.id()));
        values.truncate(limit);
        Ok(values)
    }

    fn update_organization(
        &self,
        organization: Organization,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        let mut state = self.write()?;
        let current = state.organizations.get(organization.id()).ok_or_else(|| {
            OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: organization.id().to_string(),
            }
        })?;
        validate_organization_update(current, &organization, expected_revision)?;
        if organization.lifecycle() == OrganizationLifecycle::Archived
            && (state.team_bindings.values().any(|binding| {
                binding.organization_id() == organization.id()
                    && binding.lifecycle() == OrganizationBindingLifecycle::Active
            }) || state.policy_bindings.values().any(|binding| {
                binding.organization_id() == organization.id()
                    && binding.lifecycle() == OrganizationBindingLifecycle::Active
            }))
        {
            return Err(OrganizationGovernanceRepositoryError::ActiveBindingsRemain(
                organization.id().clone(),
            ));
        }
        state
            .organizations
            .insert(organization.id().clone(), organization);
        Ok(())
    }

    fn insert_team_binding(
        &self,
        binding: OrganizationTeamBinding,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        binding.validate()?;
        if binding.lifecycle() != OrganizationBindingLifecycle::Draft {
            return Err(OrganizationGovernanceRepositoryError::InvalidLifecycle {
                aggregate: "Organization Team binding",
            });
        }
        let mut state = self.write()?;
        let organization = state
            .organizations
            .get(binding.organization_id())
            .ok_or_else(|| OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: binding.organization_id().to_string(),
            })?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            ));
        }
        if state.team_bindings.contains_key(binding.id()) {
            return Err(OrganizationGovernanceRepositoryError::AlreadyExists {
                aggregate: "Organization Team binding",
                id: binding.id().to_string(),
            });
        }
        state.team_bindings.insert(binding.id().clone(), binding);
        Ok(())
    }

    fn get_team_binding(
        &self,
        binding_id: &OrganizationTeamBindingId,
    ) -> Result<Option<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError> {
        Ok(self.read()?.team_bindings.get(binding_id).cloned())
    }

    fn get_active_team_binding(
        &self,
        team_id: &TeamId,
        at: i64,
    ) -> Result<Option<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError> {
        Ok(self
            .read()?
            .team_bindings
            .values()
            .find(|binding| binding.team_id() == team_id && binding.is_effective_at(at))
            .cloned())
    }

    fn list_team_bindings(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError> {
        validate_limit(limit)?;
        let mut values = self
            .read()?
            .team_bindings
            .values()
            .filter(|binding| binding.organization_id() == organization_id)
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.id().cmp(right.id()));
        values.truncate(limit);
        Ok(values)
    }

    fn update_team_binding(
        &self,
        binding: OrganizationTeamBinding,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        let mut state = self.write()?;
        let current = state.team_bindings.get(binding.id()).ok_or_else(|| {
            OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization Team binding",
                id: binding.id().to_string(),
            }
        })?;
        validate_team_binding_update(current, &binding, expected_revision)?;
        let organization = state
            .organizations
            .get(binding.organization_id())
            .ok_or_else(|| OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: binding.organization_id().to_string(),
            })?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            ));
        }
        if binding.lifecycle() == OrganizationBindingLifecycle::Active {
            if organization.lifecycle() != OrganizationLifecycle::Active {
                return Err(
                    OrganizationGovernanceRepositoryError::OrganizationNotActive(
                        organization.id().clone(),
                    ),
                );
            }
            if state.team_bindings.values().any(|other| {
                other.id() != binding.id()
                    && other.team_id() == binding.team_id()
                    && other.lifecycle() == OrganizationBindingLifecycle::Active
            }) {
                return Err(
                    OrganizationGovernanceRepositoryError::ActiveTeamOwnerConflict(
                        binding.team_id().clone(),
                    ),
                );
            }
        }
        state.team_bindings.insert(binding.id().clone(), binding);
        Ok(())
    }

    fn insert_policy_binding(
        &self,
        binding: OrganizationPolicyBinding,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        binding.validate()?;
        if binding.lifecycle() != OrganizationBindingLifecycle::Draft {
            return Err(OrganizationGovernanceRepositoryError::InvalidLifecycle {
                aggregate: "Organization policy binding",
            });
        }
        let mut state = self.write()?;
        let organization = state
            .organizations
            .get(binding.organization_id())
            .ok_or_else(|| OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: binding.organization_id().to_string(),
            })?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            ));
        }
        if state.policy_bindings.contains_key(binding.id()) {
            return Err(OrganizationGovernanceRepositoryError::AlreadyExists {
                aggregate: "Organization policy binding",
                id: binding.id().to_string(),
            });
        }
        state.policy_bindings.insert(binding.id().clone(), binding);
        Ok(())
    }

    fn get_policy_binding(
        &self,
        binding_id: &OrganizationPolicyBindingId,
    ) -> Result<Option<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError> {
        Ok(self.read()?.policy_bindings.get(binding_id).cloned())
    }

    fn get_active_policy_binding(
        &self,
        target: &OrganizationPolicyTarget,
        at: i64,
    ) -> Result<Option<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError> {
        Ok(self
            .read()?
            .policy_bindings
            .values()
            .find(|binding| binding.target() == target && binding.is_effective_at(at))
            .cloned())
    }

    fn list_policy_bindings(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError> {
        validate_limit(limit)?;
        let mut values = self
            .read()?
            .policy_bindings
            .values()
            .filter(|binding| binding.organization_id() == organization_id)
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.id().cmp(right.id()));
        values.truncate(limit);
        Ok(values)
    }

    fn update_policy_binding(
        &self,
        binding: OrganizationPolicyBinding,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        let mut state = self.write()?;
        let current = state.policy_bindings.get(binding.id()).ok_or_else(|| {
            OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization policy binding",
                id: binding.id().to_string(),
            }
        })?;
        validate_policy_binding_update(current, &binding, expected_revision)?;
        let organization = state
            .organizations
            .get(binding.organization_id())
            .ok_or_else(|| OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: binding.organization_id().to_string(),
            })?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            ));
        }
        if binding.lifecycle() == OrganizationBindingLifecycle::Active {
            if organization.lifecycle() != OrganizationLifecycle::Active {
                return Err(
                    OrganizationGovernanceRepositoryError::OrganizationNotActive(
                        organization.id().clone(),
                    ),
                );
            }
            if state.policy_bindings.values().any(|other| {
                other.id() != binding.id()
                    && other.target() == binding.target()
                    && other.lifecycle() == OrganizationBindingLifecycle::Active
            }) {
                return Err(OrganizationGovernanceRepositoryError::ActivePolicyOwnerConflict);
            }
        }
        state.policy_bindings.insert(binding.id().clone(), binding);
        Ok(())
    }

    fn append_boundary_evidence(
        &self,
        evidence: OrganizationBoundaryEvidence,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        evidence.validate()?;
        let mut state = self.write()?;
        if state.evidence.contains_key(evidence.id()) {
            return Err(OrganizationGovernanceRepositoryError::AlreadyExists {
                aggregate: "Organization boundary evidence",
                id: evidence.id().to_string(),
            });
        }
        state.evidence.insert(evidence.id().clone(), evidence);
        Ok(())
    }

    fn get_boundary_evidence(
        &self,
        evidence_id: &OrganizationBoundaryEvidenceId,
    ) -> Result<Option<OrganizationBoundaryEvidence>, OrganizationGovernanceRepositoryError> {
        Ok(self.read()?.evidence.get(evidence_id).cloned())
    }

    fn list_boundary_evidence(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationBoundaryEvidence>, OrganizationGovernanceRepositoryError> {
        validate_limit(limit)?;
        let mut values = self
            .read()?
            .evidence
            .values()
            .filter(|evidence| evidence.references().organization_id() == organization_id)
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            (left.resolved_at(), left.id()).cmp(&(right.resolved_at(), right.id()))
        });
        values.truncate(limit);
        Ok(values)
    }
}

#[derive(Clone)]
pub struct SqliteOrganizationGovernanceRepository {
    database: Arc<Database>,
}

impl SqliteOrganizationGovernanceRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    fn encode<T: Serialize>(value: &T) -> Result<String, OrganizationGovernanceRepositoryError> {
        serde_json::to_string(value)
            .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))
    }

    fn decode<T: DeserializeOwned + Serialize>(
        aggregate: &'static str,
        raw: String,
    ) -> Result<T, OrganizationGovernanceRepositoryError> {
        let decoded: T = serde_json::from_str(&raw).map_err(|error| {
            OrganizationGovernanceRepositoryError::Persistence(format!(
                "invalid persisted {aggregate}: {error}"
            ))
        })?;
        let original: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
            OrganizationGovernanceRepositoryError::Persistence(error.to_string())
        })?;
        let canonical = serde_json::to_value(&decoded).map_err(|error| {
            OrganizationGovernanceRepositoryError::Persistence(error.to_string())
        })?;
        if original != canonical {
            return Err(
                OrganizationGovernanceRepositoryError::PersistedStateMismatch { aggregate },
            );
        }
        Ok(decoded)
    }

    fn duplicate(error: &rusqlite::Error) -> bool {
        error.to_string().contains("UNIQUE constraint failed")
    }

    fn map_write_error(
        error: rusqlite::Error,
        aggregate: &'static str,
        id: String,
    ) -> OrganizationGovernanceRepositoryError {
        let message = error.to_string();
        if message.contains("active Team owner")
            || message.contains("organization_team_bindings.team_id")
        {
            return TeamId::new(id)
                .map(OrganizationGovernanceRepositoryError::ActiveTeamOwnerConflict)
                .unwrap_or_else(|_| {
                    OrganizationGovernanceRepositoryError::Persistence(message.clone())
                });
        }
        if message.contains("active policy target")
            || message.contains("organization_policy_bindings") && message.contains("UNIQUE")
        {
            return OrganizationGovernanceRepositoryError::ActivePolicyOwnerConflict;
        }
        if message.contains("Active bindings cannot be archived")
            || message.contains("with Active bindings cannot be archived")
        {
            return OrganizationId::new(id)
                .map(OrganizationGovernanceRepositoryError::ActiveBindingsRemain)
                .unwrap_or_else(|_| {
                    OrganizationGovernanceRepositoryError::Persistence(message.clone())
                });
        }
        if Self::duplicate(&error) {
            return OrganizationGovernanceRepositoryError::AlreadyExists { aggregate, id };
        }
        OrganizationGovernanceRepositoryError::Persistence(message)
    }

    fn get_organization_tx(
        transaction: &Transaction<'_>,
        organization_id: &OrganizationId,
    ) -> Result<Option<Organization>, OrganizationGovernanceRepositoryError> {
        let row = transaction
            .query_row(
                "SELECT organization_id,organization_json,lifecycle_state,revision,created_at,updated_at
                 FROM agent_os_organizations WHERE organization_id=?1",
                [organization_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| {
                OrganizationGovernanceRepositoryError::Persistence(error.to_string())
            })?;
        row.map(|(id, json, lifecycle, revision, created_at, updated_at)| {
            let value: Organization = Self::decode("Organization", json)?;
            if value.id().as_str() != id
                || value.lifecycle().as_str() != lifecycle
                || value.revision() != revision as u64
                || value.created_at() != created_at
                || value.updated_at() != updated_at
            {
                return Err(
                    OrganizationGovernanceRepositoryError::PersistedStateMismatch {
                        aggregate: "Organization",
                    },
                );
            }
            Ok(value)
        })
        .transpose()
    }
}

fn layer_name(layer: PermissionPolicyLayer) -> &'static str {
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

fn decode_organization_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, String, i64, i64, i64)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn validate_organization_row(
    row: (String, String, String, i64, i64, i64),
) -> Result<Organization, OrganizationGovernanceRepositoryError> {
    let value: Organization =
        SqliteOrganizationGovernanceRepository::decode("Organization", row.1)?;
    if value.id().as_str() != row.0
        || value.lifecycle().as_str() != row.2
        || value.revision() != row.3 as u64
        || value.created_at() != row.4
        || value.updated_at() != row.5
    {
        return Err(
            OrganizationGovernanceRepositoryError::PersistedStateMismatch {
                aggregate: "Organization",
            },
        );
    }
    Ok(value)
}

fn validate_team_binding_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        Option<i64>,
        i64,
        i64,
    ),
) -> Result<OrganizationTeamBinding, OrganizationGovernanceRepositoryError> {
    let value: OrganizationTeamBinding =
        SqliteOrganizationGovernanceRepository::decode("Organization Team binding", row.1)?;
    if value.id().as_str() != row.0
        || value.organization_id().as_str() != row.2
        || value.team_id().as_str() != row.3
        || value.lifecycle().as_str() != row.4
        || value.revision() != row.5 as u64
        || value.valid_from() != row.6
        || value.valid_until() != row.7
        || value.created_at() != row.8
        || value.updated_at() != row.9
    {
        return Err(
            OrganizationGovernanceRepositoryError::PersistedStateMismatch {
                aggregate: "Organization Team binding",
            },
        );
    }
    Ok(value)
}

#[allow(clippy::type_complexity)]
fn validate_policy_binding_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        Option<String>,
        String,
        i64,
        i64,
        Option<i64>,
        i64,
        i64,
    ),
) -> Result<OrganizationPolicyBinding, OrganizationGovernanceRepositoryError> {
    let value: OrganizationPolicyBinding =
        SqliteOrganizationGovernanceRepository::decode("Organization policy binding", row.1)?;
    if value.id().as_str() != row.0
        || value.organization_id().as_str() != row.2
        || value.target().kind() != row.3
        || value.target().record_id().as_str() != row.4
        || value.target().policy_ref().policy_id().as_str() != row.5
        || i64::from(value.target().policy_ref().version()) != row.6
        || layer_name(value.target().policy_ref().layer()) != row.7
        || value.target().scope_binding_id().map(|id| id.as_str()) != row.8.as_deref()
        || value.lifecycle().as_str() != row.9
        || value.revision() != row.10 as u64
        || value.valid_from() != row.11
        || value.valid_until() != row.12
        || value.created_at() != row.13
        || value.updated_at() != row.14
    {
        return Err(
            OrganizationGovernanceRepositoryError::PersistedStateMismatch {
                aggregate: "Organization policy binding",
            },
        );
    }
    Ok(value)
}

fn validate_evidence_row(
    row: (String, String, String, String, i64),
) -> Result<OrganizationBoundaryEvidence, OrganizationGovernanceRepositoryError> {
    let value: OrganizationBoundaryEvidence =
        SqliteOrganizationGovernanceRepository::decode("Organization boundary evidence", row.1)?;
    if value.id().as_str() != row.0
        || value.references().organization_id().as_str() != row.2
        || value.outcome().as_str() != row.3
        || value.resolved_at() != row.4
    {
        return Err(
            OrganizationGovernanceRepositoryError::PersistedStateMismatch {
                aggregate: "Organization boundary evidence",
            },
        );
    }
    Ok(value)
}

impl OrganizationGovernanceRepository for SqliteOrganizationGovernanceRepository {
    fn insert_organization(
        &self,
        organization: Organization,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        organization.validate()?;
        if organization.lifecycle() != OrganizationLifecycle::Draft || organization.revision() != 1
        {
            return Err(OrganizationGovernanceRepositoryError::InvalidLifecycle {
                aggregate: "Organization",
            });
        }
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_organizations
             (organization_id,organization_json,lifecycle_state,revision,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                organization.id().as_str(),
                Self::encode(&organization)?,
                organization.lifecycle().as_str(),
                organization.revision() as i64,
                organization.created_at(),
                organization.updated_at()
            ],
        )
        .map_err(|error| {
            Self::map_write_error(error, "Organization", organization.id().to_string())
        })?;
        Ok(())
    }

    fn get_organization(
        &self,
        organization_id: &OrganizationId,
    ) -> Result<Option<Organization>, OrganizationGovernanceRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        conn.query_row(
            "SELECT organization_id,organization_json,lifecycle_state,revision,created_at,updated_at
             FROM agent_os_organizations WHERE organization_id=?1",
            [organization_id.as_str()],
            decode_organization_row,
        )
        .optional()
        .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?
        .map(validate_organization_row)
        .transpose()
    }

    fn list_organizations(
        &self,
        limit: usize,
    ) -> Result<Vec<Organization>, OrganizationGovernanceRepositoryError> {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn
            .prepare(
                "SELECT organization_id,organization_json,lifecycle_state,revision,created_at,updated_at
                 FROM agent_os_organizations ORDER BY organization_id LIMIT ?1",
            )
            .map_err(|error| {
                OrganizationGovernanceRepositoryError::Persistence(error.to_string())
            })?;
        let values = statement
            .query_map([limit as i64], decode_organization_row)
            .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?
            .map(|row| {
                row.map_err(|error| {
                    OrganizationGovernanceRepositoryError::Persistence(error.to_string())
                })
                .and_then(validate_organization_row)
            })
            .collect();
        values
    }

    fn update_organization(
        &self,
        organization: Organization,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        let mut conn = lock_conn!(self.database.conn);
        let transaction = conn.transaction().map_err(|error| {
            OrganizationGovernanceRepositoryError::Persistence(error.to_string())
        })?;
        let current =
            Self::get_organization_tx(&transaction, organization.id())?.ok_or_else(|| {
                OrganizationGovernanceRepositoryError::NotFound {
                    aggregate: "Organization",
                    id: organization.id().to_string(),
                }
            })?;
        validate_organization_update(&current, &organization, expected_revision)?;
        let changed = transaction
            .execute(
                "UPDATE agent_os_organizations SET organization_json=?1,lifecycle_state=?2,
                 revision=?3,updated_at=?4 WHERE organization_id=?5 AND revision=?6",
                params![
                    Self::encode(&organization)?,
                    organization.lifecycle().as_str(),
                    organization.revision() as i64,
                    organization.updated_at(),
                    organization.id().as_str(),
                    expected_revision as i64
                ],
            )
            .map_err(|error| {
                Self::map_write_error(error, "Organization", organization.id().to_string())
            })?;
        if changed != 1 {
            return Err(OrganizationGovernanceRepositoryError::RevisionConflict {
                aggregate: "Organization",
                expected: expected_revision,
                current: current.revision(),
            });
        }
        transaction
            .commit()
            .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))
    }

    fn insert_team_binding(
        &self,
        binding: OrganizationTeamBinding,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        binding.validate()?;
        let organization = self
            .get_organization(binding.organization_id())?
            .ok_or_else(|| OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: binding.organization_id().to_string(),
            })?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            ));
        }
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_organization_team_bindings
             (binding_id,organization_id,team_id,binding_json,lifecycle_state,revision,
              valid_from,valid_until,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                binding.id().as_str(),
                binding.organization_id().as_str(),
                binding.team_id().as_str(),
                Self::encode(&binding)?,
                binding.lifecycle().as_str(),
                binding.revision() as i64,
                binding.valid_from(),
                binding.valid_until(),
                binding.created_at(),
                binding.updated_at()
            ],
        )
        .map_err(|error| {
            Self::map_write_error(error, "Organization Team binding", binding.id().to_string())
        })?;
        Ok(())
    }

    fn get_team_binding(
        &self,
        binding_id: &OrganizationTeamBindingId,
    ) -> Result<Option<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        conn.query_row(
            "SELECT binding_id,binding_json,organization_id,team_id,lifecycle_state,revision,
                    valid_from,valid_until,created_at,updated_at
             FROM agent_os_organization_team_bindings WHERE binding_id=?1",
            [binding_id.as_str()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .optional()
        .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?
        .map(validate_team_binding_row)
        .transpose()
    }

    fn get_active_team_binding(
        &self,
        team_id: &TeamId,
        at: i64,
    ) -> Result<Option<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        conn.query_row(
            "SELECT binding_id,binding_json,organization_id,team_id,lifecycle_state,revision,
                    valid_from,valid_until,created_at,updated_at
             FROM agent_os_organization_team_bindings
             WHERE team_id=?1 AND lifecycle_state='active' AND valid_from<=?2
               AND (valid_until IS NULL OR valid_until>=?2) LIMIT 1",
            params![team_id.as_str(), at],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .optional()
        .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?
        .map(validate_team_binding_row)
        .transpose()
    }

    fn list_team_bindings(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationTeamBinding>, OrganizationGovernanceRepositoryError> {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let mut statement=conn.prepare("SELECT binding_id,binding_json,organization_id,team_id,lifecycle_state,revision,valid_from,valid_until,created_at,updated_at FROM agent_os_organization_team_bindings WHERE organization_id=?1 ORDER BY binding_id LIMIT ?2").map_err(|error|OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?;
        let values = statement
            .query_map(params![organization_id.as_str(), limit as i64], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            })
            .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?
            .map(|row| {
                row.map_err(|error| {
                    OrganizationGovernanceRepositoryError::Persistence(error.to_string())
                })
                .and_then(validate_team_binding_row)
            })
            .collect();
        values
    }

    fn update_team_binding(
        &self,
        binding: OrganizationTeamBinding,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        let current = self.get_team_binding(binding.id())?.ok_or_else(|| {
            OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization Team binding",
                id: binding.id().to_string(),
            }
        })?;
        validate_team_binding_update(&current, &binding, expected_revision)?;
        let organization = self
            .get_organization(binding.organization_id())?
            .ok_or_else(|| OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: binding.organization_id().to_string(),
            })?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            ));
        }
        if binding.lifecycle() == OrganizationBindingLifecycle::Active
            && organization.lifecycle() != OrganizationLifecycle::Active
        {
            return Err(
                OrganizationGovernanceRepositoryError::OrganizationNotActive(
                    organization.id().clone(),
                ),
            );
        }
        let conn = lock_conn!(self.database.conn);
        let changed=conn.execute("UPDATE agent_os_organization_team_bindings SET binding_json=?1,lifecycle_state=?2,revision=?3,updated_at=?4 WHERE binding_id=?5 AND revision=?6",params![Self::encode(&binding)?,binding.lifecycle().as_str(),binding.revision() as i64,binding.updated_at(),binding.id().as_str(),expected_revision as i64]).map_err(|error|Self::map_write_error(error,"Organization Team binding",binding.team_id().to_string()))?;
        if changed != 1 {
            return Err(OrganizationGovernanceRepositoryError::RevisionConflict {
                aggregate: "Organization Team binding",
                expected: expected_revision,
                current: current.revision(),
            });
        }
        Ok(())
    }

    fn insert_policy_binding(
        &self,
        binding: OrganizationPolicyBinding,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        binding.validate()?;
        let organization = self
            .get_organization(binding.organization_id())?
            .ok_or_else(|| OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: binding.organization_id().to_string(),
            })?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            ));
        }
        let target = binding.target();
        let conn = lock_conn!(self.database.conn);
        conn.execute("INSERT INTO agent_os_organization_policy_bindings (binding_id,organization_id,target_kind,policy_record_id,policy_id,policy_version,policy_layer,policy_scope_binding_id,binding_json,lifecycle_state,revision,valid_from,valid_until,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",params![binding.id().as_str(),binding.organization_id().as_str(),target.kind(),target.record_id().as_str(),target.policy_ref().policy_id().as_str(),i64::from(target.policy_ref().version()),layer_name(target.policy_ref().layer()),target.scope_binding_id().map(|id|id.as_str()),Self::encode(&binding)?,binding.lifecycle().as_str(),binding.revision() as i64,binding.valid_from(),binding.valid_until(),binding.created_at(),binding.updated_at()]).map_err(|error|Self::map_write_error(error,"Organization policy binding",binding.id().to_string()))?;
        Ok(())
    }

    fn get_policy_binding(
        &self,
        binding_id: &OrganizationPolicyBindingId,
    ) -> Result<Option<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        conn.query_row("SELECT binding_id,binding_json,organization_id,target_kind,policy_record_id,policy_id,policy_version,policy_layer,policy_scope_binding_id,lifecycle_state,revision,valid_from,valid_until,created_at,updated_at FROM agent_os_organization_policy_bindings WHERE binding_id=?1",[binding_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?,row.get(11)?,row.get(12)?,row.get(13)?,row.get(14)?))).optional().map_err(|error|OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?.map(validate_policy_binding_row).transpose()
    }

    fn get_active_policy_binding(
        &self,
        target: &OrganizationPolicyTarget,
        at: i64,
    ) -> Result<Option<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        conn.query_row("SELECT binding_id,binding_json,organization_id,target_kind,policy_record_id,policy_id,policy_version,policy_layer,policy_scope_binding_id,lifecycle_state,revision,valid_from,valid_until,created_at,updated_at FROM agent_os_organization_policy_bindings WHERE target_kind=?1 AND policy_record_id=?2 AND policy_id=?3 AND policy_version=?4 AND policy_layer=?5 AND COALESCE(policy_scope_binding_id,'')=COALESCE(?6,'') AND lifecycle_state='active' AND valid_from<=?7 AND (valid_until IS NULL OR valid_until>=?7) LIMIT 1",params![target.kind(),target.record_id().as_str(),target.policy_ref().policy_id().as_str(),i64::from(target.policy_ref().version()),layer_name(target.policy_ref().layer()),target.scope_binding_id().map(|id|id.as_str()),at],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?,row.get(11)?,row.get(12)?,row.get(13)?,row.get(14)?))).optional().map_err(|error|OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?.map(validate_policy_binding_row).transpose()
    }

    fn list_policy_bindings(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationPolicyBinding>, OrganizationGovernanceRepositoryError> {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let mut statement=conn.prepare("SELECT binding_id,binding_json,organization_id,target_kind,policy_record_id,policy_id,policy_version,policy_layer,policy_scope_binding_id,lifecycle_state,revision,valid_from,valid_until,created_at,updated_at FROM agent_os_organization_policy_bindings WHERE organization_id=?1 ORDER BY binding_id LIMIT ?2").map_err(|error|OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?;
        let values = statement
            .query_map(params![organization_id.as_str(), limit as i64], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                ))
            })
            .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?
            .map(|row| {
                row.map_err(|error| {
                    OrganizationGovernanceRepositoryError::Persistence(error.to_string())
                })
                .and_then(validate_policy_binding_row)
            })
            .collect();
        values
    }

    fn update_policy_binding(
        &self,
        binding: OrganizationPolicyBinding,
        expected_revision: u64,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        let current = self.get_policy_binding(binding.id())?.ok_or_else(|| {
            OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization policy binding",
                id: binding.id().to_string(),
            }
        })?;
        validate_policy_binding_update(&current, &binding, expected_revision)?;
        let organization = self
            .get_organization(binding.organization_id())?
            .ok_or_else(|| OrganizationGovernanceRepositoryError::NotFound {
                aggregate: "Organization",
                id: binding.organization_id().to_string(),
            })?;
        if organization.lifecycle() == OrganizationLifecycle::Archived {
            return Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(
                organization.id().clone(),
            ));
        }
        if binding.lifecycle() == OrganizationBindingLifecycle::Active
            && organization.lifecycle() != OrganizationLifecycle::Active
        {
            return Err(
                OrganizationGovernanceRepositoryError::OrganizationNotActive(
                    organization.id().clone(),
                ),
            );
        }
        let conn = lock_conn!(self.database.conn);
        let changed=conn.execute("UPDATE agent_os_organization_policy_bindings SET binding_json=?1,lifecycle_state=?2,revision=?3,updated_at=?4 WHERE binding_id=?5 AND revision=?6",params![Self::encode(&binding)?,binding.lifecycle().as_str(),binding.revision() as i64,binding.updated_at(),binding.id().as_str(),expected_revision as i64]).map_err(|error|Self::map_write_error(error,"Organization policy binding",binding.id().to_string()))?;
        if changed != 1 {
            return Err(OrganizationGovernanceRepositoryError::RevisionConflict {
                aggregate: "Organization policy binding",
                expected: expected_revision,
                current: current.revision(),
            });
        }
        Ok(())
    }

    fn append_boundary_evidence(
        &self,
        evidence: OrganizationBoundaryEvidence,
    ) -> Result<(), OrganizationGovernanceRepositoryError> {
        evidence.validate()?;
        let conn = lock_conn!(self.database.conn);
        conn.execute("INSERT INTO agent_os_organization_boundary_evidence (evidence_id,organization_id,evidence_json,outcome,resolved_at) VALUES (?1,?2,?3,?4,?5)",params![evidence.id().as_str(),evidence.references().organization_id().as_str(),Self::encode(&evidence)?,evidence.outcome().as_str(),evidence.resolved_at()]).map_err(|error|Self::map_write_error(error,"Organization boundary evidence",evidence.id().to_string()))?;
        Ok(())
    }

    fn get_boundary_evidence(
        &self,
        evidence_id: &OrganizationBoundaryEvidenceId,
    ) -> Result<Option<OrganizationBoundaryEvidence>, OrganizationGovernanceRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        conn.query_row("SELECT evidence_id,evidence_json,organization_id,outcome,resolved_at FROM agent_os_organization_boundary_evidence WHERE evidence_id=?1",[evidence_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional().map_err(|error|OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?.map(validate_evidence_row).transpose()
    }

    fn list_boundary_evidence(
        &self,
        organization_id: &OrganizationId,
        limit: usize,
    ) -> Result<Vec<OrganizationBoundaryEvidence>, OrganizationGovernanceRepositoryError> {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let mut statement=conn.prepare("SELECT evidence_id,evidence_json,organization_id,outcome,resolved_at FROM agent_os_organization_boundary_evidence WHERE organization_id=?1 ORDER BY resolved_at,evidence_id LIMIT ?2").map_err(|error|OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?;
        let values = statement
            .query_map(params![organization_id.as_str(), limit as i64], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(|error| OrganizationGovernanceRepositoryError::Persistence(error.to_string()))?
            .map(|row| {
                row.map_err(|error| {
                    OrganizationGovernanceRepositoryError::Persistence(error.to_string())
                })
                .and_then(validate_evidence_row)
            })
            .collect();
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organization_governance::{
        OrganizationBoundaryDenialReason, OrganizationBoundaryOutcome,
        OrganizationBoundaryReferences, OrganizationLifecycle,
    };
    use crate::permission_domain::{
        PermissionAction, PermissionPolicy, PermissionPolicyId, PermissionRule,
        PermissionRuleEffect,
    };
    use crate::permission_policy_operations::{PermissionPolicyRecord, PermissionPolicyRecordId};
    use crate::permission_policy_operations_repository::{
        PermissionPolicyOperationsRepository, SqlitePermissionPolicyOperationsRepository,
    };
    use std::collections::BTreeMap;

    fn organization(id: &str, at: i64) -> Organization {
        Organization::new(
            OrganizationId::new(id).unwrap(),
            id,
            "Bounded organization purpose",
            "owner:test",
            "provenance:cod-031",
            at,
        )
        .unwrap()
    }

    #[test]
    fn in_memory_enforces_one_active_organization_per_team_and_scoped_lists() {
        let repository = InMemoryOrganizationGovernanceRepository::default();
        for id in ["organization:one", "organization:two"] {
            let draft = organization(id, 1);
            repository.insert_organization(draft.clone()).unwrap();
            repository
                .update_organization(
                    draft
                        .transition_to(OrganizationLifecycle::Active, 1, 2)
                        .unwrap(),
                    1,
                )
                .unwrap();
        }
        let team_id = TeamId::new("team:shared").unwrap();
        let first = OrganizationTeamBinding::new_draft(
            OrganizationTeamBindingId::new("organization-team-binding:first").unwrap(),
            OrganizationId::new("organization:one").unwrap(),
            team_id.clone(),
            2,
            None,
            "provenance:cod-031",
            2,
        )
        .unwrap();
        repository.insert_team_binding(first.clone()).unwrap();
        repository
            .update_team_binding(first.activate(1, 3).unwrap(), 1)
            .unwrap();
        let second = OrganizationTeamBinding::new_draft(
            OrganizationTeamBindingId::new("organization-team-binding:second").unwrap(),
            OrganizationId::new("organization:two").unwrap(),
            team_id,
            2,
            None,
            "provenance:cod-031",
            2,
        )
        .unwrap();
        repository.insert_team_binding(second.clone()).unwrap();
        assert!(matches!(
            repository.update_team_binding(second.activate(1, 3).unwrap(), 1),
            Err(OrganizationGovernanceRepositoryError::ActiveTeamOwnerConflict(_))
        ));
        assert_eq!(
            repository
                .list_team_bindings(&OrganizationId::new("organization:one").unwrap(), 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            repository
                .list_team_bindings(&OrganizationId::new("organization:two").unwrap(), 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn archived_organization_is_read_only() {
        let repository = InMemoryOrganizationGovernanceRepository::default();
        let draft = organization("organization:archived", 1);
        repository.insert_organization(draft.clone()).unwrap();
        let archived = draft
            .transition_to(OrganizationLifecycle::Archived, 1, 2)
            .unwrap();
        repository.update_organization(archived, 1).unwrap();
        let binding = OrganizationTeamBinding::new_draft(
            OrganizationTeamBindingId::new("organization-team-binding:late").unwrap(),
            OrganizationId::new("organization:archived").unwrap(),
            TeamId::new("team:late").unwrap(),
            2,
            None,
            "provenance:cod-031",
            2,
        )
        .unwrap();
        assert!(matches!(
            repository.insert_team_binding(binding),
            Err(OrganizationGovernanceRepositoryError::ArchivedReadOnly(_))
        ));
    }

    #[test]
    fn sqlite_round_trip_validates_canonical_rows_and_append_only_evidence() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteOrganizationGovernanceRepository::new(database.clone());
        let draft = organization("organization:sqlite", 1);
        repository.insert_organization(draft.clone()).unwrap();
        let active = draft
            .transition_to(OrganizationLifecycle::Active, 1, 2)
            .unwrap();
        repository.update_organization(active.clone(), 1).unwrap();
        assert_eq!(
            repository.get_organization(active.id()).unwrap(),
            Some(active.clone())
        );

        let references = OrganizationBoundaryReferences::new(
            active.id().clone(),
            Some(active.revision()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("workflow:sqlite".into()),
            None,
            None,
        )
        .unwrap();
        let evidence = OrganizationBoundaryEvidence::new(
            OrganizationBoundaryEvidenceId::new("organization-boundary:sqlite").unwrap(),
            references,
            OrganizationBoundaryOutcome::Denied(
                OrganizationBoundaryDenialReason::InactiveOrganization,
            ),
            3,
            "provenance:cod-031",
            "audit:sqlite",
        )
        .unwrap();
        repository
            .append_boundary_evidence(evidence.clone())
            .unwrap();
        assert_eq!(
            repository.get_boundary_evidence(evidence.id()).unwrap(),
            Some(evidence)
        );

        {
            let conn = database.conn.lock().unwrap();
            conn.execute_batch(
                "DROP TRIGGER trg_agent_os_organization_update_consistent;
                 DROP TRIGGER trg_agent_os_organization_update_guard;
                 UPDATE agent_os_organizations SET lifecycle_state='suspended'
                 WHERE organization_id='organization:sqlite';",
            )
            .unwrap();
        }
        assert!(matches!(
            repository.get_organization(active.id()),
            Err(
                OrganizationGovernanceRepositoryError::PersistedStateMismatch {
                    aggregate: "Organization"
                }
            )
        ));
    }

    #[test]
    fn sqlite_policy_binding_freezes_exact_published_record() {
        let database = Arc::new(Database::memory().unwrap());
        let organizations = SqliteOrganizationGovernanceRepository::new(database.clone());
        let policies = SqlitePermissionPolicyOperationsRepository::new(database);

        let draft_organization = organization("organization:policy-sqlite", 1);
        organizations
            .insert_organization(draft_organization.clone())
            .unwrap();
        organizations
            .update_organization(
                draft_organization
                    .transition_to(OrganizationLifecycle::Active, 1, 2)
                    .unwrap(),
                1,
            )
            .unwrap();

        let policy = PermissionPolicy::new(
            PermissionPolicyId::new("permission-policy:sqlite").unwrap(),
            1,
            PermissionPolicyLayer::Team,
            "owner:test",
            vec![PermissionRule::new(
                PermissionRuleEffect::Deny,
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:sqlite",
                BTreeMap::new(),
            )
            .unwrap()],
        )
        .unwrap();
        let draft_record = PermissionPolicyRecord::new_draft(
            PermissionPolicyRecordId::new("policy-record:sqlite").unwrap(),
            policy,
            "provenance:cod-031",
            None,
            1,
        )
        .unwrap();
        policies.insert_policy_record(draft_record.clone()).unwrap();
        let record = draft_record.publish(1, 2).unwrap();
        policies.update_policy_record(record.clone(), 1).unwrap();

        let target = OrganizationPolicyTarget::PolicyRecord {
            record_id: record.id().clone(),
            policy_ref: record.policy_ref(),
        };
        let draft_binding = OrganizationPolicyBinding::new_draft(
            OrganizationPolicyBindingId::new("organization-policy-binding:sqlite").unwrap(),
            OrganizationId::new("organization:policy-sqlite").unwrap(),
            target,
            2,
            None,
            "provenance:cod-031",
            2,
        )
        .unwrap();
        organizations
            .insert_policy_binding(draft_binding.clone())
            .unwrap();
        let active = draft_binding.activate(1, 3).unwrap();
        organizations
            .update_policy_binding(active.clone(), 1)
            .unwrap();
        assert_eq!(
            organizations.get_policy_binding(active.id()).unwrap(),
            Some(active)
        );
    }
}
