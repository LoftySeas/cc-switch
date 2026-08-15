//! Agent OS Agent identity and lifecycle commands.

use tauri::State;

use crate::agent_domain::{Agent, AgentLifecycle, CreateAgentInput, UpdateAgentInput};
use crate::services::agent::AgentService;
use crate::store::AppState;

#[tauri::command]
pub fn list_agents(state: State<'_, AppState>) -> Result<Vec<Agent>, String> {
    AgentService::list(&state.db).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_agent(state: State<'_, AppState>, id: String) -> Result<Agent, String> {
    AgentService::get(&state.db, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_agent(state: State<'_, AppState>, input: CreateAgentInput) -> Result<Agent, String> {
    AgentService::create(&state.db, input).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_agent(
    state: State<'_, AppState>,
    id: String,
    input: UpdateAgentInput,
) -> Result<Agent, String> {
    AgentService::update(&state.db, &id, input).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_agent_lifecycle(
    state: State<'_, AppState>,
    id: String,
    lifecycle_state: AgentLifecycle,
    expected_revision: i64,
) -> Result<Agent, String> {
    AgentService::set_lifecycle(&state.db, &id, lifecycle_state, expected_revision)
        .map_err(|error| error.to_string())
}
