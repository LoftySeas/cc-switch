use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, RwLock,
    },
};

use serde_json::json;

use crate::{
    agent_provider_adapter::{
        AgentProviderAdapterRepository, AgentProviderIntegrationAdapterRepository,
        InMemoryAgentProviderAdapterRepository, InMemoryAgentProviderIntegrationAdapterRepository,
        LegacyProviderCompatibilityAdapter,
    },
    agent_provider_domain::{
        AgentProviderAdapterId, AgentProviderDescriptor, AgentProviderId, LegacyProviderReference,
        ProviderCapability, ProviderMetadata,
    },
    capability_domain::CapabilitySnapshotId,
    database::Database,
    execution_domain::{ExecutionGovernanceEvidence, ExecutionModelBinding, ExecutionRequest},
    model_domain::{
        ModelAvailability, ModelAvailabilityId, ModelAvailabilityStatus, ModelCapability,
        ModelDescriptor, ModelId, ModelMetadata,
    },
    model_registry::{InMemoryModelRegistry, ModelRegistry},
    model_routing::{ModelCapabilityRequirement, ModelRouteRequest, ModelRoutingPolicy},
    permission_domain::{AuthorizationDecisionId, PermissionGrantId},
    provider::Provider,
    role_domain::RoleAssignmentId,
    runtime_activation_adapter::{
        CommandRuntimeAdapter, CommandRuntimeHost, CommandRuntimeInput, CommandRuntimeOutput,
        CommandRuntimeProbe, CommandRuntimeSpec, InMemoryRuntimeLifecycleAdapterRepository,
        RuntimeActivationAdapterError, RuntimeLifecycleAdapterRepository,
    },
    runtime_domain::{
        AgentRuntimeBinding, ExecutionContext, RuntimeAdapterId, RuntimeAvailability,
        RuntimeBindingId, RuntimeBindingLifecycle, RuntimeCapability, RuntimeDescriptor,
        RuntimeExecutionId, RuntimeExecutionState, RuntimeId,
    },
    runtime_execution::{
        ExecutionAdmission, ExecutionAdmissionGate, ExecutionPipeline,
        InMemoryRuntimeExecutionAdapterRepository, RuntimeExecutionAdapterRepository,
        RuntimeExecutionCoordinator, RuntimeExecutionError,
    },
    runtime_instance_domain::{RuntimeInstance, RuntimeInstanceId, RuntimeInstanceLifecycle},
    runtime_instance_repository::{InMemoryRuntimeInstanceRepository, RuntimeInstanceRepository},
    services::{
        execution_activation::ExecutionActivationService,
        model_routing::{ModelRouter, PolicyModelRoutingService},
        provider_integration::ProviderIntegrationService,
        runtime_activation::RuntimeActivationService,
    },
    InMemoryExecutionHistoryRepository,
};

struct AllowGate;

impl ExecutionAdmissionGate for AllowGate {
    fn admit(
        &self,
        _request: &ExecutionRequest,
    ) -> Result<ExecutionAdmission, RuntimeExecutionError> {
        ExecutionAdmission::new("admission:phase2")
    }
}

struct FakeCommandHost {
    availability: RwLock<RuntimeAvailability>,
    executions: AtomicUsize,
}

impl FakeCommandHost {
    fn new(availability: RuntimeAvailability) -> Self {
        Self {
            availability: RwLock::new(availability),
            executions: AtomicUsize::new(0),
        }
    }
}

impl CommandRuntimeHost for FakeCommandHost {
    fn probe(
        &self,
        _spec: &CommandRuntimeSpec,
    ) -> Result<CommandRuntimeProbe, RuntimeActivationAdapterError> {
        Ok(CommandRuntimeProbe {
            availability: *self.availability.read().unwrap(),
            runtime_version: Some("phase2-test".into()),
            diagnostics: Vec::new(),
        })
    }

    fn execute(
        &self,
        _spec: &CommandRuntimeSpec,
        input: &CommandRuntimeInput,
    ) -> Result<CommandRuntimeOutput, RuntimeActivationAdapterError> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(CommandRuntimeOutput {
            summary: format!("executed {} with {}", input.execution_id, input.model_id),
            artifact_references: vec!["artifact:phase2".into()],
        })
    }
}

