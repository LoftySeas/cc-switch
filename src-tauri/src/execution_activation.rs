//! Explicit handoff from Runtime activation, Model routing, and Provider
//! integration into the existing Execution request boundary.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    agent_provider_domain::PreparedProviderBinding,
    model_routing::ResolvedModelRoute,
    runtime_domain::{RuntimeExecutionId, RuntimeId},
    runtime_instance_domain::{RuntimeInstance, RuntimeInstanceId},
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecutionActivationDomainError {
    #[error("Runtime instance is not ready for execution activation")]
    RuntimeNotReady,
    #[error("Provider binding does not match the resolved Model route")]
    ProviderBindingMismatch,
    #[error("Execution activation identity does not match Provider binding")]
    ExecutionIdentityMismatch,
    #[error("Execution activation timestamp is invalid")]
    InvalidTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionActivationPlan {
    execution_id: RuntimeExecutionId,
    runtime_instance_id: RuntimeInstanceId,
    runtime_id: RuntimeId,
    route: ResolvedModelRoute,
    provider_binding: PreparedProviderBinding,
    prepared_at: i64,
}

impl ExecutionActivationPlan {
    pub fn new(
        execution_id: RuntimeExecutionId,
        runtime: &RuntimeInstance,
        route: ResolvedModelRoute,
        provider_binding: PreparedProviderBinding,
        prepared_at: i64,
    ) -> Result<Self, ExecutionActivationDomainError> {
        if !runtime.lifecycle().accepts_execution() {
            return Err(ExecutionActivationDomainError::RuntimeNotReady);
        }
        if provider_binding.execution_id() != &execution_id {
            return Err(ExecutionActivationDomainError::ExecutionIdentityMismatch);
        }
        if provider_binding.provider_id() != route.availability().provider_id()
            || provider_binding.model_id() != route.model().model_id()
            || provider_binding.availability_id() != route.availability().id()
            || provider_binding.provider_model_reference()
                != route.availability().provider_model_reference()
        {
            return Err(ExecutionActivationDomainError::ProviderBindingMismatch);
        }
        if prepared_at < runtime.updated_at()
            || prepared_at < route.routed_at()
            || prepared_at < provider_binding.prepared_at()
        {
            return Err(ExecutionActivationDomainError::InvalidTimestamp);
        }
        Ok(Self {
            execution_id,
            runtime_instance_id: runtime.id().clone(),
            runtime_id: runtime.runtime_id().clone(),
            route,
            provider_binding,
            prepared_at,
        })
    }

    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }

    pub fn runtime_instance_id(&self) -> &RuntimeInstanceId {
        &self.runtime_instance_id
    }

    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }

    pub fn route(&self) -> &ResolvedModelRoute {
        &self.route
    }

    pub fn provider_binding(&self) -> &PreparedProviderBinding {
        &self.provider_binding
    }

    pub fn prepared_at(&self) -> i64 {
        self.prepared_at
    }
}
