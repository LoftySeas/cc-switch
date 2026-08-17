//! Read-only bridge from Governance evidence to the Execution admission boundary.
//!
//! Policy evaluation happens before this gate. The gate only verifies that the
//! immutable Capability snapshot, Role Assignment, Authorization Decision, and
//! Permission Grant are mutually consistent with the accepted execution.

use crate::{
    capability_registry::CapabilitySnapshotRepository,
    execution_domain::ExecutionRequest,
    permission_domain::AuthorizationDecisionStatus,
    permission_repository::PermissionRepository,
    role_repository::RoleRepository,
    runtime_execution::{ExecutionAdmission, ExecutionAdmissionGate, RuntimeExecutionError},
};

pub struct GovernedExecutionAdmissionGate<C, P, R> {
    capabilities: C,
    permissions: P,
    roles: R,
}

impl<C, P, R> GovernedExecutionAdmissionGate<C, P, R> {
    pub fn new(capabilities: C, permissions: P, roles: R) -> Self {
        Self {
            capabilities,
            permissions,
            roles,
        }
    }
}

impl<C, P, R> ExecutionAdmissionGate for GovernedExecutionAdmissionGate<C, P, R>
where
    C: CapabilitySnapshotRepository,
    P: PermissionRepository,
    R: RoleRepository,
{
    fn admit(
        &self,
        request: &ExecutionRequest,
    ) -> Result<ExecutionAdmission, RuntimeExecutionError> {
        let evidence = request.governance();
        let snapshot = self
            .capabilities
            .get_snapshot(evidence.capability_snapshot_id())
            .map_err(reject)?
            .ok_or_else(|| rejection("Capability snapshot is missing"))?;
        if snapshot.execution_id() != request.execution_id() || !snapshot.is_satisfied() {
            return Err(rejection(
                "Capability snapshot is unsatisfied or belongs to another execution",
            ));
        }

        let assignment = self
            .roles
            .get_assignment(evidence.role_assignment_id())
            .map_err(reject)?
            .ok_or_else(|| rejection("Role Assignment is missing"))?;
        if assignment.agent_id() != request.context().binding().agent_id()
            || !assignment.is_effective(request.accepted_at())
            || request.correlation_ref() != Some(assignment.scope().reference())
        {
            return Err(rejection(
                "Role Assignment is ineffective, out of scope, or belongs to another Agent",
            ));
        }

        let decision = self
            .permissions
            .get_decision(evidence.authorization_decision_id())
            .map_err(reject)?
            .ok_or_else(|| rejection("Authorization Decision is missing"))?;
        if decision.status() != AuthorizationDecisionStatus::Allowed
            || decision.execution_id() != request.execution_id()
            || decision.role_assignment_id() != evidence.role_assignment_id()
            || decision.capability_snapshot_id() != evidence.capability_snapshot_id()
            || decision.grant_id() != Some(evidence.permission_grant_id())
        {
            return Err(rejection(
                "Authorization Decision is not an allowed decision for this execution evidence",
            ));
        }

        let grant = self
            .permissions
            .get_grant(evidence.permission_grant_id())
            .map_err(reject)?
            .ok_or_else(|| rejection("Permission Grant is missing"))?;
        if grant.decision_id() != evidence.authorization_decision_id()
            || grant.execution_id() != request.execution_id()
            || grant.agent_id() != request.context().binding().agent_id()
            || grant.role_assignment_id() != evidence.role_assignment_id()
            || grant.capability_snapshot_id() != evidence.capability_snapshot_id()
            || !grant.is_valid_at(request.accepted_at())
        {
            return Err(rejection(
                "Permission Grant is expired, inconsistent, or belongs to another execution",
            ));
        }

        ExecutionAdmission::new(format!("authorization:decision:{}", decision.id()))
    }
}

fn reject(error: impl std::fmt::Display) -> RuntimeExecutionError {
    rejection(error.to_string())
}