fn runtime_descriptor() -> RuntimeDescriptor {
    RuntimeDescriptor::new(
        RuntimeId::new("runtime:command").unwrap(),
        RuntimeAdapterId::new("adapter:command").unwrap(),
        "Controlled Command Runtime",
        1,
        vec![RuntimeCapability::new("execution:non-interactive", 1).unwrap()],
    )
    .unwrap()
}

fn command_adapter(host: Arc<FakeCommandHost>) -> Arc<CommandRuntimeAdapter<FakeCommandHost>> {
    Arc::new(
        CommandRuntimeAdapter::new(
            runtime_descriptor(),
            CommandRuntimeSpec::new(
                std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/phase2-test-runtime")),
                Vec::new(),
                Vec::new(),
                None,
                Vec::new(),
                Some(4096),
            )
            .unwrap(),
            host,
        )
        .unwrap(),
    )
}

fn runtime_instance() -> RuntimeInstance {
    RuntimeInstance::new(
        RuntimeInstanceId::new("instance:command").unwrap(),
        RuntimeId::new("runtime:command").unwrap(),
        RuntimeAdapterId::new("adapter:command").unwrap(),
        10,
    )
    .unwrap()
}

fn execution_request(model_binding: ExecutionModelBinding) -> ExecutionRequest {
    let binding = AgentRuntimeBinding::new(
        RuntimeBindingId::new("binding:phase2").unwrap(),
        "agent:phase2",
        RuntimeId::new("runtime:command").unwrap(),
        10,
    )
    .unwrap()
    .transition_to(RuntimeBindingLifecycle::Active, 1, 11)
    .unwrap();
    let context = ExecutionContext::new(
        RuntimeExecutionId::new("execution:phase2").unwrap(),
        binding,
        vec!["context:phase2".into()],
        12,
    )
    .unwrap();
    ExecutionRequest::new(
        context,
        "execute through controlled Runtime adapter",
        model_binding,
        ExecutionGovernanceEvidence::new(
            CapabilitySnapshotId::new("snapshot:phase2").unwrap(),
            PermissionGrantId::new("grant:phase2").unwrap(),
            RoleAssignmentId::new("assignment:phase2").unwrap(),
            AuthorizationDecisionId::new("decision:phase2").unwrap(),
        ),
        Some("workflow:phase2".into()),
        13,
    )
    .unwrap()
}

#[test]
fn activated_command_runtime_is_required_for_governed_execution() {
    let host = Arc::new(FakeCommandHost::new(RuntimeAvailability::Ready));
    let adapter = command_adapter(host.clone());
    let instances = InMemoryRuntimeInstanceRepository::default();
    let lifecycle_adapters = InMemoryRuntimeLifecycleAdapterRepository::default();
    lifecycle_adapters.register(adapter.clone()).unwrap();
    let activation = RuntimeActivationService::new(instances.clone(), lifecycle_adapters);
    activation.register_instance(runtime_instance()).unwrap();
    let ready = activation
        .activate(&RuntimeInstanceId::new("instance:command").unwrap(), 1, 20)
        .unwrap();
    assert_eq!(ready.lifecycle(), RuntimeInstanceLifecycle::Ready);
    *host.availability.write().unwrap() = RuntimeAvailability::Degraded;
    let degraded = activation
        .refresh_health(ready.id(), ready.revision(), 25)
        .unwrap();
    assert_eq!(degraded.lifecycle(), RuntimeInstanceLifecycle::Degraded);

    let execution_adapters = InMemoryRuntimeExecutionAdapterRepository::default();
    execution_adapters.register(adapter).unwrap();
    let pipeline = RuntimeExecutionCoordinator::new(
        InMemoryExecutionHistoryRepository::default(),
        execution_adapters,
        AllowGate,
    );
    let record = pipeline
        .execute(
            execution_request(ExecutionModelBinding::runtime_local(
                ModelId::new("model:local").unwrap(),
            )),
            30,
        )
        .unwrap();

    assert_eq!(record.state(), RuntimeExecutionState::Succeeded);
    assert_eq!(host.executions.load(Ordering::SeqCst), 1);
    let stopped = activation
        .deactivate(degraded.id(), degraded.revision(), 40)
        .unwrap();
    assert_eq!(stopped.lifecycle(), RuntimeInstanceLifecycle::Stopped);
}

