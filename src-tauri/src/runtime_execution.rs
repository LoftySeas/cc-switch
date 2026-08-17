//! Governed Runtime invocation boundary and in-process orchestration.
//!
//! The coordinator requires an admission gate before adapter invocation. Gate
//! implementations remain outside this milestone and may later be backed by
//! Capability and Permission services.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::{
    execution_domain::{ExecutionFailure, ExecutionFailureKind, ExecutionRequest, ExecutionResult},
    execution_repository::{ExecutionHistoryRepository, ExecutionRecord, ExecutionRepositoryError},
    runtime_adapter::{RuntimeAdapter, RuntimeAdapterError},
    runtime_domain::{RuntimeExecutionId, RuntimeExecutionState, RuntimeId},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAdmission {
    receipt_ref: String,
}

impl ExecutionAdmission {
    pub fn new(receipt_ref: impl Into<String>) -> Result<Self, RuntimeExecutionError> {
        let value = receipt_ref.into();
        let value = value.trim();
        if value.is_empty() {
            return Err(RuntimeExecutionError::InvalidAdmissionReceipt);
        }
        Ok(Self {
            receipt_ref: value.to_string(),
        })
    }
    pub fn receipt_ref(&self) -> &str {
        &self.receipt_ref
    }
}

pub trait ExecutionAdmissionGate: Send + Sync {
    fn admit(
        &self,
        request: &ExecutionRequest,
    ) -> Result<ExecutionAdmission, RuntimeExecutionError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInvocation {
    request: ExecutionRequest,
    admission: ExecutionAdmission,
}

impl RuntimeInvocation {
    pub fn request(&self) -> &ExecutionRequest {
        &self.request
    }
    pub fn admission(&self) -> &ExecutionAdmission {
        &self.admission
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInvocationOutput {
    pub summary: String,
    pub artifact_references: Vec<String>,
}

/// Execution-capable extension of the read-only Runtime Adapter contract. No
/// concrete Runtime is supplied by this milestone.
pub trait RuntimeExecutionAdapter: RuntimeAdapter {
    fn invoke(
        &self,
        invocation: &RuntimeInvocation,
    ) -> Result<RuntimeInvocationOutput, RuntimeExecutionError>;
}

pub trait RuntimeExecutionAdapterRepository: Send + Sync {
    fn register(
        &self,
        adapter: Arc<dyn RuntimeExecutionAdapter>,
    ) -> Result<(), RuntimeExecutionError>;
    fn get(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Option<Arc<dyn RuntimeExecutionAdapter>>, RuntimeExecutionError>;
}

#[derive(Clone, Default)]
pub struct InMemoryRuntimeExecutionAdapterRepository {
    adapters: Arc<RwLock<HashMap<RuntimeId, Arc<dyn RuntimeExecutionAdapter>>>>,
}

impl RuntimeExecutionAdapterRepository for InMemoryRuntimeExecutionAdapterRepository {
    fn register(
        &self,
        adapter: Arc<dyn RuntimeExecutionAdapter>,
    ) -> Result<(), RuntimeExecutionError> {
        adapter
            .descriptor()
            .validate()
            .map_err(RuntimeAdapterError::from)?;
        let id = adapter.descriptor().runtime_id().clone();
        let mut adapters = self
            .adapters
            .write()
            .map_err(|e| RuntimeExecutionError::RegistryLock(e.to_string()))?;
        if adapters.contains_key(&id) {
            return Err(RuntimeExecutionError::AdapterAlreadyRegistered(id));
        }
        adapters.insert(id, adapter);
        Ok(())
    }

    fn get(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Option<Arc<dyn RuntimeExecutionAdapter>>, RuntimeExecutionError> {
        let adapters = self
            .adapters
            .read()
            .map_err(|e| RuntimeExecutionError::RegistryLock(e.to_string()))?;
        Ok(adapters.get(runtime_id).cloned())
    }
}

#[derive(Debug, Error)]
pub enum RuntimeExecutionError {
    #[error(transparent)]
    History(#[from] ExecutionRepositoryError),
    #[error(transparent)]
    Adapter(#[from] RuntimeAdapterError),
    #[error("Execution admission receipt is empty")]
    InvalidAdmissionReceipt,
    #[error("Execution admission was rejected: {0}")]
    AdmissionRejected(String),
    #[error("No execution adapter is registered for Runtime: {0}")]
    AdapterNotRegistered(RuntimeId),
    #[error("Execution adapter is already registered for Runtime: {0}")]
    AdapterAlreadyRegistered(RuntimeId),
    #[error("Runtime invocation failed: {0}")]
    InvocationFailed(String),
    #[error("Runtime execution adapter registry lock failed: {0}")]
    RegistryLock(String),
}

/// Runtime-neutral orchestration boundary for one execution attempt.
pub trait ExecutionPipeline: Send + Sync {
    fn execute(
        &self,
        request: ExecutionRequest,
        occurred_at: i64,
    ) -> Result<ExecutionRecord, RuntimeExecutionError>;
}

pub struct RuntimeExecutionCoordinator<H, A, G> {
    history: H,
    adapters: A,
    gate: G,
}

struct RuntimeFailure {
    kind: ExecutionFailureKind,
    code: &'static str,
    message: String,
    retry_safe: bool,
}

impl<H, A, G> RuntimeExecutionCoordinator<H, A, G>
where
    H: ExecutionHistoryRepository,
    A: RuntimeExecutionAdapterRepository,
    G: ExecutionAdmissionGate,
{
    pub fn new(history: H, adapters: A, gate: G) -> Self {
        Self {
            history,
            adapters,
            gate,
        }
    }

    fn execute_inner(
        &self,
        request: ExecutionRequest,
        now: i64,
    ) -> Result<ExecutionRecord, RuntimeExecutionError> {
        let execution_id = request.execution_id().clone();
        let accepted = self.history.accept(request.clone())?;
        let preparing = self.history.transition(
            &execution_id,
            RuntimeExecutionState::Preparing,
            accepted.revision(),
            now,
            "execution preparation started",
        )?;

        let admission = match self.gate.admit(&request) {
            Ok(admission) => admission,
            Err(error) => {
                return self.fail(
                    &execution_id,
                    preparing,
                    now,
                    RuntimeFailure {
                        kind: ExecutionFailureKind::AdmissionRejected,
                        code: "admission_rejected",
                        message: error.to_string(),
                        retry_safe: false,
                    },
                )
            }
        };

        let runtime_id = request.context().binding().runtime_id().clone();
        let adapter = match self.adapters.get(&runtime_id)? {
            Some(adapter) => adapter,
            None => {
                return self.fail(
                    &execution_id,
                    preparing,
                    now,
                    RuntimeFailure {
                        kind: ExecutionFailureKind::RuntimeUnavailable,
                        code: "runtime_adapter_unavailable",
                        message: format!(
                            "No execution adapter is registered for Runtime {runtime_id}"
                        ),
                        retry_safe: true,
                    },
                )
            }
        };

        if let Err(error) = adapter.validate_context(request.context()) {
            return self.fail(
                &execution_id,
                preparing,
                now,
                RuntimeFailure {
                    kind: ExecutionFailureKind::ContextRejected,
                    code: "runtime_context_rejected",
                    message: error.to_string(),
                    retry_safe: false,
                },
            );
        }

        let running = self.history.transition(
            &execution_id,
            RuntimeExecutionState::Running,
            preparing.revision(),
            now,
            "Runtime invocation started",
        )?;
        let invocation = RuntimeInvocation { request, admission };
        match adapter.invoke(&invocation) {
            Ok(output) => {
                let terminal = self.history.transition(
                    &execution_id,
                    RuntimeExecutionState::Succeeded,
                    running.revision(),
                    now,
                    "Runtime invocation succeeded",
                )?;
                let result = ExecutionResult::new(
                    execution_id,
                    RuntimeExecutionState::Succeeded,
                    output.summary,
                    output.artifact_references,
                    None,
                    now,
                )
                .map_err(ExecutionRepositoryError::from)?;
                Ok(self.history.store_result(result, terminal.revision())?)
            }
            Err(error) => self.fail(
                &execution_id,
                running,
                now,
                RuntimeFailure {
                    kind: ExecutionFailureKind::InvocationFailed,
                    code: "runtime_invocation_failed",
                    message: error.to_string(),
                    retry_safe: true,
                },
            ),
        }
    }

    fn fail(
        &self,
        execution_id: &RuntimeExecutionId,
        record: ExecutionRecord,
        now: i64,
        failure: RuntimeFailure,
    ) -> Result<ExecutionRecord, RuntimeExecutionError> {
        let terminal = self.history.transition(
            execution_id,
            RuntimeExecutionState::Failed,
            record.revision(),
            now,
            failure.code,
        )?;
        let domain_failure = ExecutionFailure::new(
            failure.kind,
            failure.code,
            failure.message.clone(),
            failure.retry_safe,
        )
        .map_err(ExecutionRepositoryError::from)?;
        let result = ExecutionResult::new(
            execution_id.clone(),
            RuntimeExecutionState::Failed,
            failure.message,
            Vec::new(),
            Some(domain_failure),
            now,
        )
        .map_err(ExecutionRepositoryError::from)?;
        Ok(self.history.store_result(result, terminal.revision())?)
    }
}

impl<H, A, G> ExecutionPipeline for RuntimeExecutionCoordinator<H, A, G>
where
    H: ExecutionHistoryRepository,
    A: RuntimeExecutionAdapterRepository,
    G: ExecutionAdmissionGate,
{
    fn execute(
        &self,
        request: ExecutionRequest,
        occurred_at: i64,
    ) -> Result<ExecutionRecord, RuntimeExecutionError> {
        self.execute_inner(request, occurred_at)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::{
        execution_domain::{
            ExecutionGovernanceEvidence, ExecutionModelBinding, ExecutionTransition,
        },
        execution_repository::InMemoryExecutionHistoryRepository,
        model_domain::ModelId,
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeAdapterId, RuntimeAvailability,
            RuntimeBindingId, RuntimeBindingLifecycle, RuntimeDescriptor, RuntimeProbe,
        },
    };

    struct Gate {
        allow: bool,
        calls: Arc<AtomicUsize>,
    }

    impl ExecutionAdmissionGate for Gate {
        fn admit(
            &self,
            request: &ExecutionRequest,
        ) -> Result<ExecutionAdmission, RuntimeExecutionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(!request
                .governance()
                .capability_snapshot_id()
                .as_str()
                .is_empty());
            assert!(!request
                .governance()
                .permission_grant_id()
                .as_str()
                .is_empty());
            if self.allow {
                ExecutionAdmission::new("admission:one")
            } else {
                Err(RuntimeExecutionError::AdmissionRejected(
                    "governance denied execution".into(),
                ))
            }
        }
    }

    struct StubAdapter {
        descriptor: RuntimeDescriptor,
        calls: Arc<AtomicUsize>,
        fail: bool,
    }

    impl RuntimeAdapter for StubAdapter {
        fn descriptor(&self) -> &RuntimeDescriptor {
            &self.descriptor
        }

        fn probe(&self) -> Result<RuntimeProbe, RuntimeAdapterError> {
            Ok(RuntimeProbe {
                runtime_id: self.descriptor.runtime_id().clone(),
                availability: RuntimeAvailability::Ready,
                runtime_version: None,
                capabilities: Vec::new(),
                diagnostics: Vec::new(),
            })
        }
    }

    impl RuntimeExecutionAdapter for StubAdapter {
        fn invoke(
            &self,
            invocation: &RuntimeInvocation,
        ) -> Result<RuntimeInvocationOutput, RuntimeExecutionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(invocation.admission().receipt_ref(), "admission:one");
            if self.fail {
                Err(RuntimeExecutionError::InvocationFailed(
                    "stub failed".into(),
                ))
            } else {
                Ok(RuntimeInvocationOutput {
                    summary: "stub completed".into(),
                    artifact_references: vec!["artifact:one".into()],
                })
            }
        }
    }

    fn request() -> ExecutionRequest {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding:one").unwrap(),
            "agent:one",
            RuntimeId::new("runtime:one").unwrap(),
            10,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 11)
        .unwrap();
        let context = ExecutionContext::new(
            RuntimeExecutionId::new("execution:one").unwrap(),
            binding,
            vec!["context:one".into()],
            12,
        )
        .unwrap();
        ExecutionRequest::new(
            context,
            "perform bounded work",
            ExecutionModelBinding::runtime_local(ModelId::new("model:one").unwrap()),
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

    fn adapter(calls: Arc<AtomicUsize>, fail: bool) -> Arc<dyn RuntimeExecutionAdapter> {
        Arc::new(StubAdapter {
            descriptor: RuntimeDescriptor::new(
                RuntimeId::new("runtime:one").unwrap(),
                RuntimeAdapterId::new("adapter:one").unwrap(),
                "Stub Runtime",
                1,
                Vec::new(),
            )
            .unwrap(),
            calls,
            fail,
        })
    }

    #[test]
    fn coordinator_admits_before_invocation_and_records_success_history() {
        let gate_calls = Arc::new(AtomicUsize::new(0));
        let adapter_calls = Arc::new(AtomicUsize::new(0));
        let adapters = InMemoryRuntimeExecutionAdapterRepository::default();
        adapters
            .register(adapter(adapter_calls.clone(), false))
            .unwrap();
        let coordinator = RuntimeExecutionCoordinator::new(
            InMemoryExecutionHistoryRepository::default(),
            adapters,
            Gate {
                allow: true,
                calls: gate_calls.clone(),
            },
        );

        let record = coordinator.execute(request(), 20).unwrap();

        assert_eq!(record.state(), RuntimeExecutionState::Succeeded);
        assert_eq!(
            record
                .transitions()
                .iter()
                .map(ExecutionTransition::to)
                .collect::<Vec<_>>(),
            vec![
                RuntimeExecutionState::Preparing,
                RuntimeExecutionState::Running,
                RuntimeExecutionState::Succeeded,
            ]
        );
        assert_eq!(
            record.result().unwrap().artifact_references(),
            &["artifact:one"]
        );
        assert_eq!(gate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(adapter_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn admission_rejection_never_invokes_runtime_and_is_a_terminal_record() {
        let gate_calls = Arc::new(AtomicUsize::new(0));
        let adapter_calls = Arc::new(AtomicUsize::new(0));
        let adapters = InMemoryRuntimeExecutionAdapterRepository::default();
        adapters
            .register(adapter(adapter_calls.clone(), false))
            .unwrap();
        let coordinator = RuntimeExecutionCoordinator::new(
            InMemoryExecutionHistoryRepository::default(),
            adapters,
            Gate {
                allow: false,
                calls: gate_calls.clone(),
            },
        );

        let record = coordinator.execute(request(), 20).unwrap();

        assert_eq!(record.state(), RuntimeExecutionState::Failed);
        assert_eq!(
            record.result().unwrap().failure().unwrap().kind(),
            &ExecutionFailureKind::AdmissionRejected
        );
        assert_eq!(gate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(adapter_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn adapter_failure_is_normalized_without_leaking_runtime_state() {
        let adapters = InMemoryRuntimeExecutionAdapterRepository::default();
        adapters
            .register(adapter(Arc::new(AtomicUsize::new(0)), true))
            .unwrap();
        let coordinator = RuntimeExecutionCoordinator::new(
            InMemoryExecutionHistoryRepository::default(),
            adapters,
            Gate {
                allow: true,
                calls: Arc::new(AtomicUsize::new(0)),
            },
        );

        let record = coordinator.execute(request(), 20).unwrap();

        assert_eq!(record.state(), RuntimeExecutionState::Failed);
        let failure = record.result().unwrap().failure().unwrap();
        assert_eq!(failure.kind(), &ExecutionFailureKind::InvocationFailed);
        assert!(failure.retry_safe());
    }

    #[test]
    fn missing_adapter_is_recorded_as_retry_safe_runtime_unavailability() {
        let coordinator = RuntimeExecutionCoordinator::new(
            InMemoryExecutionHistoryRepository::default(),
            InMemoryRuntimeExecutionAdapterRepository::default(),
            Gate {
                allow: true,
                calls: Arc::new(AtomicUsize::new(0)),
            },
        );

        let record = coordinator.execute(request(), 20).unwrap();

        assert_eq!(record.state(), RuntimeExecutionState::Failed);
        let failure = record.result().unwrap().failure().unwrap();
        assert_eq!(failure.kind(), &ExecutionFailureKind::RuntimeUnavailable);
        assert!(failure.retry_safe());
    }
}
