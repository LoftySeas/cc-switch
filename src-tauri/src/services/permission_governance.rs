//! Deterministic, explainable, deny-by-default Permission evaluation.

use std::collections::HashSet;

use thiserror::Error;

use crate::{
    capability_registry::{CapabilityRegistryError, CapabilitySnapshotRepository},
    permission_domain::{
        ApprovalEvidence, AuthorizationDecision, AuthorizationDecisionId,
        AuthorizationDecisionStatus, PermissionCeiling, PermissionClaim, PermissionGrant,
        PermissionGrantId, PermissionPolicy, PermissionPolicyId, PermissionPolicyVersionRef,
        PermissionRequest, PermissionRequestId, PermissionRule, PermissionRuleEffect,
    },
    permission_repository::{PermissionRepository, PermissionRepositoryError},
    role_domain::RoleAssignment,
    role_repository::{RoleRepository, RoleRepositoryError},
};

#[derive(Debug, Error)]
pub enum PermissionGovernanceError {
    #[error(transparent)]
    PermissionRepository(#[from] PermissionRepositoryError),
    #[error(transparent)]
    CapabilityRepository(#[from] CapabilityRegistryError),
    #[error(transparent)]
    RoleRepository(#[from] RoleRepositoryError),
    #[error("Role Assignment is missing: {0}")]
    MissingRoleAssignment(String),
    #[error("Role Assignment is not effective for the Permission Request: {0}")]
    InactiveRoleAssignment(String),
    #[error("Permission Request Agent does not match Role Assignment")]
    RoleAgentMismatch,
    #[error("Permission Request scope does not match Role Assignment")]
    RoleScopeMismatch,
    #[error("Capability snapshot is missing: {0}")]
    MissingCapabilitySnapshot(String),
    #[error("Capability snapshot does not belong to this execution")]
    CapabilityExecutionMismatch,
    #[error("Permission ceiling is missing: {0} v{1}")]
    MissingCeiling(String, u16),
    #[error("Permission ceiling does not belong to the requesting Agent")]
    CeilingAgentMismatch,
    #[error("Permission policy is missing: {0}")]
    MissingPolicy(String),
    #[error("Role Assignment Permission constraint policy reference is invalid: {0}")]
    InvalidRolePolicyReference(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationEvaluation {
    decision: AuthorizationDecision,
    grant: Option<PermissionGrant>,
}

impl AuthorizationEvaluation {
    pub fn decision(&self) -> &AuthorizationDecision {
        &self.decision
    }
    pub fn grant(&self) -> Option<&PermissionGrant> {
        self.grant.as_ref()
    }
}

pub struct PermissionGovernanceService<P, C, R> {
    permissions: P,
    capabilities: C,
    roles: R,
}

impl<P, C, R> PermissionGovernanceService<P, C, R>
where
    P: PermissionRepository,
    C: CapabilitySnapshotRepository,
    R: RoleRepository,
{
    pub fn new(permissions: P, capabilities: C, roles: R) -> Self {
        Self {
            permissions,
            capabilities,
            roles,
        }
    }

    pub fn register_policy(
        &self,
        policy: PermissionPolicy,
    ) -> Result<(), PermissionGovernanceError> {
        Ok(self.permissions.register_policy(policy)?)
    }

    pub fn register_ceiling(
        &self,
        ceiling: PermissionCeiling,
    ) -> Result<(), PermissionGovernanceError> {
        Ok(self.permissions.register_ceiling(ceiling)?)
    }

    pub fn get_request(
        &self,
        request_id: &PermissionRequestId,
    ) -> Result<Option<PermissionRequest>, PermissionGovernanceError> {
        Ok(self.permissions.get_request(request_id)?)
    }

    pub fn get_decision(
        &self,
        decision_id: &AuthorizationDecisionId,
    ) -> Result<Option<AuthorizationDecision>, PermissionGovernanceError> {
        Ok(self.permissions.get_decision(decision_id)?)
    }

    pub fn get_grant(
        &self,
        grant_id: &PermissionGrantId,
    ) -> Result<Option<PermissionGrant>, PermissionGovernanceError> {
        Ok(self.permissions.get_grant(grant_id)?)
    }

    pub fn evaluate(
        &self,
        request: PermissionRequest,
        decided_at: i64,
    ) -> Result<AuthorizationEvaluation, PermissionGovernanceError> {
        let assignment = self.require_assignment(&request)?;
        let snapshot = self
            .capabilities
            .get_snapshot(request.capability_snapshot_id())?
            .ok_or_else(|| {
                PermissionGovernanceError::MissingCapabilitySnapshot(
                    request.capability_snapshot_id().to_string(),
                )
            })?;
        if snapshot.execution_id() != request.execution_id() {
            return Err(PermissionGovernanceError::CapabilityExecutionMismatch);
        }
        let ceiling = self
            .permissions
            .get_ceiling(request.ceiling_id(), request.ceiling_version())?
            .ok_or_else(|| {
                PermissionGovernanceError::MissingCeiling(
                    request.ceiling_id().to_string(),
                    request.ceiling_version(),
                )
            })?;
        if ceiling.agent_id() != request.agent_id() {
            return Err(PermissionGovernanceError::CeilingAgentMismatch);
        }
        let policies = self.load_policies(&request, &assignment)?;
        let mut denied_reasons = if decided_at > request.requested_until() {
            vec!["Permission Request validity has expired".into()]
        } else {
            Vec::new()
        };
        if !policies.iter().any(|policy| {
            policy.layer() == crate::permission_domain::PermissionPolicyLayer::Repository
        }) {
            denied_reasons.push("No repository-level governing policy is present".into());
        }
        let mut approval_reasons = Vec::new();

        for claim in request.claims() {
            if !snapshot.satisfies(claim.enforcement_capability_id()) {
                denied_reasons.push(format!(
                    "Enforcement Capability {} is not satisfied for {} on {}",
                    claim.enforcement_capability_id(),
                    claim.action(),
                    claim.resource()
                ));
                continue;
            }
            evaluate_rules(
                "Agent Permission ceiling",
                ceiling.rules(),
                claim,
                request.approvals(),
                decided_at,
                &mut denied_reasons,
                &mut approval_reasons,
            );
            for policy in &policies {
                evaluate_rules(
                    &format!("Policy {} v{}", policy.id(), policy.version()),
                    policy.rules(),
                    claim,
                    request.approvals(),
                    decided_at,
                    &mut denied_reasons,
                    &mut approval_reasons,
                );
            }
        }

        let status = if !denied_reasons.is_empty() {
            AuthorizationDecisionStatus::Denied
        } else if !approval_reasons.is_empty() {
            AuthorizationDecisionStatus::RequiresApproval
        } else {
            AuthorizationDecisionStatus::Allowed
        };
        let reasons = match status {
            AuthorizationDecisionStatus::Denied => denied_reasons,
            AuthorizationDecisionStatus::RequiresApproval => approval_reasons,
            AuthorizationDecisionStatus::Allowed => vec![
                "Every requested claim is allowed by every policy layer and the Agent ceiling"
                    .into(),
            ],
        };
        let decision_id = AuthorizationDecisionId::new(uuid::Uuid::new_v4().to_string())
            .map_err(PermissionRepositoryError::from)?;
        let grant_id = (status == AuthorizationDecisionStatus::Allowed)
            .then(|| PermissionGrantId::new(uuid::Uuid::new_v4().to_string()))
            .transpose()
            .map_err(PermissionRepositoryError::from)?;
        let grant = grant_id
            .as_ref()
            .map(|grant_id| {
                PermissionGrant::new(grant_id.clone(), decision_id.clone(), &request, decided_at)
            })
            .transpose()
            .map_err(PermissionRepositoryError::from)?;
        let decision = AuthorizationDecision::new(
            decision_id,
            &request,
            status,
            policies
                .iter()
                .map(PermissionPolicyVersionRef::from_policy)
                .collect(),
            reasons,
            grant_id,
            decided_at,
        )
        .map_err(PermissionRepositoryError::from)?;
        self.permissions
            .record_evaluation(request, decision.clone(), grant.clone())?;
        Ok(AuthorizationEvaluation { decision, grant })
    }

    fn require_assignment(
        &self,
        request: &PermissionRequest,
    ) -> Result<RoleAssignment, PermissionGovernanceError> {
        let assignment = self
            .roles
            .get_assignment(request.role_assignment_id())?
            .ok_or_else(|| {
                PermissionGovernanceError::MissingRoleAssignment(
                    request.role_assignment_id().to_string(),
                )
            })?;
        if assignment.agent_id() != request.agent_id() {
            return Err(PermissionGovernanceError::RoleAgentMismatch);
        }
        if assignment.scope().reference() != request.scope_ref() {
            return Err(PermissionGovernanceError::RoleScopeMismatch);
        }
        if !assignment.is_effective(request.requested_at()) {
            return Err(PermissionGovernanceError::InactiveRoleAssignment(
                assignment.id().to_string(),
            ));
        }
        Ok(assignment)
    }

    fn load_policies(
        &self,
        request: &PermissionRequest,
        assignment: &RoleAssignment,
    ) -> Result<Vec<PermissionPolicy>, PermissionGovernanceError> {
        let mut ids = request.policy_ids().iter().cloned().collect::<HashSet<_>>();
        for reference in assignment.permission_constraint_policy_refs() {
            ids.insert(PermissionPolicyId::new(reference.clone()).map_err(|_| {
                PermissionGovernanceError::InvalidRolePolicyReference(reference.clone())
            })?);
        }
        let mut policies = ids
            .into_iter()
            .map(|id| {
                self.permissions
                    .latest_policy(&id)?
                    .ok_or_else(|| PermissionGovernanceError::MissingPolicy(id.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        policies.sort_by(|left, right| {
            left.layer()
                .cmp(&right.layer())
                .then_with(|| left.id().cmp(right.id()))
        });
        Ok(policies)
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_rules(
    source: &str,
    rules: &[PermissionRule],
    claim: &PermissionClaim,
    approvals: &[ApprovalEvidence],
    evaluated_at: i64,
    denied_reasons: &mut Vec<String>,
    approval_reasons: &mut Vec<String>,
) {
    let matching = rules
        .iter()
        .filter(|rule| selector_matches(rule, claim))
        .collect::<Vec<_>>();
    if matching
        .iter()
        .any(|rule| rule.effect() == PermissionRuleEffect::Deny)
    {
        denied_reasons.push(format!(
            "{source} explicitly denies {} on {}",
            claim.action(),
            claim.resource()
        ));
        return;
    }
    let conditional = matching
        .iter()
        .any(|rule| rule.effect() == PermissionRuleEffect::RequireApproval);
    let allowed = matching.iter().any(|rule| {
        matches!(
            rule.effect(),
            PermissionRuleEffect::Allow | PermissionRuleEffect::RequireApproval
        ) && constraints_allow(rule, claim)
    });
    if !allowed {
        denied_reasons.push(format!(
            "{source} has no matching allow for {} on {}",
            claim.action(),
            claim.resource()
        ));
        return;
    }
    if conditional
        && !approvals.iter().any(|approval| {
            approval.action() == claim.action()
                && approval.resource() == claim.resource()
                && approval.valid_until() >= evaluated_at
        })
    {
        approval_reasons.push(format!(
            "{source} requires current approval for {} on {}",
            claim.action(),
            claim.resource()
        ));
    }
}

fn selector_matches(rule: &PermissionRule, claim: &PermissionClaim) -> bool {
    (rule.action().as_str() == "*" || rule.action() == claim.action())
        && (rule.resource_selector() == "*" || rule.resource_selector() == claim.resource())
}

fn constraints_allow(rule: &PermissionRule, claim: &PermissionClaim) -> bool {
    claim.constraints().iter().all(|(key, requested)| {
        rule.constraints()
            .get(key)
            .is_some_and(|allowed| allowed == "*" || allowed == requested)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        capability_domain::{
            CapabilityDefinition, CapabilityEvidence, CapabilityEvidenceId,
            CapabilityEvidenceSourceKind, CapabilityId, CapabilityRequirement,
            CapabilityRequirementLevel, CapabilitySupportState,
        },
        capability_registry::{CapabilityRegistry, InMemoryCapabilityRegistry},
        permission_domain::{
            ApprovalEvidence, PermissionAction, PermissionCeiling, PermissionCeilingId,
            PermissionClaim, PermissionPolicyLayer, PermissionRequestId,
        },
        permission_repository::InMemoryPermissionRepository,
        role_domain::{
            RoleAssignmentId, RoleAssignmentLifecycle, RoleAssignmentScope,
            RoleAssignmentScopeKind, RoleId,
        },
        role_repository::InMemoryRoleRepository,
        runtime_domain::RuntimeExecutionId,
        services::capability_governance::CapabilityGovernanceService,
    };

    const REPOSITORY_POLICY: &str = "policy:repository";
    const ROLE_POLICY: &str = "policy:role-constraint";

    struct Fixture {
        permissions: InMemoryPermissionRepository,
        capabilities: InMemoryCapabilityRegistry,
        roles: InMemoryRoleRepository,
        snapshot_id: crate::capability_domain::CapabilitySnapshotId,
    }

    fn rule(effect: PermissionRuleEffect) -> PermissionRule {
        PermissionRule::new(
            effect,
            PermissionAction::new("workspace.write").unwrap(),
            "workspace:/repo",
            BTreeMap::from([("mode".into(), "patch".into())]),
        )
        .unwrap()
    }

    fn fixture(role_effect: PermissionRuleEffect) -> Fixture {
        let capabilities = InMemoryCapabilityRegistry::default();
        let capability_id = CapabilityId::new("workspace.write-enforcement").unwrap();
        capabilities
            .register_definition(
                CapabilityDefinition::new(
                    capability_id.clone(),
                    1,
                    "Workspace write enforcement",
                    "Enforces bounded workspace writes",
                    BTreeMap::new(),
                )
                .unwrap(),
            )
            .unwrap();
        capabilities
            .register_evidence(
                CapabilityEvidence::new(
                    CapabilityEvidenceId::new("evidence:write").unwrap(),
                    capability_id.clone(),
                    "runtime:one",
                    CapabilityEvidenceSourceKind::Runtime,
                    1,
                    CapabilitySupportState::Supported,
                    BTreeMap::new(),
                    10,
                    100,
                    "probe:one",
                )
                .unwrap(),
            )
            .unwrap();
        let snapshot = CapabilityGovernanceService::new(capabilities.clone())
            .resolve(
                RuntimeExecutionId::new("execution:one").unwrap(),
                vec![CapabilityRequirement::new(
                    capability_id,
                    1,
                    CapabilityRequirementLevel::Required,
                    BTreeMap::new(),
                    None,
                    None,
                )
                .unwrap()],
                vec!["runtime:one".into()],
                10,
            )
            .unwrap();

        let roles = InMemoryRoleRepository::default();
        roles
            .register_definition(
                crate::role_domain::RoleDefinition::new(
                    RoleId::new("role:developer").unwrap(),
                    1,
                    "Developer",
                    "Perform bounded implementation work",
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap(),
            )
            .unwrap();
        let draft_assignment = crate::role_domain::RoleAssignment::new(
            RoleAssignmentId::new("assignment:one").unwrap(),
            "agent:one",
            "membership:one",
            RoleId::new("role:developer").unwrap(),
            1,
            RoleAssignmentScope::new(RoleAssignmentScopeKind::Task, "task:one").unwrap(),
            Vec::new(),
            vec![ROLE_POLICY.into()],
            "provenance:owner",
            5,
            None,
            1,
        )
        .unwrap();
        roles.insert_assignment(draft_assignment.clone()).unwrap();
        let assignment = draft_assignment
            .transition_to(RoleAssignmentLifecycle::Active, 1, 5)
            .unwrap();
        roles.update_assignment(assignment, 1).unwrap();

        let permissions = InMemoryPermissionRepository::default();
        for (id, layer, effect) in [
            (
                REPOSITORY_POLICY,
                PermissionPolicyLayer::Repository,
                PermissionRuleEffect::Allow,
            ),
            (
                ROLE_POLICY,
                PermissionPolicyLayer::RoleAssignment,
                role_effect,
            ),
        ] {
            permissions
                .register_policy(
                    PermissionPolicy::new(
                        PermissionPolicyId::new(id).unwrap(),
                        1,
                        layer,
                        "owner:one",
                        vec![rule(effect)],
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        permissions
            .register_ceiling(
                PermissionCeiling::new(
                    PermissionCeilingId::new("ceiling:agent-one").unwrap(),
                    1,
                    "agent:one",
                    vec![rule(PermissionRuleEffect::Allow)],
                )
                .unwrap(),
            )
            .unwrap();

        Fixture {
            permissions,
            capabilities,
            roles,
            snapshot_id: snapshot.id().clone(),
        }
    }

    fn request(
        snapshot_id: crate::capability_domain::CapabilitySnapshotId,
        approvals: Vec<ApprovalEvidence>,
    ) -> PermissionRequest {
        PermissionRequest::new(
            PermissionRequestId::new(uuid::Uuid::new_v4().to_string()).unwrap(),
            RuntimeExecutionId::new("execution:one").unwrap(),
            "agent:one",
            RoleAssignmentId::new("assignment:one").unwrap(),
            "task:one",
            snapshot_id,
            PermissionCeilingId::new("ceiling:agent-one").unwrap(),
            1,
            vec![PermissionPolicyId::new(REPOSITORY_POLICY).unwrap()],
            vec![PermissionClaim::new(
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:/repo",
                BTreeMap::from([("mode".into(), "patch".into())]),
                CapabilityId::new("workspace.write-enforcement").unwrap(),
            )
            .unwrap()],
            approvals,
            10,
            100,
        )
        .unwrap()
    }

    #[test]
    fn all_layers_and_enforcement_capability_are_required_for_a_grant() {
        let fixture = fixture(PermissionRuleEffect::Allow);
        let service = PermissionGovernanceService::new(
            fixture.permissions.clone(),
            fixture.capabilities,
            fixture.roles,
        );
        let permission_request = request(fixture.snapshot_id, Vec::new());
        let request_id = permission_request.id().clone();
        let evaluation = service.evaluate(permission_request, 10).unwrap();

        assert_eq!(
            evaluation.decision().status(),
            AuthorizationDecisionStatus::Allowed
        );
        let grant = evaluation.grant().unwrap();
        assert_eq!(grant.agent_id(), "agent:one");
        assert_eq!(grant.claims().len(), 1);
        assert!(fixture.permissions.get_grant(grant.id()).unwrap().is_some());
        assert!(fixture
            .permissions
            .get_request(&request_id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn role_constraint_deny_overrides_every_allow() {
        let fixture = fixture(PermissionRuleEffect::Deny);
        let service = PermissionGovernanceService::new(
            fixture.permissions,
            fixture.capabilities,
            fixture.roles,
        );
        let evaluation = service
            .evaluate(request(fixture.snapshot_id, Vec::new()), 10)
            .unwrap();

        assert_eq!(
            evaluation.decision().status(),
            AuthorizationDecisionStatus::Denied
        );
        assert!(evaluation.grant().is_none());
    }

    #[test]
    fn silence_never_satisfies_an_approval_rule() {
        let fixture = fixture(PermissionRuleEffect::RequireApproval);
        let service = PermissionGovernanceService::new(
            fixture.permissions,
            fixture.capabilities,
            fixture.roles,
        );
        let evaluation = service
            .evaluate(request(fixture.snapshot_id, Vec::new()), 10)
            .unwrap();

        assert_eq!(
            evaluation.decision().status(),
            AuthorizationDecisionStatus::RequiresApproval
        );
        assert!(evaluation.grant().is_none());
    }

    #[test]
    fn matching_current_approval_produces_a_bounded_grant() {
        let fixture = fixture(PermissionRuleEffect::RequireApproval);
        let service = PermissionGovernanceService::new(
            fixture.permissions,
            fixture.capabilities,
            fixture.roles,
        );
        let approval = ApprovalEvidence::new(
            "approval:owner",
            PermissionAction::new("workspace.write").unwrap(),
            "workspace:/repo",
            20,
        )
        .unwrap();
        let evaluation = service
            .evaluate(request(fixture.snapshot_id, vec![approval]), 10)
            .unwrap();

        assert_eq!(
            evaluation.decision().status(),
            AuthorizationDecisionStatus::Allowed
        );
        assert!(evaluation.grant().unwrap().is_valid_at(20));
    }
}
