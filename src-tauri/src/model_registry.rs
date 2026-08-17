//! Model descriptor and availability registry boundary.
//!
//! The registry stores catalog facts only. It contains no selection, routing,
//! prompt, Provider API, or execution behavior.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::agent_provider_domain::{AgentProviderDomainError, AgentProviderId};
use crate::model_domain::{
    ModelAvailability, ModelAvailabilityId, ModelDescriptor, ModelDomainError, ModelId,
};

#[derive(Debug, Error)]
pub enum ModelRegistryError {
    #[error(transparent)]
    InvalidModel(#[from] ModelDomainError),
    #[error(transparent)]
    InvalidProvider(#[from] AgentProviderDomainError),
    #[error("Model is already registered: {0}")]
    ModelAlreadyRegistered(ModelId),
    #[error("Model was not found: {0}")]
    ModelNotFound(ModelId),
    #[error("Model availability is already registered: {0}")]
    AvailabilityAlreadyRegistered(ModelAvailabilityId),
    #[error("Model availability was not found: {0}")]
    AvailabilityNotFound(ModelAvailabilityId),
    #[error("Provider is not registered for Model availability: {0}")]
    ProviderNotFound(AgentProviderId),
    #[error("The Provider Model reference is already registered for Provider {provider_id}: {provider_model_reference}")]
    ProviderModelReferenceAlreadyRegistered {
        provider_id: AgentProviderId,
        provider_model_reference: String,
    },
    #[error("Model registry lock failed: {0}")]
    RegistryLock(String),
    #[error("Provider registry lookup failed: {0}")]
    ProviderLookup(String),
}

pub trait ModelRegistry: Send + Sync {
    fn register_model(&self, descriptor: ModelDescriptor) -> Result<(), ModelRegistryError>;
    fn get_model(&self, model_id: &ModelId) -> Result<Option<ModelDescriptor>, ModelRegistryError>;
    fn list_models(&self) -> Result<Vec<ModelDescriptor>, ModelRegistryError>;
    fn register_availability(
        &self,
        availability: ModelAvailability,
    ) -> Result<(), ModelRegistryError>;
    fn get_availability(
        &self,
        availability_id: &ModelAvailabilityId,
    ) -> Result<Option<ModelAvailability>, ModelRegistryError>;
    fn list_for_model(
        &self,
        model_id: &ModelId,
    ) -> Result<Vec<ModelAvailability>, ModelRegistryError>;
    fn list_for_provider(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Vec<ModelAvailability>, ModelRegistryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryModelRegistry {
    models: Arc<RwLock<HashMap<ModelId, ModelDescriptor>>>,
    availability: Arc<RwLock<HashMap<ModelAvailabilityId, ModelAvailability>>>,
}

impl InMemoryModelRegistry {
    fn sorted_availability(
        values: impl Iterator<Item = ModelAvailability>,
    ) -> Vec<ModelAvailability> {
        let mut values = values.collect::<Vec<_>>();
        values.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        values
    }
}

impl ModelRegistry for InMemoryModelRegistry {
    fn register_model(&self, descriptor: ModelDescriptor) -> Result<(), ModelRegistryError> {
        descriptor.validate()?;
        let model_id = descriptor.model_id().clone();
        let mut models = self
            .models
            .write()
            .map_err(|error| ModelRegistryError::RegistryLock(error.to_string()))?;
        if models.contains_key(&model_id) {
            return Err(ModelRegistryError::ModelAlreadyRegistered(model_id));
        }
        models.insert(model_id, descriptor);
        Ok(())
    }

    fn get_model(&self, model_id: &ModelId) -> Result<Option<ModelDescriptor>, ModelRegistryError> {
        let models = self
            .models
            .read()
            .map_err(|error| ModelRegistryError::RegistryLock(error.to_string()))?;
        Ok(models.get(model_id).cloned())
    }

    fn list_models(&self) -> Result<Vec<ModelDescriptor>, ModelRegistryError> {
        let models = self
            .models
            .read()
            .map_err(|error| ModelRegistryError::RegistryLock(error.to_string()))?;
        let mut models = models.values().cloned().collect::<Vec<_>>();
        models.sort_by(|left, right| left.model_id().as_str().cmp(right.model_id().as_str()));
        Ok(models)
    }

    fn register_availability(
        &self,
        availability: ModelAvailability,
    ) -> Result<(), ModelRegistryError> {
        availability.validate()?;
        if self.get_model(availability.model_id())?.is_none() {
            return Err(ModelRegistryError::ModelNotFound(
                availability.model_id().clone(),
            ));
        }
        let mut values = self
            .availability
            .write()
            .map_err(|error| ModelRegistryError::RegistryLock(error.to_string()))?;
        if values.contains_key(availability.id()) {
            return Err(ModelRegistryError::AvailabilityAlreadyRegistered(
                availability.id().clone(),
            ));
        }
        if values.values().any(|existing| {
            existing.provider_id() == availability.provider_id()
                && existing.provider_model_reference() == availability.provider_model_reference()
        }) {
            return Err(
                ModelRegistryError::ProviderModelReferenceAlreadyRegistered {
                    provider_id: availability.provider_id().clone(),
                    provider_model_reference: availability.provider_model_reference().to_string(),
                },
            );
        }
        values.insert(availability.id().clone(), availability);
        Ok(())
    }

    fn get_availability(
        &self,
        availability_id: &ModelAvailabilityId,
    ) -> Result<Option<ModelAvailability>, ModelRegistryError> {
        let values = self
            .availability
            .read()
            .map_err(|error| ModelRegistryError::RegistryLock(error.to_string()))?;
        Ok(values.get(availability_id).cloned())
    }

    fn list_for_model(
        &self,
        model_id: &ModelId,
    ) -> Result<Vec<ModelAvailability>, ModelRegistryError> {
        let values = self
            .availability
            .read()
            .map_err(|error| ModelRegistryError::RegistryLock(error.to_string()))?;
        Ok(Self::sorted_availability(
            values
                .values()
                .filter(|availability| availability.model_id() == model_id)
                .cloned(),
        ))
    }

    fn list_for_provider(
        &self,
        provider_id: &AgentProviderId,
    ) -> Result<Vec<ModelAvailability>, ModelRegistryError> {
        let values = self
            .availability
            .read()
            .map_err(|error| ModelRegistryError::RegistryLock(error.to_string()))?;
        Ok(Self::sorted_availability(
            values
                .values()
                .filter(|availability| availability.provider_id() == provider_id)
                .cloned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::model_domain::{ModelAvailabilityStatus, ModelCapability, ModelMetadata};

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

    fn availability(id: &str, model_id: &str, provider_id: &str) -> ModelAvailability {
        ModelAvailability::new(
            ModelAvailabilityId::new(id).expect("valid availability ID"),
            ModelId::new(model_id).expect("valid model ID"),
            AgentProviderId::new(provider_id).expect("valid provider ID"),
            format!("native-{model_id}"),
            ModelAvailabilityStatus::Declared,
            1_000,
        )
        .expect("valid availability")
    }

    #[test]
    fn registry_keeps_model_and_availability_identity_independent() {
        let registry = InMemoryModelRegistry::default();
        registry
            .register_model(descriptor("model:one"))
            .expect("model registers");
        let availability = availability("availability:one", "model:one", "provider:one");
        registry
            .register_availability(availability.clone())
            .expect("availability registers");

        assert_eq!(
            registry
                .get_model(&ModelId::new("model:one").expect("valid ID"))
                .expect("lookup succeeds"),
            Some(descriptor("model:one"))
        );
        assert_eq!(
            registry
                .list_for_provider(
                    &AgentProviderId::new("provider:one").expect("valid provider ID")
                )
                .expect("lookup succeeds"),
            vec![availability]
        );
    }

    #[test]
    fn availability_requires_registered_model() {
        let registry = InMemoryModelRegistry::default();
        let result = registry.register_availability(availability(
            "availability:one",
            "model:missing",
            "provider:one",
        ));
        assert!(matches!(result, Err(ModelRegistryError::ModelNotFound(_))));
    }

    #[test]
    fn same_model_can_be_declared_by_multiple_providers() {
        let registry = InMemoryModelRegistry::default();
        registry
            .register_model(descriptor("model:one"))
            .expect("model registers");
        registry
            .register_availability(availability(
                "availability:one",
                "model:one",
                "provider:one",
            ))
            .expect("first availability registers");
        registry
            .register_availability(availability(
                "availability:two",
                "model:one",
                "provider:two",
            ))
            .expect("second availability registers");

        assert_eq!(
            registry
                .list_for_model(&ModelId::new("model:one").expect("valid model ID"))
                .expect("lookup succeeds")
                .len(),
            2
        );
    }
}
