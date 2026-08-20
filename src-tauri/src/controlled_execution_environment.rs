//! Controlled execution preparation domain and repository boundary.
//!
//! A prepared environment is immutable evidence that an existing Execution
//! Request aligns with activated Runtime/Provider boundaries and an explicit
//! Model resolution. This module exposes no execution or invocation operation.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    database::{lock_conn, Database},
    error::AppError,
    execution_domain::ExecutionRequest,
    execution_readiness::{
        ActivationEvidenceAgePolicy, ProviderActivationSnapshot, RuntimeActivationSnapshot,
    },
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
    #[error("Runtime or Provider activation snapshot does not match the prepared environment")]
    ActivationSnapshotMismatch,
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

    pub fn validate(&self) -> Result<(), ControlledExecutionEnvironmentDomainError> {
        ExecutionIsolationId::new(self.isolation_id.as_str())?;
        if self.level != ExecutionIsolationLevel::PreparationOnly || self.prepared_at < 0 {
            return Err(ControlledExecutionEnvironmentDomainError::IsolationMismatch);
        }
        reference(
            "Isolation boundary reference",
            self.boundary_reference.clone(),
        )?;
        Ok(())
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
#[serde(
    rename_all = "camelCase",
    try_from = "ControlledExecutionEnvironmentDto"
)]
pub struct ControlledExecutionEnvironment {
    environment_id: ControlledExecutionEnvironmentId,
    execution_request: ExecutionRequest,
    resolution: ResolvedModel,
    runtime_activation: RuntimeActivationSnapshot,
    provider_activation: ProviderActivationSnapshot,
    isolation: ExecutionIsolationEvidence,
    requested_at: i64,
    evidence_max_age_millis: i64,
    prepared_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlledExecutionEnvironmentDto {
    environment_id: ControlledExecutionEnvironmentId,
    execution_request: ExecutionRequest,
    resolution: ResolvedModel,
    runtime_activation: RuntimeActivationSnapshot,
    provider_activation: ProviderActivationSnapshot,
    isolation: ExecutionIsolationEvidence,
    requested_at: i64,
    evidence_max_age_millis: i64,
    prepared_at: i64,
}

impl TryFrom<ControlledExecutionEnvironmentDto> for ControlledExecutionEnvironment {
    type Error = ControlledExecutionEnvironmentDomainError;

