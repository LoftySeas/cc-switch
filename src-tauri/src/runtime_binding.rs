//! Runtime Binding aggregate repository.
//!
//! Bindings are independent, revisioned relationship objects. The repository
//! never deletes bindings and never mutates Agent or Runtime identity.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::runtime_domain::{
    AgentRuntimeBinding, RuntimeBindingId, RuntimeBindingLifecycle, RuntimeDomainError, RuntimeId,
};

#[derive(Debug, Error)]
pub enum RuntimeBindingError {
    #[error(transparent)]
    InvalidDomain(#[from] RuntimeDomainError),
    #[error("Runtime binding is already registered: {0}")]
    AlreadyRegistered(RuntimeBindingId),
    #[error("A non-retired Runtime binding already exists for Agent {agent_id} and Runtime {runtime_id}")]
    RelationshipAlreadyRegistered {
        agent_id: String,
        runtime_id: RuntimeId,
    },
    #[error("Runtime binding was not found: {0}")]
    NotFound(RuntimeBindingId),
    #[error("Runtime binding revision conflict for {binding_id}: expected {expected}, current {current}")]
    RevisionConflict {
        binding_id: RuntimeBindingId,
        expected: i64,
        current: i64,
    },
    #[error("Runtime binding identity changed during update: {0}")]
    IdentityChanged(RuntimeBindingId),
    #[error("Runtime binding update does not match a valid lifecycle transition: {0}")]
    InvalidUpdate(RuntimeBindingId),
    #[error("New Runtime binding must start as draft revision 1: {0}")]
    InvalidInitialState(RuntimeBindingId),
    #[error("Runtime binding registry lock failed: {0}")]
    RegistryLock(String),
    #[error("Agent lookup failed: {0}")]
    AgentLookup(String),
    #[error("Runtime lookup failed: {0}")]
    RuntimeLookup(String),
    #[error("Agent was not found for Runtime binding: {0}")]
    AgentNotFound(String),
    #[error("Retired Agent cannot receive a new Runtime binding: {0}")]
    AgentRetired(String),
    #[error("Runtime binding can activate only for an active Agent: {0}")]
    AgentNotActive(String),
    #[error("Runtime is not registered for binding: {0}")]
    RuntimeNotRegistered(RuntimeId),
}

pub trait RuntimeBindingRepository: Send + Sync {
    fn insert(&self, binding: AgentRuntimeBinding) -> Result<(), RuntimeBindingError>;
    fn get(
        &self,
        binding_id: &RuntimeBindingId,
    ) -> Result<Option<AgentRuntimeBinding>, RuntimeBindingError>;
    fn list(&self) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError>;
    fn list_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError>;
    fn list_for_runtime(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError>;
    fn update(
        &self,
        binding: AgentRuntimeBinding,
        expected_revision: i64,
    ) -> Result<(), RuntimeBindingError>;
}

#[derive(Clone, Default)]
pub struct InMemoryRuntimeBindingRepository {
    bindings: Arc<RwLock<HashMap<RuntimeBindingId, AgentRuntimeBinding>>>,
}

impl InMemoryRuntimeBindingRepository {
    fn sorted(bindings: impl Iterator<Item = AgentRuntimeBinding>) -> Vec<AgentRuntimeBinding> {
        let mut bindings = bindings.collect::<Vec<_>>();
        bindings.sort_by(|left, right| {
            left.created_at()
                .cmp(&right.created_at())
                .then_with(|| left.id().as_str().cmp(right.id().as_str()))
        });
        bindings
    }
}

impl RuntimeBindingRepository for InMemoryRuntimeBindingRepository {
    fn insert(&self, binding: AgentRuntimeBinding) -> Result<(), RuntimeBindingError> {
        binding.validate()?;
        if binding.lifecycle_state() != RuntimeBindingLifecycle::Draft
            || binding.revision() != 1
            || binding.created_at() != binding.updated_at()
        {
            return Err(RuntimeBindingError::InvalidInitialState(
                binding.id().clone(),
            ));
        }
        let mut bindings = self
            .bindings
            .write()
            .map_err(|error| RuntimeBindingError::RegistryLock(error.to_string()))?;
        if bindings.contains_key(binding.id()) {
            return Err(RuntimeBindingError::AlreadyRegistered(binding.id().clone()));
        }
        if bindings.values().any(|existing| {
            existing.agent_id() == binding.agent_id()
                && existing.runtime_id() == binding.runtime_id()
                && existing.lifecycle_state() != RuntimeBindingLifecycle::Retired
        }) {
            return Err(RuntimeBindingError::RelationshipAlreadyRegistered {
                agent_id: binding.agent_id().to_string(),
                runtime_id: binding.runtime_id().clone(),
            });
        }
        bindings.insert(binding.id().clone(), binding);
        Ok(())
    }

