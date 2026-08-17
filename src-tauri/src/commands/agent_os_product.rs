//! Agent OS product commands. All writes pass through backend Domain services.

use tauri::State;

use crate::{
    execution_repository::ExecutionRecord,
    runtime_domain::RuntimeExecutionId,
    services::agent_os_product::AgentOsProductService,
    store::AppState,
    workflow_domain::{WorkflowDefinition, WorkflowId, WorkflowRun, WorkflowRunId, WorkflowTask},
};

#[tauri::command]
pub fn list_agent_os_workflows(
    state: State<'_, AppState>,
) -> Result<Vec<WorkflowDefinition>, String> {
    AgentOsProductService::new(state.db.clone())
        .list_workflows()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_agent_os_workflow_runs(
    state: State<'_, AppState>,
    workflow_id: String,
) -> Result<Vec<WorkflowRun>, String> {
    let workflow_id = WorkflowId::new(workflow_id).map_err(|error| error.to_string())?;
    AgentOsProductService::new(state.db.clone())
        .list_workflow_runs(&workflow_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_agent_os_workflow_tasks(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Vec<WorkflowTask>, String> {
    let run_id = WorkflowRunId::new(run_id).map_err(|error| error.to_string())?;
    AgentOsProductService::new(state.db.clone())
        .list_workflow_tasks(&run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_agent_os_workflow_run(
    state: State<'_, AppState>,
    run_id: String,
    expected_revision: u64,
) -> Result<WorkflowRun, String> {
    let run_id = WorkflowRunId::new(run_id).map_err(|error| error.to_string())?;
    AgentOsProductService::new(state.db.clone())
        .cancel_workflow_run(&run_id, expected_revision)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_agent_os_executions(
    state: State<'_, AppState>,
) -> Result<Vec<ExecutionRecord>, String> {
    AgentOsProductService::new(state.db.clone())
        .list_executions()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_agent_os_execution(
    state: State<'_, AppState>,
    execution_id: String,
) -> Result<Option<ExecutionRecord>, String> {
    let execution_id = RuntimeExecutionId::new(execution_id).map_err(|error| error.to_string())?;
    AgentOsProductService::new(state.db.clone())
        .get_execution(&execution_id)
        .map_err(|error| error.to_string())
}
