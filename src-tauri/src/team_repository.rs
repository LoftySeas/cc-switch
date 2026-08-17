//! Repository boundary for Team organization aggregates.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::team_domain::{
    Team, TeamDomainError, TeamId, TeamLifecycle, TeamMembership, TeamMembershipId,
    TeamMembershipLifecycle, TeamRelationship, TeamRelationshipId, TeamRelationshipLifecycle,
};

#[derive(Debug, Error)]
pub enum TeamRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] TeamDomainError),
    #[error("Team is already registered: {0}")]
    TeamAlreadyRegistered(TeamId),
    #[error("Team was not found: {0}")]
    TeamNotFound(TeamId),
    #[error("Team Membership is already registered: {0}")]
    MembershipAlreadyRegistered(TeamMembershipId),
    #[error("Team Membership was not found: {0}")]
    MembershipNotFound(TeamMembershipId),
    #[error("Team Relationship is already registered: {0}")]
    RelationshipAlreadyRegistered(TeamRelationshipId),
    #[error("Team Relationship was not found: {0}")]
    RelationshipNotFound(TeamRelationshipId),
    #[error("{aggregate} identity changed during update")]
    IdentityChanged { aggregate: &'static str },
    #[error("{aggregate} must be created in its initial lifecycle at revision 1")]
    InvalidInitialState { aggregate: &'static str },
    #[error("{aggregate} update is not one legal lifecycle transition")]
    InvalidUpdate { aggregate: &'static str },
    #[error("Team repository lock failed: {0}")]
    RegistryLock(String),
}

pub trait TeamRepository: Send + Sync {
    fn insert_team(&self, team: Team) -> Result<(), TeamRepositoryError>;
    fn get_team(&self, team_id: &TeamId) -> Result<Option<Team>, TeamRepositoryError>;
    fn list_teams(&self) -> Result<Vec<Team>, TeamRepositoryError>;
    fn update_team(&self, team: Team, expected_revision: u64) -> Result<(), TeamRepositoryError>;

    fn insert_membership(&self, membership: TeamMembership) -> Result<(), TeamRepositoryError>;
    fn get_membership(
        &self,
        membership_id: &TeamMembershipId,
    ) -> Result<Option<TeamMembership>, TeamRepositoryError>;
    fn list_memberships(
        &self,
        team_id: &TeamId,
    ) -> Result<Vec<TeamMembership>, TeamRepositoryError>;
    fn update_membership(
        &self,
        membership: TeamMembership,
        expected_revision: u64,
    ) -> Result<(), TeamRepositoryError>;

    fn insert_relationship(
        &self,
        relationship: TeamRelationship,
    ) -> Result<(), TeamRepositoryError>;
    fn get_relationship(
        &self,
        relationship_id: &TeamRelationshipId,
    ) -> Result<Option<TeamRelationship>, TeamRepositoryError>;
    fn list_relationships(
        &self,
        team_id: &TeamId,
    ) -> Result<Vec<TeamRelationship>, TeamRepositoryError>;
    fn update_relationship(
        &self,
        relationship: TeamRelationship,
        expected_revision: u64,
    ) -> Result<(), TeamRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryTeamRepository {
    teams: Arc<RwLock<HashMap<TeamId, Team>>>,
    memberships: Arc<RwLock<HashMap<TeamMembershipId, TeamMembership>>>,
    relationships: Arc<RwLock<HashMap<TeamRelationshipId, TeamRelationship>>>,
}

impl TeamRepository for InMemoryTeamRepository {
    fn insert_team(&self, team: Team) -> Result<(), TeamRepositoryError> {
        if team.lifecycle() != TeamLifecycle::Draft || team.revision() != 1 {
            return Err(TeamRepositoryError::InvalidInitialState { aggregate: "Team" });
        }
        let mut teams = self
            .teams
            .write()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        if teams.contains_key(team.id()) {
            return Err(TeamRepositoryError::TeamAlreadyRegistered(
                team.id().clone(),
            ));
        }
        teams.insert(team.id().clone(), team);
        Ok(())
    }

    fn get_team(&self, team_id: &TeamId) -> Result<Option<Team>, TeamRepositoryError> {
        let teams = self
            .teams
            .read()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        Ok(teams.get(team_id).cloned())
    }

    fn list_teams(&self) -> Result<Vec<Team>, TeamRepositoryError> {
        let teams = self
            .teams
            .read()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        let mut result = teams.values().cloned().collect::<Vec<_>>();
        result.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(result)
    }

    fn update_team(&self, team: Team, expected_revision: u64) -> Result<(), TeamRepositoryError> {
        let mut teams = self
            .teams
            .write()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        let current = teams
            .get(team.id())
            .ok_or_else(|| TeamRepositoryError::TeamNotFound(team.id().clone()))?;
        if current.id() != team.id()
            || current.created_at() != team.created_at()
            || current.owner_ref() != team.owner_ref()
        {
            return Err(TeamRepositoryError::IdentityChanged { aggregate: "Team" });
        }
        if current.revision() != expected_revision
            || team.revision() != expected_revision + 1
            || !current.lifecycle().can_transition_to(team.lifecycle())
        {
            return Err(TeamRepositoryError::InvalidUpdate { aggregate: "Team" });
        }
        teams.insert(team.id().clone(), team);
        Ok(())
    }

    fn insert_membership(&self, membership: TeamMembership) -> Result<(), TeamRepositoryError> {
        if membership.lifecycle() != TeamMembershipLifecycle::Invited || membership.revision() != 1
        {
            return Err(TeamRepositoryError::InvalidInitialState {
                aggregate: "Team Membership",
            });
        }
        if self.get_team(membership.team_id())?.is_none() {
            return Err(TeamRepositoryError::TeamNotFound(
                membership.team_id().clone(),
            ));
        }
        let mut memberships = self
            .memberships
            .write()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        if memberships.contains_key(membership.id()) {
            return Err(TeamRepositoryError::MembershipAlreadyRegistered(
                membership.id().clone(),
            ));
        }
        memberships.insert(membership.id().clone(), membership);
        Ok(())
    }

    fn get_membership(
        &self,
        membership_id: &TeamMembershipId,
    ) -> Result<Option<TeamMembership>, TeamRepositoryError> {
        let memberships = self
            .memberships
            .read()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        Ok(memberships.get(membership_id).cloned())
    }

    fn list_memberships(
        &self,
        team_id: &TeamId,
    ) -> Result<Vec<TeamMembership>, TeamRepositoryError> {
        let memberships = self
            .memberships
            .read()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        let mut result = memberships
            .values()
            .filter(|membership| membership.team_id() == team_id)
            .cloned()
            .collect::<Vec<_>>();
        result.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(result)
    }

    fn update_membership(
        &self,
        membership: TeamMembership,
        expected_revision: u64,
    ) -> Result<(), TeamRepositoryError> {
        let mut memberships = self
            .memberships
            .write()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        let current = memberships
            .get(membership.id())
            .ok_or_else(|| TeamRepositoryError::MembershipNotFound(membership.id().clone()))?;
        if current.team_id() != membership.team_id()
            || current.agent_id() != membership.agent_id()
            || current.created_at() != membership.created_at()
        {
            return Err(TeamRepositoryError::IdentityChanged {
                aggregate: "Team Membership",
            });
        }
        if current.revision() != expected_revision
            || membership.revision() != expected_revision + 1
            || !current
                .lifecycle()
                .can_transition_to(membership.lifecycle())
        {
            return Err(TeamRepositoryError::InvalidUpdate {
                aggregate: "Team Membership",
            });
        }
        memberships.insert(membership.id().clone(), membership);
        Ok(())
    }

    fn insert_relationship(
        &self,
        relationship: TeamRelationship,
    ) -> Result<(), TeamRepositoryError> {
        if relationship.lifecycle() != TeamRelationshipLifecycle::Active
            || relationship.revision() != 1
        {
            return Err(TeamRepositoryError::InvalidInitialState {
                aggregate: "Team Relationship",
            });
        }
        let source = self
            .get_membership(relationship.source_membership_id())?
            .ok_or_else(|| {
                TeamRepositoryError::MembershipNotFound(relationship.source_membership_id().clone())
            })?;
        let target = self
            .get_membership(relationship.target_membership_id())?
            .ok_or_else(|| {
                TeamRepositoryError::MembershipNotFound(relationship.target_membership_id().clone())
            })?;
        if source.team_id() != relationship.team_id() || target.team_id() != relationship.team_id()
        {
            return Err(TeamRepositoryError::IdentityChanged {
                aggregate: "Team Relationship",
            });
        }
        let mut relationships = self
            .relationships
            .write()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        if relationships.contains_key(relationship.id()) {
            return Err(TeamRepositoryError::RelationshipAlreadyRegistered(
                relationship.id().clone(),
            ));
        }
        relationships.insert(relationship.id().clone(), relationship);
        Ok(())
    }

    fn get_relationship(
        &self,
        relationship_id: &TeamRelationshipId,
    ) -> Result<Option<TeamRelationship>, TeamRepositoryError> {
        let relationships = self
            .relationships
            .read()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        Ok(relationships.get(relationship_id).cloned())
    }

    fn list_relationships(
        &self,
        team_id: &TeamId,
    ) -> Result<Vec<TeamRelationship>, TeamRepositoryError> {
        let relationships = self
            .relationships
            .read()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        let mut result = relationships
            .values()
            .filter(|relationship| relationship.team_id() == team_id)
            .cloned()
            .collect::<Vec<_>>();
        result.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(result)
    }

    fn update_relationship(
        &self,
        relationship: TeamRelationship,
        expected_revision: u64,
    ) -> Result<(), TeamRepositoryError> {
        let mut relationships = self
            .relationships
            .write()
            .map_err(|error| TeamRepositoryError::RegistryLock(error.to_string()))?;
        let current = relationships
            .get(relationship.id())
            .ok_or_else(|| TeamRepositoryError::RelationshipNotFound(relationship.id().clone()))?;
        if current.team_id() != relationship.team_id()
            || current.source_membership_id() != relationship.source_membership_id()
            || current.target_membership_id() != relationship.target_membership_id()
            || current.relationship_kind() != relationship.relationship_kind()
            || current.created_at() != relationship.created_at()
        {
            return Err(TeamRepositoryError::IdentityChanged {
                aggregate: "Team Relationship",
            });
        }
        if current.revision() != expected_revision
            || relationship.revision() != expected_revision + 1
            || !current
                .lifecycle()
                .can_transition_to(relationship.lifecycle())
        {
            return Err(TeamRepositoryError::InvalidUpdate {
                aggregate: "Team Relationship",
            });
        }
        relationships.insert(relationship.id().clone(), relationship);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team() -> Team {
        Team::new(
            TeamId::new("team:one").unwrap(),
            "Delivery Team",
            "Deliver governed changes",
            "owner:one",
            Vec::new(),
            Vec::new(),
            1,
        )
        .unwrap()
    }

    #[test]
    fn repository_preserves_membership_identity_and_history() {
        let repository = InMemoryTeamRepository::default();
        repository.insert_team(team()).unwrap();
        let membership = TeamMembership::new(
            TeamMembershipId::new("membership:one").unwrap(),
            TeamId::new("team:one").unwrap(),
            "agent:one",
            None,
            Vec::new(),
            "owner:one",
            2,
            None,
            1,
        )
        .unwrap();
        repository.insert_membership(membership.clone()).unwrap();
        let active = membership
            .transition_to(TeamMembershipLifecycle::Active, 1, 2)
            .unwrap();
        repository.update_membership(active.clone(), 1).unwrap();

        assert_eq!(
            repository
                .get_membership(active.id())
                .unwrap()
                .unwrap()
                .agent_id(),
            "agent:one"
        );
    }
}
