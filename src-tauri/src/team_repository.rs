//! Repository boundary for Team organization aggregates.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use rusqlite::{params, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use crate::{
    database::{lock_conn, Database},
    error::AppError,
    team_domain::{
        Team, TeamDomainError, TeamId, TeamLifecycle, TeamMembership, TeamMembershipId,
        TeamMembershipLifecycle, TeamRelationship, TeamRelationshipId, TeamRelationshipLifecycle,
    },
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
    #[error("Team persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for TeamRepositoryError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
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

#[derive(Clone)]
pub struct SqliteTeamRepository {
    database: Arc<Database>,
}

impl SqliteTeamRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
    fn encode<T: Serialize>(value: &T) -> Result<String, TeamRepositoryError> {
        serde_json::to_string(value)
            .map_err(|error| TeamRepositoryError::Persistence(error.to_string()))
    }
    fn decode<T: DeserializeOwned>(value: String) -> Result<T, TeamRepositoryError> {
        serde_json::from_str(&value)
            .map_err(|error| TeamRepositoryError::Persistence(error.to_string()))
    }
    fn duplicate(error: &rusqlite::Error) -> bool {
        error.to_string().contains("UNIQUE constraint failed")
    }
}

impl TeamRepository for SqliteTeamRepository {
    fn insert_team(&self, team: Team) -> Result<(), TeamRepositoryError> {
        if team.lifecycle() != TeamLifecycle::Draft || team.revision() != 1 {
            return Err(TeamRepositoryError::InvalidInitialState { aggregate: "Team" });
        }
        let conn = lock_conn!(self.database.conn);
        conn.execute("INSERT INTO agent_os_teams (team_id, team_json, lifecycle_state, revision, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6)", params![team.id().as_str(), Self::encode(&team)?, team_lifecycle(team.lifecycle()), team.revision() as i64, team.created_at(), team.updated_at()])
            .map_err(|error| if Self::duplicate(&error) { TeamRepositoryError::TeamAlreadyRegistered(team.id().clone()) } else { TeamRepositoryError::Persistence(error.to_string()) })?;
        Ok(())
    }
    fn get_team(&self, id: &TeamId) -> Result<Option<Team>, TeamRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT team_json FROM agent_os_teams WHERE team_id=?1",
                [id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| TeamRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode).transpose()
    }
    fn list_teams(&self) -> Result<Vec<Team>, TeamRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn
            .prepare("SELECT team_json FROM agent_os_teams ORDER BY team_id")
            .map_err(|error| TeamRepositoryError::Persistence(error.to_string()))?;
        let result = collect_json(
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| TeamRepositoryError::Persistence(error.to_string()))?,
        );
        result
    }
    fn update_team(&self, team: Team, expected_revision: u64) -> Result<(), TeamRepositoryError> {
        let current = self
            .get_team(team.id())?
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
        let conn = lock_conn!(self.database.conn);
        let changed = conn.execute("UPDATE agent_os_teams SET team_json=?1,lifecycle_state=?2,revision=?3,updated_at=?4 WHERE team_id=?5 AND revision=?6", params![Self::encode(&team)?, team_lifecycle(team.lifecycle()), team.revision() as i64, team.updated_at(), team.id().as_str(), expected_revision as i64]).map_err(|error| TeamRepositoryError::Persistence(error.to_string()))?;
        if changed != 1 {
            return Err(TeamRepositoryError::InvalidUpdate { aggregate: "Team" });
        }
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
        let conn = lock_conn!(self.database.conn);
        conn.execute("INSERT INTO agent_os_team_memberships (membership_id,team_id,agent_id,membership_json,lifecycle_state,revision,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![membership.id().as_str(), membership.team_id().as_str(), membership.agent_id(), Self::encode(&membership)?, membership_lifecycle(membership.lifecycle()), membership.revision() as i64, membership.created_at(), membership.updated_at()]).map_err(|error| if Self::duplicate(&error) { TeamRepositoryError::MembershipAlreadyRegistered(membership.id().clone()) } else { TeamRepositoryError::Persistence(error.to_string()) })?;
        Ok(())
    }
    fn get_membership(
        &self,
        id: &TeamMembershipId,
    ) -> Result<Option<TeamMembership>, TeamRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT membership_json FROM agent_os_team_memberships WHERE membership_id=?1",
                [id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| TeamRepositoryError::Persistence(e.to_string()))?;
        value.map(Self::decode).transpose()
    }
    fn list_memberships(
        &self,
        team_id: &TeamId,
    ) -> Result<Vec<TeamMembership>, TeamRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement=conn.prepare("SELECT membership_json FROM agent_os_team_memberships WHERE team_id=?1 ORDER BY membership_id").map_err(|e|TeamRepositoryError::Persistence(e.to_string()))?;
        let result = collect_json(
            statement
                .query_map([team_id.as_str()], |row| row.get::<_, String>(0))
                .map_err(|e| TeamRepositoryError::Persistence(e.to_string()))?,
        );
        result
    }
    fn update_membership(
        &self,
        membership: TeamMembership,
        expected_revision: u64,
    ) -> Result<(), TeamRepositoryError> {
        let current = self
            .get_membership(membership.id())?
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
        let conn = lock_conn!(self.database.conn);
        let changed=conn.execute("UPDATE agent_os_team_memberships SET membership_json=?1,lifecycle_state=?2,revision=?3,updated_at=?4 WHERE membership_id=?5 AND revision=?6",params![Self::encode(&membership)?,membership_lifecycle(membership.lifecycle()),membership.revision() as i64,membership.updated_at(),membership.id().as_str(),expected_revision as i64]).map_err(|e|TeamRepositoryError::Persistence(e.to_string()))?;
        if changed != 1 {
            return Err(TeamRepositoryError::InvalidUpdate {
                aggregate: "Team Membership",
            });
        }
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
        let conn = lock_conn!(self.database.conn);
        conn.execute("INSERT INTO agent_os_team_relationships (relationship_id,team_id,source_membership_id,target_membership_id,relationship_kind,relationship_json,lifecycle_state,revision,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![relationship.id().as_str(),relationship.team_id().as_str(),relationship.source_membership_id().as_str(),relationship.target_membership_id().as_str(),relationship.relationship_kind(),Self::encode(&relationship)?,relationship_lifecycle(relationship.lifecycle()),relationship.revision() as i64,relationship.created_at(),relationship.updated_at()]).map_err(|e|if Self::duplicate(&e){TeamRepositoryError::RelationshipAlreadyRegistered(relationship.id().clone())}else{TeamRepositoryError::Persistence(e.to_string())})?;
        Ok(())
    }
    fn get_relationship(
        &self,
        id: &TeamRelationshipId,
    ) -> Result<Option<TeamRelationship>, TeamRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value=conn.query_row("SELECT relationship_json FROM agent_os_team_relationships WHERE relationship_id=?1",[id.as_str()],|row|row.get::<_,String>(0)).optional().map_err(|e|TeamRepositoryError::Persistence(e.to_string()))?;
        value.map(Self::decode).transpose()
    }
    fn list_relationships(
        &self,
        team_id: &TeamId,
    ) -> Result<Vec<TeamRelationship>, TeamRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement=conn.prepare("SELECT relationship_json FROM agent_os_team_relationships WHERE team_id=?1 ORDER BY relationship_id").map_err(|e|TeamRepositoryError::Persistence(e.to_string()))?;
        let result = collect_json(
            statement
                .query_map([team_id.as_str()], |row| row.get::<_, String>(0))
                .map_err(|e| TeamRepositoryError::Persistence(e.to_string()))?,
        );
        result
    }
    fn update_relationship(
        &self,
        relationship: TeamRelationship,
        expected_revision: u64,
    ) -> Result<(), TeamRepositoryError> {
        let current = self
            .get_relationship(relationship.id())?
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
        let conn = lock_conn!(self.database.conn);
        let changed=conn.execute("UPDATE agent_os_team_relationships SET relationship_json=?1,lifecycle_state=?2,revision=?3,updated_at=?4 WHERE relationship_id=?5 AND revision=?6",params![Self::encode(&relationship)?,relationship_lifecycle(relationship.lifecycle()),relationship.revision() as i64,relationship.updated_at(),relationship.id().as_str(),expected_revision as i64]).map_err(|e|TeamRepositoryError::Persistence(e.to_string()))?;
        if changed != 1 {
            return Err(TeamRepositoryError::InvalidUpdate {
                aggregate: "Team Relationship",
            });
        }
        Ok(())
    }
}

