//! Model capability matching and deterministic routing service.

use thiserror::Error;

use crate::{
    agent_provider_adapter::{AgentProviderAdapterError, AgentProviderAdapterRepository},
    agent_provider_domain::{AgentProviderDomainError, ProviderAvailability},
    model_domain::{ModelAvailability, ModelAvailabilityStatus, ModelDescriptor},
    model_registry::{ModelRegistry, ModelRegistryError},
    model_routing::{
        ModelCapabilityRequirement, ModelRouteRequest, ModelRoutingDomainError, ResolvedModelRoute,
    },
};

#[derive(Debug, Error)]
pub enum ModelRoutingError {
    #[error(transparent)]
    Domain(#[from] ModelRoutingDomainError),
    #[error(transparent)]
    Registry(#[from] ModelRegistryError),
    #[error(transparent)]
    Provider(#[from] AgentProviderAdapterError),
    #[error(transparent)]
    ProviderDomain(#[from] AgentProviderDomainError),
    #[error("No Model route satisfies capability, policy, availability, and Provider readiness")]
    NoEligibleRoute,
}

pub trait ModelRouter: Send + Sync {
    fn route(&self, request: &ModelRouteRequest) -> Result<ResolvedModelRoute, ModelRoutingError>;
}

pub struct PolicyModelRoutingService<M, P> {
    models: M,
    providers: P,
}

impl<M, P> PolicyModelRoutingService<M, P>
where
    M: ModelRegistry,
    P: AgentProviderAdapterRepository,
{
    pub fn new(models: M, providers: P) -> Self {
        Self { models, providers }
    }

    fn model_matches(model: &ModelDescriptor, requirements: &[ModelCapabilityRequirement]) -> bool {
        requirements.iter().all(|requirement| {
            model.capabilities().iter().any(|capability| {
                capability.name() == requirement.name()
                    && capability.version() >= requirement.minimum_version()
                    && requirement
                        .required_metadata()
                        .iter()
                        .all(|(key, value)| capability.metadata().get(key) == Some(value))
            })
        })
    }

    fn availability_is_fresh(
        availability: &ModelAvailability,
        request: &ModelRouteRequest,
    ) -> bool {
        if availability.observed_at() > request.routed_at() {
            return false;
        }
        request
            .policy()
            .maximum_availability_age()
            .is_none_or(|age| request.routed_at() - availability.observed_at() <= age)
    }

    fn preference_rank<T: PartialEq>(value: &T, preferences: &[T]) -> usize {
        preferences
            .iter()
            .position(|preferred| preferred == value)
            .unwrap_or(usize::MAX)
    }
}

impl<M, P> ModelRouter for PolicyModelRoutingService<M, P>
where
    M: ModelRegistry,
    P: AgentProviderAdapterRepository,
{
    fn route(&self, request: &ModelRouteRequest) -> Result<ResolvedModelRoute, ModelRoutingError> {
        let policy = request.policy();
        let mut candidates = Vec::new();
        for model in self.models.list_models()? {
            if !policy.allowed_model_ids().is_empty()
                && !policy.allowed_model_ids().contains(model.model_id())
            {
                continue;
            }
            if !Self::model_matches(&model, request.requirements()) {
                continue;
            }
            for availability in self.models.list_for_model(model.model_id())? {
                if availability.status() != ModelAvailabilityStatus::Declared
                    || !Self::availability_is_fresh(&availability, request)
                    || (!policy.allowed_provider_ids().is_empty()
                        && !policy
                            .allowed_provider_ids()
                            .contains(availability.provider_id()))
                {
                    continue;
                }
                let Some(provider) = self.providers.get(availability.provider_id())? else {
                    continue;
                };
                let probe = provider.probe()?;
                probe.validate()?;
                if probe.provider_id != *availability.provider_id()
                    || probe.availability != ProviderAvailability::Registered
                {
                    continue;
                }
                candidates.push((
                    Self::preference_rank(model.model_id(), policy.preferred_model_ids()),
                    Self::preference_rank(
                        availability.provider_id(),
                        policy.preferred_provider_ids(),
                    ),
                    model.clone(),
                    availability,
                ));
            }
        }
        candidates.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.model_id().as_str().cmp(right.2.model_id().as_str()))
                .then_with(|| {
                    left.3
                        .provider_id()
                        .as_str()
                        .cmp(right.3.provider_id().as_str())
                })
                .then_with(|| left.3.id().as_str().cmp(right.3.id().as_str()))
        });
        let (_, _, model, availability) = candidates
            .into_iter()
            .next()
            .ok_or(ModelRoutingError::NoEligibleRoute)?;
        ResolvedModelRoute::new(
            model,
            availability,
            request
                .requirements()
                .iter()
                .map(|requirement| requirement.name().to_string())
                .collect(),
            request.routed_at(),
        )
        .map_err(Into::into)
    }
}
