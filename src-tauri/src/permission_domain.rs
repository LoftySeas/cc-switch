//! Deny-by-default Permission policy, request, decision, and Grant contracts.
//!
//! Permission is independent from Capability and Role. Capability evidence is a
//! prerequisite for enforceability; Role Assignment supplies context and
//! constraints but neither one creates authority.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    capability_domain::{CapabilityId, CapabilitySnapshotId},
    role_domain::RoleAssignmentId,
    runtime_domain::RuntimeExecutionId,
};

const MAX_ID_LENGTH: usize = 192;
const MAX_TEXT_LENGTH: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PermissionDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Permission policy version must be positive")]
    InvalidVersion,
    #[error("Permission policy or ceiling must contain at least one rule")]
    EmptyRules,
    #[error("Permission Request must contain at least one bounded claim")]
    EmptyClaims,
    #[error("Permission Request must reference at least one governing policy")]
    EmptyPolicies,
    #[error("Permission timestamp or validity interval is invalid")]
    InvalidValidity,
    #[error("Allowed authorization decision requires a Grant reference")]
    AllowedWithoutGrant,
    #[error("Non-allowed authorization decision cannot reference a Grant")]
    NonAllowedWithGrant,
    #[error("Permission Grant must contain at least one claim")]
    EmptyGrant,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, PermissionDomainError> {
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

typed_id!(PermissionAction, "Permission action");
typed_id!(PermissionPolicyId, "Permission policy ID");
typed_id!(PermissionCeilingId, "Permission ceiling ID");
typed_id!(PermissionRequestId, "Permission Request ID");
typed_id!(AuthorizationDecisionId, "Authorization Decision ID");
typed_id!(PermissionGrantId, "Permission Grant ID");

fn identifier(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, PermissionDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(PermissionDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(PermissionDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_control) || value.chars().any(char::is_whitespace) {
        return Err(PermissionDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn text(field: &'static str, value: impl Into<String>) -> Result<String, PermissionDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(PermissionDomainError::Empty { field });
    }
    if value.chars().count() > MAX_TEXT_LENGTH {
        return Err(PermissionDomainError::TooLong {
            field,
            max: MAX_TEXT_LENGTH,
        });
    }
    Ok(value.to_string())
}

fn validated_constraints(
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, PermissionDomainError> {
    for (key, value) in &values {
        identifier("Permission constraint key", key.clone())?;
        text("Permission constraint value", value.clone())?;
    }
    Ok(values)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicyLayer {
    Repository,
    HumanOwner,
    Team,
    Workflow,
    RoleAssignment,
    Workspace,
    Environment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRuleEffect {
    Allow,
    Deny,
    RequireApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    effect: PermissionRuleEffect,
    action: PermissionAction,
    resource_selector: String,
    constraints: BTreeMap<String, String>,
}

impl PermissionRule {
    pub fn new(
        effect: PermissionRuleEffect,
        action: PermissionAction,
        resource_selector: impl Into<String>,
        constraints: BTreeMap<String, String>,
    ) -> Result<Self, PermissionDomainError> {
        Ok(Self {
            effect,
            action,
            resource_selector: identifier("Permission resource selector", resource_selector)?,
            constraints: validated_constraints(constraints)?,
        })
    }

    pub fn effect(&self) -> PermissionRuleEffect {
        self.effect
    }
    pub fn action(&self) -> &PermissionAction {
        &self.action
    }
    pub fn resource_selector(&self) -> &str {
        &self.resource_selector
    }
    pub fn constraints(&self) -> &BTreeMap<String, String> {
        &self.constraints
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPolicy {
    id: PermissionPolicyId,
    version: u16,
    layer: PermissionPolicyLayer,
    owner_ref: String,
    rules: Vec<PermissionRule>,
}

impl PermissionPolicy {
    pub fn new(
        id: PermissionPolicyId,
        version: u16,
        layer: PermissionPolicyLayer,
        owner_ref: impl Into<String>,
        rules: Vec<PermissionRule>,
    ) -> Result<Self, PermissionDomainError> {
        if version == 0 {
            return Err(PermissionDomainError::InvalidVersion);
        }
        if rules.is_empty() {
            return Err(PermissionDomainError::EmptyRules);
        }
        Ok(Self {
            id,
            version,
            layer,
            owner_ref: identifier("Permission policy owner reference", owner_ref)?,
            rules,
        })
    }

    pub fn id(&self) -> &PermissionPolicyId {
        &self.id
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn layer(&self) -> PermissionPolicyLayer {
        self.layer
    }
    pub fn owner_ref(&self) -> &str {
        &self.owner_ref
    }
    pub fn rules(&self) -> &[PermissionRule] {
        &self.rules
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCeiling {
    id: PermissionCeilingId,
    version: u16,
    agent_id: String,
    rules: Vec<PermissionRule>,
}

impl PermissionCeiling {
    pub fn new(
        id: PermissionCeilingId,
        version: u16,
        agent_id: impl Into<String>,
        rules: Vec<PermissionRule>,
    ) -> Result<Self, PermissionDomainError> {
        if version == 0 {
            return Err(PermissionDomainError::InvalidVersion);
        }
        if rules.is_empty() {
            return Err(PermissionDomainError::EmptyRules);
        }
        Ok(Self {
            id,
            version,
            agent_id: identifier("Agent ID", agent_id)?,
            rules,
        })
    }

    pub fn id(&self) -> &PermissionCeilingId {
        &self.id
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn rules(&self) -> &[PermissionRule] {
        &self.rules
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionClaim {
    action: PermissionAction,
    resource: String,
    constraints: BTreeMap<String, String>,
    enforcement_capability_id: CapabilityId,
}

impl PermissionClaim {
    pub fn new(
        action: PermissionAction,
        resource: impl Into<String>,
        constraints: BTreeMap<String, String>,
        enforcement_capability_id: CapabilityId,
    ) -> Result<Self, PermissionDomainError> {
        Ok(Self {
            action,
            resource: identifier("Permission resource", resource)?,
            constraints: validated_constraints(constraints)?,
            enforcement_capability_id,
        })
    }

    pub fn action(&self) -> &PermissionAction {
        &self.action
    }
    pub fn resource(&self) -> &str {
        &self.resource
    }
    pub fn constraints(&self) -> &BTreeMap<String, String> {
        &self.constraints
    }
    pub fn enforcement_capability_id(&self) -> &CapabilityId {
        &self.enforcement_capability_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalEvidence {
    reference: String,
    action: PermissionAction,
    resource: String,
    valid_until: i64,
}

impl ApprovalEvidence {
    pub fn new(
        reference: impl Into<String>,
        action: PermissionAction,
        resource: impl Into<String>,
        valid_until: i64,
    ) -> Result<Self, PermissionDomainError> {
        if valid_until < 0 {
            return Err(PermissionDomainError::InvalidValidity);
        }
        Ok(Self {
            reference: identifier("Approval evidence reference", reference)?,
            action,
            resource: identifier("Approval resource", resource)?,
            valid_until,
        })
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }
    pub fn action(&self) -> &PermissionAction {
        &self.action
    }
    pub fn resource(&self) -> &str {
        &self.resource
    }
    pub fn valid_until(&self) -> i64 {
        self.valid_until
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    id: PermissionRequestId,
    execution_id: RuntimeExecutionId,
    agent_id: String,
    role_assignment_id: RoleAssignmentId,
    scope_ref: String,
    capability_snapshot_id: CapabilitySnapshotId,
    ceiling_id: PermissionCeilingId,
    ceiling_version: u16,
    policy_ids: Vec<PermissionPolicyId>,
    claims: Vec<PermissionClaim>,
    approvals: Vec<ApprovalEvidence>,
    requested_at: i64,
    requested_until: i64,
}

impl PermissionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: PermissionRequestId,
        execution_id: RuntimeExecutionId,
        agent_id: impl Into<String>,
        role_assignment_id: RoleAssignmentId,
        scope_ref: impl Into<String>,
        capability_snapshot_id: CapabilitySnapshotId,
        ceiling_id: PermissionCeilingId,
        ceiling_version: u16,
        policy_ids: Vec<PermissionPolicyId>,
        claims: Vec<PermissionClaim>,
        approvals: Vec<ApprovalEvidence>,
        requested_at: i64,
        requested_until: i64,
    ) -> Result<Self, PermissionDomainError> {
        if ceiling_version == 0 {
            return Err(PermissionDomainError::InvalidVersion);
        }
        if claims.is_empty() {
            return Err(PermissionDomainError::EmptyClaims);
        }
        if policy_ids.is_empty() {
            return Err(PermissionDomainError::EmptyPolicies);
        }
        if requested_at < 0 || requested_until < requested_at {
            return Err(PermissionDomainError::InvalidValidity);
        }
        Ok(Self {
            id,
            execution_id,
            agent_id: identifier("Agent ID", agent_id)?,
            role_assignment_id,
            scope_ref: identifier("Permission Request scope reference", scope_ref)?,
            capability_snapshot_id,
            ceiling_id,
            ceiling_version,
            policy_ids,
            claims,
            approvals,
            requested_at,
            requested_until,
        })
    }

    pub fn id(&self) -> &PermissionRequestId {
        &self.id
    }
    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn role_assignment_id(&self) -> &RoleAssignmentId {
        &self.role_assignment_id
    }
    pub fn scope_ref(&self) -> &str {
        &self.scope_ref
    }
    pub fn capability_snapshot_id(&self) -> &CapabilitySnapshotId {
        &self.capability_snapshot_id
    }
    pub fn ceiling_id(&self) -> &PermissionCeilingId {
        &self.ceiling_id
    }
    pub fn ceiling_version(&self) -> u16 {
        self.ceiling_version
    }
    pub fn policy_ids(&self) -> &[PermissionPolicyId] {
        &self.policy_ids
    }
    pub fn claims(&self) -> &[PermissionClaim] {
        &self.claims
    }
    pub fn approvals(&self) -> &[ApprovalEvidence] {
        &self.approvals
    }
    pub fn requested_at(&self) -> i64 {
        self.requested_at
    }
    pub fn requested_until(&self) -> i64 {
        self.requested_until
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPolicyVersionRef {
    policy_id: PermissionPolicyId,
    version: u16,
    layer: PermissionPolicyLayer,
}

impl PermissionPolicyVersionRef {
    pub(crate) fn from_policy(policy: &PermissionPolicy) -> Self {
        Self {
            policy_id: policy.id().clone(),
            version: policy.version(),
            layer: policy.layer(),
        }
    }
    pub fn policy_id(&self) -> &PermissionPolicyId {
        &self.policy_id
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn layer(&self) -> PermissionPolicyLayer {
        self.layer
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationDecisionStatus {
    Allowed,
    Denied,
    RequiresApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationDecision {
    id: AuthorizationDecisionId,
    request_id: PermissionRequestId,
    execution_id: RuntimeExecutionId,
    status: AuthorizationDecisionStatus,
    policy_versions: Vec<PermissionPolicyVersionRef>,
    ceiling_id: PermissionCeilingId,
    ceiling_version: u16,
    role_assignment_id: RoleAssignmentId,
    capability_snapshot_id: CapabilitySnapshotId,
    reasons: Vec<String>,
    grant_id: Option<PermissionGrantId>,
    decided_at: i64,
}

impl AuthorizationDecision {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: AuthorizationDecisionId,
        request: &PermissionRequest,
        status: AuthorizationDecisionStatus,
        policy_versions: Vec<PermissionPolicyVersionRef>,
        reasons: Vec<String>,
        grant_id: Option<PermissionGrantId>,
        decided_at: i64,
    ) -> Result<Self, PermissionDomainError> {
        if decided_at < request.requested_at() {
            return Err(PermissionDomainError::InvalidValidity);
        }
        if status == AuthorizationDecisionStatus::Allowed && grant_id.is_none() {
            return Err(PermissionDomainError::AllowedWithoutGrant);
        }
        if status != AuthorizationDecisionStatus::Allowed && grant_id.is_some() {
            return Err(PermissionDomainError::NonAllowedWithGrant);
        }
        Ok(Self {
            id,
            request_id: request.id().clone(),
            execution_id: request.execution_id().clone(),
            status,
            policy_versions,
            ceiling_id: request.ceiling_id().clone(),
            ceiling_version: request.ceiling_version(),
            role_assignment_id: request.role_assignment_id().clone(),
            capability_snapshot_id: request.capability_snapshot_id().clone(),
            reasons: reasons
                .into_iter()
                .map(|reason| text("Authorization Decision reason", reason))
                .collect::<Result<Vec<_>, _>>()?,
            grant_id,
            decided_at,
        })
    }

    pub fn id(&self) -> &AuthorizationDecisionId {
        &self.id
    }
    pub fn request_id(&self) -> &PermissionRequestId {
        &self.request_id
    }
    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }
    pub fn status(&self) -> AuthorizationDecisionStatus {
        self.status
    }
    pub fn policy_versions(&self) -> &[PermissionPolicyVersionRef] {
        &self.policy_versions
    }
    pub fn ceiling_id(&self) -> &PermissionCeilingId {
        &self.ceiling_id
    }
    pub fn ceiling_version(&self) -> u16 {
        self.ceiling_version
    }
    pub fn role_assignment_id(&self) -> &RoleAssignmentId {
        &self.role_assignment_id
    }
    pub fn capability_snapshot_id(&self) -> &CapabilitySnapshotId {
        &self.capability_snapshot_id
    }
    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
    pub fn grant_id(&self) -> Option<&PermissionGrantId> {
        self.grant_id.as_ref()
    }
    pub fn decided_at(&self) -> i64 {
        self.decided_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionGrant {
    id: PermissionGrantId,
    decision_id: AuthorizationDecisionId,
    request_id: PermissionRequestId,
    execution_id: RuntimeExecutionId,
    agent_id: String,
    role_assignment_id: RoleAssignmentId,
    capability_snapshot_id: CapabilitySnapshotId,
    claims: Vec<PermissionClaim>,
    issued_at: i64,
    expires_at: i64,
}

impl PermissionGrant {
    pub(crate) fn new(
        id: PermissionGrantId,
        decision_id: AuthorizationDecisionId,
        request: &PermissionRequest,
        issued_at: i64,
    ) -> Result<Self, PermissionDomainError> {
        if request.claims().is_empty() {
            return Err(PermissionDomainError::EmptyGrant);
        }
        if issued_at < request.requested_at() || issued_at > request.requested_until() {
            return Err(PermissionDomainError::InvalidValidity);
        }
        Ok(Self {
            id,
            decision_id,
            request_id: request.id().clone(),
            execution_id: request.execution_id().clone(),
            agent_id: request.agent_id().to_string(),
            role_assignment_id: request.role_assignment_id().clone(),
            capability_snapshot_id: request.capability_snapshot_id().clone(),
            claims: request.claims().to_vec(),
            issued_at,
            expires_at: request.requested_until(),
        })
    }

    pub fn id(&self) -> &PermissionGrantId {
        &self.id
    }
    pub fn decision_id(&self) -> &AuthorizationDecisionId {
        &self.decision_id
    }
    pub fn request_id(&self) -> &PermissionRequestId {
        &self.request_id
    }
    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn role_assignment_id(&self) -> &RoleAssignmentId {
        &self.role_assignment_id
    }
    pub fn capability_snapshot_id(&self) -> &CapabilitySnapshotId {
        &self.capability_snapshot_id
    }
    pub fn claims(&self) -> &[PermissionClaim] {
        &self.claims
    }
    pub fn issued_at(&self) -> i64 {
        self.issued_at
    }
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
    pub fn is_valid_at(&self, at: i64) -> bool {
        at >= self.issued_at && at <= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_policy_fails_closed() {
        assert!(matches!(
            PermissionPolicy::new(
                PermissionPolicyId::new("policy:repository").unwrap(),
                1,
                PermissionPolicyLayer::Repository,
                "owner:repository",
                Vec::new(),
            ),
            Err(PermissionDomainError::EmptyRules)
        ));
    }

    #[test]
    fn permission_claim_keeps_capability_as_enforcement_evidence_only() {
        let claim = PermissionClaim::new(
            PermissionAction::new("workspace.write").unwrap(),
            "workspace:/repo",
            BTreeMap::new(),
            CapabilityId::new("workspace.write-enforcement").unwrap(),
        )
        .unwrap();
        assert_eq!(claim.action().as_str(), "workspace.write");
        assert_eq!(
            claim.enforcement_capability_id().as_str(),
            "workspace.write-enforcement"
        );
    }
}
