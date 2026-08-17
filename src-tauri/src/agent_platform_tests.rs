//! Milestone 6 cross-boundary conformance tests.

use std::collections::BTreeMap;

use crate::{
    agent_domain::{Agent, AgentLifecycle},
    capability_domain::{
        CapabilityDefinition, CapabilityEvidence, CapabilityEvidenceId,
        CapabilityEvidenceSourceKind, CapabilityId, CapabilityRequirement,
        CapabilityRequirementLevel, CapabilitySupportState,
    },
    capability_registry::{CapabilityRegistry, InMemoryCapabilityRegistry},
    collaboration_domain::{
        CollaborationMessage, CollaborationMessageId, CollaborationMessageKind, Handoff, HandoffId,
        HandoffLifecycle,
    },
    collaboration_repository::InMemoryCollaborationRepository,
    database::Database,
    execution_domain::{
        ExecutionGovernanceEvidence, ExecutionModelBinding, ExecutionRequest, ExecutionResult,
    },
    execution_repository::{ExecutionHistoryRepository, InMemoryExecutionHistoryRepository},
    model_domain::ModelId,
    permission_domain::{
        PermissionAction, PermissionCeiling, PermissionCeilingId, PermissionClaim,
        PermissionPolicy, PermissionPolicyId, PermissionPolicyLayer, PermissionRequest,
        PermissionRequestId, PermissionRule, PermissionRuleEffect,
    },
    permission_repository::{InMemoryPermissionRepository, PermissionRepository},
    role_domain::{
        RoleAssignment, RoleAssignmentId, RoleAssignmentLifecycle, RoleAssignmentScope,
        RoleAssignmentScopeKind, RoleDefinition, RoleId,
    },
    role_repository::{InMemoryRoleRepository, RoleRepository},
    runtime_domain::{
        AgentRuntimeBinding, ExecutionContext, RuntimeBindingId, RuntimeBindingLifecycle,
        RuntimeExecutionId, RuntimeExecutionState, RuntimeId,
    },
    services::{
        agent_collaboration::{AgentCollaborationError, AgentCollaborationService},
        capability_governance::CapabilityGovernanceService,
        permission_governance::PermissionGovernanceService,
        workflow_orchestration::WorkflowOrchestrationService,
    },
    team_domain::{
        Team, TeamId, TeamLifecycle, TeamMembership, TeamMembershipId, TeamMembershipLifecycle,
    },
    team_repository::{InMemoryTeamRepository, TeamRepository},
    workflow_domain::{
        WorkflowDefinition, WorkflowId, WorkflowRunId, WorkflowStepDefinition, WorkflowStepId,
        WorkflowStepState, WorkflowTask, WorkflowTaskId,
    },
    workflow_governance::GovernedWorkflowParticipationGate,
    workflow_repository::{InMemoryWorkflowRepository, WorkflowRepository},
};

const TEAM_ID: &str = "team:delivery";
const POLICY_ID: &str = "policy:repository";
const CAPABILITY_ID: &str = "collaboration.enforcement";

type PlatformParticipationGate = GovernedWorkflowParticipationGate<
    InMemoryCapabilityRegistry,
    InMemoryPermissionRepository,
    InMemoryRoleRepository,
    InMemoryTeamRepository,
    InMemoryExecutionHistoryRepository,
>;
type PlatformWorkflowService = WorkflowOrchestrationService<
    InMemoryWorkflowRepository,
    InMemoryTeamRepository,
    InMemoryRoleRepository,
    InMemoryExecutionHistoryRepository,
    PlatformParticipationGate,
>;

#[derive(Clone)]
struct PlatformRepositories {
    capabilities: InMemoryCapabilityRegistry,
    permissions: InMemoryPermissionRepository,
    roles: InMemoryRoleRepository,
    teams: InMemoryTeamRepository,
    executions: InMemoryExecutionHistoryRepository,
    workflows: InMemoryWorkflowRepository,
    collaboration: InMemoryCollaborationRepository,
}

struct PlatformFixture {
    db: Database,
    repositories: PlatformRepositories,
    build_assignment: RoleAssignment,
    review_assignment: RoleAssignment,
}

