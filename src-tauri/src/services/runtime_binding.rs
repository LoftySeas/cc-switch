//! Runtime Binding management service.
//!
//! The service validates Agent and Runtime identities and manages only the
//! relationship lifecycle. It exposes no Runtime execution operation.

use crate::agent_domain::AgentLifecycle;
use crate::database::Database;
use crate::runtime_adapter::RuntimeAdapterRepository;
use crate::runtime_binding::{RuntimeBindingError, RuntimeBindingRepository};
use crate::runtime_domain::{
    AgentRuntimeBinding, RuntimeBindingId, RuntimeBindingLifecycle, RuntimeId,
};

pub struct RuntimeBindingService<B, R> {
    bindings: B,
    runtimes: R,
}

impl<B, R> RuntimeBindingService<B, R>
where
    B: RuntimeBindingRepository,
    R: RuntimeAdapterRepository,
{
    pub fn new(bindings: B, runtimes: R) -> Self {
        Self { bindings, runtimes }
    }

    pub fn create(
        &self,
        db: &Database,
        agent_id: &str,
        runtime_id: &RuntimeId,
    ) -> Result<AgentRuntimeBinding, RuntimeBindingError> {
        let agent = db
            .get_agent(agent_id)
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?
            .ok_or_else(|| RuntimeBindingError::AgentNotFound(agent_id.to_string()))?;
        if agent.lifecycle_state == AgentLifecycle::Retired {
            return Err(RuntimeBindingError::AgentRetired(agent_id.to_string()));
        }
        self.require_runtime(runtime_id)?;

        let now = chrono::Utc::now().timestamp_millis();
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new(uuid::Uuid::new_v4().to_string())?,
            agent.id,
            runtime_id.clone(),
            now,
        )?;
        self.bindings.insert(binding.clone())?;
        Ok(binding)
    }

    pub fn get(
        &self,
        binding_id: &RuntimeBindingId,
    ) -> Result<AgentRuntimeBinding, RuntimeBindingError> {
        self.bindings
            .get(binding_id)?
            .ok_or_else(|| RuntimeBindingError::NotFound(binding_id.clone()))
    }

    pub fn list(&self) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError> {
        self.bindings.list()
    }

    pub fn list_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError> {
        self.bindings.list_for_agent(agent_id)
    }

    pub fn list_for_runtime(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Vec<AgentRuntimeBinding>, RuntimeBindingError> {
        self.bindings.list_for_runtime(runtime_id)
    }

    pub fn set_lifecycle(
        &self,
        db: &Database,
        binding_id: &RuntimeBindingId,
        target: RuntimeBindingLifecycle,
        expected_revision: i64,
    ) -> Result<AgentRuntimeBinding, RuntimeBindingError> {
        let binding = self.get(binding_id)?;
        let updated = binding.transition_to(
            target,
            expected_revision,
            chrono::Utc::now().timestamp_millis(),
        )?;
        if updated == binding {
            return Ok(updated);
        }
        if target == RuntimeBindingLifecycle::Active {
            self.validate_activation(db, &binding)?;
        }
        self.bindings.update(updated.clone(), expected_revision)?;
        Ok(updated)
    }

    pub fn validate_identity(
        &self,
        db: &Database,
        binding_id: &RuntimeBindingId,
    ) -> Result<AgentRuntimeBinding, RuntimeBindingError> {
        let binding = self.get(binding_id)?;
        binding.validate()?;
        db.get_agent(binding.agent_id())
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?
            .ok_or_else(|| RuntimeBindingError::AgentNotFound(binding.agent_id().to_string()))?;
        self.require_runtime(binding.runtime_id())?;
        Ok(binding)
    }

    fn validate_activation(
        &self,
        db: &Database,
        binding: &AgentRuntimeBinding,
    ) -> Result<(), RuntimeBindingError> {
        let agent = db
            .get_agent(binding.agent_id())
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?
            .ok_or_else(|| RuntimeBindingError::AgentNotFound(binding.agent_id().to_string()))?;
        if agent.lifecycle_state != AgentLifecycle::Active {
            return Err(RuntimeBindingError::AgentNotActive(
                binding.agent_id().to_string(),
            ));
        }
        self.require_runtime(binding.runtime_id())
    }

    fn require_runtime(&self, runtime_id: &RuntimeId) -> Result<(), RuntimeBindingError> {
        if self
            .runtimes
            .get(runtime_id)
            .map_err(|error| RuntimeBindingError::RuntimeLookup(error.to_string()))?
            .is_none()
        {
            return Err(RuntimeBindingError::RuntimeNotRegistered(
                runtime_id.clone(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::agent_domain::{CreateAgentInput, UpdateAgentInput};
    use crate::runtime_adapter::{
        InMemoryRuntimeAdapterRepository, RuntimeAdapter, RuntimeAdapterError,
        RuntimeAdapterRepository,
    };
    use crate::runtime_binding::InMemoryRuntimeBindingRepository;
    use crate::runtime_domain::{
        RuntimeAdapterId, RuntimeAvailability, RuntimeCapability, RuntimeDescriptor, RuntimeProbe,
    };
    use crate::services::agent::AgentService;

    struct StubRuntimeAdapter {
        descriptor: RuntimeDescriptor,
    }

    impl StubRuntimeAdapter {
        fn new(runtime_id: RuntimeId) -> Self {
            Self {
                descriptor:
                    RuntimeDescriptor::new(
                        runtime_id,
                        RuntimeAdapterId::new("adapter:test").expect("valid adapter ID"),
                        "Binding test Runtime",
                        1,
                        vec![RuntimeCapability::new("binding:validation", 1)
                            .expect("valid capability")],
                    )
                    .expect("valid descriptor"),
            }
        }
    }

    impl RuntimeAdapter for StubRuntimeAdapter {
        fn descriptor(&self) -> &RuntimeDescriptor {
            &self.descriptor
        }

        fn probe(&self) -> Result<RuntimeProbe, RuntimeAdapterError> {
            Ok(RuntimeProbe {
                runtime_id: self.descriptor.runtime_id().clone(),
                availability: RuntimeAvailability::Ready,
                runtime_version: None,
                capabilities: vec![],
                diagnostics: vec![],
            })
        }
    }

    type TestService =
        RuntimeBindingService<InMemoryRuntimeBindingRepository, InMemoryRuntimeAdapterRepository>;

    fn setup() -> Result<(Database, TestService, String, RuntimeId), RuntimeBindingError> {
        let db = Database::memory().map_err(|error| {
            RuntimeBindingError::AgentLookup(format!("database setup failed: {error}"))
        })?;
        let agent = AgentService::create(
            &db,
            CreateAgentInput {
                name: "Binding Agent".to_string(),
                description: String::new(),
                owner: "local-user".to_string(),
            },
        )
        .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;
        let runtime_id = RuntimeId::new("runtime:test")?;
        let runtimes = InMemoryRuntimeAdapterRepository::default();
        runtimes
            .register(Arc::new(StubRuntimeAdapter::new(runtime_id.clone())))
            .map_err(|error| RuntimeBindingError::RuntimeLookup(error.to_string()))?;
        let service =
            RuntimeBindingService::new(InMemoryRuntimeBindingRepository::default(), runtimes);
        Ok((db, service, agent.id, runtime_id))
    }

    #[test]
    fn creates_independent_binding_and_supports_both_lookup_directions(
    ) -> Result<(), RuntimeBindingError> {
        let (db, service, agent_id, runtime_id) = setup()?;
        let binding = service.create(&db, &agent_id, &runtime_id)?;

        assert_ne!(binding.id().as_str(), agent_id);
        assert_ne!(binding.id().as_str(), runtime_id.as_str());
        assert_eq!(binding.lifecycle_state(), RuntimeBindingLifecycle::Draft);
        assert_eq!(service.get(binding.id())?, binding);
        assert_eq!(service.list_for_agent(&agent_id)?, vec![binding.clone()]);
        assert_eq!(service.list_for_runtime(&runtime_id)?, vec![binding]);
        Ok(())
    }

    #[test]
    fn activation_requires_active_agent_and_registered_runtime() -> Result<(), RuntimeBindingError>
    {
        let (db, service, agent_id, runtime_id) = setup()?;
        let binding = service.create(&db, &agent_id, &runtime_id)?;
        assert!(matches!(
            service.set_lifecycle(
                &db,
                binding.id(),
                RuntimeBindingLifecycle::Active,
                binding.revision(),
            ),
            Err(RuntimeBindingError::AgentNotActive(_))
        ));

        let agent = AgentService::get(&db, &agent_id)
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;
        AgentService::set_lifecycle(&db, &agent_id, AgentLifecycle::Active, agent.revision)
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;
        let active = service.set_lifecycle(
            &db,
            binding.id(),
            RuntimeBindingLifecycle::Active,
            binding.revision(),
        )?;
        assert_eq!(active.lifecycle_state(), RuntimeBindingLifecycle::Active);
        Ok(())
    }

    #[test]
    fn lifecycle_is_revision_guarded_and_retirement_is_terminal() -> Result<(), RuntimeBindingError>
    {
        let (db, service, agent_id, runtime_id) = setup()?;
        let agent = AgentService::get(&db, &agent_id)
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;
        AgentService::set_lifecycle(&db, &agent_id, AgentLifecycle::Active, agent.revision)
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;
        let binding = service.create(&db, &agent_id, &runtime_id)?;
        let active =
            service.set_lifecycle(&db, binding.id(), RuntimeBindingLifecycle::Active, 1)?;
        assert!(service
            .set_lifecycle(&db, binding.id(), RuntimeBindingLifecycle::Suspended, 1,)
            .is_err());
        let retired = service.set_lifecycle(
            &db,
            binding.id(),
            RuntimeBindingLifecycle::Retired,
            active.revision(),
        )?;
        assert!(retired.lifecycle_state().is_terminal());
        assert!(service
            .set_lifecycle(
                &db,
                binding.id(),
                RuntimeBindingLifecycle::Active,
                retired.revision(),
            )
            .is_err());
        assert_eq!(service.list()?.len(), 1);
        Ok(())
    }

    #[test]
    fn creation_rejects_missing_identities_and_duplicate_live_relationships(
    ) -> Result<(), RuntimeBindingError> {
        let (db, service, agent_id, runtime_id) = setup()?;
        service.create(&db, &agent_id, &runtime_id)?;
        assert!(matches!(
            service.create(&db, &agent_id, &runtime_id),
            Err(RuntimeBindingError::RelationshipAlreadyRegistered { .. })
        ));
        assert!(matches!(
            service.create(&db, "agent:missing", &runtime_id),
            Err(RuntimeBindingError::AgentNotFound(_))
        ));
        assert!(matches!(
            service.create(
                &db,
                &agent_id,
                &RuntimeId::new("runtime:missing").expect("valid Runtime ID"),
            ),
            Err(RuntimeBindingError::RuntimeNotRegistered(_))
        ));
        Ok(())
    }

    #[test]
    fn metadata_updates_to_agent_do_not_change_binding_identity() -> Result<(), RuntimeBindingError>
    {
        let (db, service, agent_id, runtime_id) = setup()?;
        let binding = service.create(&db, &agent_id, &runtime_id)?;
        let agent = AgentService::get(&db, &agent_id)
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;
        AgentService::update(
            &db,
            &agent_id,
            UpdateAgentInput {
                expected_revision: agent.revision,
                name: Some("Renamed Agent".to_string()),
                description: None,
                owner: None,
            },
        )
        .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;

        let validated = service.validate_identity(&db, binding.id())?;
        assert_eq!(validated.agent_id(), agent_id);
        assert_eq!(validated.runtime_id(), &runtime_id);
        Ok(())
    }

    #[test]
    fn retired_agent_cannot_receive_new_binding() -> Result<(), RuntimeBindingError> {
        let (db, service, agent_id, runtime_id) = setup()?;
        let agent = AgentService::get(&db, &agent_id)
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;
        AgentService::set_lifecycle(&db, &agent_id, AgentLifecycle::Retired, agent.revision)
            .map_err(|error| RuntimeBindingError::AgentLookup(error.to_string()))?;

        assert!(matches!(
            service.create(&db, &agent_id, &runtime_id),
            Err(RuntimeBindingError::AgentRetired(_))
        ));
        Ok(())
    }
}