    fn try_from(value: ControlledExecutionEnvironmentDto) -> Result<Self, Self::Error> {
        let environment = Self {
            environment_id: value.environment_id,
            execution_request: value.execution_request,
            resolution: value.resolution,
            runtime_activation: value.runtime_activation,
            provider_activation: value.provider_activation,
            isolation: value.isolation,
            requested_at: value.requested_at,
            evidence_max_age_millis: value.evidence_max_age_millis,
            prepared_at: value.prepared_at,
        };
        environment.validate()?;
        Ok(environment)
    }
}

impl ControlledExecutionEnvironment {
    pub fn new(
        request: &ExecutionEnvironmentPreparationRequest,
        resolution: ResolvedModel,
        runtime_activation: RuntimeActivationSnapshot,
        provider_activation: ProviderActivationSnapshot,
        isolation: ExecutionIsolationEvidence,
        evidence_age_policy: ActivationEvidenceAgePolicy,
        prepared_at: i64,
    ) -> Result<Self, ControlledExecutionEnvironmentDomainError> {
        request.validate()?;
        let environment = Self {
            environment_id: request.environment_id().clone(),
            execution_request: request.execution_request().clone(),
            resolution,
            runtime_activation,
            provider_activation,
            isolation,
            requested_at: request.requested_at(),
            evidence_max_age_millis: evidence_age_policy.max_age_millis(),
            prepared_at,
        };
        environment.validate()?;
        Ok(environment)
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
    pub fn runtime_activation(&self) -> &RuntimeActivationSnapshot {
        &self.runtime_activation
    }
    pub fn provider_activation(&self) -> &ProviderActivationSnapshot {
        &self.provider_activation
    }
    pub fn isolation(&self) -> &ExecutionIsolationEvidence {
        &self.isolation
    }
    pub fn requested_at(&self) -> i64 {
        self.requested_at
    }
    pub fn evidence_max_age_millis(&self) -> i64 {
        self.evidence_max_age_millis
    }
    pub fn prepared_at(&self) -> i64 {
        self.prepared_at
    }

    pub fn validate(&self) -> Result<(), ControlledExecutionEnvironmentDomainError> {
        ControlledExecutionEnvironmentId::new(self.environment_id.as_str())?;
        self.execution_request
            .validate()
            .map_err(|_| ControlledExecutionEnvironmentDomainError::ResolutionMismatch)?;
        self.resolution
            .validate()
            .map_err(|_| ControlledExecutionEnvironmentDomainError::ResolutionMismatch)?;
        self.runtime_activation.validate()?;
        self.provider_activation.validate()?;
        self.isolation.validate()?;
        let age_policy = ActivationEvidenceAgePolicy::new(self.evidence_max_age_millis)?;

        let execution = &self.execution_request;
        let model_binding = execution.model_binding();
        if execution.context().binding().runtime_id() != self.resolution.runtime_id()
            || model_binding.model_id() != self.resolution.model().model_id()
            || model_binding.provider_id() != Some(self.resolution.provider_id())
            || model_binding.model_availability_id() != Some(self.resolution.availability().id())
        {
            return Err(ControlledExecutionEnvironmentDomainError::ResolutionMismatch);
        }
        if self.runtime_activation.instance_id() != self.resolution.runtime_instance_id()
            || self.runtime_activation.runtime_id() != self.resolution.runtime_id()
            || self.provider_activation.instance_id() != self.resolution.provider_instance_id()
            || self.provider_activation.provider_id() != self.resolution.provider_id()
        {
            return Err(ControlledExecutionEnvironmentDomainError::ActivationSnapshotMismatch);
        }
        if self.requested_at < execution.accepted_at()
            || self.resolution.requested_at() < execution.accepted_at()
            || self.resolution.resolved_at() < self.resolution.requested_at()
            || self.requested_at < self.resolution.resolved_at()
            || self.runtime_activation.snapshot_at() < self.requested_at
            || self.provider_activation.snapshot_at() < self.runtime_activation.snapshot_at()
            || self.isolation.prepared_at() < self.provider_activation.snapshot_at()
            || self.prepared_at < self.isolation.prepared_at()
        {
            return Err(ControlledExecutionEnvironmentDomainError::InvalidTimestamp);
        }
        age_policy.validate_observation(
            self.runtime_activation.health_observed_at(),
            self.runtime_activation.snapshot_at(),
        )?;
        age_policy.validate_observation(
            self.provider_activation.observation_at(),
            self.provider_activation.snapshot_at(),
        )?;
        let identities = [
            self.environment_id.as_str(),
            execution.execution_id().as_str(),
            self.resolution.resolution_id().as_str(),
            self.isolation.isolation_id().as_str(),
            self.resolution.runtime_instance_id().as_str(),
            self.resolution.provider_instance_id().as_str(),
            self.resolution.runtime_id().as_str(),
            self.resolution.provider_id().as_str(),
            self.resolution.model().model_id().as_str(),
            self.resolution.availability().id().as_str(),
        ];
        if identities.iter().copied().collect::<HashSet<_>>().len() != identities.len() {
            return Err(ControlledExecutionEnvironmentDomainError::IdentityCollision);
        }
        Ok(())
    }
}

pub trait ControlledExecutionPreparationContract {
    type Error;

