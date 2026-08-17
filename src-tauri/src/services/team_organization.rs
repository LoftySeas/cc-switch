//! Team organization application service.

use thiserror::Error;

use crate::{
    agent_domain::AgentLifecycle,
    database::Database,
    team_domain::{
        Team, TeamDomainError, TeamId, TeamLifecycle, TeamMembership, TeamMembershipId,
        TeamMembershipLifecycle, TeamRelationship, TeamRelationshipId, TeamRelationshipLifecycle,
    },
    team_repository::{TeamRepository, TeamRepositoryError},
};

#[derive(Debug, Error)]
pub enum TeamOrganizationError {
    #[error(transparent)]
    Domain(#[from] TeamDomainError),
    #[error(transparent)]
    Repository(#[from] TeamRepositoryError),
    #[error("Agent lookup failed: {0}")]
    AgentLookup(String),
    #[error("Agent was not found: {0}")]
    AgentNotFound(String),
    #[error("Agent is not active: {0}")]
    AgentNotActive(String),
    #[error("Team is not active: {0}")]
    TeamNotActive(TeamId),
    #[error("Team is archived: {0}")]
    TeamArchived(TeamId),
    #[error("Team Membership is not effective: {0}")]
    MembershipNotEffective(TeamMembershipId),
    #[error("Relationship endpoints must belong to the declared Team")]
    CrossTeamRelationship,
}

pub struct TeamOrganizationService<R> {
    repository: R,
}

impl<R> TeamOrganizationService<R>
where
    R: TeamRepository,
{
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn create_team(&self, team: Team) -> Result<Team, TeamOrganizationError> {
        self.repository.insert_team(team.clone())?;
        Ok(team)
    }

    pub fn get_team(&self, team_id: &TeamId) -> Result<Team, TeamOrganizationError> {
        self.repository
            .get_team(team_id)?
            .ok_or_else(|| TeamRepositoryError::TeamNotFound(team_id.clone()).into())
    }

    pub fn set_team_lifecycle(
        &self,
        team_id: &TeamId,
        target: TeamLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Team, TeamOrganizationError> {
        let current = self.get_team(team_id)?;
        let updated = current.transition_to(target, expected_revision, updated_at)?;
        if updated != current {
            self.repository
                .update_team(updated.clone(), expected_revision)?;
        }
        Ok(updated)
    }

    pub fn invite_member(
        &self,
        db: &Database,
        membership: TeamMembership,
    ) -> Result<TeamMembership, TeamOrganizationError> {
        let team = self.get_team(membership.team_id())?;
        if team.lifecycle() == TeamLifecycle::Archived {
            return Err(TeamOrganizationError::TeamArchived(team.id().clone()));
        }
        self.require_agent(db, membership.agent_id(), false)?;
        self.repository.insert_membership(membership.clone())?;
        Ok(membership)
    }

    pub fn get_membership(
        &self,
        membership_id: &TeamMembershipId,
    ) -> Result<TeamMembership, TeamOrganizationError> {
        self.repository
            .get_membership(membership_id)?
            .ok_or_else(|| TeamRepositoryError::MembershipNotFound(membership_id.clone()).into())
    }

    pub fn set_membership_lifecycle(
        &self,
        db: &Database,
        membership_id: &TeamMembershipId,
        target: TeamMembershipLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<TeamMembership, TeamOrganizationError> {
        let current = self.get_membership(membership_id)?;
        if target == TeamMembershipLifecycle::Active {
            let team = self.get_team(current.team_id())?;
            if team.lifecycle() != TeamLifecycle::Active {
                return Err(TeamOrganizationError::TeamNotActive(team.id().clone()));
            }
            self.require_agent(db, current.agent_id(), true)?;
            if updated_at < current.valid_from()
                || current
                    .valid_until()
                    .is_some_and(|until| updated_at > until)
            {
                return Err(TeamOrganizationError::MembershipNotEffective(
                    current.id().clone(),
                ));
            }
        }
        let updated = current.transition_to(target, expected_revision, updated_at)?;
        if updated != current {
            self.repository
                .update_membership(updated.clone(), expected_revision)?;
        }
        Ok(updated)
    }

    pub fn create_relationship(
        &self,
        relationship: TeamRelationship,
        at: i64,
    ) -> Result<TeamRelationship, TeamOrganizationError> {
        let team = self.get_team(relationship.team_id())?;
        if team.lifecycle() != TeamLifecycle::Active {
            return Err(TeamOrganizationError::TeamNotActive(team.id().clone()));
        }
        let source = self.get_membership(relationship.source_membership_id())?;
        let target = self.get_membership(relationship.target_membership_id())?;
        if source.team_id() != relationship.team_id() || target.team_id() != relationship.team_id()
        {
            return Err(TeamOrganizationError::CrossTeamRelationship);
        }
        if relationship.created_at() != at {
            return Err(TeamDomainError::InvalidTimestamp.into());
        }
        self.repository.insert_relationship(relationship.clone())?;
        Ok(relationship)
    }

    pub fn end_relationship(
        &self,
        relationship_id: &TeamRelationshipId,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<TeamRelationship, TeamOrganizationError> {
        let current = self
            .repository
            .get_relationship(relationship_id)?
            .ok_or_else(|| TeamRepositoryError::RelationshipNotFound(relationship_id.clone()))?;
        let updated = current.transition_to(
            TeamRelationshipLifecycle::Ended,
            expected_revision,
            updated_at,
        )?;
        if updated != current {
            self.repository
                .update_relationship(updated.clone(), expected_revision)?;
        }
        Ok(updated)
    }

    fn require_agent(
        &self,
        db: &Database,
        agent_id: &str,
        require_active: bool,
    ) -> Result<(), TeamOrganizationError> {
        let agent = db
            .get_agent(agent_id)
            .map_err(|error| TeamOrganizationError::AgentLookup(error.to_string()))?
            .ok_or_else(|| TeamOrganizationError::AgentNotFound(agent_id.to_string()))?;
        if require_active && agent.lifecycle_state != AgentLifecycle::Active {
            return Err(TeamOrganizationError::AgentNotActive(agent_id.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent_domain::Agent,
        team_domain::{TeamMembershipId, TeamRelationshipId},
        team_repository::InMemoryTeamRepository,
    };

    fn active_agent(id: &str) -> Agent {
        Agent {
            id: id.into(),
            name: id.into(),
            description: "Team participant".into(),
            owner: "owner:one".into(),
            lifecycle_state: AgentLifecycle::Active,
            revision: 1,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn activates_membership_without_changing_agent_identity_or_granting_authority() {
        let db = Database::memory().unwrap();
        db.insert_agent(&active_agent("agent:one")).unwrap();
        let service = TeamOrganizationService::new(InMemoryTeamRepository::default());
        let team = service
            .create_team(
                Team::new(
                    TeamId::new("team:one").unwrap(),
                    "Delivery Team",
                    "Deliver governed work",
                    "owner:one",
                    Vec::new(),
                    Vec::new(),
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        service
            .set_team_lifecycle(team.id(), TeamLifecycle::Active, 1, 2)
            .unwrap();
        let invited = service
            .invite_member(
                &db,
                TeamMembership::new(
                    TeamMembershipId::new("membership:one").unwrap(),
                    team.id().clone(),
                    "agent:one",
                    None,
                    Vec::new(),
                    "owner:one",
                    2,
                    None,
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        let active = service
            .set_membership_lifecycle(&db, invited.id(), TeamMembershipLifecycle::Active, 1, 2)
            .unwrap();

        assert_eq!(active.agent_id(), "agent:one");
        assert!(active.is_effective(2));
    }

    #[test]
    fn relationships_require_effective_memberships_in_one_team() {
        let db = Database::memory().unwrap();
        db.insert_agent(&active_agent("agent:author")).unwrap();
        db.insert_agent(&active_agent("agent:reviewer")).unwrap();
        let repository = InMemoryTeamRepository::default();
        let service = TeamOrganizationService::new(repository);
        let team = service
            .create_team(
                Team::new(
                    TeamId::new("team:one").unwrap(),
                    "Delivery Team",
                    "Deliver governed work",
                    "owner:one",
                    Vec::new(),
                    Vec::new(),
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        service
            .set_team_lifecycle(team.id(), TeamLifecycle::Active, 1, 2)
            .unwrap();
        for (membership_id, agent_id) in [
            ("membership:author", "agent:author"),
            ("membership:reviewer", "agent:reviewer"),
        ] {
            let invited = service
                .invite_member(
                    &db,
                    TeamMembership::new(
                        TeamMembershipId::new(membership_id).unwrap(),
                        team.id().clone(),
                        agent_id,
                        None,
                        Vec::new(),
                        "owner:one",
                        2,
                        None,
                        1,
                    )
                    .unwrap(),
                )
                .unwrap();
            service
                .set_membership_lifecycle(&db, invited.id(), TeamMembershipLifecycle::Active, 1, 2)
                .unwrap();
        }
        let relationship = TeamRelationship::new(
            TeamRelationshipId::new("relationship:review").unwrap(),
            team.id().clone(),
            TeamMembershipId::new("membership:author").unwrap(),
            TeamMembershipId::new("membership:reviewer").unwrap(),
            "reviews",
            "owner:one",
            3,
        )
        .unwrap();

        assert_eq!(
            service
                .create_relationship(relationship, 3)
                .unwrap()
                .relationship_kind(),
            "reviews"
        );
    }
}
