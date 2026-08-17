//! Append-only execution history repository boundary.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::{
    execution_domain::{
        ExecutionDomainError, ExecutionRequest, ExecutionResult, ExecutionTransition,
    },
    runtime_domain::{RuntimeExecutionId, RuntimeExecutionState},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRecord {
    request: ExecutionRequest,
    state: RuntimeExecutionState,
    revision: u64,
    transitions: Vec<ExecutionTransition>,
    result: Option<ExecutionResult>,
}

impl ExecutionRecord {
    fn accepted(request: ExecutionRequest) -> Self {
        Self {
            request,
            state: RuntimeExecutionState::Accepted,
            revision: 1,
            transitions: Vec::new(),
            result: None,
        }
    }

    pub fn request(&self) -> &ExecutionRequest {
        &self.request
    }
    pub fn state(&self) -> RuntimeExecutionState {
        self.state
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn transitions(&self) -> &[ExecutionTransition] {
        &self.transitions
    }
    pub fn result(&self) -> Option<&ExecutionResult> {
        self.result.as_ref()
    }
}

#[derive(Debug, Error)]
pub enum ExecutionRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] ExecutionDomainError),
    #[error("Execution is already registered: {0}")]
    AlreadyRegistered(RuntimeExecutionId),
    #[error("Execution was not found: {0}")]
    NotFound(RuntimeExecutionId),
    #[error(
        "Execution revision conflict for {execution_id}: expected {expected}, current {current}"
    )]
    RevisionConflict {
        execution_id: RuntimeExecutionId,
        expected: u64,
        current: u64,
    },
    #[error("Execution result identity or state does not match its record: {0}")]
    ResultMismatch(RuntimeExecutionId),
    #[error("Execution already has a terminal result: {0}")]
    ResultAlreadyStored(RuntimeExecutionId),
    #[error("Execution history lock failed: {0}")]
    RegistryLock(String),
}

