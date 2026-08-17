//! Agent OS Model catalog service.
//!
//! The service validates explicit Provider–Model availability relationships. It
//! does not select, route, invoke, or bind a Model to an Agent.

use crate::agent_provider_adapter::AgentProviderAdapterRepository;
use crate::agent_provider_domain::AgentProviderId;
use crate::model_domain::{ModelAvailability, ModelAvailabilityId, ModelDescriptor, ModelId};
use crate::model_registry::{ModelRegistry, ModelRegistryError};

pub struct ModelCatalogService<M, P> {
    models: M,
    providers: P,
}

impl<M, P> ModelCatalogService<M, P>
where
    M: ModelRegistry,
    P: AgentProviderAdapterRepository,
{
    pub fn new(models: M, providers: P) -> Self {
        Self { models, providers }
    }

    pub fn register_model(&self, descriptor: ModelDescriptor) -> Result<(), ModelRegistryError> {
        self.models.register_model(descriptor)
    }

    pub fn get_model(&self, model_id: &ModelId) -> Result<ModelDescriptor, ModelRegistryError> {
        self.models
            .get_model(model_id)?
            .ok_or_else(|| ModelRegistryError::ModelNotFound(model_id.clone()))
    }

    pub fn list_models(&self) -> Result<Vec<ModelDescriptor>, ModelRegistryError> {
        self.models.list_models()
    }

    pub fn declare_provider_availability(
        &self,
        availability: ModelAvailability,
    ) -> Result<(), ModelRegistryError> {
        availability.validate()?;
        let provider_id = availability.provider_id();
        if self
            .providers
            .get(provider_id)
            .map_err(|error| ModelRegistryError::ProviderLookup(error.to_string()))?
            .is_none()
        {
            return Err(ModelRegistryError::ProviderNotFound(provider_id.clone()));
        }
        self.models.register_availability(availability)
    }

    pub fn get_availability(
        &self,
        availability_id: &ModelAvailabilityId,
    ) -> Result<ModelAvailability, ModelRegistryError> {
        self.models
            .get_availability(availability_id)?
            .ok_or_else(|| ModelRegistryError::AvailabilityNotFound(availability_id.clone()))
    }

    pub fn list_for_model(
        &self,
        model_id: &ModelId,
    ) -> Result<Vec<ModelAvailability>, ModelRegistryError> {
        self.models.list_for_model(model_id)
    }

    pub fn list_for_provider(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Vec<ModelAvailability>, ModelRegistryError> {
        self.models.list_for_provider(provider_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::agent_provider_adapter::{
        AgentProviderAdapter, AgentProviderAdapterError, AgentProviderAdapterRepository,
        InMemoryAgentProviderAdapterRepository,
    };
    use crate::agent_provider_domain::{
        AgentProviderAdapterId, AgentProviderDescriptor, ProviderAvailability, ProviderMetadata,
        ProviderProbe,
    };
    use crate::model_domain::{ModelAvailabilityStatus, ModelCapability, ModelMetadata};
    use crate::model_registry::InMemoryModelRegistry;

    struct StubProviderAdapter {
        descriptor: AgentProviderDescriptor,
    }

    impl StubProviderAdapter {
        fn new(id: &str) -> Self {
            Self {
                descriptor: AgentProviderDescriptor::new(
                    AgentProviderId::new(id).expect("valid Provider ID"),
                    AgentProviderAdapterId::new(format!("adapter:{id}")).expect("valid adapter ID"),
                    id,
                    1,
                    ProviderMetadata::default(),
                    vec![],
                )
                .expect("valid descriptor"),
            }
        }
    }

    impl AgentProviderAdapter for StubProviderAdapter {
        fn descriptor(&self) -> &AgentProviderDescriptor {
            &self.descriptor
        }

        fn probe(&self) -> Result<ProviderProbe, AgentProviderAdapterError> {
            Ok(ProviderProbe {
                provider_id: self.descriptor.provider_id().clone(),
                availability: ProviderAvailability::Registered,
                diagnostics: vec![],
            })
        }
    }

    type TestService =
        ModelCatalogService<InMemoryModelRegistry, InMemoryAgentProviderAdapterRepository>;

    fn setup() -> TestService {
        let providers = InMemoryAgentProviderAdapterRepository::default();
        providers
            .register(Arc::new(StubProviderAdapter::new("provider:one")))
            .expect("provider registers");
        ModelCatalogService::new(InMemoryModelRegistry::default(), providers)
    }

    fn descriptor(id: &str) -> ModelDescriptor {
        ModelDescriptor::new(
            ModelId::new(id).expect("valid model ID"),
            id,
            ModelMetadata::default(),
            vec![ModelCapability::new("text:generate", 1, BTreeMap::new())
                .expect("valid capability")],
        )
        .expect("valid descriptor")
    }

    fn availability(provider_id: &str) -> ModelAvailability {
        ModelAvailability::new(
            ModelAvailabilityId::new("availability:one").expect("valid availability ID"),
            ModelId::new("model:one").expect("valid model ID"),
            AgentProviderId::new(provider_id).expect("valid provider ID"),
            "native-model-one",
            ModelAvailabilityStatus::Declared,
            1_000,
        )
        .expect("valid availability")
    }

    #[test]
    fn availability_requires_independently_registered_provider_and_model() {
        let service = setup();
        assert!(matches!(
            service.declare_provider_availability(availability("provider:one")),
            Err(ModelRegistryError::ModelNotFound(_))
        ));

        service
            .register_model(descriptor("model:one"))
            .expect("model registers");
        service
            .declare_provider_availability(availability("provider:one"))
            .expect("availability registers");
        assert_eq!(
            service
                .list_for_provider(
                    &AgentProviderId::new("provider:one").expect("valid provider ID")
                )
                .expect("availability lists")
                .len(),
            1
        );
    }

    #[test]
    fn missing_provider_fails_without_routing_or_fallback() {
        let service = setup();
        service
            .register_model(descriptor("model:one"))
            .expect("model registers");

        assert!(matches!(
            service.declare_provider_availability(availability("provider:missing")),
            Err(ModelRegistryError::ProviderNotFound(_))
        ));
    }
}
