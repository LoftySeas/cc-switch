//! Productization boundary that composes an active Runtime instance, a resolved
//! Model route, and an execution-scoped Provider compatibility binding.
//!
//! The planner does not create Agent identity, grant Permission, or invoke the
//! Runtime. The existing governed Execution pipeline remains the only productive
//! invocation boundary.

use thiserror::Error;

use crate::{
    agent_provider_domain::{AgentProviderDomainError, ProviderBindingRequest},
    execution_activation::{ExecutionActivationDomainError, ExecutionActivationPlan},
    model_routing::{ModelRouteRequest, ModelRoutingDomainError},
    runtime_domain::RuntimeExecutionId,
    runtime_instance_domain::RuntimeInstanceId,
    runtime_instance_repository::{RuntimeInstanceRepository, RuntimeInstanceRepositoryError},
    services::{
        model_routing::{ModelRouter, ModelRoutingError},
        provider_integration::{ProviderBindingPreparer, ProviderIntegrationError},
    },
};

#[derive(Debug, Error)]
pub enum ExecutionActivationError {
    #[error(transparent)]
    Domain(#[from] ExecutionActivationDomainError),
    #[error(transparent)]
    ModelDomain(#[from] ModelRoutingDomainError),
    #[error(transparent)]
    ProviderDomain(#[from] AgentProviderDomainError),
    #[error(transparent)]
    RuntimeRepository(#[from] RuntimeInstanceRepositoryError),
    #[error(transparent)]
    Routing(#[from] ModelRoutingError),
    #[error(transparent)]
    Provider(#[from] ProviderIntegrationError),
    #[error("Runtime instance was not found: {0}")]
    RuntimeNotFound(RuntimeInstanceId),
}

pub struct ExecutionActivationService<I, M, P> {
    instances: I,
    router: M,
    providers: P,
}

impl<I, M, P> ExecutionActivationService<I, M, P>
where
    I: RuntimeInstanceRepository,
    M: ModelRouter,
    P: ProviderBindingPreparer,
{
    pub fn new(instances: I, router: M, providers: P) -> Self {
        Self {
            instances,
            router,
            providers,
        }
    }

    pub fn prepare(
        &self,
        execution_id: RuntimeExecutionId,
        runtime_instance_id: &RuntimeInstanceId,
        route_request: &ModelRouteRequest,
        prepared_at: i64,
    ) -> Result<ExecutionActivationPlan, ExecutionActivationError> {
        let runtime = self.instances.get(runtime_instance_id)?.ok_or_else(|| {
            ExecutionActivationError::RuntimeNotFound(runtime_instance_id.clone())
        })?;
        let route = self.router.route(route_request)?;
        let provider_request = ProviderBindingRequest::new(
            execution_id.clone(),
            route.availability().provider_id().clone(),
            route.model().model_id().clone(),
            route.availability().id().clone(),
            route.availability().provider_model_reference(),
        )?;
        let provider_binding = self.providers.prepare(&provider_request, prepared_at)?;
        Ok(ExecutionActivationPlan::new(
            execution_id,
            &runtime,
            route,
            provider_binding,
            prepared_at,
        )?)
    }
}
