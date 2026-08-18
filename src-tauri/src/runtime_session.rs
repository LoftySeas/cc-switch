//! Adapter-owned Runtime Session boundary.
//!
//! A Runtime Session is an operational control relationship with one activated
//! Runtime instance. It is not an Agent, Execution, Provider, Model, native
//! conversation, or execution-history record.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    runtime_activation_adapter::{RuntimeActivationAdapterError, RuntimeLifecycleAdapter},
    runtime_domain::{RuntimeAdapterId, RuntimeId, RuntimeProbe},
    runtime_instance_domain::{RuntimeInstanceId, RuntimeInstanceLifecycle},
    runtime_instance_repository::{RuntimeInstanceRepository, RuntimeInstanceRepositoryError},
};

const MAX_ID_LENGTH: usize = 160;
const MAX_SESSION_REF_LENGTH: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeSessionId(String);

impl RuntimeSessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, RuntimeSessionError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty()
            || value.chars().count() > MAX_ID_LENGTH
            || value.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(RuntimeSessionError::InvalidIdentifier("Runtime Session ID"));
        }
        Ok(Self(value.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuntimeSessionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSessionHandle {
    session_ref: String,
}

impl RuntimeSessionHandle {
    pub fn new(value: impl Into<String>) -> Result<Self, RuntimeSessionError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty()
            || value.chars().count() > MAX_SESSION_REF_LENGTH
            || value.chars().any(char::is_control)
        {
            return Err(RuntimeSessionError::InvalidSessionReference);
        }
        Ok(Self {
            session_ref: value.to_string(),
        })
    }
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSessionLifecycle {
    Opening,
    Active,
    Closing,
    Closed,
    Failed,
}

impl RuntimeSessionLifecycle {
    fn can_transition_to(self, target: Self) -> bool {
        use RuntimeSessionLifecycle::*;
        matches!(
            (self, target),
            (Opening, Active | Failed) | (Active, Closing | Failed) | (Closing, Closed | Failed)
        )
    }
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSession {
    id: RuntimeSessionId,
    runtime_instance_id: RuntimeInstanceId,
    runtime_id: RuntimeId,
    adapter_id: RuntimeAdapterId,
    lifecycle: RuntimeSessionLifecycle,
    session_ref: Option<String>,
    last_probe: Option<RuntimeProbe>,
    revision: u64,
    created_at: i64,
    updated_at: i64,
}

impl RuntimeSession {
    pub fn new(
        id: RuntimeSessionId,
        runtime_instance_id: RuntimeInstanceId,
        runtime_id: RuntimeId,
        adapter_id: RuntimeAdapterId,
        created_at: i64,
    ) -> Result<Self, RuntimeSessionError> {
        if created_at < 0 {
            return Err(RuntimeSessionError::InvalidTimestamp);
        }
        if [
            id.as_str(),
            runtime_instance_id.as_str(),
            runtime_id.as_str(),
            adapter_id.as_str(),
        ]
        .into_iter()
        .collect::<HashSet<_>>()
        .len()
            != 4
        {
            return Err(RuntimeSessionError::InvalidIdentity);
        }
        Ok(Self {
            id,
            runtime_instance_id,
            runtime_id,
            adapter_id,
            lifecycle: RuntimeSessionLifecycle::Opening,
            session_ref: None,
            last_probe: None,
            revision: 1,
            created_at,
            updated_at: created_at,
        })
    }
    pub fn id(&self) -> &RuntimeSessionId {
        &self.id
    }
    pub fn runtime_instance_id(&self) -> &RuntimeInstanceId {
        &self.runtime_instance_id
    }
    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }
    pub fn adapter_id(&self) -> &RuntimeAdapterId {
        &self.adapter_id
    }
    pub fn lifecycle(&self) -> RuntimeSessionLifecycle {
        self.lifecycle
    }
    pub fn session_ref(&self) -> Option<&str> {
        self.session_ref.as_deref()
    }
    pub fn last_probe(&self) -> Option<&RuntimeProbe> {
        self.last_probe.as_ref()
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

    fn validate(&self) -> Result<(), RuntimeSessionError> {
        RuntimeSessionId::new(self.id.as_str())?;
        if self.created_at < 0 || self.updated_at < self.created_at || self.revision == 0 {
            return Err(RuntimeSessionError::InvalidTimestamp);
        }
        if [
            self.id.as_str(),
            self.runtime_instance_id.as_str(),
            self.runtime_id.as_str(),
            self.adapter_id.as_str(),
        ]
        .into_iter()
        .collect::<HashSet<_>>()
        .len()
            != 4
        {
            return Err(RuntimeSessionError::InvalidIdentity);
        }
        if let Some(session_ref) = self.session_ref.as_deref() {
            RuntimeSessionHandle::new(session_ref)?;
        }
        if let Some(probe) = self.last_probe.as_ref() {
            if probe.runtime_id != self.runtime_id {
                return Err(RuntimeSessionError::ProbeIdentityMismatch);
            }
            probe
                .validate()
                .map_err(|error| RuntimeSessionError::InvalidProbe(error.to_string()))?;
        }
        if matches!(
            self.lifecycle,
            RuntimeSessionLifecycle::Active
                | RuntimeSessionLifecycle::Closing
                | RuntimeSessionLifecycle::Closed
        ) && (self.session_ref.is_none() || self.last_probe.is_none())
        {
            return Err(RuntimeSessionError::InvalidInitialState);
        }
        Ok(())
    }

    fn transition(
        &self,
        target: RuntimeSessionLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, RuntimeSessionError> {
        if self.revision != expected_revision {
            return Err(RuntimeSessionError::RevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if updated_at < self.updated_at {
            return Err(RuntimeSessionError::InvalidTimestamp);
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(RuntimeSessionError::InvalidTransition {
                from: self.lifecycle,
                to: target,
            });
        }
        let mut updated = self.clone();
        updated.lifecycle = target;
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }
    fn activate(
        &self,
        handle: RuntimeSessionHandle,
        probe: RuntimeProbe,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, RuntimeSessionError> {
        if probe.runtime_id != self.runtime_id {
            return Err(RuntimeSessionError::ProbeIdentityMismatch);
        }
        probe
            .validate()
            .map_err(|error| RuntimeSessionError::InvalidProbe(error.to_string()))?;
        let mut updated = self.transition(
            RuntimeSessionLifecycle::Active,
            expected_revision,
            updated_at,
        )?;
        updated.session_ref = Some(handle.session_ref);
        updated.last_probe = Some(probe);
        Ok(updated)
    }
    fn record_probe(
        &self,
        probe: RuntimeProbe,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, RuntimeSessionError> {
        if self.lifecycle != RuntimeSessionLifecycle::Active {
            return Err(RuntimeSessionError::NotActive(self.id.clone()));
        }
        if self.revision != expected_revision {
            return Err(RuntimeSessionError::RevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if probe.runtime_id != self.runtime_id {
            return Err(RuntimeSessionError::ProbeIdentityMismatch);
        }
        if updated_at < self.updated_at {
            return Err(RuntimeSessionError::InvalidTimestamp);
        }
        probe
            .validate()
            .map_err(|error| RuntimeSessionError::InvalidProbe(error.to_string()))?;
        let mut updated = self.clone();
        updated.last_probe = Some(probe);
        updated.revision += 1;
        updated.updated_at = updated_at;
        Ok(updated)
    }
}

#[derive(Debug, Error)]
pub enum RuntimeSessionError {
    #[error("{0} is invalid")]
    InvalidIdentifier(&'static str),
    #[error("Runtime Session reference is invalid")]
    InvalidSessionReference,
    #[error("Runtime Session identities must remain distinct")]
    InvalidIdentity,
    #[error("Runtime Session state is internally inconsistent")]
    InvalidInitialState,
    #[error("Runtime Session timestamp order is invalid")]
    InvalidTimestamp,
    #[error("Invalid Runtime Session transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: RuntimeSessionLifecycle,
        to: RuntimeSessionLifecycle,
    },
    #[error("Runtime Session revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("Runtime Session is not active: {0}")]
    NotActive(RuntimeSessionId),
    #[error("Runtime Session was not found: {0}")]
    NotFound(RuntimeSessionId),
    #[error("Runtime Session is already registered: {0}")]
    AlreadyRegistered(RuntimeSessionId),
    #[error("Runtime Session identity changed: {0}")]
    IdentityChanged(RuntimeSessionId),
    #[error("Runtime instance is not active: {0}")]
    RuntimeInstanceNotActive(RuntimeInstanceId),
    #[error("Runtime Session probe identity does not match its Runtime")]
    ProbeIdentityMismatch,
    #[error("Runtime Session probe is invalid: {0}")]
    InvalidProbe(String),
    #[error("Runtime Session adapter is already registered: {0}")]
    AdapterAlreadyRegistered(RuntimeId),
    #[error("Runtime Session adapter was not found: {0}")]
    AdapterNotFound(RuntimeId),
    #[error("Runtime Session registry lock failed: {0}")]
    RegistryLock(String),
    #[error(transparent)]
    RuntimeRepository(#[from] RuntimeInstanceRepositoryError),
    #[error(transparent)]
    RuntimeAdapter(#[from] RuntimeActivationAdapterError),
}

pub trait RuntimeSessionRepository: Send + Sync {
    fn insert(&self, session: RuntimeSession) -> Result<(), RuntimeSessionError>;
    fn get(&self, id: &RuntimeSessionId) -> Result<Option<RuntimeSession>, RuntimeSessionError>;
    fn list(&self) -> Result<Vec<RuntimeSession>, RuntimeSessionError>;
    fn update(
        &self,
        session: RuntimeSession,
        expected_revision: u64,
    ) -> Result<(), RuntimeSessionError>;
}

#[derive(Clone, Default)]
pub struct InMemoryRuntimeSessionRepository {
    sessions: Arc<RwLock<HashMap<RuntimeSessionId, RuntimeSession>>>,
}

impl RuntimeSessionRepository for InMemoryRuntimeSessionRepository {
    fn insert(&self, session: RuntimeSession) -> Result<(), RuntimeSessionError> {
        session.validate()?;
        if session.lifecycle() != RuntimeSessionLifecycle::Opening || session.revision() != 1 {
            return Err(RuntimeSessionError::InvalidInitialState);
        }
        let mut sessions = self
            .sessions
            .write()
            .map_err(|e| RuntimeSessionError::RegistryLock(e.to_string()))?;
        if sessions.contains_key(session.id()) {
            return Err(RuntimeSessionError::AlreadyRegistered(session.id().clone()));
        }
        sessions.insert(session.id().clone(), session);
        Ok(())
    }
    fn get(&self, id: &RuntimeSessionId) -> Result<Option<RuntimeSession>, RuntimeSessionError> {
        Ok(self
            .sessions
            .read()
            .map_err(|e| RuntimeSessionError::RegistryLock(e.to_string()))?
            .get(id)
            .cloned())
    }
    fn list(&self) -> Result<Vec<RuntimeSession>, RuntimeSessionError> {
        let mut values = self
            .sessions
            .read()
            .map_err(|e| RuntimeSessionError::RegistryLock(e.to_string()))?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|a, b| a.id().as_str().cmp(b.id().as_str()));
        Ok(values)
    }
    fn update(
        &self,
        session: RuntimeSession,
        expected_revision: u64,
    ) -> Result<(), RuntimeSessionError> {
        session.validate()?;
        let mut sessions = self
            .sessions
            .write()
            .map_err(|e| RuntimeSessionError::RegistryLock(e.to_string()))?;
        let current = sessions
            .get(session.id())
            .ok_or_else(|| RuntimeSessionError::NotFound(session.id().clone()))?;
        if current.revision() != expected_revision || session.revision() != expected_revision + 1 {
            return Err(RuntimeSessionError::RevisionConflict {
                expected: expected_revision,
                current: current.revision(),
            });
        }
        if current.runtime_instance_id() != session.runtime_instance_id()
            || current.runtime_id() != session.runtime_id()
            || current.adapter_id() != session.adapter_id()
            || current.created_at() != session.created_at()
        {
            return Err(RuntimeSessionError::IdentityChanged(session.id().clone()));
        }
        sessions.insert(session.id().clone(), session);
        Ok(())
    }
}

pub trait RuntimeSessionAdapter: RuntimeLifecycleAdapter {
    fn open_session(
        &self,
        instance_id: &RuntimeInstanceId,
        session_id: &RuntimeSessionId,
    ) -> Result<RuntimeSessionHandle, RuntimeSessionError>;
    fn probe_session(
        &self,
        instance_id: &RuntimeInstanceId,
        handle: &RuntimeSessionHandle,
    ) -> Result<RuntimeProbe, RuntimeSessionError>;
    fn close_session(
        &self,
        instance_id: &RuntimeInstanceId,
        handle: &RuntimeSessionHandle,
    ) -> Result<(), RuntimeSessionError>;
}

pub trait RuntimeSessionAdapterRepository: Send + Sync {
    fn register(&self, adapter: Arc<dyn RuntimeSessionAdapter>) -> Result<(), RuntimeSessionError>;
    fn get(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Option<Arc<dyn RuntimeSessionAdapter>>, RuntimeSessionError>;
}

#[derive(Clone, Default)]
pub struct InMemoryRuntimeSessionAdapterRepository {
    adapters: Arc<RwLock<HashMap<RuntimeId, Arc<dyn RuntimeSessionAdapter>>>>,
}

impl RuntimeSessionAdapterRepository for InMemoryRuntimeSessionAdapterRepository {
    fn register(&self, adapter: Arc<dyn RuntimeSessionAdapter>) -> Result<(), RuntimeSessionError> {
        let id = adapter.descriptor().runtime_id().clone();
        adapter
            .descriptor()
            .validate()
            .map_err(|e| RuntimeSessionError::InvalidProbe(e.to_string()))?;
        let mut adapters = self
            .adapters
            .write()
            .map_err(|e| RuntimeSessionError::RegistryLock(e.to_string()))?;
        if adapters.contains_key(&id) {
            return Err(RuntimeSessionError::AdapterAlreadyRegistered(id));
        }
        adapters.insert(id, adapter);
        Ok(())
    }
    fn get(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Option<Arc<dyn RuntimeSessionAdapter>>, RuntimeSessionError> {
        Ok(self
            .adapters
            .read()
            .map_err(|e| RuntimeSessionError::RegistryLock(e.to_string()))?
            .get(runtime_id)
            .cloned())
    }
}

pub struct RuntimeSessionService<I, S, A> {
    instances: I,
    sessions: S,
    adapters: A,
}

impl<
        I: RuntimeInstanceRepository,
        S: RuntimeSessionRepository,
        A: RuntimeSessionAdapterRepository,
    > RuntimeSessionService<I, S, A>
{
    pub fn new(instances: I, sessions: S, adapters: A) -> Self {
        Self {
            instances,
            sessions,
            adapters,
        }
    }
    pub fn open(
        &self,
        id: RuntimeSessionId,
        instance_id: &RuntimeInstanceId,
        occurred_at: i64,
    ) -> Result<RuntimeSession, RuntimeSessionError> {
        let instance = self
            .instances
            .get(instance_id)?
            .ok_or_else(|| RuntimeSessionError::RuntimeInstanceNotActive(instance_id.clone()))?;
        if !matches!(
            instance.lifecycle(),
            RuntimeInstanceLifecycle::Ready | RuntimeInstanceLifecycle::Degraded
        ) {
            return Err(RuntimeSessionError::RuntimeInstanceNotActive(
                instance_id.clone(),
            ));
        }
        let adapter = self
            .adapters
            .get(instance.runtime_id())?
            .ok_or_else(|| RuntimeSessionError::AdapterNotFound(instance.runtime_id().clone()))?;
        if adapter.descriptor().adapter_id() != instance.adapter_id() {
            return Err(RuntimeSessionError::InvalidIdentity);
        }
        let opening = RuntimeSession::new(
            id,
            instance.id().clone(),
            instance.runtime_id().clone(),
            instance.adapter_id().clone(),
            occurred_at,
        )?;
        self.sessions.insert(opening.clone())?;
        let handle = match adapter.open_session(instance.id(), opening.id()) {
            Ok(handle) => handle,
            Err(error) => {
                let failed = opening.transition(RuntimeSessionLifecycle::Failed, 1, occurred_at)?;
                self.sessions.update(failed, 1)?;
                return Err(error);
            }
        };
        let probe = match adapter.probe_session(instance.id(), &handle) {
            Ok(probe) => probe,
            Err(error) => {
                let _ = adapter.close_session(instance.id(), &handle);
                let failed = opening.transition(RuntimeSessionLifecycle::Failed, 1, occurred_at)?;
                self.sessions.update(failed, 1)?;
                return Err(error);
            }
        };
        let active = match opening.activate(handle.clone(), probe, 1, occurred_at) {
            Ok(active) => active,
            Err(error) => {
                let _ = adapter.close_session(instance.id(), &handle);
                let failed = opening.transition(RuntimeSessionLifecycle::Failed, 1, occurred_at)?;
                self.sessions.update(failed, 1)?;
                return Err(error);
            }
        };
        if let Err(error) = self.sessions.update(active.clone(), 1) {
            let _ = adapter.close_session(instance.id(), &handle);
            return Err(error);
        }
        Ok(active)
    }
    pub fn refresh(
        &self,
        id: &RuntimeSessionId,
        expected_revision: u64,
        observed_at: i64,
    ) -> Result<RuntimeSession, RuntimeSessionError> {
        let current = self.require(id)?;
        if current.revision() != expected_revision {
            return Err(RuntimeSessionError::RevisionConflict {
                expected: expected_revision,
                current: current.revision(),
            });
        }
        let handle = RuntimeSessionHandle::new(
            current
                .session_ref()
                .ok_or_else(|| RuntimeSessionError::NotActive(id.clone()))?,
        )?;
        let adapter = self.require_adapter(current.runtime_id())?;
        let probe = adapter.probe_session(current.runtime_instance_id(), &handle)?;
        let updated = current.record_probe(probe, expected_revision, observed_at)?;
        self.sessions.update(updated.clone(), expected_revision)?;
        Ok(updated)
    }
    pub fn close(
        &self,
        id: &RuntimeSessionId,
        expected_revision: u64,
        occurred_at: i64,
    ) -> Result<RuntimeSession, RuntimeSessionError> {
        let current = self.require(id)?;
        let handle = RuntimeSessionHandle::new(
            current
                .session_ref()
                .ok_or_else(|| RuntimeSessionError::NotActive(id.clone()))?,
        )?;
        let adapter = self.require_adapter(current.runtime_id())?;
        let closing = current.transition(
            RuntimeSessionLifecycle::Closing,
            expected_revision,
            occurred_at,
        )?;
        self.sessions.update(closing.clone(), expected_revision)?;
        if let Err(error) = adapter.close_session(closing.runtime_instance_id(), &handle) {
            let failed = closing.transition(
                RuntimeSessionLifecycle::Failed,
                closing.revision(),
                occurred_at,
            )?;
            self.sessions.update(failed, closing.revision())?;
            return Err(error);
        }
        let closed = closing.transition(
            RuntimeSessionLifecycle::Closed,
            closing.revision(),
            occurred_at,
        )?;
        self.sessions.update(closed.clone(), closing.revision())?;
        Ok(closed)
    }
    pub fn get(&self, id: &RuntimeSessionId) -> Result<RuntimeSession, RuntimeSessionError> {
        self.require(id)
    }
    pub fn list(&self) -> Result<Vec<RuntimeSession>, RuntimeSessionError> {
        self.sessions.list()
    }
    fn require(&self, id: &RuntimeSessionId) -> Result<RuntimeSession, RuntimeSessionError> {
        self.sessions
            .get(id)?
            .ok_or_else(|| RuntimeSessionError::NotFound(id.clone()))
    }
    fn require_adapter(
        &self,
        id: &RuntimeId,
    ) -> Result<Arc<dyn RuntimeSessionAdapter>, RuntimeSessionError> {
        self.adapters
            .get(id)?
            .ok_or_else(|| RuntimeSessionError::AdapterNotFound(id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{Arc, RwLock},
    };

    use super::*;
    use crate::{
        runtime_activation_adapter::{
            CommandRuntimeAdapter, CommandRuntimeHost, CommandRuntimeInput, CommandRuntimeOutput,
            CommandRuntimeProbe, CommandRuntimeSpec, InMemoryRuntimeLifecycleAdapterRepository,
            RuntimeActivationAdapterError, RuntimeLifecycleAdapterRepository,
        },
        runtime_domain::{
            RuntimeAdapterId, RuntimeAvailability, RuntimeCapability, RuntimeDescriptor,
        },
        runtime_instance_domain::RuntimeInstance,
        runtime_instance_repository::InMemoryRuntimeInstanceRepository,
        services::runtime_activation::RuntimeActivationService,
    };

    struct FakeHost {
        availability: RwLock<RuntimeAvailability>,
    }

    impl CommandRuntimeHost for FakeHost {
        fn probe(
            &self,
            _spec: &CommandRuntimeSpec,
        ) -> Result<CommandRuntimeProbe, RuntimeActivationAdapterError> {
            Ok(CommandRuntimeProbe {
                availability: *self.availability.read().unwrap(),
                runtime_version: Some("cod-025-test".into()),
                diagnostics: Vec::new(),
            })
        }

        fn execute(
            &self,
            _spec: &CommandRuntimeSpec,
            _input: &CommandRuntimeInput,
        ) -> Result<CommandRuntimeOutput, RuntimeActivationAdapterError> {
            unreachable!("Runtime Session lifecycle does not execute work")
        }
    }

    fn runtime_descriptor() -> RuntimeDescriptor {
        RuntimeDescriptor::new(
            RuntimeId::new("runtime:cod-025").unwrap(),
            RuntimeAdapterId::new("adapter:cod-025").unwrap(),
            "COD-025 Test Runtime",
            1,
            vec![RuntimeCapability::new("session:control", 1).unwrap()],
        )
        .unwrap()
    }

    fn command_adapter(host: Arc<FakeHost>) -> Arc<CommandRuntimeAdapter<FakeHost>> {
        Arc::new(
            CommandRuntimeAdapter::new(
                runtime_descriptor(),
                CommandRuntimeSpec::new(
                    std::env::current_exe()
                        .unwrap_or_else(|_| PathBuf::from("/cod-025-test-runtime")),
                    Vec::new(),
                    Vec::new(),
                    None,
                    Vec::new(),
                    Some(4096),
                )
                .unwrap(),
                host,
            )
            .unwrap(),
        )
    }

    fn runtime_instance() -> RuntimeInstance {
        RuntimeInstance::new(
            RuntimeInstanceId::new("instance:cod-025").unwrap(),
            RuntimeId::new("runtime:cod-025").unwrap(),
            RuntimeAdapterId::new("adapter:cod-025").unwrap(),
            10,
        )
        .unwrap()
    }

    #[test]
    fn command_adapter_manages_session_lifecycle_and_capability_probe() {
        let host = Arc::new(FakeHost {
            availability: RwLock::new(RuntimeAvailability::Ready),
        });
        let adapter = command_adapter(host.clone());
        let instances = InMemoryRuntimeInstanceRepository::default();
        let lifecycle_adapters = InMemoryRuntimeLifecycleAdapterRepository::default();
        lifecycle_adapters.register(adapter.clone()).unwrap();
        let activation = RuntimeActivationService::new(instances.clone(), lifecycle_adapters);
        activation.register_instance(runtime_instance()).unwrap();
        let active_instance = activation
            .activate(&RuntimeInstanceId::new("instance:cod-025").unwrap(), 1, 11)
            .unwrap();

        let session_adapters = InMemoryRuntimeSessionAdapterRepository::default();
        session_adapters.register(adapter).unwrap();
        let service = RuntimeSessionService::new(
            instances,
            InMemoryRuntimeSessionRepository::default(),
            session_adapters,
        );
        let opened = service
            .open(
                RuntimeSessionId::new("session:cod-025").unwrap(),
                active_instance.id(),
                12,
            )
            .unwrap();

        assert_eq!(opened.lifecycle(), RuntimeSessionLifecycle::Active);
        assert!(opened
            .session_ref()
            .unwrap()
            .starts_with("command-runtime-session:"));
        assert_eq!(opened.last_probe().unwrap().capabilities.len(), 1);
        assert_eq!(
            opened.last_probe().unwrap().runtime_version.as_deref(),
            Some("cod-025-test")
        );

        *host.availability.write().unwrap() = RuntimeAvailability::Degraded;
        let refreshed = service.refresh(opened.id(), opened.revision(), 13).unwrap();
        assert_eq!(
            refreshed.last_probe().unwrap().availability,
            RuntimeAvailability::Degraded
        );

        let closed = service
            .close(refreshed.id(), refreshed.revision(), 14)
            .unwrap();
        assert_eq!(closed.lifecycle(), RuntimeSessionLifecycle::Closed);
        assert!(closed.lifecycle().is_terminal());
    }

    #[test]
    fn session_requires_an_active_runtime_instance() {
        let instances = InMemoryRuntimeInstanceRepository::default();
        instances.insert(runtime_instance()).unwrap();
        let session_adapters = InMemoryRuntimeSessionAdapterRepository::default();
        session_adapters
            .register(command_adapter(Arc::new(FakeHost {
                availability: RwLock::new(RuntimeAvailability::Ready),
            })))
            .unwrap();
        let service = RuntimeSessionService::new(
            instances,
            InMemoryRuntimeSessionRepository::default(),
            session_adapters,
        );

        assert!(matches!(
            service.open(
                RuntimeSessionId::new("session:inactive").unwrap(),
                &RuntimeInstanceId::new("instance:cod-025").unwrap(),
                11,
            ),
            Err(RuntimeSessionError::RuntimeInstanceNotActive(_))
        ));
    }

    #[test]
    fn repository_rejects_stale_revision_and_identity_mutation() {
        let repository = InMemoryRuntimeSessionRepository::default();
        let session = RuntimeSession::new(
            RuntimeSessionId::new("session:repository").unwrap(),
            RuntimeInstanceId::new("instance:repository").unwrap(),
            RuntimeId::new("runtime:repository").unwrap(),
            RuntimeAdapterId::new("adapter:repository").unwrap(),
            10,
        )
        .unwrap();
        repository.insert(session.clone()).unwrap();
        let failed = session
            .transition(RuntimeSessionLifecycle::Failed, 1, 11)
            .unwrap();
        repository.update(failed.clone(), 1).unwrap();

        assert!(matches!(
            repository.update(failed, 1),
            Err(RuntimeSessionError::RevisionConflict { .. })
        ));
    }
}