fn rejection(message: impl Into<String>) -> RuntimeExecutionError {
    RuntimeExecutionError::AdmissionRejected(message.into())
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
        execution_domain::{ExecutionGovernanceEvidence, ExecutionModelBinding, ExecutionRequest},
        model_domain::ModelId,
        permission_domain::{
            PermissionAction, PermissionCeiling, PermissionCeilingId, PermissionClaim,
            PermissionPolicy, PermissionPolicyId, PermissionPolicyLayer, PermissionRequest,
            PermissionRequestId, PermissionRule, PermissionRuleEffect,
        },
        permission_repository::{InMemoryPermissionRepository, PermissionRepository},
        role_domain::{
            RoleAssignment, RoleAssignmentId, RoleAssignmentLifecycle, RoleAssignmentScope,
            RoleAssignmentScopeKind, RoleId,
        },
        role_repository::{InMemoryRoleRepository, RoleRepository},
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeBindingId, RuntimeBindingLifecycle,
            RuntimeExecutionId, RuntimeId,
        },
        services::{
            capability_governance::CapabilityGovernanceService,
            permission_governance::PermissionGovernanceService,
        },
    };

    fn allow_rule() -> PermissionRule {
        PermissionRule::new(
            PermissionRuleEffect::Allow,
            PermissionAction::new("workspace.write").unwrap(),
            "workspace:/repo",
            BTreeMap::new(),
        )
        .unwrap()
    }

    fn execution_request(
        execution_id: &str,
        evidence: ExecutionGovernanceEvidence,
    ) -> ExecutionRequest {
        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding:one").unwrap(),
            "agent:one",
            RuntimeId::new("runtime:one").unwrap(),
            1,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 2)
        .unwrap();
        let context = ExecutionContext::new(
            RuntimeExecutionId::new(execution_id).unwrap(),
            binding,
            vec!["context:one".into()],
            3,
        )
        .unwrap();
        ExecutionRequest::new(
            context,
            "perform governed work",
            ExecutionModelBinding::runtime_local(ModelId::new("model:one").unwrap()),
            evidence,
            Some("task:one".into()),
            20,
        )
        .unwrap()
    }

    fn fixture() -> (
        GovernedExecutionAdmissionGate<
            InMemoryCapabilityRegistry,
            InMemoryPermissionRepository,
            InMemoryRoleRepository,
        >,
        ExecutionRequest,
    ) {
        let execution_id = RuntimeExecutionId::new("execution:one").unwrap();
        let capability_id = CapabilityId::new("workspace.write-enforcement").unwrap();
        let capabilities = InMemoryCapabilityRegistry::default();
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
                execution_id.clone(),
                vec![CapabilityRequirement::new(
                    capability_id.clone(),
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
        let draft_assignment = RoleAssignment::new(
            RoleAssignmentId::new("assignment:one").unwrap(),
            "agent:one",
            "membership:one",
            RoleId::new("role:developer").unwrap(),
            1,
            RoleAssignmentScope::new(RoleAssignmentScopeKind::Task, "task:one").unwrap(),
            Vec::new(),
            Vec::new(),
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
        roles.update_assignment(assignment.clone(), 1).unwrap();

        let permissions = InMemoryPermissionRepository::default();
        let policy_id = PermissionPolicyId::new("policy:repository").unwrap();
        permissions
            .register_policy(
                PermissionPolicy::new(
                    policy_id.clone(),
                    1,
                    PermissionPolicyLayer::Repository,
                    "owner:repository",
                    vec![allow_rule()],
                )
                .unwrap(),
            )
            .unwrap();
        let ceiling_id = PermissionCeilingId::new("ceiling:agent-one").unwrap();
        permissions
            .register_ceiling(
                PermissionCeiling::new(ceiling_id.clone(), 1, "agent:one", vec![allow_rule()])
                    .unwrap(),
            )
            .unwrap();
        let permission_request = PermissionRequest::new(
            PermissionRequestId::new("permission-request:one").unwrap(),
            execution_id,
            "agent:one",
            assignment.id().clone(),
            "task:one",
            snapshot.id().clone(),
            ceiling_id,
            1,
            vec![policy_id],
            vec![PermissionClaim::new(
                PermissionAction::new("workspace.write").unwrap(),
                "workspace:/repo",
                BTreeMap::new(),
                capability_id,
            )
            .unwrap()],
            Vec::new(),
            10,
            100,
        )
        .unwrap();
        let evaluation = PermissionGovernanceService::new(
            permissions.clone(),
            capabilities.clone(),
            roles.clone(),
        )
        .evaluate(permission_request, 10)
        .unwrap();
        let grant = evaluation.grant().unwrap();
        let evidence = ExecutionGovernanceEvidence::new(
            snapshot.id().clone(),
            grant.id().clone(),
            assignment.id().clone(),
            evaluation.decision().id().clone(),
        );
        let request = execution_request("execution:one", evidence);

        (
            GovernedExecutionAdmissionGate::new(capabilities, permissions, roles),
            request,
        )
    }

    #[test]
    fn consistent_immutable_governance_evidence_is_admitted() {
        let (gate, request) = fixture();

        let admission = gate.admit(&request).unwrap();

        assert!(admission
            .receipt_ref()
            .starts_with("authorization:decision:"));
    }

    #[test]
    fn governance_evidence_cannot_be_reused_for_another_execution() {
        let (gate, request) = fixture();
        let other = execution_request("execution:two", request.governance().clone());

        assert!(matches!(
            gate.admit(&other),
            Err(RuntimeExecutionError::AdmissionRejected(_))
        ));
    }
}