#[test]
fn unavailable_runtime_activation_fails_closed_and_records_failed_lifecycle() {
    let adapter = command_adapter(Arc::new(FakeCommandHost::new(
        RuntimeAvailability::Unavailable,
    )));
    let instances = InMemoryRuntimeInstanceRepository::default();
    let adapters = InMemoryRuntimeLifecycleAdapterRepository::default();
    adapters.register(adapter).unwrap();
    let activation = RuntimeActivationService::new(instances, adapters);
    activation.register_instance(runtime_instance()).unwrap();
    let id = RuntimeInstanceId::new("instance:command").unwrap();

    assert!(activation.activate(&id, 1, 20).is_err());
    assert_eq!(
        activation.get(&id).unwrap().lifecycle(),
        RuntimeInstanceLifecycle::Failed
    );
}

fn provider_descriptor() -> AgentProviderDescriptor {
    AgentProviderDescriptor::new(
        AgentProviderId::new("provider:stable").unwrap(),
        AgentProviderAdapterId::new("adapter:provider").unwrap(),
        "Existing Provider Compatibility",
        1,
        ProviderMetadata::default(),
        vec![ProviderCapability::new("model:binding", 1, BTreeMap::new()).unwrap()],
    )
    .unwrap()
}

fn model_descriptor(id: &str, capability: &str, version: u16) -> ModelDescriptor {
    ModelDescriptor::new(
        ModelId::new(id).unwrap(),
        id,
        ModelMetadata::default(),
        vec![ModelCapability::new(capability, version, BTreeMap::new()).unwrap()],
    )
    .unwrap()
}

type ProviderCatalog = InMemoryAgentProviderAdapterRepository;
type ProviderIntegrations = InMemoryAgentProviderIntegrationAdapterRepository;

fn provider_and_models() -> (
    Arc<Database>,
    ProviderCatalog,
    ProviderIntegrations,
    InMemoryModelRegistry,
) {
    let database = Arc::new(Database::memory().unwrap());
    database
        .save_provider(
            "claude",
            &Provider::with_id(
                "legacy-provider".into(),
                "Legacy Provider".into(),
                json!({"apiKey": "must-remain-in-legacy-storage"}),
                None,
            ),
        )
        .unwrap();
    let adapter = Arc::new(
        LegacyProviderCompatibilityAdapter::new(
            provider_descriptor(),
            LegacyProviderReference::new("claude", "legacy-provider").unwrap(),
            database.clone(),
        )
        .unwrap(),
    );
    let catalog = ProviderCatalog::default();
    catalog.register(adapter.clone()).unwrap();
    let integrations = ProviderIntegrations::default();
    integrations.register_integration(adapter).unwrap();

    let models = InMemoryModelRegistry::default();
    models
        .register_model(model_descriptor("model:basic", "text:generate", 1))
        .unwrap();
    models
        .register_model(model_descriptor(
            "model:reasoning",
            "reasoning:structured",
            2,
        ))
        .unwrap();
    models
        .register_availability(
            ModelAvailability::new(
                ModelAvailabilityId::new("availability:basic").unwrap(),
                ModelId::new("model:basic").unwrap(),
                AgentProviderId::new("provider:stable").unwrap(),
                "native-basic",
                ModelAvailabilityStatus::Declared,
                100,
            )
            .unwrap(),
        )
        .unwrap();
    models
        .register_availability(
            ModelAvailability::new(
                ModelAvailabilityId::new("availability:reasoning").unwrap(),
                ModelId::new("model:reasoning").unwrap(),
                AgentProviderId::new("provider:stable").unwrap(),
                "native-reasoning",
                ModelAvailabilityStatus::Declared,
                100,
            )
            .unwrap(),
        )
        .unwrap();
    (database, catalog, integrations, models)
}