    fn prepare(
        &self,
        request: ExecutionEnvironmentPreparationRequest,
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
    #[error(transparent)]
    InvalidDomain(#[from] ControlledExecutionEnvironmentDomainError),
    #[error("Controlled execution environment is already recorded: {0}")]
    AlreadyRecorded(ControlledExecutionEnvironmentId),
    #[error("Controlled execution environment repository lock failed: {0}")]
    RegistryLock(String),
    #[error("Controlled execution environment persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for ControlledExecutionEnvironmentRepositoryError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
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
        environment.validate()?;
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
        let environment = values.get(environment_id).cloned();
        if let Some(environment) = &environment {
            environment.validate()?;
        }
        Ok(environment)
    }

    fn list(
        &self,
    ) -> Result<Vec<ControlledExecutionEnvironment>, ControlledExecutionEnvironmentRepositoryError>
    {
        let values = self.environments.read().map_err(|error| {
            ControlledExecutionEnvironmentRepositoryError::RegistryLock(error.to_string())
        })?;
        let mut values = values.values().cloned().collect::<Vec<_>>();
        for environment in &values {
            environment.validate()?;
        }
        values.sort_by(|left, right| {
            left.environment_id()
                .as_str()
                .cmp(right.environment_id().as_str())
        });
        Ok(values)
    }
}

#[derive(Clone)]
pub struct SqliteControlledExecutionEnvironmentRepository {
    database: Arc<Database>,
}

struct StoredControlledExecutionEnvironment {
    environment_id: String,
    execution_id: String,
    model_resolution_id: String,
    runtime_instance_id: String,
    provider_instance_id: String,
    environment_json: String,
    requested_at: i64,
    prepared_at: i64,
}

impl StoredControlledExecutionEnvironment {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            environment_id: row.get(0)?,
            execution_id: row.get(1)?,
            model_resolution_id: row.get(2)?,
            runtime_instance_id: row.get(3)?,
            provider_instance_id: row.get(4)?,
            environment_json: row.get(5)?,
            requested_at: row.get(6)?,
            prepared_at: row.get(7)?,
        })
    }
}

impl SqliteControlledExecutionEnvironmentRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    fn decode(
        stored: StoredControlledExecutionEnvironment,
    ) -> Result<ControlledExecutionEnvironment, ControlledExecutionEnvironmentRepositoryError> {
        let environment =
            serde_json::from_str::<ControlledExecutionEnvironment>(&stored.environment_json)
                .map_err(|error| {
                    ControlledExecutionEnvironmentRepositoryError::Persistence(error.to_string())
                })?;
        environment.validate()?;
        if environment.environment_id().as_str() != stored.environment_id
            || environment.execution_request().execution_id().as_str() != stored.execution_id
            || environment.resolution().resolution_id().as_str() != stored.model_resolution_id
            || environment.runtime_activation().instance_id().as_str() != stored.runtime_instance_id
            || environment.provider_activation().instance_id().as_str()
                != stored.provider_instance_id
            || environment.requested_at() != stored.requested_at
            || environment.prepared_at() != stored.prepared_at
        {
            return Err(ControlledExecutionEnvironmentRepositoryError::Persistence(
                "Controlled environment indexed columns do not match immutable evidence".into(),
            ));
        }
        Ok(environment)
    }
}

