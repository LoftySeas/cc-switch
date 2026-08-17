//! Workflow definition, run, participation, and task coordination service.
//!
//! The service advances explicit state only. It validates an existing Execution
//! and governance evidence through `WorkflowParticipationGate`, but never invokes
//! a Runtime or selects Provider/Model resources.

use thiserror::Error;

use crate::{
    database::Database,
    execution_repository::{ExecutionHistoryRepository, ExecutionRepositoryError},
    role_repository::RoleRepository,
    runtime_domain::RuntimeExecutionState,
    team_domain::TeamLifecycle,
    team_repository::TeamRepository,
    workflow_domain::{
        WorkflowDefinition, WorkflowDomainError, WorkflowId, WorkflowRun, WorkflowRunId,
        WorkflowStepState, WorkflowTask, WorkflowTaskId, WorkflowTaskLifecycle,
    },
    workflow_governance::{WorkflowGovernanceError, WorkflowParticipationGate},
    workflow_repository::{WorkflowRepository, WorkflowRepositoryError},
};

#[derive(Debug, Error)]
pub enum WorkflowOrchestrationError {
    #[error(transparent)]
    Domain(#[from] WorkflowDomainError),
    #[error(transparent)]
    Repository(#[from] WorkflowRepositoryError),
    #[error(transparent)]
    Governance(#[from] WorkflowGovernanceError),
    #[error("Team repository failed: {0}")]
    TeamRepository(String),
    #[error("Role repository failed: {0}")]
    RoleRepository(String),
    #[error("Execution repository failed: {0}")]
    ExecutionRepository(String),
    #[error("Workflow Team is missing or inactive")]
    TeamNotActive,
    #[error("Workflow is not listed by the Team's explicit compatibility references")]
    TeamWorkflowIncompatible,
    #[error("Workflow Step Role definition is missing")]
    RoleDefinitionMissing,
    #[error("Workflow Task does not match its Run, Step, or current attempt")]
    TaskMismatch,
    #[error("Workflow Task cannot be synchronized from the current Execution state")]
    ExecutionStateMismatch,
}

pub struct WorkflowOrchestrationService<W, T, R, E, G> {
    workflows: W,
    teams: T,
    roles: R,
    executions: E,
    participation_gate: G,
}

impl<W, T, R, E, G> WorkflowOrchestrationService<W, T, R, E, G>
where
    W: WorkflowRepository,
    T: TeamRepository,
    R: RoleRepository,
    E: ExecutionHistoryRepository,
    G: WorkflowParticipationGate,
{
    pub fn new(workflows: W, teams: T, roles: R, executions: E, participation_gate: G) -> Self {
        Self {
            workflows,
            teams,
            roles,
            executions,
            participation_gate,
        }
    }

    pub fn register_definition(
        &self,
        definition: WorkflowDefinition,
    ) -> Result<WorkflowDefinition, WorkflowOrchestrationError> {
        let team = self
            .teams
            .get_team(definition.team_id())
            .map_err(team_repository)?
            .ok_or(WorkflowOrchestrationError::TeamNotActive)?;
        if team.lifecycle() == TeamLifecycle::Archived {
            return Err(WorkflowOrchestrationError::TeamNotActive);
        }
        if !team.compatible_workflow_refs().is_empty()
            && !team
                .compatible_workflow_refs()
                .iter()
                .any(|reference| reference == definition.id().as_str())
        {
            return Err(WorkflowOrchestrationError::TeamWorkflowIncompatible);
        }
        for step in definition.steps() {
            if self
                .roles
                .get_definition(step.role_id(), step.role_version())
                .map_err(role_repository)?
                .is_none()
            {
                return Err(WorkflowOrchestrationError::RoleDefinitionMissing);
            }
        }
        self.workflows.register_definition(definition.clone())?;
        Ok(definition)
    }

    pub fn get_definition(
        &self,
        workflow_id: &WorkflowId,
        version: u16,
    ) -> Result<WorkflowDefinition, WorkflowOrchestrationError> {
        self.workflows
            .get_definition(workflow_id, version)?
            .ok_or_else(|| WorkflowRepositoryError::DefinitionNotFound {
                id: workflow_id.clone(),
                version,
            })
            .map_err(Into::into)
    }

    pub fn create_run(
        &self,
        run_id: WorkflowRunId,
        workflow_id: &WorkflowId,
        version: u16,
        created_at: i64,
    ) -> Result<WorkflowRun, WorkflowOrchestrationError> {
        let definition = self.get_definition(workflow_id, version)?;
        self.require_active_team(definition.team_id())?;
        let run = WorkflowRun::new(run_id, &definition, created_at)?;
        self.workflows.insert_run(run.clone())?;
        Ok(run)
    }

    pub fn get_run(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<WorkflowRun, WorkflowOrchestrationError> {
        self.workflows
            .get_run(run_id)?
            .ok_or_else(|| WorkflowRepositoryError::RunNotFound(run_id.clone()).into())
    }

    pub fn activate_run(
        &self,
        run_id: &WorkflowRunId,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<WorkflowRun, WorkflowOrchestrationError> {
        let run = self.get_run(run_id)?;
        self.require_active_team(run.team_id())?;
        let definition = self.get_definition(run.workflow_id(), run.workflow_version())?;
        let updated = run.activate(&definition, expected_revision, updated_at)?;
        self.workflows
            .update_run(updated.clone(), expected_revision)?;
        Ok(updated)
    }

    pub fn assign_task(
        &self,
        db: &Database,
        task: WorkflowTask,
        at: i64,
    ) -> Result<WorkflowTask, WorkflowOrchestrationError> {
        let run = self.get_run(task.run_id())?;
        let definition = self.get_definition(run.workflow_id(), run.workflow_version())?;
        let step = definition
            .step(task.step_id())
            .ok_or_else(|| WorkflowDomainError::StepNotFound(task.step_id().clone()))?;
        let existing = self.workflows.list_tasks(run.id())?;
        let expected_attempt = existing
            .iter()
            .filter(|existing| existing.step_id() == task.step_id())
            .count()
            + 1;
        if task.run_id() != run.id()
            || task.attempt() as usize != expected_attempt
            || task.created_at() != at
            || run.step_state(task.step_id()) != Some(WorkflowStepState::Ready)
            || existing.iter().any(|existing| {
                existing.step_id() == task.step_id() && !existing.lifecycle().is_terminal()
            })
        {
            return Err(WorkflowOrchestrationError::TaskMismatch);
        }
        self.participation_gate
            .validate(db, &definition, &run, step, &task, at)?;
        self.workflows.insert_task(task.clone())?;
        Ok(task)
    }

    pub fn get_task(
        &self,
        task_id: &WorkflowTaskId,
    ) -> Result<WorkflowTask, WorkflowOrchestrationError> {
        self.workflows
            .get_task(task_id)?
            .ok_or_else(|| WorkflowRepositoryError::TaskNotFound(task_id.clone()).into())
    }

    pub fn start_task(
        &self,
        task_id: &WorkflowTaskId,
        expected_task_revision: u64,
        expected_run_revision: u64,
        updated_at: i64,
    ) -> Result<(WorkflowTask, WorkflowRun), WorkflowOrchestrationError> {
        let task = self.get_task(task_id)?;
        let run = self.get_run(task.run_id())?;
        let updated_task = task.transition_to(
            WorkflowTaskLifecycle::Running,
            expected_task_revision,
            updated_at,
        )?;
        let updated_run = run.start_step(task.step_id(), expected_run_revision, updated_at)?;
        self.workflows.update_task_and_run(
            updated_task.clone(),
            expected_task_revision,
            updated_run.clone(),
            expected_run_revision,
        )?;
        Ok((updated_task, updated_run))
    }

    pub fn synchronize_task(
        &self,
        task_id: &WorkflowTaskId,
        expected_task_revision: u64,
        expected_run_revision: u64,
        updated_at: i64,
    ) -> Result<(WorkflowTask, WorkflowRun), WorkflowOrchestrationError> {
        let task = self.get_task(task_id)?;
        let execution = self
            .executions
            .get(task.execution_id())
            .map_err(execution_repository)?
            .ok_or(WorkflowOrchestrationError::ExecutionStateMismatch)?;
        if execution.state().is_terminal()
            && execution
                .result()
                .is_none_or(|result| result.state() != execution.state())
        {
            return Err(WorkflowOrchestrationError::ExecutionStateMismatch);
        }
        let target = match execution.state() {
            RuntimeExecutionState::Accepted | RuntimeExecutionState::Preparing => {
                if task.lifecycle() == WorkflowTaskLifecycle::Assigned {
                    return Ok((task.clone(), self.get_run(task.run_id())?));
                }
                return Err(WorkflowOrchestrationError::ExecutionStateMismatch);
            }
            RuntimeExecutionState::Running
            | RuntimeExecutionState::Cancelling
            | RuntimeExecutionState::Lost => WorkflowTaskLifecycle::Running,
            RuntimeExecutionState::WaitingForInput => WorkflowTaskLifecycle::Waiting,
            RuntimeExecutionState::Succeeded => WorkflowTaskLifecycle::Succeeded,
            RuntimeExecutionState::Failed => WorkflowTaskLifecycle::Failed,
            RuntimeExecutionState::Cancelled => WorkflowTaskLifecycle::Cancelled,
        };
        let run = self.get_run(task.run_id())?;
        let step_target = match target {
            WorkflowTaskLifecycle::Assigned => {
                return Err(WorkflowOrchestrationError::ExecutionStateMismatch)
            }
            WorkflowTaskLifecycle::Running => WorkflowStepState::Running,
            WorkflowTaskLifecycle::Waiting => WorkflowStepState::Waiting,
            WorkflowTaskLifecycle::Succeeded => WorkflowStepState::Succeeded,
            WorkflowTaskLifecycle::Failed => WorkflowStepState::Failed,
            WorkflowTaskLifecycle::Cancelled => WorkflowStepState::Cancelled,
        };
        if task.lifecycle() == target && run.step_state(task.step_id()) == Some(step_target) {
            return Ok((task, run));
        }
        let updated_task = task.transition_to(target, expected_task_revision, updated_at)?;
        let updated_run = run.transition_step(
            task.step_id(),
            step_target,
            expected_run_revision,
            updated_at,
        )?;
        self.workflows.update_task_and_run(
            updated_task.clone(),
            expected_task_revision,
            updated_run.clone(),
            expected_run_revision,
        )?;

        if target == WorkflowTaskLifecycle::Succeeded {
            let definition =
                self.get_definition(updated_run.workflow_id(), updated_run.workflow_version())?;
            let released =
                updated_run.release_dependents(&definition, updated_run.revision(), updated_at)?;
            if released != updated_run {
                self.workflows
                    .update_run(released.clone(), updated_run.revision())?;
                return Ok((updated_task, released));
            }
        }
        Ok((updated_task, updated_run))
    }

    pub fn cancel_run(
        &self,
        run_id: &WorkflowRunId,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<WorkflowRun, WorkflowOrchestrationError> {
        let run = self.get_run(run_id)?;
        let updated = run.cancel(expected_revision, updated_at)?;
        self.workflows
            .update_run(updated.clone(), expected_revision)?;
        Ok(updated)
    }

    fn require_active_team(
        &self,
        team_id: &crate::team_domain::TeamId,
    ) -> Result<(), WorkflowOrchestrationError> {
        let team = self
            .teams
            .get_team(team_id)
            .map_err(team_repository)?
            .ok_or(WorkflowOrchestrationError::TeamNotActive)?;
        if team.lifecycle() != TeamLifecycle::Active {
            return Err(WorkflowOrchestrationError::TeamNotActive);
        }
        Ok(())
    }
}

fn team_repository(error: impl std::fmt::Display) -> WorkflowOrchestrationError {
    WorkflowOrchestrationError::TeamRepository(error.to_string())
}

fn role_repository(error: impl std::fmt::Display) -> WorkflowOrchestrationError {
    WorkflowOrchestrationError::RoleRepository(error.to_string())
}

fn execution_repository(error: ExecutionRepositoryError) -> WorkflowOrchestrationError {
    WorkflowOrchestrationError::ExecutionRepository(error.to_string())
}
