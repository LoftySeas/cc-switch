//! Append-only execution history repository boundary.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    database::{lock_conn, Database},
    error::AppError,
    execution_domain::{
        ExecutionDomainError, ExecutionRequest, ExecutionResult, ExecutionTransition,
    },
    runtime_domain::{RuntimeExecutionId, RuntimeExecutionState},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[error("Execution persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for ExecutionRepositoryError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
}

/// SQLite-backed execution history. The serialized record is a durable
/// snapshot containing the complete append-only transition list; optimistic
/// revision checks keep concurrent writers explicit.
#[derive(Clone)]
pub struct SqliteExecutionHistoryRepository {
    database: Arc<Database>,
}

impl SqliteExecutionHistoryRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    fn encode(record: &ExecutionRecord) -> Result<String, ExecutionRepositoryError> {
        serde_json::to_string(record)
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))
    }

    fn decode(value: String) -> Result<ExecutionRecord, ExecutionRepositoryError> {
        let record: ExecutionRecord = serde_json::from_str(&value)
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        record.request.validate()?;
        Ok(record)
    }
}

impl ExecutionHistoryRepository for SqliteExecutionHistoryRepository {
    fn accept(
        &self,
        request: ExecutionRequest,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError> {
        request.validate()?;
        let execution_id = request.execution_id().clone();
        let accepted_at = request.accepted_at();
        let record = ExecutionRecord::accepted(request);
        let value = Self::encode(&record)?;
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_execution_records
             (execution_id, record_json, revision, accepted_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                execution_id.as_str(),
                value,
                record.revision as i64,
                accepted_at
            ],
        )
        .map_err(|error| {
            if error.to_string().contains("UNIQUE constraint failed") {
                ExecutionRepositoryError::AlreadyRegistered(execution_id.clone())
            } else {
                ExecutionRepositoryError::Persistence(error.to_string())
            }
        })?;
        Ok(record)
    }

    fn get(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Option<ExecutionRecord>, ExecutionRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT record_json FROM agent_os_execution_records WHERE execution_id = ?1",
                [execution_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode).transpose()
    }

    fn list(&self) -> Result<Vec<ExecutionRecord>, ExecutionRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn
            .prepare(
                "SELECT record_json FROM agent_os_execution_records
                 ORDER BY accepted_at, execution_id",
            )
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        rows.map(|row| {
            row.map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))
                .and_then(Self::decode)
        })
        .collect()
    }

    fn transition(
        &self,
        execution_id: &RuntimeExecutionId,
        target: RuntimeExecutionState,
        expected_revision: u64,
        occurred_at: i64,
        reason: &str,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        let value = transaction
            .query_row(
                "SELECT record_json FROM agent_os_execution_records WHERE execution_id = ?1",
                [execution_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?
            .ok_or_else(|| ExecutionRepositoryError::NotFound(execution_id.clone()))?;
        let mut record = Self::decode(value)?;
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
        let changed = transaction
            .execute(
                "UPDATE agent_os_execution_records
                 SET record_json = ?1, revision = ?2
                 WHERE execution_id = ?3 AND revision = ?4",
                params![
                    Self::encode(&record)?,
                    record.revision as i64,
                    execution_id.as_str(),
                    expected_revision as i64
                ],
            )
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        if changed != 1 {
            return Err(ExecutionRepositoryError::RevisionConflict {
                execution_id: execution_id.clone(),
                expected: expected_revision,
                current: expected_revision + 1,
            });
        }
        transaction
            .commit()
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        Ok(record)
    }

    fn store_result(
        &self,
        result: ExecutionResult,
        expected_revision: u64,
    ) -> Result<ExecutionRecord, ExecutionRepositoryError> {
        let execution_id = result.execution_id().clone();
        let conn = lock_conn!(self.database.conn);
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        let value = transaction
            .query_row(
                "SELECT record_json FROM agent_os_execution_records WHERE execution_id = ?1",
                [execution_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?
            .ok_or_else(|| ExecutionRepositoryError::NotFound(execution_id.clone()))?;
        let mut record = Self::decode(value)?;
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
        let changed = transaction
            .execute(
                "UPDATE agent_os_execution_records
                 SET record_json = ?1, revision = ?2
                 WHERE execution_id = ?3 AND revision = ?4",
                params![
                    Self::encode(&record)?,
                    record.revision as i64,
                    execution_id.as_str(),
                    expected_revision as i64
                ],
            )
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        if changed != 1 {
            return Err(ExecutionRepositoryError::RevisionConflict {
                execution_id,
                expected: expected_revision,
                current: expected_revision + 1,
            });
        }
        transaction
            .commit()
            .map_err(|error| ExecutionRepositoryError::Persistence(error.to_string()))?;
        Ok(record)
    }
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
            ExecutionGovernanceEvidence::new(
                crate::capability_domain::CapabilitySnapshotId::new("capability:snapshot").unwrap(),
                crate::permission_domain::PermissionGrantId::new("permission:grant").unwrap(),
                crate::role_domain::RoleAssignmentId::new("assignment:one").unwrap(),
                crate::permission_domain::AuthorizationDecisionId::new("decision:one").unwrap(),
            ),
            None,
            13,
        )
        .unwrap()
    }

    #[test]
    fn sqlite_history_survives_repository_recreation_and_preserves_revision() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteExecutionHistoryRepository::new(database.clone());
        let request = request("durable");
        let id = request.execution_id().clone();
        let accepted = repository.accept(request).unwrap();
        let preparing = repository
            .transition(
                &id,
                RuntimeExecutionState::Preparing,
                accepted.revision(),
                14,
                "prepare",
            )
            .unwrap();
        drop(repository);

        let reopened = SqliteExecutionHistoryRepository::new(database);
        let restored = reopened.get(&id).unwrap().unwrap();
        assert_eq!(restored.state(), RuntimeExecutionState::Preparing);
        assert_eq!(restored.revision(), preparing.revision());
        assert_eq!(restored.transitions().len(), 1);
        assert!(matches!(
            reopened.transition(
                &id,
                RuntimeExecutionState::Running,
                accepted.revision(),
                15,
                "stale",
            ),
            Err(ExecutionRepositoryError::RevisionConflict { .. })
        ));
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
