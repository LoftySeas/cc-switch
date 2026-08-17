//! Execution-scoped Provider integration service.
//!
//! This service prepares a non-secret compatibility binding after Model routing.
//! It never invokes a Provider or chooses a Model.

use thiserror::Error;

use crate::{
    agent_provider_adapter::{
        AgentProviderAdapterError, AgentProviderIntegrationAdapterRepository,
    },
    agent_provider_domain::{PreparedProviderBinding, ProviderBindingRequest},
};

#[derive(Debug, Error)]
pub enum ProviderIntegrationError {
    #[error(transparent)]
    Adapter(#[from] AgentProviderAdapterError),
}

pub struct ProviderIntegrationService<R> {
    adapters: R,
}

pub trait ProviderBindingPreparer: Send + Sync {
    fn prepare(
        &self,
        request: &ProviderBindingRequest,
        prepared_at: i64,
    ) -> Result<PreparedProviderBinding, ProviderIntegrationError>;
}

impl<R> ProviderIntegrationService<R>
where
    R: AgentProviderIntegrationAdapterRepository,
{
    pub fn new(adapters: R) -> Self {
        Self { adapters }
    }

    pub fn prepare_binding(
        &self,
        request: &ProviderBindingRequest,
        prepared_at: i64,
    ) -> Result<PreparedProviderBinding, ProviderIntegrationError> {
        let adapter = self
            .adapters
            .get_integration(request.provider_id())?
            .ok_or_else(|| {
                AgentProviderAdapterError::NotRegistered(request.provider_id().clone())
            })?;
        let binding = adapter.prepare_binding(request, prepared_at)?;
        if binding.execution_id() != request.execution_id()
            || binding.provider_id() != request.provider_id()
            || binding.model_id() != request.model_id()
            || binding.availability_id() != request.availability_id()
            || binding.provider_model_reference() != request.provider_model_reference()
            || binding.prepared_at() != prepared_at
        {
            return Err(AgentProviderAdapterError::BindingResultMismatch.into());
        }
        Ok(binding)
    }
}

impl<R> ProviderBindingPreparer for ProviderIntegrationService<R>
where
    R: AgentProviderIntegrationAdapterRepository,
{
    fn prepare(
        &self,
        request: &ProviderBindingRequest,
        prepared_at: i64,
    ) -> Result<PreparedProviderBinding, ProviderIntegrationError> {
        self.prepare_binding(request, prepared_at)
    }
}
