//! Agent OS Agent aggregate application service.

use crate::agent_domain::{Agent, AgentLifecycle, CreateAgentInput, UpdateAgentInput};
use crate::database::Database;
use crate::error::AppError;

pub struct AgentService;

impl AgentService {
    pub fn list(db: &Database) -> Result<Vec<Agent>, AppError> {
        db.list_agents()
    }

    pub fn get(db: &Database, id: &str) -> Result<Agent, AppError> {
        db.get_agent(id)?
            .ok_or_else(|| AppError::InvalidInput(format!("Agent not found: {id}")))
    }

    pub fn create(db: &Database, input: CreateAgentInput) -> Result<Agent, AppError> {
        let name = required_text("Agent name", input.name)?;
        let owner = required_text("Agent owner", input.owner)?;
        let now = chrono::Utc::now().timestamp_millis();
        let agent = Agent {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description: input.description.trim().to_string(),
            owner,
            lifecycle_state: AgentLifecycle::Draft,
            created_at: now,
            updated_at: now,
        };
        db.insert_agent(&agent)?;
        Ok(agent)
    }

    pub fn update(db: &Database, id: &str, input: UpdateAgentInput) -> Result<Agent, AppError> {
        let mut agent = Self::get(db, id)?;
        ensure_mutable(&agent)?;

        if let Some(name) = input.name {
            agent.name = required_text("Agent name", name)?;
        }
        if let Some(description) = input.description {
            agent.description = description.trim().to_string();
        }
        if let Some(owner) = input.owner {
            agent.owner = required_text("Agent owner", owner)?;
        }
        agent.updated_at = chrono::Utc::now().timestamp_millis();
        if !db.update_agent(&agent)? {
            return Err(AppError::Conflict(format!(
                "Agent disappeared during update: {id}"
            )));
        }
        Ok(agent)
    }

    pub fn set_lifecycle(
        db: &Database,
        id: &str,
        target: AgentLifecycle,
    ) -> Result<Agent, AppError> {
        let mut agent = Self::get(db, id)?;
        if agent.lifecycle_state == target {
            return Ok(agent);
        }
        if !valid_transition(agent.lifecycle_state, target) {
            return Err(AppError::Conflict(format!(
                "Invalid Agent lifecycle transition: {} -> {}",
                agent.lifecycle_state.as_str(),
                target.as_str()
            )));
        }
        agent.lifecycle_state = target;
        agent.updated_at = chrono::Utc::now().timestamp_millis();
        if !db.update_agent(&agent)? {
            return Err(AppError::Conflict(format!(
                "Agent disappeared during lifecycle transition: {id}"
            )));
        }
        Ok(agent)
    }
}

fn required_text(field: &str, value: String) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::InvalidInput(format!("{field} is empty")));
    }
    Ok(value.to_string())
}

fn ensure_mutable(agent: &Agent) -> Result<(), AppError> {
    if agent.lifecycle_state == AgentLifecycle::Retired {
        return Err(AppError::Conflict(format!(
            "Retired Agent is immutable: {}",
            agent.id
        )));
    }
    Ok(())
}

fn valid_transition(current: AgentLifecycle, target: AgentLifecycle) -> bool {
    current != AgentLifecycle::Retired && current != target
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> CreateAgentInput {
        CreateAgentInput {
            name: "  Architect  ".to_string(),
            description: "  Owns boundaries  ".to_string(),
            owner: "  local-user  ".to_string(),
        }
    }

    #[test]
    fn creates_draft_and_enforces_retired_immutability() -> Result<(), AppError> {
        let db = Database::memory()?;
        let agent = AgentService::create(&db, input())?;
        assert_eq!(agent.name, "Architect");
        assert_eq!(agent.lifecycle_state, AgentLifecycle::Draft);

        let active = AgentService::set_lifecycle(&db, &agent.id, AgentLifecycle::Active)?;
        let retired = AgentService::set_lifecycle(&db, &active.id, AgentLifecycle::Retired)?;
        let result = AgentService::update(
            &db,
            &retired.id,
            UpdateAgentInput {
                name: Some("Changed".to_string()),
                description: None,
                owner: None,
            },
        );
        assert!(matches!(result, Err(AppError::Conflict(_))));
        Ok(())
    }

    #[test]
    fn retired_lifecycle_cannot_be_reopened() -> Result<(), AppError> {
        let db = Database::memory()?;
        let agent = AgentService::create(&db, input())?;
        let retired = AgentService::set_lifecycle(&db, &agent.id, AgentLifecycle::Retired)?;
        let result = AgentService::set_lifecycle(&db, &retired.id, AgentLifecycle::Draft);
        assert!(matches!(result, Err(AppError::Conflict(_))));
        Ok(())
    }
}