fn wildcard_allow_rule() -> PermissionRule {
    PermissionRule::new(
        PermissionRuleEffect::Allow,
        PermissionAction::new("*").unwrap(),
        "*",
        BTreeMap::new(),
    )
    .unwrap()
}

fn active_agent(id: &str) -> Agent {
    Agent {
        id: id.into(),
        name: id.into(),
        description: "Agent Platform participant".into(),
        owner: "owner:one".into(),
        lifecycle_state: AgentLifecycle::Active,
        revision: 1,
        created_at: 1,
        updated_at: 1,
    }
}

fn activate_membership(
    repository: &InMemoryTeamRepository,
    id: &str,
    agent_id: &str,
) -> TeamMembership {
    let invited = TeamMembership::new(
        TeamMembershipId::new(id).unwrap(),
        TeamId::new(TEAM_ID).unwrap(),
        agent_id,
        None,
        Vec::new(),
        "owner:one",
        2,
        None,
        1,
    )
    .unwrap();
    repository.insert_membership(invited.clone()).unwrap();
    let active = invited
        .transition_to(TeamMembershipLifecycle::Active, 1, 2)
        .unwrap();
    repository.update_membership(active.clone(), 1).unwrap();
    active
}

fn activate_assignment(
    repository: &InMemoryRoleRepository,
    id: &str,
    agent_id: &str,
    membership_id: &str,
    role_id: &str,
    step_id: &str,
) -> RoleAssignment {
    let draft = RoleAssignment::new(
        RoleAssignmentId::new(id).unwrap(),
        agent_id,
        membership_id,
        RoleId::new(role_id).unwrap(),
        1,
        RoleAssignmentScope::new(RoleAssignmentScopeKind::WorkflowStep, step_id).unwrap(),
        Vec::new(),
        Vec::new(),
        "owner:one",
        3,
        None,
        2,
    )
    .unwrap();
    repository.insert_assignment(draft.clone()).unwrap();
    let active = draft
        .transition_to(RoleAssignmentLifecycle::Active, 1, 3)
        .unwrap();
    repository.update_assignment(active.clone(), 1).unwrap();
    active
}

fn workflow_definition() -> WorkflowDefinition {
    let capability = CapabilityRequirement::new(
        CapabilityId::new(CAPABILITY_ID).unwrap(),
        1,
        CapabilityRequirementLevel::Required,
        BTreeMap::new(),
        None,
        None,
    )
    .unwrap();
    let build = WorkflowStepDefinition::new(
        WorkflowStepId::new("step:build").unwrap(),
        "Build",
        "Produce a governed implementation",
        RoleId::new("role:developer").unwrap(),
        1,
        Vec::new(),
        vec![capability.clone()],
        vec!["permission:bounded-build".into()],
        vec!["Implementation evidence exists".into()],
    )
    .unwrap();
    let review = WorkflowStepDefinition::new(
        WorkflowStepId::new("step:review").unwrap(),
        "Review",
        "Review the governed implementation",
        RoleId::new("role:reviewer").unwrap(),
        1,
        vec![build.id().clone()],
        vec![capability],
        vec!["permission:bounded-review".into()],
        vec!["Review evidence exists".into()],
    )
    .unwrap();
    WorkflowDefinition::new(
        WorkflowId::new("workflow:delivery").unwrap(),
        1,
        TeamId::new(TEAM_ID).unwrap(),
        "Governed delivery",
        "Coordinate implementation and independent review",
        vec![build, review],
        4,
    )
    .unwrap()
}

