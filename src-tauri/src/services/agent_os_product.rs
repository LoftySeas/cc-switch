//! Product-facing Agent OS queries and bounded operational actions.

use std::sync::Arc;

use thiserror::Error;

use crate::{
    database::Database,
    execution_repository::{
        ExecutionHistoryRepository, ExecutionRecord, ExecutionRepositoryError,
        SqliteExecutionHistoryRepository,
    },
    runtime_domain::RuntimeExecutionId,
    workflow_domain::{
        WorkflowDefinition, WorkflowDomainError, WorkflowId, WorkflowRun, WorkflowRunId,
        WorkflowTask,
    },
    workflow_repository::{SqliteWorkflowRepository, WorkflowRepository, WorkflowRepositoryError},
};

#[derive(Debug, Error)]
pub enum AgentOsProductError {
    #[error(transparent)]
    Workflow(#[from] WorkflowRepositoryError),
    #[error(transparent)]
    WorkflowDomain(#[from] WorkflowDomainError),
    #[error(transparent)]
    Execution(#[from] ExecutionRepositoryError),
}

pub struct AgentOsProductService {
    workflows: SqliteWorkflowRepository,
    executions: SqliteExecutionHistoryRepository,
}

impl AgentOsProductService {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            workflows: SqliteWorkflowRepository::new(database.clone()),
            executions: SqliteExecutionHistoryRepository::new(database),
        }
    }

    pub fn list_workflows(&self) -> Result<Vec<WorkflowDefinition>, AgentOsProductError> {
        Ok(self.workflows.list_definitions()?)
    }

    pub fn list_workflow_runs(
        &self,
        workflow_id: &WorkflowId,
    ) -> Result<Vec<WorkflowRun>, AgentOsProductError> {
        Ok(self.workflows.list_runs(workflow_id)?)
    }

    pub fn list_workflow_tasks(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Vec<WorkflowTask>, AgentOsProductError> {
        Ok(self.workflows.list_tasks(run_id)?)
    }

    pub fn cancel_workflow_run(
        &self,
        run_id: &WorkflowRunId,
        expected_revision: u64,
    ) -> Result<WorkflowRun, AgentOsProductError> {
        let run = self
            .workflows
            .get_run(run_id)?
            .ok_or_else(|| WorkflowRepositoryError::RunNotFound(run_id.clone()))?;
        let updated_at = chrono::Utc::now().timestamp_millis();
        let updated = run.cancel(expected_revision, updated_at)?;
        self.workflows
            .update_run(updated.clone(), expected_revision)?;
        Ok(updated)
    }

    pub fn list_executions(&self) -> Result<Vec<ExecutionRecord>, AgentOsProductError> {
        Ok(self.executions.list()?)
    }

    pub fn get_execution(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Option<ExecutionRecord>, AgentOsProductError> {
        Ok(self.executions.get(execution_id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        role_domain::RoleId,
        team_domain::TeamId,
        workflow_domain::{WorkflowRunLifecycle, WorkflowStepDefinition, WorkflowStepId},
    };

    fn definition() -> WorkflowDefinition {
        WorkflowDefinition::new(
            WorkflowId::new("workflow:product").unwrap(),
            1,
            TeamId::new("team:product").unwrap(),
            "Product workflow",
            "Expose governed state",
            vec![WorkflowStepDefinition::new(
                WorkflowStepId::new("step:inspect").unwrap(),
                "Inspect",
                "Inspect bounded state",
                RoleId::new("role:reviewer").unwrap(),
                1,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                vec!["State is visible".into()],
            )
            .unwrap()],
            1,
        )
        .unwrap()
    }

    #[test]
    fn product_queries_and_cancel_preserve_workflow_domain_rules() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteWorkflowRepository::new(database.clone());
        let definition = definition();
        repository.register_definition(definition.clone()).unwrap();
        repository
            .insert_run(
                WorkflowRun::new(WorkflowRunId::new("run:product").unwrap(), &definition, 2)
                    .unwrap(),
            )
            .unwrap();

        let service = AgentOsProductService::new(database);
        assert_eq!(service.list_workflows().unwrap(), vec![definition]);
        let cancelled = service
            .cancel_workflow_run(&WorkflowRunId::new("run:product").unwrap(), 1)
            .unwrap();
        assert_eq!(cancelled.lifecycle(), WorkflowRunLifecycle::Cancelled);
        assert_eq!(cancelled.revision(), 2);
    }
}