fn route_request() -> ModelRouteRequest {
    ModelRouteRequest::new(
        vec![ModelCapabilityRequirement::new("reasoning:structured", 2, BTreeMap::new()).unwrap()],
        ModelRoutingPolicy::new(
            Vec::new(),
            vec![AgentProviderId::new("provider:stable").unwrap()],
            vec![ModelId::new("model:reasoning").unwrap()],
            vec![AgentProviderId::new("provider:stable").unwrap()],
            Some(50),
        )
        .unwrap(),
        110,
    )
    .unwrap()
}

#[test]
fn routing_matches_capability_and_provider_integration_remains_non_secret() {
    let (database, catalog, integrations, models) = provider_and_models();
    let router = PolicyModelRoutingService::new(models, catalog);
    let route = router.route(&route_request()).unwrap();
    assert_eq!(route.model().model_id().as_str(), "model:reasoning");
    assert_eq!(
        route.availability().provider_id().as_str(),
        "provider:stable"
    );

    let provider = ProviderIntegrationService::new(integrations);
    let request = crate::ProviderBindingRequest::new(
        RuntimeExecutionId::new("execution:route").unwrap(),
        route.availability().provider_id().clone(),
        route.model().model_id().clone(),
        route.availability().id().clone(),
        route.availability().provider_model_reference(),
    )
    .unwrap();
    let binding = provider.prepare_binding(&request, 120).unwrap();
    let serialized = serde_json::to_string(&binding).unwrap();
    assert!(!serialized.contains("must-remain-in-legacy-storage"));
    assert!(!serialized.contains("agentId"));
    assert_eq!(binding.model_id().as_str(), "model:reasoning");
    assert_eq!(
        database
            .get_provider_by_id("legacy-provider", "claude")
            .unwrap()
            .unwrap()
            .settings_config["apiKey"],
        "must-remain-in-legacy-storage"
    );
}

#[test]
fn execution_activation_plan_composes_distinct_runtime_provider_and_model_identities() {
    let (_database, catalog, integrations, models) = provider_and_models();
    let instances = InMemoryRuntimeInstanceRepository::default();
    let registered = runtime_instance();
    instances.insert(registered.clone()).unwrap();
    let activating = registered
        .transition_to(RuntimeInstanceLifecycle::Activating, 1, 20)
        .unwrap();
    instances.update(activating.clone(), 1).unwrap();
    let ready = activating
        .transition_to(RuntimeInstanceLifecycle::Ready, 2, 21)
        .unwrap();
    instances.update(ready, 2).unwrap();

    let planner = ExecutionActivationService::new(
        instances,
        PolicyModelRoutingService::new(models, catalog),
        ProviderIntegrationService::new(integrations),
    );
    let plan = planner
        .prepare(
            RuntimeExecutionId::new("execution:plan").unwrap(),
            &RuntimeInstanceId::new("instance:command").unwrap(),
            &route_request(),
            120,
        )
        .unwrap();

    assert_eq!(plan.runtime_id().as_str(), "runtime:command");
    assert_eq!(plan.route().model().model_id().as_str(), "model:reasoning");
    assert_eq!(
        plan.provider_binding().provider_id().as_str(),
        "provider:stable"
    );
    assert_ne!(
        plan.runtime_id().as_str(),
        plan.route().model().model_id().as_str()
    );
    assert_ne!(
        plan.runtime_id().as_str(),
        plan.provider_binding().provider_id().as_str()
    );
}

#[test]
fn routing_fails_closed_for_unmet_model_capability() {
    let (_database, catalog, _integrations, models) = provider_and_models();
    let router = PolicyModelRoutingService::new(models, catalog);
    let request = ModelRouteRequest::new(
        vec![ModelCapabilityRequirement::new("vision:generate", 1, BTreeMap::new()).unwrap()],
        ModelRoutingPolicy::default(),
        110,
    )
    .unwrap();
    assert!(router.route(&request).is_err());
}

#[test]
fn routing_rejects_stale_availability_even_when_capability_matches() {
    let (_database, catalog, _integrations, models) = provider_and_models();
    let router = PolicyModelRoutingService::new(models, catalog);
    let request = ModelRouteRequest::new(
        vec![ModelCapabilityRequirement::new("reasoning:structured", 2, BTreeMap::new()).unwrap()],
        ModelRoutingPolicy::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Some(5)).unwrap(),
        110,
    )
    .unwrap();
    assert!(router.route(&request).is_err());
}
