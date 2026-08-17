//! Versioned Permission policies and immutable authorization audit repositories.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::permission_domain::{
    AuthorizationDecision, AuthorizationDecisionId, PermissionCeiling, PermissionCeilingId,
    PermissionDomainError, PermissionGrant, PermissionGrantId, PermissionPolicy,
    PermissionPolicyId, PermissionRequest, PermissionRequestId,
};

#[derive(Debug, Error)]
pub enum PermissionRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] PermissionDomainError),
    #[error("Permission policy is already registered: {id} v{version}")]
    PolicyAlreadyRegistered {
        id: PermissionPolicyId,
        version: u16,
    },
    #[error("Permission ceiling is already registered: {id} v{version}")]
    CeilingAlreadyRegistered {
        id: PermissionCeilingId,
        version: u16,
    },
    #[error("Authorization Decision is already recorded: {0}")]
    DecisionAlreadyRecorded(AuthorizationDecisionId),
    #[error("Permission Request is already recorded: {0}")]
    RequestAlreadyRecorded(PermissionRequestId),
    #[error("Permission Grant is already recorded: {0}")]
    GrantAlreadyRecorded(PermissionGrantId),
    #[error("Authorization Decision and Permission Grant are inconsistent")]
    EvaluationMismatch,
    #[error("Permission repository lock failed: {0}")]
    RegistryLock(String),
}

pub trait PermissionRepository: Send + Sync {
    fn register_policy(&self, policy: PermissionPolicy) -> Result<(), PermissionRepositoryError>;
    fn latest_policy(
        &self,
        policy_id: &PermissionPolicyId,
    ) -> Result<Option<PermissionPolicy>, PermissionRepositoryError>;
    fn register_ceiling(&self, ceiling: PermissionCeiling)
        -> Result<(), PermissionRepositoryError>;
    fn get_ceiling(
        &self,
        ceiling_id: &PermissionCeilingId,
        version: u16,
    ) -> Result<Option<PermissionCeiling>, PermissionRepositoryError>;
    fn record_evaluation(
        &self,
        request: PermissionRequest,
        decision: AuthorizationDecision,
        grant: Option<PermissionGrant>,
    ) -> Result<(), PermissionRepositoryError>;
    fn get_request(
        &self,
        request_id: &PermissionRequestId,
    ) -> Result<Option<PermissionRequest>, PermissionRepositoryError>;
    fn get_decision(
        &self,
        decision_id: &AuthorizationDecisionId,
    ) -> Result<Option<AuthorizationDecision>, PermissionRepositoryError>;
    fn get_grant(
        &self,
        grant_id: &PermissionGrantId,
    ) -> Result<Option<PermissionGrant>, PermissionRepositoryError>;
}

type EvaluationMaps = (
    HashMap<PermissionRequestId, PermissionRequest>,
    HashMap<AuthorizationDecisionId, AuthorizationDecision>,
    HashMap<PermissionGrantId, PermissionGrant>,
);

#[derive(Clone, Default)]
pub struct InMemoryPermissionRepository {
    policies: Arc<RwLock<HashMap<(PermissionPolicyId, u16), PermissionPolicy>>>,
    ceilings: Arc<RwLock<HashMap<(PermissionCeilingId, u16), PermissionCeiling>>>,
    evaluations: Arc<RwLock<EvaluationMaps>>,
}

impl PermissionRepository for InMemoryPermissionRepository {
    fn register_policy(&self, policy: PermissionPolicy) -> Result<(), PermissionRepositoryError> {
        let key = (policy.id().clone(), policy.version());
        let mut policies = self
            .policies
            .write()
            .map_err(|e| PermissionRepositoryError::RegistryLock(e.to_string()))?;
        if policies.contains_key(&key) {
            return Err(PermissionRepositoryError::PolicyAlreadyRegistered {
                id: key.0,
                version: key.1,
            });
        }
        policies.insert(key, policy);
        Ok(())
    }

    fn latest_policy(
        &self,
        policy_id: &PermissionPolicyId,
    ) -> Result<Option<PermissionPolicy>, PermissionRepositoryError> {
        let policies = self
            .policies
            .read()
            .map_err(|e| PermissionRepositoryError::RegistryLock(e.to_string()))?;
        Ok(policies
            .values()
            .filter(|policy| policy.id() == policy_id)
            .max_by_key(|policy| policy.version())
            .cloned())
    }

