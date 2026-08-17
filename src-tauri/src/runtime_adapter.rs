//! Extensible Runtime Adapter contract and repository boundary.
//!
//! The foundation contract exposes description, read-only probing, and context
//! validation. The separate `runtime_execution` module owns the governed
//! invocation extension introduced by the execution milestone.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::runtime_domain::{
    ExecutionContext, RuntimeDescriptor, RuntimeDomainError, RuntimeId, RuntimeProbe,
};

#[derive(Debug, Error)]
pub enum RuntimeAdapterError {
    #[error(transparent)]
    InvalidDomain(#[from] RuntimeDomainError),
    #[error("Runtime adapter is already registered: {0}")]
    AlreadyRegistered(RuntimeId),
    #[error("Runtime adapter is not registered: {0}")]
    NotRegistered(RuntimeId),
    #[error("Runtime adapter lock failed: {0}")]
    RegistryLock(String),
    #[error("Runtime adapter returned identity {observed} for requested Runtime {expected}")]
    IdentityMismatch {
        expected: RuntimeId,
        observed: RuntimeId,
    },
    #[error("Runtime adapter {runtime_id} rejected context: {message}")]
    ContextRejected {
        runtime_id: RuntimeId,
        message: String,
    },
    #[error("Runtime adapter {runtime_id} probe failed: {message}")]
    ProbeFailed {
        runtime_id: RuntimeId,
        message: String,
    },
}

/// Runtime-neutral foundation point. Implementations describe and inspect one
/// runtime boundary; execution requires the separate governed extension.
pub trait RuntimeAdapter: Send + Sync {
    fn descriptor(&self) -> &RuntimeDescriptor;

    /// Observe availability without changing external configuration or state.
    fn probe(&self) -> Result<RuntimeProbe, RuntimeAdapterError>;

    /// Validate whether the adapter recognizes the resolved Runtime binding and
    /// bounded context. The default keeps identities aligned without execution.
    fn validate_context(&self, context: &ExecutionContext) -> Result<(), RuntimeAdapterError> {
        context.validate()?;
        let expected = self.descriptor().runtime_id();
        let observed = context.binding().runtime_id();
        if expected != observed {
            return Err(RuntimeAdapterError::IdentityMismatch {
                expected: expected.clone(),
                observed: observed.clone(),
            });
        }
        Ok(())
    }
}

/// Repository abstraction for Runtime adapters. Storage and plugin discovery
/// remain replaceable and are not part of Runtime identity.
pub trait RuntimeAdapterRepository: Send + Sync {
    fn register(&self, adapter: Arc<dyn RuntimeAdapter>) -> Result<(), RuntimeAdapterError>;
    fn get(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Option<Arc<dyn RuntimeAdapter>>, RuntimeAdapterError>;
    fn list(&self) -> Result<Vec<Arc<dyn RuntimeAdapter>>, RuntimeAdapterError>;
}

/// Process-local repository used by the foundation service and tests. It stores
/// adapter contracts only and contains no runtime-specific implementation.
#[derive(Clone, Default)]
pub struct InMemoryRuntimeAdapterRepository {
    adapters: Arc<RwLock<HashMap<RuntimeId, Arc<dyn RuntimeAdapter>>>>,
}

impl RuntimeAdapterRepository for InMemoryRuntimeAdapterRepository {
    fn register(&self, adapter: Arc<dyn RuntimeAdapter>) -> Result<(), RuntimeAdapterError> {
        adapter.descriptor().validate()?;
        let runtime_id = adapter.descriptor().runtime_id().clone();
        let mut adapters = self
            .adapters
            .write()
            .map_err(|error| RuntimeAdapterError::RegistryLock(error.to_string()))?;
        if adapters.contains_key(&runtime_id) {
            return Err(RuntimeAdapterError::AlreadyRegistered(runtime_id));
        }
        adapters.insert(runtime_id, adapter);
        Ok(())
    }

    fn get(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Option<Arc<dyn RuntimeAdapter>>, RuntimeAdapterError> {
        let adapters = self
            .adapters
            .read()
            .map_err(|error| RuntimeAdapterError::RegistryLock(error.to_string()))?;
        Ok(adapters.get(runtime_id).cloned())
    }

    fn list(&self) -> Result<Vec<Arc<dyn RuntimeAdapter>>, RuntimeAdapterError> {
        let adapters = self
            .adapters
            .read()
            .map_err(|error| RuntimeAdapterError::RegistryLock(error.to_string()))?;
        let mut adapters = adapters.values().cloned().collect::<Vec<_>>();
        adapters.sort_by(|left, right| {
            left.descriptor()
                .runtime_id()
                .as_str()
                .cmp(right.descriptor().runtime_id().as_str())
        });
        Ok(adapters)
    }
}