pub trait ExecutionHistoryRepository: Send + Sync {
    fn accept(
        &self,
        request: ExecutionRequest,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError>;
    fn get(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Option<ExecutionRecord>, ExecutionRepositoryError>;
    fn list(&self) -> Result<Vec<ExecutionRecord>, ExecutionRepositoryError>;
    fn transition(
        &self,
        execution_id: &RuntimeExecutionId,
        target: RuntimeExecutionState,
        expected_revision: u64,
        occurred_at: i64,
        reason: &str,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError>;
    fn store_result(
        &self,
        result: ExecutionResult,
        expected_revision: u64,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryExecutionHistoryRepository {
    records: Arc<RwLock<HashMap<RuntimeExecutionId, ExecutionRecord>>>,
}

impl ExecutionHistoryRepository for InMemoryExecutionHistoryRepository {
    fn accept(
        &self,
        request: ExecutionRequest,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError> {
        request.validate()?;
        let execution_id = request.execution_id().clone();
        let mut records = self
            .records
            .write()
            .map_err(|e| ExecutionRepositoryError::RegistryLock(e.to_string()))?;
        if records.contains_key(&execution_id) {
            return Err(ExecutionRepositoryError::AlreadyRegistered(execution_id));
        }
        let record = ExecutionRecord::accepted(request);
        records.insert(execution_id, record.clone());
        Ok(record)
    }

    fn get(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Option<ExecutionRecord>, ExecutionRepositoryError> {
        let records = self
            .records
            .read()
            .map_err(|e| ExecutionRepositoryError::RegistryLock(e.to_string()))?;
        Ok(records.get(execution_id).cloned())
    }

    fn list(&self) -> Result<Vec<ExecutionRecord>, ExecutionRepositoryError> {
        let records = self
            .records
            .read()
            .map_err(|e| ExecutionRepositoryError::RegistryLock(e.to_string()))?;
        let mut values = records.values().cloned().collect::<Vec<_>>();
        values.sort_by(|a, b| {
            a.request()
                .accepted_at()
                .cmp(&b.request().accepted_at())
                .then_with(|| {
                    a.request()
                        .execution_id()
                        .as_str()
                        .cmp(b.request().execution_id().as_str())
                })
        });
        Ok(values)
    }

    fn transition(
        &self,
        execution_id: &RuntimeExecutionId,
        target: RuntimeExecutionState,
        expected_revision: u64,
        occurred_at: i64,
        reason: &str,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError> {
        let mut records = self
            .records
            .write()
            .map_err(|e| ExecutionRepositoryError::RegistryLock(e.to_string()))?;
        let record = records
            .get_mut(execution_id)
            .ok_or_else(|| ExecutionRepositoryError::NotFound(execution_id.clone()))?;
        if record.revision != expected_revision {
            return Err(ExecutionRepositoryError::RevisionConflict {
                execution_id: execution_id.clone(),
                expected: expected_revision,
                current: record.revision,
            });
        }
        if occurred_at < record.request.accepted_at() {
            return Err(ExecutionDomainError::InvalidTimestamp.into());
        }
        let transition =
            ExecutionTransition::new(record.revision, record.state, target, occurred_at, reason)?;
        record.state = target;
        record.revision += 1;
        record.transitions.push(transition);
        Ok(record.clone())
    }

    fn store_result(
        &self,
        result: ExecutionResult,
        expected_revision: u64,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError> {
        let execution_id = result.execution_id().clone();
        let mut records = self
            .records
            .write()
            .map_err(|e| ExecutionRepositoryError::RegistryLock(e.to_string()))?;
        let record = records
            .get_mut(&execution_id)
            .ok_or_else(|| ExecutionRepositoryError::NotFound(execution_id.clone()))?;
        if record.revision != expected_revision {
            return Err(ExecutionRepositoryError::RevisionConflict {
                execution_id,
                expected: expected_revision,
                current: record.revision,
            });
        }
        if record.result.is_some() {
            return Err(ExecutionRepositoryError::ResultAlreadyStored(execution_id));
        }
        if record.state != result.state()
            || !record.state.is_terminal()
            || result.completed_at() < record.request.accepted_at()
        {
            return Err(ExecutionRepositoryError::ResultMismatch(execution_id));
        }
        record.result = Some(result);
        record.revision += 1;
        Ok(record.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        execution_domain::{ExecutionGovernanceEvidence, ExecutionModelBinding},
        model_domain::ModelId,
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeBindingId, RuntimeBindingLifecycle,
            RuntimeId,
        },
    };

    fn request(id: &str) -> ExecutionRequest {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new(format!("binding:{id}")).unwrap(),
            format!("agent:{id}"),
            RuntimeId::new(format!("runtime:{id}")).unwrap(),
            10,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 11)
        .unwrap();
        let context = ExecutionContext::new(
            RuntimeExecutionId::new(format!("execution:{id}")).unwrap(),
            binding,
            vec![format!("context:{id}")],
            12,
        )
        .unwrap();
        ExecutionRequest::new(
            context,
            "objective",
            ExecutionModelBinding::runtime_local(ModelId::new(format!("model:{id}")).unwrap()),
            ExecutionGovernanceEvidence::new("capability:snapshot", "permission:grant").unwrap(),
            None,
            13,
        )
        .unwrap()
    }

    #[test]
    fn history_is_revisioned_append_only_and_rejects_invalid_transitions() {
        let repository = InMemoryExecutionHistoryRepository::default();
        let request = request("one");
        let id = request.execution_id().clone();
        let accepted = repository.accept(request.clone()).unwrap();
        let preparing = repository
            .transition(
                &id,
                RuntimeExecutionState::Preparing,
                accepted.revision(),
                14,
                "prepare",
            )
            .unwrap();

        assert_eq!(preparing.request(), &request);
        assert_eq!(preparing.transitions().len(), 1);
        assert!(matches!(
            repository.transition(
                &id,
                RuntimeExecutionState::Succeeded,
                preparing.revision(),
                15,
                "skip"
            ),
            Err(ExecutionRepositoryError::InvalidDomain(
                ExecutionDomainError::InvalidRuntime(_)
            ))
        ));
        assert!(matches!(
            repository.transition(&id, RuntimeExecutionState::Running, 1, 15, "stale"),
            Err(ExecutionRepositoryError::RevisionConflict { .. })
        ));
        assert_eq!(repository.get(&id).unwrap().unwrap().request(), &request);
    }
}
