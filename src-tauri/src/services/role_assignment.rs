//! Role definition and Role Assignment application service.

use crate::{
    agent_domain::AgentLifecycle,
    database::Database,
    role_domain::{
        RoleAssignment, RoleAssignmentId, RoleAssignmentLifecycle, RoleDefinition, RoleId,
    },
    role_repository::{RoleRepository, RoleRepositoryError},
};

pub struct RoleAssignmentService<R> {
    repository: R,
}

impl<R> RoleAssignmentService<R>
where
    R: RoleRepository,
{
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn register_role(&self, role: RoleDefinition) -> Result<(), RoleRepositoryError> {
        self.repository.register_definition(role)
    }

    pub fn get_role(
        &self,
        role_id: &RoleId,
        version: u16,
    ) -> Result<RoleDefinition, RoleRepositoryError> {
        self.repository
            .get_definition(role_id, version)?
            .ok_or_else(|| RoleRepositoryError::DefinitionNotFound {
                id: role_id.clone(),
                version,
            })
    }

    pub fn list_roles(&self) -> Result<Vec<RoleDefinition>, RoleRepositoryError> {
        self.repository.list_definitions()
    }

    pub fn get_assignment(
        &self,
        assignment_id: &RoleAssignmentId,
    ) -> Result<RoleAssignment, RoleRepositoryError> {
        self.repository
            .get_assignment(assignment_id)?
            .ok_or_else(|| RoleRepositoryError::AssignmentNotFound(assignment_id.clone()))
    }

    pub fn list_assignments_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<RoleAssignment>, RoleRepositoryError> {
        self.repository.list_assignments_for_agent(agent_id)
    }

    pub fn create_assignment(
        &self,
        db: &Database,
        assignment: RoleAssignment,
    ) -> Result<RoleAssignment, RoleRepositoryError> {
        self.require_agent(db, assignment.agent_id(), false)?;
        self.require_role(&assignment)?;
        self.repository.insert_assignment(assignment.clone())?;
        Ok(assignment)
    }

    pub fn set_lifecycle(
        &self,
        db: &Database,
        assignment_id: &RoleAssignmentId,
        target: RoleAssignmentLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<RoleAssignment, RoleRepositoryError> {
        let current = self
            .repository
            .get_assignment(assignment_id)?
            .ok_or_else(|| RoleRepositoryError::AssignmentNotFound(assignment_id.clone()))?;
        if target == RoleAssignmentLifecycle::Active {
            self.require_agent(db, current.agent_id(), true)?;
            self.require_role(&current)?;
            if updated_at < current.valid_from()
                || current
                    .valid_until()
                    .is_some_and(|until| updated_at > until)
            {
                return Err(RoleRepositoryError::OutsideValidity(assignment_id.clone()));
            }
        }
        let updated = current.transition_to(target, expected_revision, updated_at)?;
        if updated != current {
            self.repository
                .update_assignment(updated.clone(), expected_revision)?;
        }
        Ok(updated)
    }

    fn require_role(&self, assignment: &RoleAssignment) -> Result<(), RoleRepositoryError> {
        if self
            .repository
            .get_definition(assignment.role_id(), assignment.role_version())?
            .is_none()
        {
            return Err(RoleRepositoryError::DefinitionNotFound {
                id: assignment.role_id().clone(),
                version: assignment.role_version(),
            });
        }
        Ok(())
    }

    fn require_agent(
        &self,
        db: &Database,
        agent_id: &str,
        require_active: bool,
    ) -> Result<(), RoleRepositoryError> {
        let agent = db
            .get_agent(agent_id)
            .map_err(|error| RoleRepositoryError::AgentLookup(error.to_string()))?
            .ok_or_else(|| RoleRepositoryError::AgentNotFound(agent_id.to_string()))?;
        if require_active && agent.lifecycle_state != AgentLifecycle::Active {
            return Err(RoleRepositoryError::AgentNotActive(agent_id.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent_domain::Agent,
        role_domain::{RoleAssignmentScope, RoleAssignmentScopeKind, RoleId},
        role_repository::InMemoryRoleRepository,
    };

    fn active_agent() -> Agent {
        Agent {
            id: "agent:one".into(),
            name: "Agent One".into(),
            description: "Test Agent".into(),
            owner: "owner:one".into(),
            lifecycle_state: AgentLifecycle::Active,
            revision: 1,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn role() -> RoleDefinition {
        RoleDefinition::new(
            RoleId::new("role:reviewer").unwrap(),
            2,
            "Reviewer",
            "Review bounded evidence",
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn assignment() -> RoleAssignment {
        RoleAssignment::new(
            RoleAssignmentId::new("assignment:one").unwrap(),
            "agent:one",
            "membership:one",
            RoleId::new("role:reviewer").unwrap(),
            2,
            RoleAssignmentScope::new(RoleAssignmentScopeKind::Review, "review:one").unwrap(),
            Vec::new(),
            Vec::new(),
            "provenance:owner",
            10,
            None,
            5,
        )
        .unwrap()
    }

    #[test]
    fn activation_requires_registered_role_and_active_agent() {
        let db = Database::memory().unwrap();
        db.insert_agent(&active_agent()).unwrap();
        let repository = InMemoryRoleRepository::default();
        let service = RoleAssignmentService::new(repository);
        service.register_role(role()).unwrap();
        let draft = service.create_assignment(&db, assignment()).unwrap();

        let active = service
            .set_lifecycle(&db, draft.id(), RoleAssignmentLifecycle::Active, 1, 10)
            .unwrap();

        assert!(active.is_effective(10));
        assert_eq!(active.agent_id(), "agent:one");
        assert_eq!(active.role_id().as_str(), "role:reviewer");
    }
}
