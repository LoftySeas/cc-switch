//! Append-only Role definition and revisioned Role Assignment repositories.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::role_domain::{
    RoleAssignment, RoleAssignmentId, RoleDefinition, RoleDomainError, RoleId,
};

#[derive(Debug, Error)]
pub enum RoleRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] RoleDomainError),
    #[error("Role definition is already registered: {id} v{version}")]
    DefinitionAlreadyRegistered { id: RoleId, version: u16 },
    #[error("Role definition was not found: {id} v{version}")]
    DefinitionNotFound { id: RoleId, version: u16 },
    #[error("Role Assignment is already registered: {0}")]
    AssignmentAlreadyRegistered(RoleAssignmentId),
    #[error("Role Assignment was not found: {0}")]
    AssignmentNotFound(RoleAssignmentId),
    #[error("Role Assignment revision conflict for {assignment_id}: expected {expected}, current {current}")]
    RevisionConflict {
        assignment_id: RoleAssignmentId,
        expected: u64,
        current: u64,
    },
    #[error("Role Assignment identity changed during update: {0}")]
    IdentityChanged(RoleAssignmentId),
    #[error("New Role Assignment must start as draft revision 1: {0}")]
    InvalidInitialState(RoleAssignmentId),
    #[error("Role Assignment update is not one legal lifecycle transition: {0}")]
    InvalidUpdate(RoleAssignmentId),
    #[error("Role repository lock failed: {0}")]
    RegistryLock(String),
    #[error("Agent lookup failed: {0}")]
    AgentLookup(String),
    #[error("Agent was not found for Role Assignment: {0}")]
    AgentNotFound(String),
    #[error("Role Assignment requires an active Agent: {0}")]
    AgentNotActive(String),
    #[error("Role Assignment is outside its validity interval: {0}")]
    OutsideValidity(RoleAssignmentId),
}

pub trait RoleRepository: Send + Sync {
    fn register_definition(&self, definition: RoleDefinition) -> Result<(), RoleRepositoryError>;
    fn get_definition(
        &self,
        role_id: &RoleId,
        version: u16,
    ) -> Result<Option<RoleDefinition>, RoleRepositoryError>;
    fn list_definitions(&self) -> Result<Vec<RoleDefinition>, RoleRepositoryError>;
    fn insert_assignment(&self, assignment: RoleAssignment) -> Result<(), RoleRepositoryError>;
    fn get_assignment(
        &self,
        assignment_id: &RoleAssignmentId,
    ) -> Result<Option<RoleAssignment>, RoleRepositoryError>;
    fn list_assignments_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<RoleAssignment>, RoleRepositoryError>;
    fn update_assignment(
        &self,
        assignment: RoleAssignment,
        expected_revision: u64,
    ) -> Result<(), RoleRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryRoleRepository {
    definitions: Arc<RwLock<HashMap<(RoleId, u16), RoleDefinition>>>,
    assignments: Arc<RwLock<HashMap<RoleAssignmentId, RoleAssignment>>>,
}

impl RoleRepository for InMemoryRoleRepository {
    fn register_definition(&self, definition: RoleDefinition) -> Result<(), RoleRepositoryError> {
        let key = (definition.id().clone(), definition.version());
        let mut definitions = self
            .definitions
            .write()
            .map_err(|error| RoleRepositoryError::RegistryLock(error.to_string()))?;
        if definitions.contains_key(&key) {
            return Err(RoleRepositoryError::DefinitionAlreadyRegistered {
                id: key.0,
                version: key.1,
            });
        }
        definitions.insert(key, definition);
        Ok(())
    }

    fn get_definition(
        &self,
        role_id: &RoleId,
        version: u16,
    ) -> Result<Option<RoleDefinition>, RoleRepositoryError> {
        let definitions = self
            .definitions
            .read()
            .map_err(|error| RoleRepositoryError::RegistryLock(error.to_string()))?;
        Ok(definitions.get(&(role_id.clone(), version)).cloned())
    }