fn collect_json<T: DeserializeOwned>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>>,
) -> Result<Vec<T>, TeamRepositoryError> {
    rows.map(|row| {
        row.map_err(|e| TeamRepositoryError::Persistence(e.to_string()))
            .and_then(SqliteTeamRepository::decode)
    })
    .collect()
}
fn team_lifecycle(value: TeamLifecycle) -> &'static str {
    match value {
        TeamLifecycle::Draft => "draft",
        TeamLifecycle::Active => "active",
        TeamLifecycle::Suspended => "suspended",
        TeamLifecycle::Archived => "archived",
    }
}
fn membership_lifecycle(value: TeamMembershipLifecycle) -> &'static str {
    match value {
        TeamMembershipLifecycle::Invited => "invited",
        TeamMembershipLifecycle::Active => "active",
        TeamMembershipLifecycle::Suspended => "suspended",
        TeamMembershipLifecycle::Ended => "ended",
    }
}
fn relationship_lifecycle(value: TeamRelationshipLifecycle) -> &'static str {
    match value {
        TeamRelationshipLifecycle::Active => "active",
        TeamRelationshipLifecycle::Ended => "ended",
    }
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

    #[test]
    fn sqlite_repository_persists_team_organization_without_granting_authority() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteTeamRepository::new(database.clone());
        repository.insert_team(team()).unwrap();
        for (id, agent) in [
            ("membership:one", "agent:one"),
            ("membership:two", "agent:two"),
        ] {
            repository
                .insert_membership(
                    TeamMembership::new(
                        TeamMembershipId::new(id).unwrap(),
                        TeamId::new("team:one").unwrap(),
                        agent,
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
        }
        repository
            .insert_relationship(
                TeamRelationship::new(
                    TeamRelationshipId::new("relationship:one").unwrap(),
                    TeamId::new("team:one").unwrap(),
                    TeamMembershipId::new("membership:one").unwrap(),
                    TeamMembershipId::new("membership:two").unwrap(),
                    "collaborates_with",
                    "owner:one",
                    3,
                )
                .unwrap(),
            )
            .unwrap();

        let reopened = SqliteTeamRepository::new(database);
        assert_eq!(reopened.list_teams().unwrap().len(), 1);
        assert_eq!(
            reopened
                .list_memberships(&TeamId::new("team:one").unwrap())
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            reopened
                .list_relationships(&TeamId::new("team:one").unwrap())
                .unwrap()
                .len(),
            1
        );
    }
}