    fn register_ceiling(
        &self,
        ceiling: PermissionCeiling,
    ) -> Result<(), PermissionRepositoryError> {
        let key = (ceiling.id().clone(), ceiling.version());
        let mut ceilings = self
            .ceilings
            .write()
            .map_err(|e| PermissionRepositoryError::RegistryLock(e.to_string()))?;
        if ceilings.contains_key(&key) {
            return Err(PermissionRepositoryError::CeilingAlreadyRegistered {
                id: key.0,
                version: key.1,
            });
        }
        ceilings.insert(key, ceiling);
        Ok(())
    }

    fn get_ceiling(
        &self,
        ceiling_id: &PermissionCeilingId,
        version: u16,
    ) -> Result<Option<PermissionCeiling>, PermissionRepositoryError> {
        let ceilings = self
            .ceilings
            .read()
            .map_err(|e| PermissionRepositoryError::RegistryLock(e.to_string()))?;
        Ok(ceilings.get(&(ceiling_id.clone(), version)).cloned())
    }

    fn record_evaluation(
        &self,
        request: PermissionRequest,
        decision: AuthorizationDecision,
        grant: Option<PermissionGrant>,
    ) -> Result<(), PermissionRepositoryError> {
        if decision.request_id() != request.id()
            || decision.execution_id() != request.execution_id()
            || decision.role_assignment_id() != request.role_assignment_id()
            || decision.capability_snapshot_id() != request.capability_snapshot_id()
            || decision.grant_id() != grant.as_ref().map(PermissionGrant::id)
            || grant.as_ref().is_some_and(|grant| {
                grant.request_id() != request.id()
                    || grant.agent_id() != request.agent_id()
                    || grant.decision_id() != decision.id()
                    || grant.execution_id() != decision.execution_id()
                    || grant.role_assignment_id() != decision.role_assignment_id()
                    || grant.capability_snapshot_id() != decision.capability_snapshot_id()
            })
        {
            return Err(PermissionRepositoryError::EvaluationMismatch);
        }
        let mut evaluations = self
            .evaluations
            .write()
            .map_err(|e| PermissionRepositoryError::RegistryLock(e.to_string()))?;
        if evaluations.0.contains_key(request.id()) {
            return Err(PermissionRepositoryError::RequestAlreadyRecorded(
                request.id().clone(),
            ));
        }
        if evaluations.1.contains_key(decision.id()) {
            return Err(PermissionRepositoryError::DecisionAlreadyRecorded(
                decision.id().clone(),
            ));
        }
        if let Some(grant) = &grant {
            if evaluations.2.contains_key(grant.id()) {
                return Err(PermissionRepositoryError::GrantAlreadyRecorded(
                    grant.id().clone(),
                ));
            }
        }
        if let Some(grant) = grant {
            evaluations.2.insert(grant.id().clone(), grant);
        }
        evaluations.1.insert(decision.id().clone(), decision);
        evaluations.0.insert(request.id().clone(), request);
        Ok(())
    }

    fn get_request(
        &self,
        request_id: &PermissionRequestId,
    ) -> Result<Option<PermissionRequest>, PermissionRepositoryError> {
        let evaluations = self
            .evaluations
            .read()
            .map_err(|e| PermissionRepositoryError::RegistryLock(e.to_string()))?;
        Ok(evaluations.0.get(request_id).cloned())
    }

    fn get_decision(
        &self,
        decision_id: &AuthorizationDecisionId,
    ) -> Result<Option<AuthorizationDecision>, PermissionRepositoryError> {
        let evaluations = self
            .evaluations
            .read()
            .map_err(|e| PermissionRepositoryError::RegistryLock(e.to_string()))?;
        Ok(evaluations.1.get(decision_id).cloned())
    }

    fn get_grant(
        &self,
        grant_id: &PermissionGrantId,
    ) -> Result<Option<PermissionGrant>, PermissionRepositoryError> {
        let evaluations = self
            .evaluations
            .read()
            .map_err(|e| PermissionRepositoryError::RegistryLock(e.to_string()))?;
        Ok(evaluations.2.get(grant_id).cloned())
    }
}
