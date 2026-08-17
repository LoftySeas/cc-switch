//! Versioned Workflow definition and optimistic orchestration state repository.

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
    #[error("Workflow persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for WorkflowRepositoryError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
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

#[derive(Clone)]
pub struct SqliteWorkflowRepository {
    database: Arc<Database>,
}

impl SqliteWorkflowRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    fn encode<T: Serialize>(value: &T) -> Result<String, WorkflowRepositoryError> {
        serde_json::to_string(value)
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))
    }

    fn decode<T: DeserializeOwned>(value: String) -> Result<T, WorkflowRepositoryError> {
        serde_json::from_str(&value)
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))
    }
}

impl WorkflowRepository for SqliteWorkflowRepository {
    fn register_definition(
        &self,
        definition: WorkflowDefinition,
    ) -> Result<(), WorkflowRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_workflow_definitions
             (workflow_id, version, definition_json, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                definition.id().as_str(),
                definition.version(),
                Self::encode(&definition)?,
                definition.created_at()
            ],
        )
        .map_err(|error| {
            if error.to_string().contains("UNIQUE constraint failed") {
                WorkflowRepositoryError::DefinitionAlreadyRegistered {
                    id: definition.id().clone(),
                    version: definition.version(),
                }
            } else {
                WorkflowRepositoryError::Persistence(error.to_string())
            }
        })?;
        Ok(())
    }

    fn get_definition(
        &self,
        workflow_id: &WorkflowId,
        version: u16,
    ) -> Result<Option<WorkflowDefinition>, WorkflowRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn.query_row(
            "SELECT definition_json FROM agent_os_workflow_definitions WHERE workflow_id=?1 AND version=?2",
            params![workflow_id.as_str(), version], |row| row.get::<_, String>(0),
        ).optional().map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode).transpose()
    }

    fn list_definitions(&self) -> Result<Vec<WorkflowDefinition>, WorkflowRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn.prepare("SELECT definition_json FROM agent_os_workflow_definitions ORDER BY workflow_id, version")
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        rows.map(|row| {
            row.map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))
                .and_then(Self::decode)
        })
        .collect()
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
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_workflow_runs
             (run_id, workflow_id, workflow_version, run_json, lifecycle_state, revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![run.id().as_str(), run.workflow_id().as_str(), run.workflow_version(), Self::encode(&run)?, lifecycle_json(run.lifecycle())?, run.revision() as i64, run.created_at(), run.updated_at()],
        ).map_err(|error| if error.to_string().contains("UNIQUE constraint failed") { WorkflowRepositoryError::RunAlreadyRegistered(run.id().clone()) } else { WorkflowRepositoryError::Persistence(error.to_string()) })?;
        Ok(())
    }

    fn get_run(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Option<WorkflowRun>, WorkflowRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT run_json FROM agent_os_workflow_runs WHERE run_id=?1",
                [run_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode).transpose()
    }

    fn list_runs(
        &self,
        workflow_id: &WorkflowId,
    ) -> Result<Vec<WorkflowRun>, WorkflowRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn.prepare("SELECT run_json FROM agent_os_workflow_runs WHERE workflow_id=?1 ORDER BY created_at, run_id")
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        let rows = statement
            .query_map([workflow_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        rows.map(|row| {
            row.map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))
                .and_then(Self::decode)
        })
        .collect()
    }

    fn update_run(
        &self,
        run: WorkflowRun,
        expected_revision: u64,
    ) -> Result<(), WorkflowRepositoryError> {
        let current = self
            .get_run(run.id())?
            .ok_or_else(|| WorkflowRepositoryError::RunNotFound(run.id().clone()))?;
        validate_run_update(&current, &run, expected_revision)?;
        let conn = lock_conn!(self.database.conn);
        let changed = conn.execute(
            "UPDATE agent_os_workflow_runs SET run_json=?1, lifecycle_state=?2, revision=?3, updated_at=?4 WHERE run_id=?5 AND revision=?6",
            params![Self::encode(&run)?, lifecycle_json(run.lifecycle())?, run.revision() as i64, run.updated_at(), run.id().as_str(), expected_revision as i64],
        ).map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        if changed != 1 {
            return Err(WorkflowRepositoryError::InvalidUpdate {
                aggregate: "Workflow Run",
            });
        }
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
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_workflow_tasks
             (task_id, run_id, execution_id, task_json, lifecycle_state, revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![task.id().as_str(), task.run_id().as_str(), task.execution_id().as_str(), Self::encode(&task)?, lifecycle_json(task.lifecycle())?, task.revision() as i64, task.created_at(), task.updated_at()],
        ).map_err(|error| {
            let message = error.to_string();
            if message.contains("execution_id") { WorkflowRepositoryError::ExecutionAlreadyAssigned(task.execution_id().clone()) }
            else if message.contains("UNIQUE constraint failed") { WorkflowRepositoryError::TaskAlreadyRegistered(task.id().clone()) }
            else { WorkflowRepositoryError::Persistence(message) }
        })?;
        Ok(())
    }

    fn get_task(
        &self,
        task_id: &WorkflowTaskId,
    ) -> Result<Option<WorkflowTask>, WorkflowRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT task_json FROM agent_os_workflow_tasks WHERE task_id=?1",
                [task_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode).transpose()
    }

    fn list_tasks(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Vec<WorkflowTask>, WorkflowRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn.prepare("SELECT task_json FROM agent_os_workflow_tasks WHERE run_id=?1 ORDER BY created_at, task_id")
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        let rows = statement
            .query_map([run_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        rows.map(|row| {
            row.map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))
                .and_then(Self::decode)
        })
        .collect()
    }

    fn update_task(
        &self,
        task: WorkflowTask,
        expected_revision: u64,
    ) -> Result<(), WorkflowRepositoryError> {
        let current = self
            .get_task(task.id())?
            .ok_or_else(|| WorkflowRepositoryError::TaskNotFound(task.id().clone()))?;
        validate_task_update(&current, &task, expected_revision)?;
        let conn = lock_conn!(self.database.conn);
        let changed = conn.execute(
            "UPDATE agent_os_workflow_tasks SET task_json=?1, lifecycle_state=?2, revision=?3, updated_at=?4 WHERE task_id=?5 AND revision=?6",
            params![Self::encode(&task)?, lifecycle_json(task.lifecycle())?, task.revision() as i64, task.updated_at(), task.id().as_str(), expected_revision as i64],
        ).map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        if changed != 1 {
            return Err(WorkflowRepositoryError::InvalidUpdate {
                aggregate: "Workflow Task",
            });
        }
        Ok(())
    }

    fn update_task_and_run(
        &self,
        task: WorkflowTask,
        expected_task_revision: u64,
        run: WorkflowRun,
        expected_run_revision: u64,
    ) -> Result<(), WorkflowRepositoryError> {
        let current_task = self
            .get_task(task.id())?
            .ok_or_else(|| WorkflowRepositoryError::TaskNotFound(task.id().clone()))?;
        let current_run = self
            .get_run(run.id())?
            .ok_or_else(|| WorkflowRepositoryError::RunNotFound(run.id().clone()))?;
        validate_task_update(&current_task, &task, expected_task_revision)?;
        validate_run_update(&current_run, &run, expected_run_revision)?;
        let conn = lock_conn!(self.database.conn);
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        let task_changed = transaction.execute(
            "UPDATE agent_os_workflow_tasks SET task_json=?1, lifecycle_state=?2, revision=?3, updated_at=?4 WHERE task_id=?5 AND revision=?6",
            params![Self::encode(&task)?, lifecycle_json(task.lifecycle())?, task.revision() as i64, task.updated_at(), task.id().as_str(), expected_task_revision as i64],
        ).map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        let run_changed = transaction.execute(
            "UPDATE agent_os_workflow_runs SET run_json=?1, lifecycle_state=?2, revision=?3, updated_at=?4 WHERE run_id=?5 AND revision=?6",
            params![Self::encode(&run)?, lifecycle_json(run.lifecycle())?, run.revision() as i64, run.updated_at(), run.id().as_str(), expected_run_revision as i64],
        ).map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        if task_changed != 1 || run_changed != 1 {
            return Err(WorkflowRepositoryError::InvalidUpdate {
                aggregate: "Workflow Task and Run",
            });
        }
        transaction
            .commit()
            .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))?;
        Ok(())
    }
}

fn lifecycle_json<T: Serialize>(value: T) -> Result<String, WorkflowRepositoryError> {
    serde_json::to_string(&value)
        .map(|value| value.trim_matches('"').to_string())
        .map_err(|error| WorkflowRepositoryError::Persistence(error.to_string()))
}

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