impl ControlledExecutionEnvironmentRepository for SqliteControlledExecutionEnvironmentRepository {
    fn insert(
        &self,
        environment: ControlledExecutionEnvironment,
    ) -> Result<(), ControlledExecutionEnvironmentRepositoryError> {
        environment.validate()?;
        let encoded = serde_json::to_string(&environment).map_err(|error| {
            ControlledExecutionEnvironmentRepositoryError::Persistence(error.to_string())
        })?;
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_controlled_execution_environments
             (environment_id,execution_id,model_resolution_id,runtime_instance_id,
              provider_instance_id,environment_json,requested_at,prepared_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                environment.environment_id().as_str(),
                environment.execution_request().execution_id().as_str(),
                environment.resolution().resolution_id().as_str(),
                environment.runtime_activation().instance_id().as_str(),
                environment.provider_activation().instance_id().as_str(),
                encoded,
                environment.requested_at(),
                environment.prepared_at(),
            ],
        )
        .map_err(|error| {
            if error.to_string().contains("UNIQUE constraint failed") {
                ControlledExecutionEnvironmentRepositoryError::AlreadyRecorded(
                    environment.environment_id().clone(),
                )
            } else {
                ControlledExecutionEnvironmentRepositoryError::Persistence(error.to_string())
            }
        })?;
        Ok(())
    }

    fn get(
        &self,
        environment_id: &ControlledExecutionEnvironmentId,
    ) -> Result<Option<ControlledExecutionEnvironment>, ControlledExecutionEnvironmentRepositoryError>
    {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT environment_id,execution_id,model_resolution_id,runtime_instance_id,
                        provider_instance_id,environment_json,requested_at,prepared_at
                 FROM agent_os_controlled_execution_environments
                 WHERE environment_id=?1",
                [environment_id.as_str()],
                StoredControlledExecutionEnvironment::from_row,
            )
            .optional()
            .map_err(|error| {
                ControlledExecutionEnvironmentRepositoryError::Persistence(error.to_string())
            })?;
        value.map(Self::decode).transpose()
    }

    fn list(
        &self,
    ) -> Result<Vec<ControlledExecutionEnvironment>, ControlledExecutionEnvironmentRepositoryError>
    {
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn
            .prepare(
                "SELECT environment_id,execution_id,model_resolution_id,runtime_instance_id,
                        provider_instance_id,environment_json,requested_at,prepared_at
                 FROM agent_os_controlled_execution_environments
                 ORDER BY prepared_at, environment_id",
            )
            .map_err(|error| {
                ControlledExecutionEnvironmentRepositoryError::Persistence(error.to_string())
            })?;
        let rows = statement
            .query_map([], StoredControlledExecutionEnvironment::from_row)
            .map_err(|error| {
                ControlledExecutionEnvironmentRepositoryError::Persistence(error.to_string())
            })?;
        let mut environments = Vec::new();
        for row in rows {
            environments.push(Self::decode(row.map_err(|error| {
                ControlledExecutionEnvironmentRepositoryError::Persistence(error.to_string())
            })?)?);
        }
        Ok(environments)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        agent_provider_domain::{
            AgentProviderAdapterId, AgentProviderId, ProviderAvailability, ProviderProbe,
        },
        agent_provider_instance::{AgentProviderInstance, AgentProviderInstanceLifecycle},
        capability_domain::CapabilitySnapshotId,
        execution_domain::{ExecutionGovernanceEvidence, ExecutionModelBinding},
        model_domain::{
            ModelAvailability, ModelAvailabilityId, ModelAvailabilityStatus, ModelDescriptor,
            ModelId, ModelMetadata,
        },
        permission_domain::{AuthorizationDecisionId, PermissionGrantId},
        role_domain::RoleAssignmentId,
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeAdapterId, RuntimeBindingId,
            RuntimeBindingLifecycle, RuntimeExecutionId, RuntimeId,
        },
        runtime_instance_domain::{
            RuntimeHealthObservation, RuntimeHealthStatus, RuntimeInstance,
            RuntimeInstanceLifecycle,
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

    fn activation_snapshots() -> (RuntimeActivationSnapshot, ProviderActivationSnapshot) {
        let runtime = RuntimeInstance::new(
            crate::runtime_instance_domain::RuntimeInstanceId::new("runtime-instance:controlled")
                .unwrap(),
            RuntimeId::new("runtime:controlled").unwrap(),
            RuntimeAdapterId::new("runtime-adapter:controlled").unwrap(),
            10,
        )
        .unwrap()
        .transition_to(RuntimeInstanceLifecycle::Activating, 1, 11)
        .unwrap();
        let runtime = runtime
            .record_health(
                RuntimeHealthObservation::new(RuntimeHealthStatus::Healthy, 12, vec![]).unwrap(),
                2,
            )
            .unwrap()
            .transition_to(RuntimeInstanceLifecycle::Ready, 3, 12)
            .unwrap();
        let provider_id = AgentProviderId::new("provider:controlled").unwrap();
        let provider = AgentProviderInstance::new(
            crate::agent_provider_instance::AgentProviderInstanceId::new(
                "provider-instance:controlled",
            )
            .unwrap(),
            provider_id.clone(),
            AgentProviderAdapterId::new("provider-adapter:controlled").unwrap(),
            10,
        )
        .unwrap()
        .transition_to(AgentProviderInstanceLifecycle::Activating, 1, 11)
        .unwrap();
        let provider = provider
            .record_probe(
                ProviderProbe {
                    provider_id,
                    availability: ProviderAvailability::Registered,
                    diagnostics: vec![],
                },
                2,
                12,
            )
            .unwrap()
            .transition_to(AgentProviderInstanceLifecycle::Ready, 3, 12)
            .unwrap();
        let policy = ActivationEvidenceAgePolicy::new(60).unwrap();
        (
            RuntimeActivationSnapshot::capture(&runtime, 17, policy).unwrap(),
            ProviderActivationSnapshot::capture(&provider, 17, policy).unwrap(),
        )
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
        let (runtime, provider) = activation_snapshots();
        let environment = ControlledExecutionEnvironment::new(
            &request,
            resolved_model(),
            runtime,
            provider,
            isolation,
            ActivationEvidenceAgePolicy::new(60).unwrap(),
            18,
        )
        .unwrap();
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
        let (runtime, provider) = activation_snapshots();
        assert!(matches!(
            ControlledExecutionEnvironment::new(
                &request,
                resolved_model(),
                runtime,
                provider,
                isolation,
                ActivationEvidenceAgePolicy::new(60).unwrap(),
                18,
            ),
            Err(ControlledExecutionEnvironmentDomainError::ResolutionMismatch)
        ));
    }

    #[test]
    fn invalid_persisted_environment_is_rejected_during_deserialization() {
        let request = preparation_request();
        let isolation = ExecutionIsolationEvidence::preparation_only(
            request.isolation_id().clone(),
            "isolation-boundary:memory",
            17,
        )
        .unwrap();
        let (runtime, provider) = activation_snapshots();
        let environment = ControlledExecutionEnvironment::new(
            &request,
            resolved_model(),
            runtime,
            provider,
            isolation,
            ActivationEvidenceAgePolicy::new(60).unwrap(),
            18,
        )
        .unwrap();
        let mut value = serde_json::to_value(environment).unwrap();
        value["runtimeActivation"]["instanceRevision"] = serde_json::json!(0);
        assert!(serde_json::from_value::<ControlledExecutionEnvironment>(value).is_err());

        let request = preparation_request();
        let isolation = ExecutionIsolationEvidence::preparation_only(
            request.isolation_id().clone(),
            "isolation-boundary:memory",
            17,
        )
        .unwrap();
        let (runtime, provider) = activation_snapshots();
        let environment = ControlledExecutionEnvironment::new(
            &request,
            resolved_model(),
            runtime,
            provider,
            isolation,
            ActivationEvidenceAgePolicy::new(60).unwrap(),
            18,
        )
        .unwrap();
        let mut value = serde_json::to_value(environment).unwrap();
        value["runtimeActivation"]["adapterId"] = serde_json::json!("invalid adapter");
        assert!(serde_json::from_value::<ControlledExecutionEnvironment>(value).is_err());
    }

    #[test]
    fn sqlite_environment_round_trip_validates_and_is_immutable() {
        let request = preparation_request();
        let isolation = ExecutionIsolationEvidence::preparation_only(
            request.isolation_id().clone(),
            "isolation-boundary:memory",
            17,
        )
        .unwrap();
        let (runtime, provider) = activation_snapshots();
        let environment = ControlledExecutionEnvironment::new(
            &request,
            resolved_model(),
            runtime,
            provider,
            isolation,
            ActivationEvidenceAgePolicy::new(60).unwrap(),
            18,
        )
        .unwrap();
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteControlledExecutionEnvironmentRepository::new(database.clone());
        repository.insert(environment.clone()).unwrap();
        assert_eq!(
            repository.get(environment.environment_id()).unwrap(),
            Some(environment.clone())
        );

        let conn = database.conn.lock().unwrap();
        assert!(conn
            .execute(
                "UPDATE agent_os_controlled_execution_environments SET prepared_at=19
                 WHERE environment_id='environment:controlled'",
                [],
            )
            .is_err());
        assert!(conn
            .execute(
                "DELETE FROM agent_os_controlled_execution_environments
                 WHERE environment_id='environment:controlled'",
                [],
            )
            .is_err());
        conn.execute_batch(
            "DROP TRIGGER trg_agent_os_controlled_environment_update_forbidden;
             UPDATE agent_os_controlled_execution_environments
             SET execution_id='execution:tampered'
             WHERE environment_id='environment:controlled';",
        )
        .unwrap();
        drop(conn);
        assert!(repository.get(environment.environment_id()).is_err());
    }
}
