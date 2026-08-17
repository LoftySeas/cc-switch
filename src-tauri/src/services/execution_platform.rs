//! Explicit orchestration for queueing and dispatching execution attempts.

use thiserror::Error;

use crate::{
    execution_domain::ExecutionRequest,
    execution_platform::{
        ExecutionAuditEvent, ExecutionAuditId, ExecutionAuditKind, ExecutionAuditRepository,
        ExecutionPlatformError, ExecutionQueueItem, ExecutionQueueItemId, ExecutionQueueRepository,
        ExecutionRetryPolicy,
    },
    runtime_domain::{RuntimeExecutionId, RuntimeExecutionState},
    runtime_execution::{ExecutionPipeline, RuntimeExecutionError},
};

pub trait RetryExecutionIdFactory: Send + Sync {
    fn next_id(
        &self,
        prior: &RuntimeExecutionId,
        next_attempt: u32,
    ) -> Result<RuntimeExecutionId, ExecutionPlatformError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionDispatchOutcome {
    Idle,
    Completed(RuntimeExecutionId),
    RetryScheduled {
        prior_execution_id: RuntimeExecutionId,
        next_execution_id: RuntimeExecutionId,
        available_at: i64,
    },
    DeadLetter(RuntimeExecutionId),
}

#[derive(Debug, Error)]
pub enum ExecutionPlatformServiceError {
    #[error(transparent)]
    Platform(#[from] ExecutionPlatformError),
    #[error(transparent)]
    Execution(#[from] RuntimeExecutionError),
    #[error("Execution pipeline returned no terminal result for {0}")]
    MissingTerminalResult(RuntimeExecutionId),
}

pub struct ExecutionPlatformService<Q, A, P, F> {
    queue: Q,
    audit: A,
    pipeline: P,
    ids: F,
    retry: ExecutionRetryPolicy,
}

impl<Q, A, P, F> ExecutionPlatformService<Q, A, P, F>
where
    Q: ExecutionQueueRepository,
    A: ExecutionAuditRepository,
    P: ExecutionPipeline,
    F: RetryExecutionIdFactory,
{
    pub fn new(queue: Q, audit: A, pipeline: P, ids: F, retry: ExecutionRetryPolicy) -> Self {
        Self {
            queue,
            audit,
            pipeline,
            ids,
            retry,
        }
    }

    pub fn submit(
        &self,
        queue_item_id: ExecutionQueueItemId,
        request: ExecutionRequest,
        priority: i32,
        available_at: i64,
    ) -> Result<ExecutionQueueItem, ExecutionPlatformServiceError> {
        let execution_id = request.execution_id().clone();
        let item = ExecutionQueueItem::pending(
            queue_item_id,
            request,
            priority,
            1,
            self.retry.max_attempts(),
            available_at,
            None,
            available_at,
        )?;
        let item = self.queue.enqueue(item)?;
        self.audit.append(Self::event(
            &execution_id,
            ExecutionAuditKind::Queued,
            1,
            available_at,
            "Execution attempt queued",
        )?)?;
        Ok(item)
    }

    pub fn dispatch_next(
        &self,
        worker: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<ExecutionDispatchOutcome, ExecutionPlatformServiceError> {
        let Some(item) = self.queue.lease_next(worker, now, lease_until)? else {
            return Ok(ExecutionDispatchOutcome::Idle);
        };
        let execution_id = item.request().execution_id().clone();
        self.audit.append(Self::event(
            &execution_id,
            ExecutionAuditKind::Leased,
            2,
            now,
            format!("Execution attempt leased by {worker}"),
        )?)?;
        self.audit.append(Self::event(
            &execution_id,
            ExecutionAuditKind::AttemptStarted,
            3,
            now,
            "Execution pipeline started",
        )?)?;

        // This is the sole productive call: the supplied pipeline retains its
        // admission gate and RuntimeExecutionAdapter boundary.
        let record = match self.pipeline.execute(item.request().clone(), now) {
            Ok(record) => record,
            Err(_) => {
                self.queue.dead_letter(item.id(), item.revision(), now)?;
                self.audit.append(Self::event(
                    &execution_id,
                    ExecutionAuditKind::DeadLetter,
                    4,
                    now,
                    "Execution pipeline failed before producing a terminal result",
                )?)?;
                return Ok(ExecutionDispatchOutcome::DeadLetter(execution_id));
            }
        };
        let Some(result) = record.result() else {
            self.queue.dead_letter(item.id(), item.revision(), now)?;
            self.audit.append(Self::event(
                &execution_id,
                ExecutionAuditKind::DeadLetter,
                4,
                now,
                "Execution pipeline returned no terminal result",
            )?)?;
            return Ok(ExecutionDispatchOutcome::DeadLetter(execution_id));
        };
        if result.state() == RuntimeExecutionState::Succeeded {
            self.queue.complete(item.id(), item.revision(), now)?;
            self.audit.append(Self::event(
                &execution_id,
                ExecutionAuditKind::Completed,
                4,
                now,
                "Execution attempt completed",
            )?)?;
            return Ok(ExecutionDispatchOutcome::Completed(execution_id));
        }

        let retry_safe = result.failure().is_some_and(|failure| failure.retry_safe());
        if self.retry.should_retry(item.attempt(), retry_safe) {
            self.queue.complete(item.id(), item.revision(), now)?;
            let next_attempt = item.attempt() + 1;
            let next_execution_id = self.ids.next_id(&execution_id, next_attempt)?;
            let available_at = now.saturating_add(self.retry.delay_seconds(next_attempt) as i64);
            let next_request = item
                .request()
                .retry_with(next_execution_id.clone(), now)
                .map_err(|error| ExecutionPlatformError::InvalidInput(error.to_string()))?;
            let next_item = ExecutionQueueItem::pending(
                ExecutionQueueItemId::new(format!("queue:{}", next_execution_id.as_str()))?,
                next_request,
                item.priority(),
                next_attempt,
                item.max_attempts(),
                available_at,
                Some(execution_id.clone()),
                now,
            )?;
            self.queue.enqueue(next_item)?;
            self.audit.append(Self::event(
                &execution_id,
                ExecutionAuditKind::RetryScheduled,
                4,
                now,
                format!("Retry {} scheduled as {}", next_attempt, next_execution_id),
            )?)?;
            self.audit.append(Self::event(
                &next_execution_id,
                ExecutionAuditKind::Queued,
                1,
                now,
                format!("Retry of {execution_id}"),
            )?)?;
            return Ok(ExecutionDispatchOutcome::RetryScheduled {
                prior_execution_id: execution_id,
                next_execution_id,
                available_at,
            });
        }

        self.queue.dead_letter(item.id(), item.revision(), now)?;
        self.audit.append(Self::event(
            &execution_id,
            ExecutionAuditKind::DeadLetter,
            4,
            now,
            "Execution attempt is not retryable or retry budget is exhausted",
        )?)?;
        Ok(ExecutionDispatchOutcome::DeadLetter(execution_id))
    }

    fn event(
        execution_id: &RuntimeExecutionId,
        kind: ExecutionAuditKind,
        sequence: u64,
        occurred_at: i64,
        details: impl Into<String>,
    ) -> Result<ExecutionAuditEvent, ExecutionPlatformError> {
        ExecutionAuditEvent::new(
            ExecutionAuditId::new(format!("audit:{}:{sequence}", execution_id.as_str()))?,
            execution_id.clone(),
            kind,
            sequence,
            occurred_at,
            details,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        capability_domain::CapabilitySnapshotId,
        database::Database,
        execution_domain::{
            ExecutionFailure, ExecutionFailureKind, ExecutionGovernanceEvidence,
            ExecutionModelBinding, ExecutionResult,
        },
        execution_platform::{
            ExecutionAuditRepository, ExecutionQueueRepository, SqliteExecutionPlatformRepository,
        },
        execution_repository::{ExecutionHistoryRepository, InMemoryExecutionHistoryRepository},
        model_domain::ModelId,
        permission_domain::{AuthorizationDecisionId, PermissionGrantId},
        role_domain::RoleAssignmentId,
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeBindingId, RuntimeBindingLifecycle,
            RuntimeId,
        },
    };

    fn request() -> ExecutionRequest {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding:platform").unwrap(),
            "agent:platform",
            RuntimeId::new("runtime:platform").unwrap(),
            1,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 2)
        .unwrap();
        ExecutionRequest::new(
            ExecutionContext::new(
                RuntimeExecutionId::new("execution:platform").unwrap(),
                binding,
                vec!["context:platform".to_string()],
                3,
            )
            .unwrap(),
            "objective",
            ExecutionModelBinding::runtime_local(ModelId::new("model:platform").unwrap()),
            ExecutionGovernanceEvidence::new(
                CapabilitySnapshotId::new("capability:snapshot").unwrap(),
                PermissionGrantId::new("permission:grant").unwrap(),
                RoleAssignmentId::new("assignment:one").unwrap(),
                AuthorizationDecisionId::new("decision:one").unwrap(),
            ),
            None,
            4,
        )
        .unwrap()
    }

    struct RetrySafeFailurePipeline {
        history: InMemoryExecutionHistoryRepository,
    }

    impl ExecutionPipeline for RetrySafeFailurePipeline {
        fn execute(
            &self,
            request: ExecutionRequest,
            occurred_at: i64,
        ) -> Result<crate::ExecutionRecord, RuntimeExecutionError> {
            let id = request.execution_id().clone();
            let accepted = self.history.accept(request)?;
            let preparing = self.history.transition(
                &id,
                RuntimeExecutionState::Preparing,
                accepted.revision(),
                occurred_at,
                "prepare",
            )?;
            let running = self.history.transition(
                &id,
                RuntimeExecutionState::Running,
                preparing.revision(),
                occurred_at,
                "run",
            )?;
            let failed = self.history.transition(
                &id,
                RuntimeExecutionState::Failed,
                running.revision(),
                occurred_at,
                "temporary",
            )?;
            let failure = ExecutionFailure::new(
                ExecutionFailureKind::InvocationFailed,
                "temporary",
                "temporary failure",
                true,
            )
            .unwrap();
            Ok(self.history.store_result(
                ExecutionResult::new(
                    id,
                    RuntimeExecutionState::Failed,
                    "temporary failure",
                    Vec::new(),
                    Some(failure),
                    occurred_at,
                )
                .unwrap(),
                failed.revision(),
            )?)
        }
    }

    struct SequentialIds;

    impl RetryExecutionIdFactory for SequentialIds {
        fn next_id(
            &self,
            prior: &RuntimeExecutionId,
            next_attempt: u32,
        ) -> Result<RuntimeExecutionId, ExecutionPlatformError> {
            RuntimeExecutionId::new(format!("{}:attempt:{next_attempt}", prior.as_str()))
                .map_err(|error| ExecutionPlatformError::InvalidInput(error.to_string()))
        }
    }

    #[test]
    fn dispatcher_uses_pipeline_and_schedules_new_execution_identity_for_retry() {
        let repository =
            SqliteExecutionPlatformRepository::new(Arc::new(Database::memory().unwrap()));
        let service = ExecutionPlatformService::new(
            repository.clone(),
            repository.clone(),
            RetrySafeFailurePipeline {
                history: InMemoryExecutionHistoryRepository::default(),
            },
            SequentialIds,
            ExecutionRetryPolicy::new(3, 5, 20).unwrap(),
        );
        service
            .submit(
                ExecutionQueueItemId::new("queue:platform").unwrap(),
                request(),
                5,
                4,
            )
            .unwrap();

        let outcome = service.dispatch_next("worker:one", 10, 20).unwrap();
        let ExecutionDispatchOutcome::RetryScheduled {
            prior_execution_id,
            next_execution_id,
            available_at,
        } = outcome
        else {
            panic!("retry must be scheduled");
        };
        assert_eq!(prior_execution_id.as_str(), "execution:platform");
        assert_eq!(next_execution_id.as_str(), "execution:platform:attempt:2");
        assert_eq!(available_at, 15);
        let retry = repository
            .get(&ExecutionQueueItemId::new("queue:execution:platform:attempt:2").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(retry.attempt(), 2);
        assert_eq!(retry.parent_execution_id(), Some(&prior_execution_id));
        assert_eq!(repository.list(&prior_execution_id).unwrap().len(), 4);
    }
}
