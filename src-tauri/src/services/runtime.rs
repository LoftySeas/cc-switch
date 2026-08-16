//! Application service for Runtime Adapter discovery and context validation.
//!
//! This service has no operation for launching productive work.

use std::sync::Arc;

use crate::runtime_adapter::{RuntimeAdapter, RuntimeAdapterError, RuntimeAdapterRepository};
use crate::runtime_domain::{ExecutionContext, RuntimeDescriptor, RuntimeId, RuntimeProbe};

pub struct RuntimeService<R> {
    repository: R,
}

impl<R> RuntimeService<R>
where
    R: RuntimeAdapterRepository,
{
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn register(&self, adapter: Arc<dyn RuntimeAdapter>) -> Result<(), RuntimeAdapterError> {
        self.repository.register(adapter)
    }

    pub fn list(&self) -> Result<Vec<RuntimeDescriptor>, RuntimeAdapterError> {
        self.repository.list().map(|adapters| {
            adapters
                .into_iter()
                .map(|item| item.descriptor().clone())
                .collect()
        })
    }

    pub fn probe(&self, runtime_id: &RuntimeId) -> Result<RuntimeProbe, RuntimeAdapterError> {
        let adapter = self.require_adapter(runtime_id)?;
        let probe = adapter.probe()?;
        probe.validate()?;
        if &probe.runtime_id != runtime_id {
            return Err(RuntimeAdapterError::IdentityMismatch {
                expected: runtime_id.clone(),
                observed: probe.runtime_id,
            });
        }
        Ok(probe)
    }

    pub fn validate_context(&self, context: &ExecutionContext) -> Result<(), RuntimeAdapterError> {
        context.validate()?;
        self.require_adapter(context.binding().runtime_id())?
            .validate_context(context)
    }

    fn require_adapter(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Arc<dyn RuntimeAdapter>, RuntimeAdapterError> {
        self.repository
            .get(runtime_id)?
            .ok_or_else(|| RuntimeAdapterError::NotRegistered(runtime_id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_adapter::InMemoryRuntimeAdapterRepository;
    use crate::runtime_domain::{
        AgentRuntimeBinding, RuntimeAdapterId, RuntimeAvailability, RuntimeBindingId,
        RuntimeBindingLifecycle, RuntimeCapability, RuntimeExecutionId,
    };

    struct StubRuntimeAdapter {
        descriptor: RuntimeDescriptor,
        availability: RuntimeAvailability,
    }

    impl StubRuntimeAdapter {
        fn new(runtime_id: &str, availability: RuntimeAvailability) -> Self {
            Self {
                descriptor: RuntimeDescriptor::new(
                    RuntimeId::new(runtime_id).expect("valid Runtime ID"),
                    RuntimeAdapterId::new(format!("adapter:{runtime_id}"))
                        .expect("valid adapter ID"),
                    format!("Stub {runtime_id}"),
                    1,
                    vec![RuntimeCapability::new("execution:validation", 1)
                        .expect("valid capability")],
                )
                .expect("valid descriptor"),
                availability,
            }
        }
    }

    impl RuntimeAdapter for StubRuntimeAdapter {
        fn descriptor(&self) -> &RuntimeDescriptor {
            &self.descriptor
        }

        fn probe(&self) -> Result<RuntimeProbe, RuntimeAdapterError> {
            Ok(RuntimeProbe {
                runtime_id: self.descriptor.runtime_id().clone(),
                availability: self.availability,
                runtime_version: None,
                capabilities: vec![],
                diagnostics: vec![],
            })
        }
    }

    fn context(runtime_id: &str) -> ExecutionContext {
        ExecutionContext::new(
            RuntimeExecutionId::new("execution-1").expect("valid execution ID"),
            AgentRuntimeBinding::new(
                RuntimeBindingId::new("binding-1").expect("valid binding ID"),
                "agent-1",
                RuntimeId::new(runtime_id).expect("valid Runtime ID"),
                1_000,
            )
            .expect("valid binding")
            .transition_to(RuntimeBindingLifecycle::Active, 1, 1_001)
            .expect("binding activates"),
            vec!["docs/task.md".to_string()],
            1_002,
        )
        .expect("valid context")
    }

    #[test]
    fn adapters_register_without_runtime_specific_service_branches() {
        let repository = InMemoryRuntimeAdapterRepository::default();
        let service = RuntimeService::new(repository);
        service
            .register(Arc::new(StubRuntimeAdapter::new(
                "runtime:alpha",
                RuntimeAvailability::Ready,
            )))
            .expect("first adapter registers");
        service
            .register(Arc::new(StubRuntimeAdapter::new(
                "runtime:beta",
                RuntimeAvailability::Degraded,
            )))
            .expect("second adapter registers");

        let descriptors = service.list().expect("descriptors list");
        assert_eq!(descriptors.len(), 2);
        assert_eq!(descriptors[0].runtime_id().as_str(), "runtime:alpha");
        assert_eq!(descriptors[1].runtime_id().as_str(), "runtime:beta");
    }

    #[test]
    fn duplicate_runtime_identity_is_rejected() {
        let service = RuntimeService::new(InMemoryRuntimeAdapterRepository::default());
        service
            .register(Arc::new(StubRuntimeAdapter::new(
                "runtime:alpha",
                RuntimeAvailability::Ready,
            )))
            .expect("first adapter registers");
        let duplicate = service.register(Arc::new(StubRuntimeAdapter::new(
            "runtime:alpha",
            RuntimeAvailability::Ready,
        )));
        assert!(matches!(
            duplicate,
            Err(RuntimeAdapterError::AlreadyRegistered(_))
        ));
    }

    #[test]
    fn service_probes_and_validates_only_the_bound_adapter() {
        let service = RuntimeService::new(InMemoryRuntimeAdapterRepository::default());
        let adapter = Arc::new(StubRuntimeAdapter::new(
            "runtime:alpha",
            RuntimeAvailability::Ready,
        ));
        service
            .register(adapter.clone())
            .expect("adapter registers");

        let probe = service
            .probe(&RuntimeId::new("runtime:alpha").expect("valid Runtime ID"))
            .expect("probe succeeds");
        assert_eq!(probe.availability, RuntimeAvailability::Ready);
        service
            .validate_context(&context("runtime:alpha"))
            .expect("context validates");
    }

    #[test]
    fn unregistered_runtime_binding_fails_before_adapter_work() {
        let service = RuntimeService::new(InMemoryRuntimeAdapterRepository::default());
        let result = service.validate_context(&context("runtime:missing"));
        assert!(matches!(result, Err(RuntimeAdapterError::NotRegistered(_))));
    }
}