    fn list_definitions(&self) -> Result<Vec<RoleDefinition>, RoleRepositoryError> {
        let definitions = self
            .definitions
            .read()
            .map_err(|error| RoleRepositoryError::RegistryLock(error.to_string()))?;
        let mut definitions = definitions.values().cloned().collect::<Vec<_>>();
        definitions.sort_by(|left, right| {
            left.id()
                .cmp(right.id())
                .then_with(|| left.version().cmp(&right.version()))
        });
        Ok(definitions)
    }

    fn insert_assignment(&self, assignment: RoleAssignment) -> Result<(), RoleRepositoryError> {
        assignment.validate()?;
        if assignment.lifecycle() != crate::role_domain::RoleAssignmentLifecycle::Draft
            || assignment.revision() != 1
            || assignment.created_at() != assignment.updated_at()
        {
            return Err(RoleRepositoryError::InvalidInitialState(
                assignment.id().clone(),
            ));
        }
        if self
            .get_definition(assignment.role_id(), assignment.role_version())?
            .is_none()
        {
            return Err(RoleRepositoryError::DefinitionNotFound {
                id: assignment.role_id().clone(),
                version: assignment.role_version(),
            });
        }
        let mut assignments = self
            .assignments
            .write()
            .map_err(|error| RoleRepositoryError::RegistryLock(error.to_string()))?;
        if assignments.contains_key(assignment.id()) {
            return Err(RoleRepositoryError::AssignmentAlreadyRegistered(
                assignment.id().clone(),
            ));
        }
        assignments.insert(assignment.id().clone(), assignment);
        Ok(())
    }

    fn get_assignment(
        &self,
        assignment_id: &RoleAssignmentId,
    ) -> Result<Option<RoleAssignment>, RoleRepositoryError> {
        let assignments = self
            .assignments
            .read()
            .map_err(|error| RoleRepositoryError::RegistryLock(error.to_string()))?;
        Ok(assignments.get(assignment_id).cloned())
    }

    fn list_assignments_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<RoleAssignment>, RoleRepositoryError> {
        let assignments = self
            .assignments
            .read()
            .map_err(|error| RoleRepositoryError::RegistryLock(error.to_string()))?;
        let mut assignments = assignments
            .values()
            .filter(|assignment| assignment.agent_id() == agent_id)
            .cloned()
            .collect::<Vec<_>>();
        assignments.sort_by(|left, right| {
            left.created_at()
                .cmp(&right.created_at())
                .then_with(|| left.id().cmp(right.id()))
        });
        Ok(assignments)
    }

    fn update_assignment(
        &self,
        assignment: RoleAssignment,
        expected_revision: u64,
    ) -> Result<(), RoleRepositoryError> {
        assignment.validate()?;
        let mut assignments = self
            .assignments
            .write()
            .map_err(|error| RoleRepositoryError::RegistryLock(error.to_string()))?;
        let current = assignments
            .get(assignment.id())
            .ok_or_else(|| RoleRepositoryError::AssignmentNotFound(assignment.id().clone()))?;
        if current.revision() != expected_revision {
            return Err(RoleRepositoryError::RevisionConflict {
                assignment_id: assignment.id().clone(),
                expected: expected_revision,
                current: current.revision(),
            });
        }
        if current.agent_id() != assignment.agent_id()
            || current.membership_ref() != assignment.membership_ref()
            || current.role_id() != assignment.role_id()
            || current.role_version() != assignment.role_version()
            || current.scope() != assignment.scope()
        {
            return Err(RoleRepositoryError::IdentityChanged(
                assignment.id().clone(),
            ));
        }
        if assignment.revision() != expected_revision + 1
            || !current
                .lifecycle()
                .can_transition_to(assignment.lifecycle())
        {
            return Err(RoleRepositoryError::InvalidUpdate(assignment.id().clone()));
        }
        assignments.insert(assignment.id().clone(), assignment);
        Ok(())
    }
}