fn fixture() -> PlatformFixture {
    let db = Database::memory().unwrap();
    db.insert_agent(&active_agent("agent:developer")).unwrap();
    db.insert_agent(&active_agent("agent:reviewer")).unwrap();

    let repositories = PlatformRepositories {
        capabilities: InMemoryCapabilityRegistry::default(),
        permissions: InMemoryPermissionRepository::default(),
        roles: InMemoryRoleRepository::default(),
        teams: InMemoryTeamRepository::default(),
        executions: InMemoryExecutionHistoryRepository::default(),
        workflows: InMemoryWorkflowRepository::default(),
        collaboration: InMemoryCollaborationRepository::default(),
    };

    let team = Team::new(
        TeamId::new(TEAM_ID).unwrap(),
        "Delivery Team",
        "Deliver governed changes",
        "owner:one",
        vec![POLICY_ID.into()],
        vec!["workflow:delivery".into()],
        1,
    )
    .unwrap();
    repositories.teams.insert_team(team.clone()).unwrap();
    repositories
        .teams
        .update_team(team.transition_to(TeamLifecycle::Active, 1, 2).unwrap(), 1)
        .unwrap();
    let developer_membership = activate_membership(
        &repositories.teams,
        "membership:developer",
        "agent:developer",
    );
    let reviewer_membership =
        activate_membership(&repositories.teams, "membership:reviewer", "agent:reviewer");

    for (id, name) in [
        ("role:developer", "Developer"),
        ("role:reviewer", "Reviewer"),
    ] {
        repositories
            .roles
            .register_definition(
                RoleDefinition::new(
                    RoleId::new(id).unwrap(),
                    1,
                    name,
                    format!("Perform bounded {name} responsibility"),
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let build_assignment = activate_assignment(
        &repositories.roles,
        "assignment:build",
        "agent:developer",
        developer_membership.id().as_str(),
        "role:developer",
        "step:build",
    );
    let review_assignment = activate_assignment(
        &repositories.roles,
        "assignment:review",
        "agent:reviewer",
        reviewer_membership.id().as_str(),
        "role:reviewer",
        "step:review",
    );

    repositories
        .capabilities
        .register_definition(
            CapabilityDefinition::new(
                CapabilityId::new(CAPABILITY_ID).unwrap(),
                1,
                "Collaboration enforcement",
                "Enforces bounded collaboration operations",
                BTreeMap::new(),
            )
            .unwrap(),
        )
        .unwrap();
    repositories
        .capabilities
        .register_evidence(
            CapabilityEvidence::new(
                CapabilityEvidenceId::new("evidence:collaboration").unwrap(),
                CapabilityId::new(CAPABILITY_ID).unwrap(),
                "runtime:test",
                CapabilityEvidenceSourceKind::Runtime,
                1,
                CapabilitySupportState::Supported,
                BTreeMap::new(),
                1,
                100,
                "probe:test",
            )
            .unwrap(),
        )
        .unwrap();

    repositories
        .permissions
        .register_policy(
            PermissionPolicy::new(
                PermissionPolicyId::new(POLICY_ID).unwrap(),
                1,
                PermissionPolicyLayer::Repository,
                "owner:repository",
                vec![wildcard_allow_rule()],
            )
            .unwrap(),
        )
        .unwrap();
    for agent_id in ["agent:developer", "agent:reviewer"] {
        repositories
            .permissions
            .register_ceiling(
                PermissionCeiling::new(
                    PermissionCeilingId::new(format!("ceiling:{agent_id}")).unwrap(),
                    1,
                    agent_id,
                    vec![wildcard_allow_rule()],
                )
                .unwrap(),
            )
            .unwrap();
    }

    let definition = workflow_definition();
    let gate = GovernedWorkflowParticipationGate::new(
        repositories.capabilities.clone(),
        repositories.permissions.clone(),
        repositories.roles.clone(),
        repositories.teams.clone(),
        repositories.executions.clone(),
    );
    let workflow_service = WorkflowOrchestrationService::new(
        repositories.workflows.clone(),
        repositories.teams.clone(),
        repositories.roles.clone(),
        repositories.executions.clone(),
        gate,
    );
    workflow_service
        .register_definition(definition.clone())
        .unwrap();
    workflow_service
        .create_run(
            WorkflowRunId::new("run:delivery").unwrap(),
            definition.id(),
            definition.version(),
            5,
        )
        .unwrap();
    workflow_service
        .activate_run(&WorkflowRunId::new("run:delivery").unwrap(), 1, 6)
        .unwrap();

    PlatformFixture {
        db,
        repositories,
        build_assignment,
        review_assignment,
    }
}

impl PlatformFixture {
    fn authorize_execution(
        &self,
        execution_id: &str,
        agent_id: &str,
        assignment: &RoleAssignment,
        actions: &[&str],
        accepted_at: i64,
    ) -> ExecutionGovernanceEvidence {
        let execution_id = RuntimeExecutionId::new(execution_id).unwrap();
        let requirement = CapabilityRequirement::new(
            CapabilityId::new(CAPABILITY_ID).unwrap(),
            1,
            CapabilityRequirementLevel::Required,
            BTreeMap::new(),
            None,
            None,
        )
        .unwrap();
        let snapshot = CapabilityGovernanceService::new(self.repositories.capabilities.clone())
            .resolve(
                execution_id.clone(),
                vec![requirement],
                vec!["runtime:test".into()],
                accepted_at - 1,
            )
            .unwrap();
        let claims = actions
            .iter()
            .map(|action| {
                PermissionClaim::new(
                    PermissionAction::new(*action).unwrap(),
                    TEAM_ID,
                    BTreeMap::new(),
                    CapabilityId::new(CAPABILITY_ID).unwrap(),
                )
                .unwrap()
            })
            .collect();
        let permission_request = PermissionRequest::new(
            PermissionRequestId::new(format!("request:{}", execution_id.as_str())).unwrap(),
            execution_id.clone(),
            agent_id,
            assignment.id().clone(),
            assignment.scope().reference(),
            snapshot.id().clone(),
            PermissionCeilingId::new(format!("ceiling:{agent_id}")).unwrap(),
            1,
            vec![PermissionPolicyId::new(POLICY_ID).unwrap()],
            claims,
            Vec::new(),
            accepted_at - 1,
            accepted_at + 100,
        )
        .unwrap();
        let evaluation = PermissionGovernanceService::new(
            self.repositories.permissions.clone(),
            self.repositories.capabilities.clone(),
            self.repositories.roles.clone(),
        )
        .evaluate(permission_request, accepted_at)
        .unwrap();
        let grant = evaluation.grant().unwrap();
        let evidence = ExecutionGovernanceEvidence::new(
            snapshot.id().clone(),
            grant.id().clone(),
            assignment.id().clone(),
            evaluation.decision().id().clone(),
        );

        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new(format!("binding:{}", execution_id.as_str())).unwrap(),
            agent_id,
            RuntimeId::new("runtime:test").unwrap(),
            accepted_at - 4,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, accepted_at - 3)
        .unwrap();
        let context = ExecutionContext::new(
            execution_id.clone(),
            binding,
            vec!["context:bounded".into()],
            accepted_at - 2,
        )
        .unwrap();
        let request = ExecutionRequest::new(
            context,
            "Perform one governed Workflow task",
            ExecutionModelBinding::runtime_local(ModelId::new("model:test").unwrap()),
            evidence.clone(),
            Some(assignment.scope().reference().into()),
            accepted_at,
        )
        .unwrap();
        self.repositories.executions.accept(request).unwrap();
        evidence
    }

    fn workflow_service(&self) -> PlatformWorkflowService {
        WorkflowOrchestrationService::new(
            self.repositories.workflows.clone(),
            self.repositories.teams.clone(),
            self.repositories.roles.clone(),
            self.repositories.executions.clone(),
            GovernedWorkflowParticipationGate::new(
                self.repositories.capabilities.clone(),
                self.repositories.permissions.clone(),
                self.repositories.roles.clone(),
                self.repositories.teams.clone(),
                self.repositories.executions.clone(),
            ),
        )
    }

    fn assign_build_task(&self, actions: &[&str]) -> WorkflowTask {
        let evidence = self.authorize_execution(
            "execution:build",
            "agent:developer",
            &self.build_assignment,
            actions,
            10,
        );
        let task = WorkflowTask::new(
            WorkflowTaskId::new("task:build").unwrap(),
            WorkflowRunId::new("run:delivery").unwrap(),
            WorkflowStepId::new("step:build").unwrap(),
            "agent:developer",
            TeamMembershipId::new("membership:developer").unwrap(),
            self.build_assignment.id().clone(),
            RuntimeExecutionId::new("execution:build").unwrap(),
            evidence,
            1,
            11,
        )
        .unwrap();
        self.workflow_service()
            .assign_task(&self.db, task, 11)
            .unwrap()
    }

    fn complete_build_task(&self) {
        let workflow = self.workflow_service();
        workflow
            .start_task(&WorkflowTaskId::new("task:build").unwrap(), 1, 2, 12)
            .unwrap();
        let execution_id = RuntimeExecutionId::new("execution:build").unwrap();
        self.repositories
            .executions
            .transition(
                &execution_id,
                RuntimeExecutionState::Preparing,
                1,
                12,
                "prepare",
            )
            .unwrap();
        self.repositories
            .executions
            .transition(&execution_id, RuntimeExecutionState::Running, 2, 13, "run")
            .unwrap();
        self.repositories
            .executions
            .transition(
                &execution_id,
                RuntimeExecutionState::Succeeded,
                3,
                14,
                "succeed",
            )
            .unwrap();
        self.repositories
            .executions
            .store_result(
                ExecutionResult::new(
                    execution_id,
                    RuntimeExecutionState::Succeeded,
                    "Build complete",
                    vec!["artifact:build".into()],
                    None,
                    14,
                )
                .unwrap(),
                4,
            )
            .unwrap();
        workflow
            .synchronize_task(&WorkflowTaskId::new("task:build").unwrap(), 2, 3, 14)
            .unwrap();
    }

    fn assign_review_task(&self, actions: &[&str]) -> WorkflowTask {
        let evidence = self.authorize_execution(
            "execution:review",
            "agent:reviewer",
            &self.review_assignment,
            actions,
            20,
        );
        let task = WorkflowTask::new(
            WorkflowTaskId::new("task:review").unwrap(),
            WorkflowRunId::new("run:delivery").unwrap(),
            WorkflowStepId::new("step:review").unwrap(),
            "agent:reviewer",
            TeamMembershipId::new("membership:reviewer").unwrap(),
            self.review_assignment.id().clone(),
            RuntimeExecutionId::new("execution:review").unwrap(),
            evidence,
            1,
            21,
        )
        .unwrap();
        self.workflow_service()
            .assign_task(&self.db, task, 21)
            .unwrap()
    }
}

#[test]
fn governed_workflow_releases_next_step_only_after_terminal_execution_result() {
    let fixture = fixture();
    fixture.assign_build_task(&["collaboration.handoff"]);
    fixture.complete_build_task();

    let run = fixture
        .repositories
        .workflows
        .get_run(&WorkflowRunId::new("run:delivery").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        run.step_state(&WorkflowStepId::new("step:build").unwrap()),
        Some(WorkflowStepState::Succeeded)
    );
    assert_eq!(
        run.step_state(&WorkflowStepId::new("step:review").unwrap()),
        Some(WorkflowStepState::Ready)
    );
}

#[test]
fn terminal_execution_without_result_cannot_complete_workflow_task() {
    let fixture = fixture();
    fixture.assign_build_task(&["collaboration.handoff"]);
    let workflow = fixture.workflow_service();
    workflow
        .start_task(&WorkflowTaskId::new("task:build").unwrap(), 1, 2, 12)
        .unwrap();
    let execution_id = RuntimeExecutionId::new("execution:build").unwrap();
    fixture
        .repositories
        .executions
        .transition(
            &execution_id,
            RuntimeExecutionState::Preparing,
            1,
            12,
            "prepare",
        )
        .unwrap();
    fixture
        .repositories
        .executions
        .transition(&execution_id, RuntimeExecutionState::Running, 2, 13, "run")
        .unwrap();
    fixture
        .repositories
        .executions
        .transition(
            &execution_id,
            RuntimeExecutionState::Succeeded,
            3,
            14,
            "succeed",
        )
        .unwrap();

    assert!(workflow
        .synchronize_task(&WorkflowTaskId::new("task:build").unwrap(), 2, 3, 14)
        .is_err());
    let run = fixture
        .repositories
        .workflows
        .get_run(&WorkflowRunId::new("run:delivery").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        run.step_state(&WorkflowStepId::new("step:build").unwrap()),
        Some(WorkflowStepState::Running)
    );
    assert_eq!(
        run.step_state(&WorkflowStepId::new("step:review").unwrap()),
        Some(WorkflowStepState::Pending)
    );
}

#[test]
fn collaboration_is_permission_bound_and_handoff_does_not_advance_workflow() {
    let fixture = fixture();
    let build_task = fixture.assign_build_task(&["collaboration.handoff"]);
    fixture.complete_build_task();

    let denied_message = CollaborationMessage::new(
        CollaborationMessageId::new("message:status").unwrap(),
        TeamId::new(TEAM_ID).unwrap(),
        WorkflowRunId::new("run:delivery").unwrap(),
        build_task.id().clone(),
        TeamMembershipId::new("membership:developer").unwrap(),
        Some(TeamMembershipId::new("membership:reviewer").unwrap()),
        CollaborationMessageKind::Status,
        "artifact:status",
        build_task.governance().authorization_decision_id().clone(),
        build_task.governance().permission_grant_id().clone(),
        15,
    )
    .unwrap();
    let collaboration = AgentCollaborationService::new(
        fixture.repositories.collaboration.clone(),
        fixture.repositories.workflows.clone(),
        fixture.repositories.teams.clone(),
        fixture.repositories.permissions.clone(),
    );
    assert!(matches!(
        collaboration.send_message(denied_message),
        Err(AgentCollaborationError::PermissionDenied)
    ));

    let proposal = CollaborationMessage::new(
        CollaborationMessageId::new("message:proposal").unwrap(),
        TeamId::new(TEAM_ID).unwrap(),
        WorkflowRunId::new("run:delivery").unwrap(),
        build_task.id().clone(),
        TeamMembershipId::new("membership:developer").unwrap(),
        Some(TeamMembershipId::new("membership:reviewer").unwrap()),
        CollaborationMessageKind::Handoff,
        "artifact:handoff-proposal",
        build_task.governance().authorization_decision_id().clone(),
        build_task.governance().permission_grant_id().clone(),
        16,
    )
    .unwrap();
    let handoff = Handoff::new(
        HandoffId::new("handoff:review").unwrap(),
        TeamId::new(TEAM_ID).unwrap(),
        WorkflowRunId::new("run:delivery").unwrap(),
        build_task.id().clone(),
        WorkflowStepId::new("step:review").unwrap(),
        TeamMembershipId::new("membership:developer").unwrap(),
        TeamMembershipId::new("membership:reviewer").unwrap(),
        proposal.id().clone(),
        16,
    )
    .unwrap();
    collaboration.propose_handoff(proposal, handoff).unwrap();

    let review_task = fixture.assign_review_task(&["collaboration.handoff"]);
    let acceptance = CollaborationMessage::new(
        CollaborationMessageId::new("message:acceptance").unwrap(),
        TeamId::new(TEAM_ID).unwrap(),
        WorkflowRunId::new("run:delivery").unwrap(),
        review_task.id().clone(),
        TeamMembershipId::new("membership:reviewer").unwrap(),
        Some(TeamMembershipId::new("membership:developer").unwrap()),
        CollaborationMessageKind::Handoff,
        "artifact:handoff-acceptance",
        review_task.governance().authorization_decision_id().clone(),
        review_task.governance().permission_grant_id().clone(),
        22,
    )
    .unwrap();
    let accepted = collaboration
        .resolve_handoff(
            &HandoffId::new("handoff:review").unwrap(),
            HandoffLifecycle::Accepted,
            acceptance,
            1,
            22,
        )
        .unwrap();
    assert_eq!(accepted.lifecycle(), HandoffLifecycle::Accepted);

    let run = fixture
        .repositories
        .workflows
        .get_run(&WorkflowRunId::new("run:delivery").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        run.step_state(&WorkflowStepId::new("step:review").unwrap()),
        Some(WorkflowStepState::Ready),
        "Handoff evidence must not implicitly start or complete a Workflow Step"
    );
}
