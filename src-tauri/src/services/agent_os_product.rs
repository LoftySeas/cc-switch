//! Product-facing Agent OS queries and bounded operational actions.

use std::sync::Arc;

use serde::Serialize;
use thiserror::Error;

use crate::{
    database::Database,
    execution_repository::{
        ExecutionHistoryRepository, ExecutionRecord, ExecutionRepositoryError,
        SqliteExecutionHistoryRepository,
    },
    runtime_domain::{RuntimeExecutionId, RuntimeExecutionState},
    workflow_domain::{
        WorkflowDefinition, WorkflowDomainError, WorkflowId, WorkflowRun, WorkflowRunId,
        WorkflowTask,
    },
    workflow_repository::{SqliteWorkflowRepository, WorkflowRepository, WorkflowRepositoryError},
};

/// Read-only product projection over immutable Execution history. It exposes
/// bounded Context and Memory references without transferring Domain ownership
/// or repository access to the presentation layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionManagementView {
    pub execution_id: String,
    pub objective: String,
    pub state: RuntimeExecutionState,
    pub revision: u64,
    pub transition_count: usize,
    pub agent_id: String,
    pub runtime_id: String,
    pub model_id: String,
    pub context_references: Vec<String>,
    pub result_summary: Option<String>,
    pub accepted_at: i64,
}

impl From<ExecutionRecord> for ExecutionManagementView {
    fn from(record: ExecutionRecord) -> Self {
        let request = record.request();
        Self {
            execution_id: request.execution_id().as_str().to_string(),
            objective: request.objective().to_string(),
            state: record.state(),
            revision: record.revision(),
            transition_count: record.transitions().len(),
            agent_id: request.context().binding().agent_id().to_string(),
            runtime_id: request
                .context()
                .binding()
                .runtime_id()
                .as_str()
                .to_string(),
            model_id: request.model_binding().model_id().as_str().to_string(),
            context_references: request.context().context_references().to_vec(),
            result_summary: record.result().map(|result| result.summary().to_string()),
            accepted_at: request.accepted_at(),
        }
    }
}

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

    pub fn list_execution_views(
        &self,
    ) -> Result<Vec<ExecutionManagementView>, AgentOsProductError> {
        Ok(self
            .executions
            .list()?
            .into_iter()
            .map(ExecutionManagementView::from)
            .collect())
    }

    pub fn get_execution(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Option<ExecutionRecord>, AgentOsProductError> {
        Ok(self.executions.get(execution_id)?)
    }

    pub fn get_execution_view(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Option<ExecutionManagementView>, AgentOsProductError> {
        Ok(self
            .executions
            .get(execution_id)?
            .map(ExecutionManagementView::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability_domain::CapabilitySnapshotId,
        execution_domain::{ExecutionGovernanceEvidence, ExecutionModelBinding, ExecutionRequest},
        model_domain::ModelId,
        permission_domain::{AuthorizationDecisionId, PermissionGrantId},
        role_domain::RoleAssignmentId,
        role_domain::RoleId,
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeBindingId, RuntimeBindingLifecycle,
            RuntimeId,
        },
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

    #[test]
    fn execution_query_projects_bounded_context_and_memory_references() {
        let database = Arc::new(Database::memory().unwrap());
        let history = SqliteExecutionHistoryRepository::new(database.clone());
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding:product-view").unwrap(),
            "agent:product-view",
            RuntimeId::new("runtime:product-view").unwrap(),
            10,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 11)
        .unwrap();
        let context = ExecutionContext::new(
            RuntimeExecutionId::new("execution:product-view").unwrap(),
            binding,
            vec![
                "context-package:context:product-view".into(),
                "memory:memory:product-view".into(),
            ],
            12,
        )
        .unwrap();
        history
            .accept(
                ExecutionRequest::new(
                    context,
                    "Inspect governed references",
                    ExecutionModelBinding::runtime_local(
                        ModelId::new("model:product-view").unwrap(),
                    ),
                    ExecutionGovernanceEvidence::new(
                        CapabilitySnapshotId::new("capability:product-view").unwrap(),
                        PermissionGrantId::new("permission:product-view").unwrap(),
                        RoleAssignmentId::new("assignment:product-view").unwrap(),
                        AuthorizationDecisionId::new("decision:product-view").unwrap(),
                    ),
                    None,
                    13,
                )
                .unwrap(),
            )
            .unwrap();

        let views = AgentOsProductService::new(database)
            .list_execution_views()
            .unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].execution_id, "execution:product-view");
        assert_eq!(views[0].agent_id, "agent:product-view");
        assert_eq!(
            views[0].context_references,
            vec![
                "context-package:context:product-view",
                "memory:memory:product-view"
            ]
        );
    }
}
