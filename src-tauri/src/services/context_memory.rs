//! Context Manager service that resolves a bounded least-privilege package for
//! one execution without changing Agent identity or granting Permission.

use thiserror::Error;

use crate::{
    context_memory_domain::{
        ContextMemoryDomainError, ContextPackage, ContextPackageId, ContextPackageLifecycle,
        ContextPolicy, KnowledgeLifecycle, KnowledgeReference, KnowledgeReferenceId, MemoryEntry,
        MemoryEntryId, MemoryLifecycle,
    },
    context_memory_repository::{ContextMemoryRepository, ContextMemoryRepositoryError},
    runtime_domain::RuntimeExecutionId,
};

#[derive(Debug, Error)]
pub enum ContextMemoryServiceError {
    #[error(transparent)]
    Domain(#[from] ContextMemoryDomainError),
    #[error(transparent)]
    Repository(#[from] ContextMemoryRepositoryError),
    #[error("Context package was not found: {0}")]
    ContextNotFound(ContextPackageId),
    #[error("Memory entry was not found: {0}")]
    MemoryNotFound(MemoryEntryId),
    #[error("Knowledge reference was not found: {0}")]
    KnowledgeNotFound(KnowledgeReferenceId),
    #[error("Context policy does not match package policy")]
    PolicyMismatch,
    #[error("Context selection exceeds policy limits")]
    PolicyLimitExceeded,
    #[error("Memory is unavailable or outside the package Agent scope: {0}")]
    MemoryUnavailable(MemoryEntryId),
    #[error("Knowledge is unavailable or outside the package Agent scope: {0}")]
    KnowledgeUnavailable(KnowledgeReferenceId),
    #[error("Context source is not permitted by policy: {0}")]
    SourceDenied(String),
    #[error("Context package must be resolved before it can be sealed")]
    ContextNotResolved,
}

pub struct ContextMemoryService<R> {
    repository: R,
}

impl<R> ContextMemoryService<R>
where
    R: ContextMemoryRepository,
{
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn remember(&self, memory: MemoryEntry) -> Result<MemoryEntry, ContextMemoryServiceError> {
        Ok(self.repository.create_memory(memory)?)
    }

    pub fn register_knowledge(
        &self,
        knowledge: KnowledgeReference,
    ) -> Result<KnowledgeReference, ContextMemoryServiceError> {
        Ok(self.repository.create_knowledge(knowledge)?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_context(
        &self,
        id: ContextPackageId,
        execution_id: RuntimeExecutionId,
        agent_id: impl Into<String>,
        policy: &ContextPolicy,
        lifetime_seconds: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryServiceError> {
        if lifetime_seconds == 0 || lifetime_seconds > policy.max_lifetime_seconds() {
            return Err(ContextMemoryServiceError::PolicyLimitExceeded);
        }
        let lifetime = i64::try_from(lifetime_seconds)
            .map_err(|_| ContextMemoryServiceError::PolicyLimitExceeded)?;
        let expires_at = now
            .checked_add(lifetime)
            .ok_or(ContextMemoryServiceError::PolicyLimitExceeded)?;
        let package = ContextPackage::draft(
            id,
            execution_id,
            agent_id,
            policy.id().clone(),
            now,
            expires_at,
        )?;
        Ok(self.repository.create_context(package)?)
    }

    pub fn resolve_context(
        &self,
        id: &ContextPackageId,
        policy: &ContextPolicy,
        memory_ids: Vec<MemoryEntryId>,
        knowledge_ids: Vec<KnowledgeReferenceId>,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryServiceError> {
        let package = self.context(id)?;
        self.ensure_policy(&package, policy)?;
        self.validate_selection(&package, policy, &memory_ids, &knowledge_ids, now)?;
        Ok(self.repository.resolve_context(
            id,
            memory_ids,
            knowledge_ids,
            expected_revision,
            now,
        )?)
    }

    pub fn seal_context(
        &self,
        id: &ContextPackageId,
        policy: &ContextPolicy,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryServiceError> {
        let package = self.context(id)?;
        self.ensure_policy(&package, policy)?;
        if package.lifecycle() != ContextPackageLifecycle::Resolved {
            return Err(ContextMemoryServiceError::ContextNotResolved);
        }
        self.validate_selection(
            &package,
            policy,
            package.memory_ids(),
            package.knowledge_reference_ids(),
            now,
        )?;
        Ok(self.repository.seal_context(id, expected_revision, now)?)
    }

    pub fn execution_references(
        &self,
        id: &ContextPackageId,
        now: i64,
    ) -> Result<Vec<String>, ContextMemoryServiceError> {
        Ok(self.context(id)?.execution_references(now)?)
    }

    pub fn archive_memory(
        &self,
        id: &MemoryEntryId,
        expected_revision: u64,
        now: i64,
    ) -> Result<MemoryEntry, ContextMemoryServiceError> {
        Ok(self.repository.transition_memory(
            id,
            MemoryLifecycle::Archived,
            expected_revision,
            now,
        )?)
    }

    pub fn expire_memory(
        &self,
        id: &MemoryEntryId,
        expected_revision: u64,
        now: i64,
    ) -> Result<MemoryEntry, ContextMemoryServiceError> {
        Ok(self.repository.transition_memory(
            id,
            MemoryLifecycle::Expired,
            expected_revision,
            now,
        )?)
    }

    pub fn revoke_memory(
        &self,
        id: &MemoryEntryId,
        expected_revision: u64,
        now: i64,
    ) -> Result<MemoryEntry, ContextMemoryServiceError> {
        Ok(self.repository.transition_memory(
            id,
            MemoryLifecycle::Revoked,
            expected_revision,
            now,
        )?)
    }

    pub fn expire_knowledge(
        &self,
        id: &KnowledgeReferenceId,
        expected_revision: u64,
        now: i64,
    ) -> Result<KnowledgeReference, ContextMemoryServiceError> {
        Ok(self.repository.transition_knowledge(
            id,
            KnowledgeLifecycle::Expired,
            expected_revision,
            now,
        )?)
    }

    pub fn revoke_knowledge(
        &self,
        id: &KnowledgeReferenceId,
        expected_revision: u64,
        now: i64,
    ) -> Result<KnowledgeReference, ContextMemoryServiceError> {
        Ok(self.repository.transition_knowledge(
            id,
            KnowledgeLifecycle::Revoked,
            expected_revision,
            now,
        )?)
    }

    pub fn expire_context(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryServiceError> {
        Ok(self.repository.expire_context(id, expected_revision, now)?)
    }

    pub fn revoke_context(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryServiceError> {
        Ok(self.repository.revoke_context(id, expected_revision, now)?)
    }

    fn context(&self, id: &ContextPackageId) -> Result<ContextPackage, ContextMemoryServiceError> {
        self.repository
            .get_context(id)?
            .ok_or_else(|| ContextMemoryServiceError::ContextNotFound(id.clone()))
    }

    fn ensure_policy(
        &self,
        package: &ContextPackage,
        policy: &ContextPolicy,
    ) -> Result<(), ContextMemoryServiceError> {
        if package.policy_id() != policy.id() {
            return Err(ContextMemoryServiceError::PolicyMismatch);
        }
        Ok(())
    }

    fn validate_selection(
        &self,
        package: &ContextPackage,
        policy: &ContextPolicy,
        memory_ids: &[MemoryEntryId],
        knowledge_ids: &[KnowledgeReferenceId],
        now: i64,
    ) -> Result<(), ContextMemoryServiceError> {
        if memory_ids.len() > policy.max_memory_entries()
            || knowledge_ids.len() > policy.max_knowledge_references()
        {
            return Err(ContextMemoryServiceError::PolicyLimitExceeded);
        }

        for id in memory_ids {
            let memory = self
                .repository
                .get_memory(id)?
                .ok_or_else(|| ContextMemoryServiceError::MemoryNotFound(id.clone()))?;
            if memory.agent_id() != package.agent_id() || !memory.is_available(now) {
                return Err(ContextMemoryServiceError::MemoryUnavailable(id.clone()));
            }
            if !policy.allows_memory(&memory) {
                return Err(ContextMemoryServiceError::SourceDenied(id.to_string()));
            }
        }

        for id in knowledge_ids {
            let knowledge = self
                .repository
                .get_knowledge(id)?
                .ok_or_else(|| ContextMemoryServiceError::KnowledgeNotFound(id.clone()))?;
            if knowledge
                .agent_scope()
                .is_some_and(|scope| scope != package.agent_id())
                || !knowledge.is_available(now)
            {
                return Err(ContextMemoryServiceError::KnowledgeUnavailable(id.clone()));
            }
            if !policy.allows_knowledge(&knowledge) {
                return Err(ContextMemoryServiceError::SourceDenied(id.to_string()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        context_memory_domain::{
            ContextPolicyId, KnowledgeSourceKind, KnowledgeTrust, MemoryContent, MemoryKind,
            MemorySensitivity,
        },
        context_memory_repository::SqliteContextMemoryRepository,
        database::Database,
        runtime_domain::{
            AgentRuntimeBinding, ExecutionContext, RuntimeBindingId, RuntimeBindingLifecycle,
            RuntimeId,
        },
    };

    fn policy() -> ContextPolicy {
        ContextPolicy::new(
            ContextPolicyId::new("policy:bounded").unwrap(),
            vec![MemoryKind::Decision, MemoryKind::Summary],
            vec![KnowledgeSourceKind::File, KnowledgeSourceKind::Artifact],
            3,
            3,
            MemorySensitivity::Internal,
            true,
            100,
        )
        .unwrap()
    }

    fn memory(id: &str, agent_id: &str, expires_at: i64) -> MemoryEntry {
        MemoryEntry::new(
            MemoryEntryId::new(id).unwrap(),
            agent_id,
            MemoryKind::Decision,
            MemoryContent::Text("Preserve the Runtime Adapter boundary".to_string()),
            MemorySensitivity::Internal,
            Some(RuntimeExecutionId::new("execution:source").unwrap()),
            10,
            expires_at,
        )
        .unwrap()
    }

    #[test]
    fn context_manager_resolves_least_privilege_refs_for_one_execution() {
        let repository = SqliteContextMemoryRepository::new(Arc::new(Database::memory().unwrap()));
        let service = ContextMemoryService::new(repository.clone());
        let memory = service
            .remember(memory("memory:one", "agent:one", 100))
            .unwrap();
        let knowledge = service
            .register_knowledge(
                KnowledgeReference::new(
                    KnowledgeReferenceId::new("knowledge:one").unwrap(),
                    Some("agent:one".to_string()),
                    KnowledgeSourceKind::File,
                    "repo://docs/architecture/agent-os-architecture-v1.md",
                    KnowledgeTrust::Verified,
                    None,
                    10,
                    100,
                )
                .unwrap(),
            )
            .unwrap();
        let package = service
            .create_context(
                ContextPackageId::new("context:one").unwrap(),
                RuntimeExecutionId::new("execution:one").unwrap(),
                "agent:one",
                &policy(),
                50,
                10,
            )
            .unwrap();
        let resolved = service
            .resolve_context(
                package.id(),
                &policy(),
                vec![memory.id().clone()],
                vec![knowledge.id().clone()],
                package.revision(),
                20,
            )
            .unwrap();
        let sealed = service
            .seal_context(package.id(), &policy(), resolved.revision(), 21)
            .unwrap();
        let references = service.execution_references(sealed.id(), 22).unwrap();

        let binding = AgentRuntimeBinding::new(
            RuntimeBindingId::new("binding:one").unwrap(),
            "agent:one",
            RuntimeId::new("runtime:one").unwrap(),
            1,
        )
        .unwrap()
        .transition_to(RuntimeBindingLifecycle::Active, 1, 2)
        .unwrap();
        let execution_context = ExecutionContext::new(
            RuntimeExecutionId::new("execution:one").unwrap(),
            binding,
            references.clone(),
            22,
        )
        .unwrap();
        assert_eq!(execution_context.context_references(), references);
        assert_eq!(sealed.agent_id(), "agent:one");
        assert_ne!(sealed.id().as_str(), sealed.agent_id());
    }

    #[test]
    fn context_manager_denies_cross_agent_unverified_and_expired_sources() {
        let repository = SqliteContextMemoryRepository::new(Arc::new(Database::memory().unwrap()));
        let service = ContextMemoryService::new(repository);
        let other_memory = service
            .remember(memory("memory:other", "agent:other", 100))
            .unwrap();
        let unverified = service
            .register_knowledge(
                KnowledgeReference::new(
                    KnowledgeReferenceId::new("knowledge:unverified").unwrap(),
                    None,
                    KnowledgeSourceKind::File,
                    "repo://docs/unverified.md",
                    KnowledgeTrust::Unverified,
                    None,
                    10,
                    100,
                )
                .unwrap(),
            )
            .unwrap();
        let expired = service
            .remember(memory("memory:expired", "agent:one", 15))
            .unwrap();
        let package = service
            .create_context(
                ContextPackageId::new("context:denied").unwrap(),
                RuntimeExecutionId::new("execution:denied").unwrap(),
                "agent:one",
                &policy(),
                50,
                10,
            )
            .unwrap();

        assert!(matches!(
            service.resolve_context(
                package.id(),
                &policy(),
                vec![other_memory.id().clone()],
                Vec::new(),
                package.revision(),
                20,
            ),
            Err(ContextMemoryServiceError::MemoryUnavailable(_))
        ));
        assert!(matches!(
            service.resolve_context(
                package.id(),
                &policy(),
                Vec::new(),
                vec![unverified.id().clone()],
                package.revision(),
                20,
            ),
            Err(ContextMemoryServiceError::SourceDenied(_))
        ));
        assert!(matches!(
            service.resolve_context(
                package.id(),
                &policy(),
                vec![expired.id().clone()],
                Vec::new(),
                package.revision(),
                20,
            ),
            Err(ContextMemoryServiceError::MemoryUnavailable(_))
        ));
    }
}