    fn get(
        &self,
        binding_id: &RuntimeBindingId,
    ) -> Result<Option<AgentRuntimeBinding>, RuntimeBindingError> {
        let bindings = self
            .bindings
            .read()
            .map_err(|error| RuntimeBindingError::RegistryLock(error.to_string()))?;
        Ok(bindings.get(binding_id).cloned())
    }

    fn list(&self) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError> {
        let bindings = self
            .bindings
            .read()
            .map_err(|error| RuntimeBindingError::RegistryLock(error.to_string()))?;
        Ok(Self::sorted(bindings.values().cloned()))
    }

    fn list_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError> {
        let bindings = self
            .bindings
            .read()
            .map_err(|error| RuntimeBindingError::RegistryLock(error.to_string()))?;
        Ok(Self::sorted(
            bindings
                .values()
                .filter(|binding| binding.agent_id() == agent_id)
                .cloned(),
        ))
    }

    fn list_for_runtime(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError> {
        let bindings = self
            .bindings
            .read()
            .map_err(|error| RuntimeBindingError::RegistryLock(error.to_string()))?;
        Ok(Self::sorted(
            bindings
                .values()
                .filter(|binding| binding.runtime_id() == runtime_id)
                .cloned(),
        ))
    }

    fn update(
        &self,
        binding: AgentRuntimeBinding,
        expected_revision: i64,
    ) -> Result<(), RuntimeBindingError> {
        binding.validate()?;
        let mut bindings = self
            .bindings
            .write()
            .map_err(|error| RuntimeBindingError::RegistryLock(error.to_string()))?;
        let current = bindings
            .get(binding.id())
            .ok_or_else(|| RuntimeBindingError::NotFound(binding.id().clone()))?;
        if current.revision() != expected_revision {
            return Err(RuntimeBindingError::RevisionConflict {
                binding_id: binding.id().clone(),
                expected: expected_revision,
                current: current.revision(),
            });
        }
        if current.agent_id() != binding.agent_id()
            || current.runtime_id() != binding.runtime_id()
            || current.created_at() != binding.created_at()
        {
            return Err(RuntimeBindingError::IdentityChanged(binding.id().clone()));
        }
        let expected_update = current.transition_to(
            binding.lifecycle_state(),
            expected_revision,
            binding.updated_at(),
        )?;
        if expected_update != binding {
            return Err(RuntimeBindingError::InvalidUpdate(binding.id().clone()));
        }
        bindings.insert(binding.id().clone(), binding);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(id: &str) -> AgentRuntimeBinding {
        AgentRuntimeBinding::new(
            RuntimeBindingId::new(id).expect("valid binding ID"),
            "agent-1",
            RuntimeId::new("runtime:test").expect("valid Runtime ID"),
            1_000,
        )
        .expect("valid binding")
    }

    #[test]
    fn repository_rejects_identity_mutation() -> Result<(), RuntimeBindingError> {
        let repository = InMemoryRuntimeBindingRepository::default();
        let original = binding("binding-1");
        repository.insert(original.clone())?;
        let changed = original.transition_to(RuntimeBindingLifecycle::Active, 1, 1_100)?;
        let mut serialized = serde_json::to_value(changed).expect("binding serializes");
        serialized["agentId"] = serde_json::Value::String("agent-2".to_string());
        let changed = serde_json::from_value(serialized).expect("binding deserializes");

        assert!(matches!(
            repository.update(changed, 1),
            Err(RuntimeBindingError::IdentityChanged(_))
        ));
        assert_eq!(repository.get(original.id())?, Some(original));
        Ok(())
    }

    #[test]
    fn retired_relationship_allows_new_independent_binding() -> Result<(), RuntimeBindingError> {
        let repository = InMemoryRuntimeBindingRepository::default();
        let original = binding("binding-1");
        repository.insert(original.clone())?;
        let retired = original.transition_to(RuntimeBindingLifecycle::Retired, 1, 1_100)?;
        repository.update(retired.clone(), 1)?;

        let replacement = binding("binding-2");
        repository.insert(replacement.clone())?;
        assert_eq!(repository.list()?, vec![retired, replacement]);
        Ok(())
    }

    #[test]
    fn repository_rejects_non_draft_initial_state() -> Result<(), RuntimeBindingError> {
        let repository = InMemoryRuntimeBindingRepository::default();
        let active =
            binding("binding-1").transition_to(RuntimeBindingLifecycle::Active, 1, 1_100)?;
        assert!(matches!(
            repository.insert(active),
            Err(RuntimeBindingError::InvalidInitialState(_))
        ));
        Ok(())
    }
}
