//! Versioned Workflow definition and optimistic orchestration state repository.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::{
    runtime_domain::RuntimeExecutionId,
    workflow_domain::{
        WorkflowDefinition, WorkflowDomainError, WorkflowId, WorkflowRun, WorkflowRunId,
        WorkflowRunLifecycle, WorkflowTask, WorkflowTaskId, WorkflowTaskLifecycle,
    },
};

#[derive(Debug, Error)]
pub enum WorkflowRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] WorkflowDomainError),
    #[error("Workflow definition is already registered: {id} v{version}")]
    DefinitionAlreadyRegistered { id: WorkflowId, version: u16 },
    #[error("Workflow definition was not found: {id} v{version}")]
    DefinitionNotFound { id: WorkflowId, version: u16 },
    #[error("Workflow Run is already registered: {0}")]
    RunAlreadyRegistered(WorkflowRunId),
    #[error("Workflow Run was not found: {0}")]
    RunNotFound(WorkflowRunId),
    #[error("Workflow Task is already registered: {0}")]
    TaskAlreadyRegistered(WorkflowTaskId),
    #[error("Workflow Task was not found: {0}")]
    TaskNotFound(WorkflowTaskId),
    #[error("Execution is already assigned to a Workflow Task: {0}")]
    ExecutionAlreadyAssigned(RuntimeExecutionId),
    #[error("{aggregate} identity changed during update")]
    IdentityChanged { aggregate: &'static str },
    #[error("{aggregate} must be created in its initial lifecycle at revision 1")]
    InvalidInitialState { aggregate: &'static str },
    #[error("{aggregate} revision update is invalid")]
    InvalidUpdate { aggregate: &'static str },
    #[error("Workflow repository lock failed: {0}")]
    RegistryLock(String),
}

pub trait WorkflowRepository: Send + Sync {
    fn register_definition(
        &self,
        definition: WorkflowDefinition,
    ) -> Result<(), WorkflowRepositoryError>;
    fn get_definition(
        &self,
        workflow_id: &WorkflowId,
        version: u16,
    ) -> Result<Option<WorkflowDefinition>, WorkflowRepositoryError>;
    fn list_definitions(&self) -> Result<Vec<WorkflowDefinition>, WorkflowRepositoryError>;

    fn insert_run(&self, run: WorkflowRun) -> Result<(), WorkflowRepositoryError>;
    fn get_run(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Option<WorkflowRun>, WorkflowRepositoryError>;
    fn list_runs(
        &self,
        workflow_id: &WorkflowId,
    ) -> Result<Vec<WorkflowRun>, WorkflowRepositoryError>;
    fn update_run(
        &self,
        run: WorkflowRun,
        expected_revision: u64,
    ) -> Result<(), WorkflowRepositoryError>;

    fn insert_task(&self, task: WorkflowTask) -> Result<(), WorkflowRepositoryError>;
    fn get_task(
        &self,
        task_id: &WorkflowTaskId,
    ) -> Result<Option<WorkflowTask>, WorkflowRepositoryError>;
    fn list_tasks(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Vec<WorkflowTask>, WorkflowRepositoryError>;
    fn update_task(
        &self,
        task: WorkflowTask,
        expected_revision: u64,
    ) -> Result<(), WorkflowRepositoryError>;
    fn update_task_and_run(
        &self,
        task: WorkflowTask,
        expected_task_revision: u64,
        run: WorkflowRun,
        expected_run_revision: u64,
    ) -> Result<(), WorkflowRepositoryError>;
}

type DefinitionKey = (WorkflowId, u16);

#[derive(Clone, Default)]
pub struct InMemoryWorkflowRepository {
    definitions: Arc<RwLock<HashMap<DefinitionKey, WorkflowDefinition>>>,
    runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>,
    tasks: Arc<RwLock<HashMap<WorkflowTaskId, WorkflowTask>>>,
}

impl WorkflowRepository for InMemoryWorkflowRepository {
    fn register_definition(
        &self,
        definition: WorkflowDefinition,
    ) -> Result<(), WorkflowRepositoryError> {
        let key = (definition.id().clone(), definition.version());
        let mut definitions = self
            .definitions
            .write()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        if definitions.contains_key(&key) {
            return Err(WorkflowRepositoryError::DefinitionAlreadyRegistered {
                id: key.0,
                version: key.1,
            });
        }
        definitions.insert(key, definition);
        Ok(())
    }

    fn get_definition(
        &self,
        workflow_id: &WorkflowId,
        version: u16,
    ) -> Result<Option<WorkflowDefinition>, WorkflowRepositoryError> {
        let definitions = self
            .definitions
            .read()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        Ok(definitions.get(&(workflow_id.clone(), version)).cloned())
    }

    fn list_definitions(&self) -> Result<Vec<WorkflowDefinition>, WorkflowRepositoryError> {
        let definitions = self
            .definitions
            .read()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        let mut result = definitions.values().cloned().collect::<Vec<_>>();
        result.sort_by(|left, right| {
            left.id()
                .cmp(right.id())
                .then_with(|| left.version().cmp(&right.version()))
        });
        Ok(result)
    }

    fn insert_run(&self, run: WorkflowRun) -> Result<(), WorkflowRepositoryError> {
        if run.lifecycle() != WorkflowRunLifecycle::Draft || run.revision() != 1 {
            return Err(WorkflowRepositoryError::InvalidInitialState {
                aggregate: "Workflow Run",
            });
        }
        if self
            .get_definition(run.workflow_id(), run.workflow_version())?
            .is_none()
        {
            return Err(WorkflowRepositoryError::DefinitionNotFound {
                id: run.workflow_id().clone(),
                version: run.workflow_version(),
            });
        }
        let mut runs = self
            .runs
            .write()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        if runs.contains_key(run.id()) {
            return Err(WorkflowRepositoryError::RunAlreadyRegistered(
                run.id().clone(),
            ));
        }
        runs.insert(run.id().clone(), run);
        Ok(())
    }

    fn get_run(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Option<WorkflowRun>, WorkflowRepositoryError> {
        let runs = self
            .runs
            .read()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        Ok(runs.get(run_id).cloned())
    }

    fn list_runs(
        &self,
        workflow_id: &WorkflowId,
    ) -> Result<Vec<WorkflowRun>, WorkflowRepositoryError> {
        let runs = self
            .runs
            .read()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        let mut result = runs
            .values()
            .filter(|run| run.workflow_id() == workflow_id)
            .cloned()
            .collect::<Vec<_>>();
        result.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(result)
    }

    fn update_run(
        &self,
        run: WorkflowRun,
        expected_revision: u64,
    ) -> Result<(), WorkflowRepositoryError> {
        let mut runs = self
            .runs
            .write()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        let current = runs
            .get(run.id())
            .ok_or_else(|| WorkflowRepositoryError::RunNotFound(run.id().clone()))?;
        validate_run_update(current, &run, expected_revision)?;
        runs.insert(run.id().clone(), run);
        Ok(())
    }

    fn insert_task(&self, task: WorkflowTask) -> Result<(), WorkflowRepositoryError> {
        if task.lifecycle() != WorkflowTaskLifecycle::Assigned || task.revision() != 1 {
            return Err(WorkflowRepositoryError::InvalidInitialState {
                aggregate: "Workflow Task",
            });
        }
        if self.get_run(task.run_id())?.is_none() {
            return Err(WorkflowRepositoryError::RunNotFound(task.run_id().clone()));
        }
        let mut tasks = self
            .tasks
            .write()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        if tasks.contains_key(task.id()) {
            return Err(WorkflowRepositoryError::TaskAlreadyRegistered(
                task.id().clone(),
            ));
        }
        if tasks
            .values()
            .any(|existing| existing.execution_id() == task.execution_id())
        {
            return Err(WorkflowRepositoryError::ExecutionAlreadyAssigned(
                task.execution_id().clone(),
            ));
        }
        tasks.insert(task.id().clone(), task);
        Ok(())
    }

    fn get_task(
        &self,
        task_id: &WorkflowTaskId,
    ) -> Result<Option<WorkflowTask>, WorkflowRepositoryError> {
        let tasks = self
            .tasks
            .read()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        Ok(tasks.get(task_id).cloned())
    }

    fn list_tasks(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Vec<WorkflowTask>, WorkflowRepositoryError> {
        let tasks = self
            .tasks
            .read()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        let mut result = tasks
            .values()
            .filter(|task| task.run_id() == run_id)
            .cloned()
            .collect::<Vec<_>>();
        result.sort_by(|left, right| {
            left.step_id()
                .cmp(right.step_id())
                .then_with(|| left.attempt().cmp(&right.attempt()))
        });
        Ok(result)
    }

    fn update_task(
        &self,
        task: WorkflowTask,
        expected_revision: u64,
    ) -> Result<(), WorkflowRepositoryError> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        let current = tasks
            .get(task.id())
            .ok_or_else(|| WorkflowRepositoryError::TaskNotFound(task.id().clone()))?;
        validate_task_update(current, &task, expected_revision)?;
        tasks.insert(task.id().clone(), task);
        Ok(())
    }

    fn update_task_and_run(
        &self,
        task: WorkflowTask,
        expected_task_revision: u64,
        run: WorkflowRun,
        expected_run_revision: u64,
    ) -> Result<(), WorkflowRepositoryError> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        let mut runs = self
            .runs
            .write()
            .map_err(|error| WorkflowRepositoryError::RegistryLock(error.to_string()))?;
        let current_task = tasks
            .get(task.id())
            .ok_or_else(|| WorkflowRepositoryError::TaskNotFound(task.id().clone()))?;
        let current_run = runs
            .get(run.id())
            .ok_or_else(|| WorkflowRepositoryError::RunNotFound(run.id().clone()))?;
        validate_task_update(current_task, &task, expected_task_revision)?;
        validate_run_update(current_run, &run, expected_run_revision)?;
        tasks.insert(task.id().clone(), task);
        runs.insert(run.id().clone(), run);
        Ok(())
    }
}

fn validate_run_update(
    current: &WorkflowRun,
    updated: &WorkflowRun,
    expected_revision: u64,
) -> Result<(), WorkflowRepositoryError> {
    if current.workflow_id() != updated.workflow_id()
        || current.workflow_version() != updated.workflow_version()
        || current.team_id() != updated.team_id()
        || current.created_at() != updated.created_at()
    {
        return Err(WorkflowRepositoryError::IdentityChanged {
            aggregate: "Workflow Run",
        });
    }
    if current.revision() != expected_revision || updated.revision() != expected_revision + 1 {
        return Err(WorkflowRepositoryError::InvalidUpdate {
            aggregate: "Workflow Run",
        });
    }
    Ok(())
}

fn validate_task_update(
    current: &WorkflowTask,
    updated: &WorkflowTask,
    expected_revision: u64,
) -> Result<(), WorkflowRepositoryError> {
    if current.run_id() != updated.run_id()
        || current.step_id() != updated.step_id()
        || current.agent_id() != updated.agent_id()
        || current.membership_id() != updated.membership_id()
        || current.role_assignment_id() != updated.role_assignment_id()
        || current.execution_id() != updated.execution_id()
        || current.governance() != updated.governance()
        || current.attempt() != updated.attempt()
        || current.created_at() != updated.created_at()
    {
        return Err(WorkflowRepositoryError::IdentityChanged {
            aggregate: "Workflow Task",
        });
    }
    if current.revision() != expected_revision || updated.revision() != expected_revision + 1 {
        return Err(WorkflowRepositoryError::InvalidUpdate {
            aggregate: "Workflow Task",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability_domain::CapabilitySnapshotId,
        execution_domain::ExecutionGovernanceEvidence,
        permission_domain::{AuthorizationDecisionId, PermissionGrantId},
        role_domain::{RoleAssignmentId, RoleId},
        team_domain::{TeamId, TeamMembershipId},
        workflow_domain::{WorkflowStepDefinition, WorkflowStepId, WorkflowTaskId},
    };

    fn definition() -> WorkflowDefinition {
        WorkflowDefinition::new(
            WorkflowId::new("workflow:one").unwrap(),
            1,
            TeamId::new("team:one").unwrap(),
            "Workflow One",
            "Coordinate one task",
            vec![WorkflowStepDefinition::new(
                WorkflowStepId::new("step:one").unwrap(),
                "Step One",
                "Complete bounded work",
                RoleId::new("role:worker").unwrap(),
                1,
                Vec::new(),
                Vec::new(),
                vec!["permission:work".into()],
                vec!["Evidence exists".into()],
            )
            .unwrap()],
            1,
        )
        .unwrap()
    }

    fn task(id: &str) -> WorkflowTask {
        WorkflowTask::new(
            WorkflowTaskId::new(id).unwrap(),
            WorkflowRunId::new("run:one").unwrap(),
            WorkflowStepId::new("step:one").unwrap(),
            "agent:one",
            TeamMembershipId::new("membership:one").unwrap(),
            RoleAssignmentId::new("assignment:one").unwrap(),
            RuntimeExecutionId::new("execution:one").unwrap(),
            ExecutionGovernanceEvidence::new(
                CapabilitySnapshotId::new("snapshot:one").unwrap(),
                PermissionGrantId::new("grant:one").unwrap(),
                RoleAssignmentId::new("assignment:one").unwrap(),
                AuthorizationDecisionId::new("decision:one").unwrap(),
            ),
            1,
            3,
        )
        .unwrap()
    }

    #[test]
    fn execution_can_back_only_one_workflow_task() {
        let repository = InMemoryWorkflowRepository::default();
        let definition = definition();
        repository.register_definition(definition.clone()).unwrap();
        repository
            .insert_run(
                WorkflowRun::new(WorkflowRunId::new("run:one").unwrap(), &definition, 2).unwrap(),
            )
            .unwrap();
        repository.insert_task(task("task:one")).unwrap();

        assert!(matches!(
            repository.insert_task(task("task:two")),
            Err(WorkflowRepositoryError::ExecutionAlreadyAssigned(_))
        ));
    }

    #[test]
    fn task_and_run_transition_is_atomic_when_either_revision_conflicts() {
        let repository = InMemoryWorkflowRepository::default();
        let definition = definition();
        repository.register_definition(definition.clone()).unwrap();
        let run = WorkflowRun::new(WorkflowRunId::new("run:one").unwrap(), &definition, 2).unwrap();
        repository.insert_run(run.clone()).unwrap();
        let task = task("task:one");
        repository.insert_task(task.clone()).unwrap();
        let updated_task = task
            .transition_to(WorkflowTaskLifecycle::Running, 1, 3)
            .unwrap();
        let updated_run = run.activate(&definition, 1, 3).unwrap();

        assert!(repository
            .update_task_and_run(updated_task, 1, updated_run, 99)
            .is_err());
        assert_eq!(
            repository.get_task(task.id()).unwrap().unwrap().lifecycle(),
            WorkflowTaskLifecycle::Assigned
        );
        assert_eq!(
            repository.get_run(run.id()).unwrap().unwrap().lifecycle(),
            WorkflowRunLifecycle::Draft
        );
    }
}
