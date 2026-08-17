//! Durable queue, retry, and audit boundaries for execution attempts.
//!
//! Queue records contain immutable `ExecutionRequest` values. Dispatch remains
//! outside this module and must use `ExecutionPipeline`, preserving the Runtime
//! Adapter as the only productive invocation boundary.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    database::{lock_conn, Database},
    error::AppError,
    execution_domain::ExecutionRequest,
    runtime_domain::RuntimeExecutionId,
};

const MAX_ID_LENGTH: usize = 128;
const MAX_DETAILS_LENGTH: usize = 2048;

type QueueRow = (
    String,
    String,
    String,
    i32,
    u32,
    u32,
    i64,
    Option<String>,
    Option<i64>,
    Option<String>,
    u64,
    i64,
    i64,
);

fn required(
    field: &'static str,
    value: impl Into<String>,
    max: usize,
) -> Result<String, ExecutionPlatformError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(ExecutionPlatformError::InvalidInput(format!(
            "{field} is empty"
        )));
    }
    if value.chars().count() > max {
        return Err(ExecutionPlatformError::InvalidInput(format!(
            "{field} exceeds {max} characters"
        )));
    }
    Ok(value.to_string())
}

macro_rules! platform_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ExecutionPlatformError> {
                let value = required($field, value, MAX_ID_LENGTH)?;
                if value
                    .chars()
                    .any(|character| character.is_whitespace() || character.is_control())
                {
                    return Err(ExecutionPlatformError::InvalidInput(format!(
                        "{} contains whitespace or control characters",
                        $field
                    )));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

platform_id!(ExecutionQueueItemId, "Execution queue item ID");
platform_id!(ExecutionAuditId, "Execution audit ID");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionQueueState {
    Pending,
    Leased,
    Completed,
    DeadLetter,
    Cancelled,
}

impl ExecutionQueueState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Leased => "leased",
            Self::Completed => "completed",
            Self::DeadLetter => "dead_letter",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Result<Self, ExecutionPlatformError> {
        match value {
            "pending" => Ok(Self::Pending),
            "leased" => Ok(Self::Leased),
            "completed" => Ok(Self::Completed),
            "dead_letter" => Ok(Self::DeadLetter),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(ExecutionPlatformError::Persistence(format!(
                "unknown queue state: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionQueueItem {
    id: ExecutionQueueItemId,
    request: ExecutionRequest,
    state: ExecutionQueueState,
    priority: i32,
    attempt: u32,
    max_attempts: u32,
    available_at: i64,
    lease_owner: Option<String>,
    lease_until: Option<i64>,
    parent_execution_id: Option<RuntimeExecutionId>,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl ExecutionQueueItem {
    #[allow(clippy::too_many_arguments)]
    pub fn pending(
        id: ExecutionQueueItemId,
        request: ExecutionRequest,
        priority: i32,
        attempt: u32,
        max_attempts: u32,
        available_at: i64,
        parent_execution_id: Option<RuntimeExecutionId>,
        created_at: i64,
    ) -> Result<Self, ExecutionPlatformError> {
        request
            .validate()
            .map_err(|error| ExecutionPlatformError::InvalidInput(error.to_string()))?;
        if attempt == 0 || max_attempts < attempt || available_at < created_at {
            return Err(ExecutionPlatformError::InvalidInput(
                "invalid attempt or queue timestamp".to_string(),
            ));
        }
        Ok(Self {
            id,
            request,
            state: ExecutionQueueState::Pending,
            priority,
            attempt,
            max_attempts,
            available_at,
            lease_owner: None,
            lease_until: None,
            parent_execution_id,
            revision: 1,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn id(&self) -> &ExecutionQueueItemId {
        &self.id
    }
    pub fn request(&self) -> &ExecutionRequest {
        &self.request
    }
    pub fn state(&self) -> ExecutionQueueState {
        self.state
    }
    pub fn priority(&self) -> i32 {
        self.priority
    }
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
    pub fn available_at(&self) -> i64 {
        self.available_at
    }
    pub fn lease_owner(&self) -> Option<&str> {
        self.lease_owner.as_deref()
    }
    pub fn lease_until(&self) -> Option<i64> {
        self.lease_until
    }
    pub fn parent_execution_id(&self) -> Option<&RuntimeExecutionId> {
        self.parent_execution_id.as_ref()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAuditKind {
    Queued,
    Leased,
    AttemptStarted,
    RetryScheduled,
    Completed,
    DeadLetter,
    Cancelled,
}

impl ExecutionAuditKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Leased => "leased",
            Self::AttemptStarted => "attempt_started",
            Self::RetryScheduled => "retry_scheduled",
            Self::Completed => "completed",
            Self::DeadLetter => "dead_letter",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Result<Self, ExecutionPlatformError> {
        match value {
            "queued" => Ok(Self::Queued),
            "leased" => Ok(Self::Leased),
            "attempt_started" => Ok(Self::AttemptStarted),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "completed" => Ok(Self::Completed),
            "dead_letter" => Ok(Self::DeadLetter),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(ExecutionPlatformError::Persistence(format!(
                "unknown audit kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAuditEvent {
    id: ExecutionAuditId,
    execution_id: RuntimeExecutionId,
    kind: ExecutionAuditKind,
    sequence: u64,
    occurred_at: i64,
    details: String,
}

impl ExecutionAuditEvent {
    pub fn new(
        id: ExecutionAuditId,
        execution_id: RuntimeExecutionId,
        kind: ExecutionAuditKind,
        sequence: u64,
        occurred_at: i64,
        details: impl Into<String>,
    ) -> Result<Self, ExecutionPlatformError> {
        if sequence == 0 {
            return Err(ExecutionPlatformError::InvalidInput(
                "audit sequence must be positive".to_string(),
            ));
        }
        Ok(Self {
            id,
            execution_id,
            kind,
            sequence,
            occurred_at,
            details: required("Audit details", details, MAX_DETAILS_LENGTH)?,
        })
    }
    pub fn id(&self) -> &ExecutionAuditId {
        &self.id
    }
    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }
    pub fn kind(&self) -> ExecutionAuditKind {
        self.kind
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn occurred_at(&self) -> i64 {
        self.occurred_at
    }
    pub fn details(&self) -> &str {
        &self.details
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionRetryPolicy {
    max_attempts: u32,
    base_delay_seconds: u64,
    max_delay_seconds: u64,
}

impl ExecutionRetryPolicy {
    pub fn new(
        max_attempts: u32,
        base_delay_seconds: u64,
        max_delay_seconds: u64,
    ) -> Result<Self, ExecutionPlatformError> {
        if max_attempts == 0 || base_delay_seconds == 0 || max_delay_seconds < base_delay_seconds {
            return Err(ExecutionPlatformError::InvalidInput(
                "invalid retry policy".to_string(),
            ));
        }
        Ok(Self {
            max_attempts,
            base_delay_seconds,
            max_delay_seconds,
        })
    }
    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
    pub fn should_retry(&self, attempt: u32, retry_safe: bool) -> bool {
        retry_safe && attempt < self.max_attempts
    }
    pub fn delay_seconds(&self, next_attempt: u32) -> u64 {
        let exponent = next_attempt.saturating_sub(2).min(31);
        self.base_delay_seconds
            .saturating_mul(1_u64 << exponent)
            .min(self.max_delay_seconds)
    }
}

#[derive(Debug, Error)]
pub enum ExecutionPlatformError {
    #[error("Invalid execution platform input: {0}")]
    InvalidInput(String),
    #[error("Execution queue item already exists: {0}")]
    AlreadyQueued(ExecutionQueueItemId),
    #[error("Execution queue item was not found: {0}")]
    QueueItemNotFound(ExecutionQueueItemId),
    #[error("Execution queue revision conflict for {id}: expected {expected}, current {current}")]
    RevisionConflict {
        id: ExecutionQueueItemId,
        expected: u64,
        current: u64,
    },
    #[error("Invalid queue transition from {from:?} to {to:?}")]
    InvalidQueueTransition {
        from: ExecutionQueueState,
        to: ExecutionQueueState,
    },
    #[error("Execution persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for ExecutionPlatformError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait ExecutionQueueRepository: Send + Sync {
    fn enqueue(
        &self,
        item: ExecutionQueueItem,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError>;
    fn get(
        &self,
        id: &ExecutionQueueItemId,
    ) -> Result<Option<ExecutionQueueItem>, ExecutionPlatformError>;
    fn lease_next(
        &self,
        worker: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<Option<ExecutionQueueItem>, ExecutionPlatformError>;
    fn complete(
        &self,
        id: &ExecutionQueueItemId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError>;
    fn dead_letter(
        &self,
        id: &ExecutionQueueItemId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError>;
    fn cancel(
        &self,
        id: &ExecutionQueueItemId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError>;
}

pub trait ExecutionAuditRepository: Send + Sync {
    fn append(&self, event: ExecutionAuditEvent) -> Result<(), ExecutionPlatformError>;
    fn list(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Vec<ExecutionAuditEvent>, ExecutionPlatformError>;
}

#[derive(Clone)]
pub struct SqliteExecutionPlatformRepository {
    database: Arc<Database>,
}

impl SqliteExecutionPlatformRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    fn decode_queue(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueueRow> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
        ))
    }

    fn materialize_queue(raw: QueueRow) -> Result<ExecutionQueueItem, ExecutionPlatformError> {
        let request: ExecutionRequest = serde_json::from_str(&raw.1)
            .map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        request
            .validate()
            .map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        Ok(ExecutionQueueItem {
            id: ExecutionQueueItemId::new(raw.0)?,
            request,
            state: ExecutionQueueState::parse(&raw.2)?,
            priority: raw.3,
            attempt: raw.4,
            max_attempts: raw.5,
            available_at: raw.6,
            lease_owner: raw.7,
            lease_until: raw.8,
            parent_execution_id: raw
                .9
                .map(RuntimeExecutionId::new)
                .transpose()
                .map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?,
            revision: raw.10,
            created_at: raw.11,
            updated_at: raw.12,
        })
    }

    fn change_state(
        &self,
        id: &ExecutionQueueItemId,
        expected_revision: u64,
        target: ExecutionQueueState,
        now: i64,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError> {
        let current = self
            .get(id)?
            .ok_or_else(|| ExecutionPlatformError::QueueItemNotFound(id.clone()))?;
        if current.revision != expected_revision {
            return Err(ExecutionPlatformError::RevisionConflict {
                id: id.clone(),
                expected: expected_revision,
                current: current.revision,
            });
        }
        if current.state != ExecutionQueueState::Leased
            || !matches!(
                target,
                ExecutionQueueState::Completed
                    | ExecutionQueueState::DeadLetter
                    | ExecutionQueueState::Cancelled
            )
        {
            return Err(ExecutionPlatformError::InvalidQueueTransition {
                from: current.state,
                to: target,
            });
        }
        let conn = lock_conn!(self.database.conn);
        let changed = conn.execute(
            "UPDATE agent_os_execution_queue SET state=?1, lease_owner=NULL, lease_until=NULL, revision=revision+1, updated_at=?2 WHERE queue_item_id=?3 AND revision=?4",
            params![target.as_str(), now, id.as_str(), expected_revision as i64],
        ).map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        if changed != 1 {
            return Err(ExecutionPlatformError::RevisionConflict {
                id: id.clone(),
                expected: expected_revision,
                current: expected_revision + 1,
            });
        }
        drop(conn);
        self.get(id)?
            .ok_or_else(|| ExecutionPlatformError::QueueItemNotFound(id.clone()))
    }
}

impl ExecutionQueueRepository for SqliteExecutionPlatformRepository {
    fn enqueue(
        &self,
        item: ExecutionQueueItem,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError> {
        if item.state != ExecutionQueueState::Pending {
            return Err(ExecutionPlatformError::InvalidQueueTransition {
                from: item.state,
                to: ExecutionQueueState::Pending,
            });
        }
        let request_json = serde_json::to_string(&item.request)
            .map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_execution_queue (queue_item_id,execution_id,request_json,state,priority,attempt,max_attempts,available_at,lease_owner,lease_until,parent_execution_id,revision,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,NULL,?9,?10,?11,?12)",
            params![item.id.as_str(), item.request.execution_id().as_str(), request_json, item.state.as_str(), item.priority, item.attempt, item.max_attempts, item.available_at, item.parent_execution_id.as_ref().map(RuntimeExecutionId::as_str), item.revision as i64, item.created_at, item.updated_at],
        ).map_err(|error| if error.to_string().contains("UNIQUE constraint failed") { ExecutionPlatformError::AlreadyQueued(item.id.clone()) } else { ExecutionPlatformError::Persistence(error.to_string()) })?;
        Ok(item)
    }

    fn get(
        &self,
        id: &ExecutionQueueItemId,
    ) -> Result<Option<ExecutionQueueItem>, ExecutionPlatformError> {
        let conn = lock_conn!(self.database.conn);
        let raw = conn.query_row(
            "SELECT queue_item_id,request_json,state,priority,attempt,max_attempts,available_at,lease_owner,lease_until,parent_execution_id,revision,created_at,updated_at FROM agent_os_execution_queue WHERE queue_item_id=?1",
            [id.as_str()], Self::decode_queue,
        ).optional().map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        raw.map(Self::materialize_queue).transpose()
    }

    fn lease_next(
        &self,
        worker: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<Option<ExecutionQueueItem>, ExecutionPlatformError> {
        let worker = required("Queue worker", worker, MAX_ID_LENGTH)?;
        if lease_until <= now {
            return Err(ExecutionPlatformError::InvalidInput(
                "lease must end after it starts".to_string(),
            ));
        }
        let conn = lock_conn!(self.database.conn);
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        let candidate = transaction.query_row(
            "SELECT queue_item_id, revision FROM agent_os_execution_queue WHERE state='pending' AND available_at<=?1 ORDER BY priority DESC, created_at, queue_item_id LIMIT 1",
            [now], |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
        ).optional().map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        let Some((id, revision)) = candidate else {
            return Ok(None);
        };
        let changed = transaction.execute(
            "UPDATE agent_os_execution_queue SET state='leased',lease_owner=?1,lease_until=?2,revision=revision+1,updated_at=?3 WHERE queue_item_id=?4 AND revision=?5 AND state='pending'",
            params![worker, lease_until, now, id, revision as i64],
        ).map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        if changed != 1 {
            return Err(ExecutionPlatformError::RevisionConflict {
                id: ExecutionQueueItemId::new(id)?,
                expected: revision,
                current: revision + 1,
            });
        }
        transaction
            .commit()
            .map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        drop(conn);
        self.get(&ExecutionQueueItemId::new(id)?)
    }

    fn complete(
        &self,
        id: &ExecutionQueueItemId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError> {
        self.change_state(id, expected_revision, ExecutionQueueState::Completed, now)
    }
    fn dead_letter(
        &self,
        id: &ExecutionQueueItemId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError> {
        self.change_state(id, expected_revision, ExecutionQueueState::DeadLetter, now)
    }
    fn cancel(
        &self,
        id: &ExecutionQueueItemId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformError> {
        self.change_state(id, expected_revision, ExecutionQueueState::Cancelled, now)
    }
}

impl ExecutionAuditRepository for SqliteExecutionPlatformRepository {
    fn append(&self, event: ExecutionAuditEvent) -> Result<(), ExecutionPlatformError> {
        let conn = lock_conn!(self.database.conn);
        conn.execute("INSERT INTO agent_os_execution_audit (audit_id,execution_id,event_kind,sequence,occurred_at,details) VALUES (?1,?2,?3,?4,?5,?6)", params![event.id.as_str(), event.execution_id.as_str(), event.kind.as_str(), event.sequence as i64, event.occurred_at, event.details]).map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        Ok(())
    }

    fn list(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Vec<ExecutionAuditEvent>, ExecutionPlatformError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn.prepare("SELECT audit_id,event_kind,sequence,occurred_at,details FROM agent_os_execution_audit WHERE execution_id=?1 ORDER BY sequence").map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        let rows = statement
            .query_map([execution_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
        rows.map(|row| {
            let (id, kind, sequence, occurred_at, details) =
                row.map_err(|error| ExecutionPlatformError::Persistence(error.to_string()))?;
            ExecutionAuditEvent::new(
                ExecutionAuditId::new(id)?,
                execution_id.clone(),
                ExecutionAuditKind::parse(&kind)?,
                sequence,
                occurred_at,
                details,
            )
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability_domain::CapabilitySnapshotId,
        execution_domain::{ExecutionGovernanceEvidence, ExecutionModelBinding},
        model_domain::ModelId,
        permission_domain::{AuthorizationDecisionId, PermissionGrantId},
        role_domain::RoleAssignmentId,
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeBindingId, RuntimeBindingLifecycle,
            RuntimeId,
        },
    };

    fn request(id: &str, accepted_at: i64) -> ExecutionRequest {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new(format!("binding:{id}")).unwrap(),
            format!("agent:{id}"),
            RuntimeId::new(format!("runtime:{id}")).unwrap(),
            1,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 2)
        .unwrap();
        ExecutionRequest::new(
            ExecutionContext::new(
                RuntimeExecutionId::new(format!("execution:{id}")).unwrap(),
                binding,
                vec![format!("context:{id}")],
                3,
            )
            .unwrap(),
            "objective",
            ExecutionModelBinding::runtime_local(ModelId::new(format!("model:{id}")).unwrap()),
            ExecutionGovernanceEvidence::new(
                CapabilitySnapshotId::new("capability:snapshot").unwrap(),
                PermissionGrantId::new("permission:grant").unwrap(),
                RoleAssignmentId::new("assignment:one").unwrap(),
                AuthorizationDecisionId::new("decision:one").unwrap(),
            ),
            None,
            accepted_at,
        )
        .unwrap()
    }

    #[test]
    fn sqlite_queue_uses_priority_and_explicit_terminal_transition() {
        let repository =
            SqliteExecutionPlatformRepository::new(Arc::new(Database::memory().unwrap()));
        for (id, priority) in [("low", 1), ("high", 10)] {
            repository
                .enqueue(
                    ExecutionQueueItem::pending(
                        ExecutionQueueItemId::new(format!("queue:{id}")).unwrap(),
                        request(id, 4),
                        priority,
                        1,
                        3,
                        4,
                        None,
                        4,
                    )
                    .unwrap(),
                )
                .unwrap();
        }

        let leased = repository.lease_next("worker:one", 5, 10).unwrap().unwrap();
        assert_eq!(leased.id().as_str(), "queue:high");
        assert_eq!(leased.state(), ExecutionQueueState::Leased);
        assert_eq!(leased.lease_owner(), Some("worker:one"));
        let completed = repository
            .complete(leased.id(), leased.revision(), 6)
            .unwrap();
        assert_eq!(completed.state(), ExecutionQueueState::Completed);
        assert!(repository
            .complete(completed.id(), completed.revision(), 7)
            .is_err());
    }

    #[test]
    fn audit_history_is_ordered_and_append_only() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteExecutionPlatformRepository::new(database.clone());
        let execution_id = RuntimeExecutionId::new("execution:audit").unwrap();
        for (sequence, kind) in [
            (1, ExecutionAuditKind::Queued),
            (2, ExecutionAuditKind::Leased),
        ] {
            repository
                .append(
                    ExecutionAuditEvent::new(
                        ExecutionAuditId::new(format!("audit:audit:{sequence}")).unwrap(),
                        execution_id.clone(),
                        kind,
                        sequence,
                        sequence as i64,
                        "event",
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let events = repository.list(&execution_id).unwrap();
        assert_eq!(
            events
                .iter()
                .map(ExecutionAuditEvent::sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        let conn = database.conn.lock().unwrap();
        assert!(conn
            .execute("DELETE FROM agent_os_execution_audit", [])
            .is_err());
        assert!(conn
            .execute("UPDATE agent_os_execution_audit SET details='changed'", [])
            .is_err());
    }

    #[test]
    fn retry_policy_requires_retry_safe_failure_and_caps_backoff() {
        let policy = ExecutionRetryPolicy::new(4, 5, 12).unwrap();
        assert!(policy.should_retry(1, true));
        assert!(!policy.should_retry(1, false));
        assert!(!policy.should_retry(4, true));
        assert_eq!(policy.delay_seconds(2), 5);
        assert_eq!(policy.delay_seconds(3), 10);
        assert_eq!(policy.delay_seconds(4), 12);
    }

    #[test]
    fn retry_request_has_new_identity_and_prior_correlation() {
        let original = request("original", 4);
        let next_id = RuntimeExecutionId::new("execution:retry").unwrap();
        let retry = original.retry_with(next_id.clone(), 10).unwrap();
        assert_eq!(retry.execution_id(), &next_id);
        assert_eq!(retry.correlation_ref(), Some("execution:original"));
        assert_eq!(retry.context().binding(), original.context().binding());
        assert_eq!(retry.model_binding(), original.model_binding());
        assert_eq!(retry.governance(), original.governance());
    }
}
