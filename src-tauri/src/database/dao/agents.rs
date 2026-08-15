//! Agent OS Agent identity persistence.

use crate::agent_domain::{Agent, AgentLifecycle};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;

fn read_agent(row: &rusqlite::Row<'_>) -> Result<Agent, rusqlite::Error> {
    let lifecycle: String = row.get(4)?;
    let lifecycle_state = AgentLifecycle::from_db(&lifecycle).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })?;

    Ok(Agent {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        owner: row.get(3)?,
        lifecycle_state,
        revision: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

impl Database {
    pub fn list_agents(&self) -> Result<Vec<Agent>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, description, owner, lifecycle_state, revision, created_at, updated_at
             FROM agents ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([], read_agent)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn get_agent(&self, id: &str) -> Result<Option<Agent>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT id, name, description, owner, lifecycle_state, revision, created_at, updated_at
             FROM agents WHERE id = ?1",
            params![id],
            read_agent,
        );
        match result {
            Ok(agent) => Ok(Some(agent)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(AppError::from(error)),
        }
    }

    pub fn insert_agent(&self, agent: &Agent) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO agents
             (id, name, description, owner, lifecycle_state, revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                agent.id,
                agent.name,
                agent.description,
                agent.owner,
                agent.lifecycle_state.as_str(),
                agent.revision,
                agent.created_at,
                agent.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Update mutable Agent metadata only when the caller observed the current revision.
    pub fn update_agent_metadata(
        &self,
        agent: &Agent,
        expected_revision: i64,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn.execute(
            "UPDATE agents
             SET name = ?2, description = ?3, owner = ?4,
                 revision = revision + 1, updated_at = ?5
             WHERE id = ?1 AND revision = ?6 AND lifecycle_state != 'retired'",
            params![
                agent.id,
                agent.name,
                agent.description,
                agent.owner,
                agent.updated_at,
                expected_revision,
            ],
        )?;
        Ok(affected == 1)
    }

    /// Change lifecycle atomically without rewriting unrelated Agent metadata.
    pub fn update_agent_lifecycle(
        &self,
        id: &str,
        lifecycle_state: AgentLifecycle,
        updated_at: i64,
        expected_revision: i64,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn.execute(
            "UPDATE agents
             SET lifecycle_state = ?2, revision = revision + 1, updated_at = ?3
             WHERE id = ?1 AND revision = ?4 AND lifecycle_state != 'retired'",
            params![id, lifecycle_state.as_str(), updated_at, expected_revision],
        )?;
        Ok(affected == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_agent(id: &str) -> Agent {
        Agent {
            id: id.to_string(),
            name: "Researcher".to_string(),
            description: "Investigates a bounded topic".to_string(),
            owner: "local-user".to_string(),
            lifecycle_state: AgentLifecycle::Draft,
            revision: 1,
            created_at: 1_000,
            updated_at: 1_000,
        }
    }

    #[test]
    fn agent_identity_round_trips_without_runtime_bindings() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut agent = sample_agent("agent-1");
        db.insert_agent(&agent)?;

        assert_eq!(db.list_agents()?, vec![agent.clone()]);
        agent.name = "Lead Researcher".to_string();
        agent.updated_at = 2_000;
        assert!(db.update_agent_metadata(&agent, 1)?);
        agent.revision = 2;
        assert_eq!(db.get_agent("agent-1")?, Some(agent));
        assert!(!db.update_agent_metadata(&sample_agent("agent-1"), 1)?);
        assert_eq!(db.get_agent("missing")?, None);
        Ok(())
    }

    #[test]
    fn lifecycle_update_is_revision_guarded() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.insert_agent(&sample_agent("agent-1"))?;

        assert!(db.update_agent_lifecycle("agent-1", AgentLifecycle::Active, 2_000, 1)?);
        assert!(!db.update_agent_lifecycle("agent-1", AgentLifecycle::Suspended, 3_000, 1)?);
        let agent = db.get_agent("agent-1")?.expect("agent exists");
        assert_eq!(agent.lifecycle_state, AgentLifecycle::Active);
        assert_eq!(agent.revision, 2);
        Ok(())
    }
}
