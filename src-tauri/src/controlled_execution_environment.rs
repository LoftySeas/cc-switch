//! Controlled execution preparation domain and repository boundary.
//!
//! A prepared environment is immutable evidence that an existing Execution
//! Request aligns with activated Runtime/Provider boundaries and an explicit
//! Model resolution. This module exposes no execution or invocation operation.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    execution_domain::ExecutionRequest,
    model_resolution::{ModelResolutionId, ResolvedModel},
};

const MAX_ID_LENGTH: usize = 160;
const MAX_REFERENCE_LENGTH: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ControlledExecutionEnvironmentDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Controlled execution environment identities must remain distinct")]
    IdentityCollision,
    #[error("Controlled execution environment timestamp order is invalid")]
    InvalidTimestamp,
    #[error("Execution Request does not match the resolved Runtime, Provider, and Model")]
    ResolutionMismatch,
    #[error("Isolation evidence does not match the preparation request")]
    IsolationMismatch,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(
                value: impl Into<String>,
            ) -> Result<Self, ControlledExecutionEnvironmentDomainError> {
                Ok(Self(identifier($field, value.into())?))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

typed_id!(
    ControlledExecutionEnvironmentId,
    "Controlled execution environment ID"
);
typed_id!(ExecutionIsolationId, "Execution isolation ID");

fn identifier(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, ControlledExecutionEnvironmentDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(ControlledExecutionEnvironmentDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(ControlledExecutionEnvironmentDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(ControlledExecutionEnvironmentDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn reference(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, ControlledExecutionEnvironmentDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(ControlledExecutionEnvironmentDomainError::Empty { field });
    }
    if value.chars().count() > MAX_REFERENCE_LENGTH {
        return Err(ControlledExecutionEnvironmentDomainError::TooLong {
            field,
            max: MAX_REFERENCE_LENGTH,
        });
    }
    if value.chars().any(char::is_control) {
        return Err(ControlledExecutionEnvironmentDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

/// COD-028 supports preparation evidence only. Additional isolation modes need
/// a separately governed milestone because they would introduce real execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionIsolationLevel {
    PreparationOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionIsolationEvidence {
    isolation_id: ExecutionIsolationId,
    level: ExecutionIsolationLevel,
    boundary_reference: String,
    prepared_at: i64,
}

impl ExecutionIsolationEvidence {
    pub fn preparation_only(
        isolation_id: ExecutionIsolationId,
        boundary_reference: impl Into<String>,
        prepared_at: i64,
    ) -> Result<Self, ControlledExecutionEnvironmentDomainError> {
        if prepared_at < 0 {
            return Err(ControlledExecutionEnvironmentDomainError::InvalidTimestamp);
        }
        Ok(Self {
            isolation_id,
            level: ExecutionIsolationLevel::PreparationOnly,
            boundary_reference: reference("Isolation boundary reference", boundary_reference)?,
            prepared_at,
        })
    }

    pub fn isolation_id(&self) -> &ExecutionIsolationId {
        &self.isolation_id
    }
    pub fn level(&self) -> ExecutionIsolationLevel {
        self.level
    }
    pub fn boundary_reference(&self) -> &str {
        &self.boundary_reference
    }
    pub fn prepared_at(&self) -> i64 {
        self.prepared_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEnvironmentPreparationRequest {
    environment_id: ControlledExecutionEnvironmentId,
    execution_request: ExecutionRequest,
    model_resolution_id: ModelResolutionId,
    isolation_id: ExecutionIsolationId,
    requested_at: i64,
}

impl ExecutionEnvironmentPreparationRequest {
    pub fn new(
        environment_id: ControlledExecutionEnvironmentId,
        execution_request: ExecutionRequest,
        model_resolution_id: ModelResolutionId,
        isolation_id: ExecutionIsolationId,
        requested_at: i64,
    ) -> Result<Self, ControlledExecutionEnvironmentDomainError> {
        let request = Self {
            environment_id,
            execution_request,
            model_resolution_id,
            isolation_id,
            requested_at,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn environment_id(&self) -> &ControlledExecutionEnvironmentId {
        &self.environment_id
    }
    pub fn execution_request(&self) -> &ExecutionRequest {
        &self.execution_request
    }
    pub fn model_resolution_id(&self) -> &ModelResolutionId {
        &self.model_resolution_id
    }
    pub fn isolation_id(&self) -> &ExecutionIsolationId {
        &self.isolation_id
    }
    pub fn requested_at(&self) -> i64 {
        self.requested_at
    }

    pub fn validate(&self) -> Result<(), ControlledExecutionEnvironmentDomainError> {
        ControlledExecutionEnvironmentId::new(self.environment_id.as_str())?;
        ExecutionIsolationId::new(self.isolation_id.as_str())?;
        ModelResolutionId::new(self.model_resolution_id.as_str()).map_err(|_| {
            ControlledExecutionEnvironmentDomainError::InvalidIdentifier {
                field: "Model resolution ID",
            }
        })?;
        self.execution_request
            .validate()
            .map_err(|_| ControlledExecutionEnvironmentDomainError::ResolutionMismatch)?;
        if self.requested_at < self.execution_request.accepted_at() {
            return Err(ControlledExecutionEnvironmentDomainError::InvalidTimestamp);
        }
        let identities = [
            self.environment_id.as_str(),
            self.execution_request.execution_id().as_str(),
            self.model_resolution_id.as_str(),
            self.isolation_id.as_str(),
        ];
        if identities.iter().copied().collect::<HashSet<_>>().len() != identities.len() {
            return Err(ControlledExecutionEnvironmentDomainError::IdentityCollision);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlledExecutionEnvironment {
    environment_id: ControlledExecutionEnvironmentId,
    execution_request: ExecutionRequest,
    resolution: ResolvedModel,
    isolation: ExecutionIsolationEvidence,
    prepared_at: i64,
}

impl ControlledExecutionEnvironment {
    pub fn new(
        request: &ExecutionEnvironmentPreparationRequest,
        resolution: ResolvedModel,
        isolation: ExecutionIsolationEvidence,
        prepared_at: i64,
    ) -> Result<Self, ControlledExecutionEnvironmentDomainError> {
        request.validate()?;
        if request.model_resolution_id() != resolution.resolution_id() {
            return Err(ControlledExecutionEnvironmentDomainError::ResolutionMismatch);
        }
        if request.isolation_id() != isolation.isolation_id()
            || isolation.level() != ExecutionIsolationLevel::PreparationOnly
        {
            return Err(ControlledExecutionEnvironmentDomainError::IsolationMismatch);
        }
        let execution = request.execution_request();
        let model_binding = execution.model_binding();
        if execution.context().binding().runtime_id() != resolution.runtime_id()
            || model_binding.model_id() != resolution.model().model_id()
            || model_binding.provider_id() != Some(resolution.provider_id())
            || model_binding.model_availability_id() != Some(resolution.availability().id())
        {
            return Err(ControlledExecutionEnvironmentDomainError::ResolutionMismatch);
        }
        if prepared_at < request.requested_at()
            || prepared_at < resolution.resolved_at()
            || prepared_at < isolation.prepared_at()
        {
            return Err(ControlledExecutionEnvironmentDomainError::InvalidTimestamp);
        }
        let identities = [
            request.environment_id().as_str(),
            execution.execution_id().as_str(),
            resolution.resolution_id().as_str(),
            isolation.isolation_id().as_str(),
            resolution.runtime_instance_id().as_str(),
            resolution.provider_instance_id().as_str(),
            resolution.runtime_id().as_str(),
            resolution.provider_id().as_str(),
            resolution.model().model_id().as_str(),
            resolution.availability().id().as_str(),
        ];
        if identities.iter().copied().collect::<HashSet<_>>().len() != identities.len() {
            return Err(ControlledExecutionEnvironmentDomainError::IdentityCollision);
        }
        Ok(Self {
            environment_id: request.environment_id().clone(),
            execution_request: execution.clone(),
            resolution,
            isolation,
            prepared_at,
        })
    }

    pub fn environment_id(&self) -> &ControlledExecutionEnvironmentId {
        &self.environment_id
    }
    pub fn execution_request(&self) -> &ExecutionRequest {
        &self.execution_request
    }
    pub fn resolution(&self) -> &ResolvedModel {
        &self.resolution
    }
    pub fn isolation(&self) -> &ExecutionIsolationEvidence {
        &self.isolation
    }
    pub fn prepared_at(&self) -> i64 {
        self.prepared_at
    }
}

pub trait ControlledExecutionPreparationContract {
    type Error;

    fn prepare(
        &self,
        request: ExecutionEnvironmentPreparationRequest,
        prepared_at: i64,
    ) -> Result<ControlledExecutionEnvironment, Self::Error>;
}

#[derive(Debug, Error)]
pub enum ExecutionIsolationBoundaryError {
    #[error(transparent)]
    InvalidEvidence(#[from] ControlledExecutionEnvironmentDomainError),
    #[error("Execution isolation boundary rejected preparation: {0}")]
    Rejected(String),
}

/// Isolation adapters may prepare non-executable evidence only. There is no
/// start, invoke, tool, filesystem, network, Provider, or Model-call operation.
pub trait ExecutionIsolationBoundary: Send + Sync {
    fn prepare_isolation(
        &self,
        request: &ExecutionEnvironmentPreparationRequest,
        prepared_at: i64,
    ) -> Result<ExecutionIsolationEvidence, ExecutionIsolationBoundaryError>;
}

#[derive(Debug, Clone)]
pub struct InMemoryPreparationIsolationBoundary {
    boundary_reference: String,
}

impl InMemoryPreparationIsolationBoundary {
    pub fn new(
        boundary_reference: impl Into<String>,
    ) -> Result<Self, ControlledExecutionEnvironmentDomainError> {
        Ok(Self {
            boundary_reference: reference("Isolation boundary reference", boundary_reference)?,
        })
    }
}

impl ExecutionIsolationBoundary for InMemoryPreparationIsolationBoundary {
    fn prepare_isolation(
        &self,
        request: &ExecutionEnvironmentPreparationRequest,
        prepared_at: i64,
    ) -> Result<ExecutionIsolationEvidence, ExecutionIsolationBoundaryError> {
        Ok(ExecutionIsolationEvidence::preparation_only(
            request.isolation_id().clone(),
            self.boundary_reference.clone(),
            prepared_at,
        )?)
    }
}

#[derive(Debug, Error)]
pub enum ControlledExecutionEnvironmentRepositoryError {
    #[error("Controlled execution environment is already recorded: {0}")]
    AlreadyRecorded(ControlledExecutionEnvironmentId),
    #[error("Controlled execution environment repository lock failed: {0}")]
    RegistryLock(String),
}

pub trait ControlledExecutionEnvironmentRepository: Send + Sync {
    fn insert(
        &self,
        environment: ControlledExecutionEnvironment,
    ) -> Result<(), ControlledExecutionEnvironmentRepositoryError>;
    fn get(
        &self,
        environment_id: &ControlledExecutionEnvironmentId,
    ) -> Result<Option<ControlledExecutionEnvironment>, ControlledExecutionEnvironmentRepositoryError>;
    fn list(
        &self,
    ) -> Result<Vec<ControlledExecutionEnvironment>, ControlledExecutionEnvironmentRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryControlledExecutionEnvironmentRepository {
    environments:
        Arc<RwLock<HashMap<ControlledExecutionEnvironmentId, ControlledExecutionEnvironment>>>,
}

impl ControlledExecutionEnvironmentRepository for InMemoryControlledExecutionEnvironmentRepository {
    fn insert(
        &self,
        environment: ControlledExecutionEnvironment,
    ) -> Result<(), ControlledExecutionEnvironmentRepositoryError> {
        let mut values = self.environments.write().map_err(|error| {
            ControlledExecutionEnvironmentRepositoryError::RegistryLock(error.to_string())
        })?;
        if values.contains_key(environment.environment_id()) {
            return Err(
                ControlledExecutionEnvironmentRepositoryError::AlreadyRecorded(
                    environment.environment_id().clone(),
                ),
            );
        }
        values.insert(environment.environment_id().clone(), environment);
        Ok(())
    }

    fn get(
        &self,
        environment_id: &ControlledExecutionEnvironmentId,
    ) -> Result<Option<ControlledExecutionEnvironment>, ControlledExecutionEnvironmentRepositoryError>
    {
        let values = self.environments.read().map_err(|error| {
            ControlledExecutionEnvironmentRepositoryError::RegistryLock(error.to_string())
        })?;
        Ok(values.get(environment_id).cloned())
    }

    fn list(
        &self,
    ) -> Result<Vec<ControlledExecutionEnvironment>, ControlledExecutionEnvironmentRepositoryError>
    {
        let values = self.environments.read().map_err(|error| {
            ControlledExecutionEnvironmentRepositoryError::RegistryLock(error.to_string())
        })?;
        let mut values = values.values().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| {
            left.environment_id()
                .as_str()
                .cmp(right.environment_id().as_str())
        });
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability_domain::CapabilitySnapshotId,
        execution_domain::{ExecutionGovernanceEvidence, ExecutionModelBinding},
        model_domain::{
            ModelAvailability, ModelAvailabilityId, ModelAvailabilityStatus, ModelDescriptor,
            ModelId, ModelMetadata,
        },
        permission_domain::{AuthorizationDecisionId, PermissionGrantId},
        role_domain::RoleAssignmentId,
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeBindingId, RuntimeBindingLifecycle,
            RuntimeExecutionId, RuntimeId,
        },
    };

    pub(crate) fn execution_request(runtime_id: &str, model_id: &str) -> ExecutionRequest {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding:controlled").unwrap(),
            "agent:controlled",
            RuntimeId::new(runtime_id).unwrap(),
            10,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 11)
        .unwrap();
        ExecutionRequest::new(
            ExecutionContext::new(
                RuntimeExecutionId::new("execution:controlled").unwrap(),
                binding,
                vec!["context:sealed".into()],
                12,
            )
            .unwrap(),
            "Prepare controlled execution",
            ExecutionModelBinding::provider_model(
                ModelId::new(model_id).unwrap(),
                crate::agent_provider_domain::AgentProviderId::new("provider:controlled").unwrap(),
                ModelAvailabilityId::new("availability:controlled").unwrap(),
            ),
            ExecutionGovernanceEvidence::new(
                CapabilitySnapshotId::new("capability-snapshot:controlled").unwrap(),
                PermissionGrantId::new("permission-grant:controlled").unwrap(),
                RoleAssignmentId::new("role-assignment:controlled").unwrap(),
                AuthorizationDecisionId::new("authorization:controlled").unwrap(),
            ),
            None,
            13,
        )
        .unwrap()
    }

    pub(crate) fn resolved_model() -> ResolvedModel {
        let resolution_request = crate::model_resolution::ModelResolutionRequest::new(
            ModelResolutionId::new("resolution:controlled").unwrap(),
            crate::runtime_instance_domain::RuntimeInstanceId::new("runtime-instance:controlled")
                .unwrap(),
            crate::agent_provider_instance::AgentProviderInstanceId::new(
                "provider-instance:controlled",
            )
            .unwrap(),
            ModelId::new("model:controlled").unwrap(),
            ModelAvailabilityId::new("availability:controlled").unwrap(),
            vec![],
            14,
        )
        .unwrap();
        ResolvedModel::new(
            &resolution_request,
            RuntimeId::new("runtime:controlled").unwrap(),
            crate::agent_provider_domain::AgentProviderId::new("provider:controlled").unwrap(),
            ModelDescriptor::new(
                ModelId::new("model:controlled").unwrap(),
                "Controlled Model",
                ModelMetadata::default(),
                vec![],
            )
            .unwrap(),
            ModelAvailability::new(
                ModelAvailabilityId::new("availability:controlled").unwrap(),
                ModelId::new("model:controlled").unwrap(),
                crate::agent_provider_domain::AgentProviderId::new("provider:controlled").unwrap(),
                "native-controlled",
                ModelAvailabilityStatus::Declared,
                14,
            )
            .unwrap(),
            15,
        )
        .unwrap()
    }

    pub(crate) fn preparation_request() -> ExecutionEnvironmentPreparationRequest {
        ExecutionEnvironmentPreparationRequest::new(
            ControlledExecutionEnvironmentId::new("environment:controlled").unwrap(),
            execution_request("runtime:controlled", "model:controlled"),
            ModelResolutionId::new("resolution:controlled").unwrap(),
            ExecutionIsolationId::new("isolation:controlled").unwrap(),
            16,
        )
        .unwrap()
    }

    #[test]
    fn prepared_environment_is_a_non_executable_snapshot() {
        let request = preparation_request();
        let isolation = ExecutionIsolationEvidence::preparation_only(
            request.isolation_id().clone(),
            "isolation-boundary:memory",
            17,
        )
        .unwrap();
        let environment =
            ControlledExecutionEnvironment::new(&request, resolved_model(), isolation, 18).unwrap();
        let value = serde_json::to_value(environment).unwrap();

        assert_eq!(value["isolation"]["level"], "preparation_only");
        assert!(value.get("providerCredential").is_none());
        assert!(value.get("modelInvocation").is_none());
        assert!(value.get("toolExecution").is_none());
    }

    #[test]
    fn mismatched_explicit_model_fails_closed() {
        let request = ExecutionEnvironmentPreparationRequest::new(
            ControlledExecutionEnvironmentId::new("environment:controlled").unwrap(),
            execution_request("runtime:controlled", "model:other"),
            ModelResolutionId::new("resolution:controlled").unwrap(),
            ExecutionIsolationId::new("isolation:controlled").unwrap(),
            16,
        )
        .unwrap();
        let isolation = ExecutionIsolationEvidence::preparation_only(
            request.isolation_id().clone(),
            "isolation-boundary:memory",
            17,
        )
        .unwrap();
        assert!(matches!(
            ControlledExecutionEnvironment::new(&request, resolved_model(), isolation, 18),
            Err(ControlledExecutionEnvironmentDomainError::ResolutionMismatch)
        ));
    }
}
