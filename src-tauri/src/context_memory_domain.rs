//! Governed Context Package, Memory, and Knowledge Reference domain types.
//!
//! These records are deliberately separate from Agent identity. Durable values
//! are time-bounded, context packages contain references rather than unbounded
//! conversation history, and secret material is represented only by opaque
//! references.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::runtime_domain::RuntimeExecutionId;

const MAX_ID_LENGTH: usize = 128;
const MAX_REFERENCE_LENGTH: usize = 1024;
const MAX_MEMORY_TEXT_LENGTH: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextMemoryDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Retention must expire after creation")]
    InvalidRetention,
    #[error("Record timestamp order is invalid")]
    InvalidTimestamp,
    #[error("Revision must be positive")]
    InvalidRevision,
    #[error("Expected revision {expected}, current revision {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("Invalid {record} lifecycle transition from {from} to {to}")]
    InvalidLifecycle {
        record: &'static str,
        from: &'static str,
        to: &'static str,
    },
    #[error("Secret memory must contain an opaque reference, never durable secret text")]
    SecretTextForbidden,
    #[error("Context policy must allow at least one bounded source")]
    EmptyContextPolicy,
    #[error("Context policy limit must be positive")]
    InvalidPolicyLimit,
    #[error("Context selection contains duplicate references")]
    DuplicateReference,
    #[error("Context package must be sealed and unexpired before use")]
    ContextUnavailable,
}

fn bounded(
    field: &'static str,
    value: impl Into<String>,
    max: usize,
) -> Result<String, ContextMemoryDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(ContextMemoryDomainError::Empty { field });
    }
    if value.chars().count() > max {
        return Err(ContextMemoryDomainError::TooLong { field, max });
    }
    Ok(value.to_string())
}

fn identifier(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, ContextMemoryDomainError> {
    let value = bounded(field, value, MAX_ID_LENGTH)?;
    if value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(ContextMemoryDomainError::InvalidIdentifier { field });
    }
    Ok(value)
}

macro_rules! context_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ContextMemoryDomainError> {
                Ok(Self(identifier($field, value)?))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            fn validate(&self) -> Result<(), ContextMemoryDomainError> {
                identifier($field, self.0.clone()).map(|_| ())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

context_id!(MemoryEntryId, "Memory entry ID");
context_id!(KnowledgeReferenceId, "Knowledge reference ID");
context_id!(ContextPackageId, "Context package ID");
context_id!(ContextPolicyId, "Context policy ID");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Observation,
    Decision,
    Preference,
    Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySensitivity {
    Public,
    Internal,
    Confidential,
    OpaqueSecret,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MemoryContent {
    Text(String),
    OpaqueReference(String),
}

impl MemoryContent {
    fn validate(&self) -> Result<(), ContextMemoryDomainError> {
        match self {
            Self::Text(value) => {
                bounded("Memory text", value.clone(), MAX_MEMORY_TEXT_LENGTH).map(|_| ())
            }
            Self::OpaqueReference(value) => bounded(
                "Opaque memory reference",
                value.clone(),
                MAX_REFERENCE_LENGTH,
            )
            .map(|_| ()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLifecycle {
    Active,
    Archived,
    Expired,
    Revoked,
}

impl MemoryLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }

    fn can_transition_to(self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Active, Self::Archived | Self::Expired | Self::Revoked)
                | (Self::Archived, Self::Expired | Self::Revoked)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    id: MemoryEntryId,
    agent_id: String,
    kind: MemoryKind,
    content: MemoryContent,
    sensitivity: MemorySensitivity,
    source_execution_id: Option<RuntimeExecutionId>,
    lifecycle: MemoryLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
    expires_at: i64,
}

impl MemoryEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: MemoryEntryId,
        agent_id: impl Into<String>,
        kind: MemoryKind,
        content: MemoryContent,
        sensitivity: MemorySensitivity,
        source_execution_id: Option<RuntimeExecutionId>,
        created_at: i64,
        expires_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        let entry = Self {
            id,
            agent_id: identifier("Agent ID reference", agent_id)?,
            kind,
            content,
            sensitivity,
            source_execution_id,
            lifecycle: MemoryLifecycle::Active,
            revision: 1,
            created_at,
            updated_at: created_at,
            expires_at,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn id(&self) -> &MemoryEntryId {
        &self.id
    }
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn kind(&self) -> MemoryKind {
        self.kind
    }
    pub fn content(&self) -> &MemoryContent {
        &self.content
    }
    pub fn sensitivity(&self) -> MemorySensitivity {
        self.sensitivity
    }
    pub fn source_execution_id(&self) -> Option<&RuntimeExecutionId> {
        self.source_execution_id.as_ref()
    }
    pub fn lifecycle(&self) -> MemoryLifecycle {
        self.lifecycle
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
    pub fn is_available(&self, now: i64) -> bool {
        self.lifecycle == MemoryLifecycle::Active && now < self.expires_at
    }

    pub fn transition(
        &self,
        target: MemoryLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        if self.revision != expected_revision {
            return Err(ContextMemoryDomainError::RevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(ContextMemoryDomainError::InvalidLifecycle {
                record: "Memory",
                from: self.lifecycle.as_str(),
                to: target.as_str(),
            });
        }
        if updated_at < self.updated_at {
            return Err(ContextMemoryDomainError::InvalidTimestamp);
        }
        let mut next = self.clone();
        next.lifecycle = target;
        next.revision += 1;
        next.updated_at = updated_at;
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), ContextMemoryDomainError> {
        self.id.validate()?;
        identifier("Agent ID reference", self.agent_id.clone())?;
        self.content.validate()?;
        if self.sensitivity == MemorySensitivity::OpaqueSecret
            && !matches!(self.content, MemoryContent::OpaqueReference(_))
        {
            return Err(ContextMemoryDomainError::SecretTextForbidden);
        }
        if self.revision == 0 {
            return Err(ContextMemoryDomainError::InvalidRevision);
        }
        if self.expires_at <= self.created_at {
            return Err(ContextMemoryDomainError::InvalidRetention);
        }
        if self.updated_at < self.created_at {
            return Err(ContextMemoryDomainError::InvalidTimestamp);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSourceKind {
    File,
    Artifact,
    Repository,
    Url,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeTrust {
    Unverified,
    Verified,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeLifecycle {
    Active,
    Archived,
    Expired,
    Revoked,
}

impl KnowledgeLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }
    fn can_transition_to(self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Active, Self::Archived | Self::Expired | Self::Revoked)
                | (Self::Archived, Self::Expired | Self::Revoked)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeReference {
    id: KnowledgeReferenceId,
    agent_scope: Option<String>,
    source_kind: KnowledgeSourceKind,
    locator: String,
    trust: KnowledgeTrust,
    source_execution_id: Option<RuntimeExecutionId>,
    lifecycle: KnowledgeLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
    expires_at: i64,
}

impl KnowledgeReference {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: KnowledgeReferenceId,
        agent_scope: Option<String>,
        source_kind: KnowledgeSourceKind,
        locator: impl Into<String>,
        trust: KnowledgeTrust,
        source_execution_id: Option<RuntimeExecutionId>,
        created_at: i64,
        expires_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        let reference = Self {
            id,
            agent_scope: agent_scope
                .map(|value| identifier("Agent scope", value))
                .transpose()?,
            source_kind,
            locator: bounded("Knowledge locator", locator, MAX_REFERENCE_LENGTH)?,
            trust,
            source_execution_id,
            lifecycle: KnowledgeLifecycle::Active,
            revision: 1,
            created_at,
            updated_at: created_at,
            expires_at,
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn id(&self) -> &KnowledgeReferenceId {
        &self.id
    }
    pub fn agent_scope(&self) -> Option<&str> {
        self.agent_scope.as_deref()
    }
    pub fn source_kind(&self) -> KnowledgeSourceKind {
        self.source_kind
    }
    pub fn locator(&self) -> &str {
        &self.locator
    }
    pub fn trust(&self) -> KnowledgeTrust {
        self.trust
    }
    pub fn source_execution_id(&self) -> Option<&RuntimeExecutionId> {
        self.source_execution_id.as_ref()
    }
    pub fn lifecycle(&self) -> KnowledgeLifecycle {
        self.lifecycle
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
    pub fn is_available(&self, now: i64) -> bool {
        self.lifecycle == KnowledgeLifecycle::Active
            && self.trust != KnowledgeTrust::Rejected
            && now < self.expires_at
    }

    pub fn transition(
        &self,
        target: KnowledgeLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        if self.revision != expected_revision {
            return Err(ContextMemoryDomainError::RevisionConflict {
                expected: expected_revision,
                current: self.revision,
            });
        }
        if !self.lifecycle.can_transition_to(target) {
            return Err(ContextMemoryDomainError::InvalidLifecycle {
                record: "Knowledge",
                from: self.lifecycle.as_str(),
                to: target.as_str(),
            });
        }
        if updated_at < self.updated_at {
            return Err(ContextMemoryDomainError::InvalidTimestamp);
        }
        let mut next = self.clone();
        next.lifecycle = target;
        next.revision += 1;
        next.updated_at = updated_at;
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), ContextMemoryDomainError> {
        self.id.validate()?;
        if let Some(agent_scope) = &self.agent_scope {
            identifier("Agent scope", agent_scope.clone())?;
        }
        bounded(
            "Knowledge locator",
            self.locator.clone(),
            MAX_REFERENCE_LENGTH,
        )?;
        if self.revision == 0 {
            return Err(ContextMemoryDomainError::InvalidRevision);
        }
        if self.expires_at <= self.created_at {
            return Err(ContextMemoryDomainError::InvalidRetention);
        }
        if self.updated_at < self.created_at {
            return Err(ContextMemoryDomainError::InvalidTimestamp);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPolicy {
    id: ContextPolicyId,
    allowed_memory_kinds: HashSet<MemoryKind>,
    allowed_knowledge_sources: HashSet<KnowledgeSourceKind>,
    max_memory_entries: usize,
    max_knowledge_references: usize,
    max_sensitivity: MemorySensitivity,
    require_verified_knowledge: bool,
    max_lifetime_seconds: u64,
}

impl ContextPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ContextPolicyId,
        allowed_memory_kinds: Vec<MemoryKind>,
        allowed_knowledge_sources: Vec<KnowledgeSourceKind>,
        max_memory_entries: usize,
        max_knowledge_references: usize,
        max_sensitivity: MemorySensitivity,
        require_verified_knowledge: bool,
        max_lifetime_seconds: u64,
    ) -> Result<Self, ContextMemoryDomainError> {
        let policy = Self {
            id,
            allowed_memory_kinds: allowed_memory_kinds.into_iter().collect(),
            allowed_knowledge_sources: allowed_knowledge_sources.into_iter().collect(),
            max_memory_entries,
            max_knowledge_references,
            max_sensitivity,
            require_verified_knowledge,
            max_lifetime_seconds,
        };
        if policy.allowed_memory_kinds.is_empty() && policy.allowed_knowledge_sources.is_empty() {
            return Err(ContextMemoryDomainError::EmptyContextPolicy);
        }
        if (policy.max_memory_entries == 0 && !policy.allowed_memory_kinds.is_empty())
            || (policy.max_knowledge_references == 0
                && !policy.allowed_knowledge_sources.is_empty())
            || policy.max_lifetime_seconds == 0
        {
            return Err(ContextMemoryDomainError::InvalidPolicyLimit);
        }
        Ok(policy)
    }

    pub fn id(&self) -> &ContextPolicyId {
        &self.id
    }
    pub fn max_memory_entries(&self) -> usize {
        self.max_memory_entries
    }
    pub fn max_knowledge_references(&self) -> usize {
        self.max_knowledge_references
    }
    pub fn max_lifetime_seconds(&self) -> u64 {
        self.max_lifetime_seconds
    }
    pub fn allows_memory(&self, memory: &MemoryEntry) -> bool {
        self.allowed_memory_kinds.contains(&memory.kind())
            && memory.sensitivity() <= self.max_sensitivity
    }
    pub fn allows_knowledge(&self, knowledge: &KnowledgeReference) -> bool {
        self.allowed_knowledge_sources
            .contains(&knowledge.source_kind())
            && (!self.require_verified_knowledge || knowledge.trust() == KnowledgeTrust::Verified)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackageLifecycle {
    Draft,
    Resolved,
    Sealed,
    Expired,
    Revoked,
}

impl ContextPackageLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Resolved => "resolved",
            Self::Sealed => "sealed",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackage {
    id: ContextPackageId,
    execution_id: RuntimeExecutionId,
    agent_id: String,
    policy_id: ContextPolicyId,
    memory_ids: Vec<MemoryEntryId>,
    knowledge_reference_ids: Vec<KnowledgeReferenceId>,
    lifecycle: ContextPackageLifecycle,
    revision: u64,
    created_at: i64,
    updated_at: i64,
    expires_at: i64,
}

impl ContextPackage {
    pub fn draft(
        id: ContextPackageId,
        execution_id: RuntimeExecutionId,
        agent_id: impl Into<String>,
        policy_id: ContextPolicyId,
        created_at: i64,
        expires_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        let package = Self {
            id,
            execution_id,
            agent_id: identifier("Agent ID reference", agent_id)?,
            policy_id,
            memory_ids: Vec::new(),
            knowledge_reference_ids: Vec::new(),
            lifecycle: ContextPackageLifecycle::Draft,
            revision: 1,
            created_at,
            updated_at: created_at,
            expires_at,
        };
        package.validate()?;
        Ok(package)
    }

    pub fn id(&self) -> &ContextPackageId {
        &self.id
    }
    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn policy_id(&self) -> &ContextPolicyId {
        &self.policy_id
    }
    pub fn memory_ids(&self) -> &[MemoryEntryId] {
        &self.memory_ids
    }
    pub fn knowledge_reference_ids(&self) -> &[KnowledgeReferenceId] {
        &self.knowledge_reference_ids
    }
    pub fn lifecycle(&self) -> ContextPackageLifecycle {
        self.lifecycle
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn created_at(&self) -> i64 {
        self.created_at
    }
    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }

    pub fn resolve(
        &self,
        memory_ids: Vec<MemoryEntryId>,
        knowledge_reference_ids: Vec<KnowledgeReferenceId>,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        self.ensure_revision(expected_revision)?;
        if self.lifecycle != ContextPackageLifecycle::Draft {
            return Err(ContextMemoryDomainError::InvalidLifecycle {
                record: "Context package",
                from: self.lifecycle.as_str(),
                to: ContextPackageLifecycle::Resolved.as_str(),
            });
        }
        if updated_at < self.updated_at || updated_at >= self.expires_at {
            return Err(ContextMemoryDomainError::InvalidTimestamp);
        }
        if memory_ids.iter().collect::<HashSet<_>>().len() != memory_ids.len()
            || knowledge_reference_ids.iter().collect::<HashSet<_>>().len()
                != knowledge_reference_ids.len()
        {
            return Err(ContextMemoryDomainError::DuplicateReference);
        }
        let mut next = self.clone();
        next.memory_ids = memory_ids;
        next.knowledge_reference_ids = knowledge_reference_ids;
        next.lifecycle = ContextPackageLifecycle::Resolved;
        next.revision += 1;
        next.updated_at = updated_at;
        next.validate()?;
        Ok(next)
    }

    pub fn seal(
        &self,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        self.transition_from(
            ContextPackageLifecycle::Resolved,
            ContextPackageLifecycle::Sealed,
            expected_revision,
            updated_at,
        )
    }
    pub fn expire(
        &self,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        if !matches!(
            self.lifecycle,
            ContextPackageLifecycle::Draft
                | ContextPackageLifecycle::Resolved
                | ContextPackageLifecycle::Sealed
        ) {
            return Err(ContextMemoryDomainError::InvalidLifecycle {
                record: "Context package",
                from: self.lifecycle.as_str(),
                to: ContextPackageLifecycle::Expired.as_str(),
            });
        }
        self.transition_to(
            ContextPackageLifecycle::Expired,
            expected_revision,
            updated_at,
        )
    }
    pub fn revoke(
        &self,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        if !matches!(
            self.lifecycle,
            ContextPackageLifecycle::Draft
                | ContextPackageLifecycle::Resolved
                | ContextPackageLifecycle::Sealed
        ) {
            return Err(ContextMemoryDomainError::InvalidLifecycle {
                record: "Context package",
                from: self.lifecycle.as_str(),
                to: ContextPackageLifecycle::Revoked.as_str(),
            });
        }
        self.transition_to(
            ContextPackageLifecycle::Revoked,
            expected_revision,
            updated_at,
        )
    }

    pub fn execution_references(&self, now: i64) -> Result<Vec<String>, ContextMemoryDomainError> {
        if self.lifecycle != ContextPackageLifecycle::Sealed || now >= self.expires_at {
            return Err(ContextMemoryDomainError::ContextUnavailable);
        }
        let mut references =
            Vec::with_capacity(1 + self.memory_ids.len() + self.knowledge_reference_ids.len());
        references.push(format!("context-package:{}", self.id));
        references.extend(self.memory_ids.iter().map(|id| format!("memory:{id}")));
        references.extend(
            self.knowledge_reference_ids
                .iter()
                .map(|id| format!("knowledge:{id}")),
        );
        Ok(references)
    }

    pub fn validate(&self) -> Result<(), ContextMemoryDomainError> {
        self.id.validate()?;
        identifier("Agent ID reference", self.agent_id.clone())?;
        self.policy_id.validate()?;
        if self.revision == 0 {
            return Err(ContextMemoryDomainError::InvalidRevision);
        }
        if self.expires_at <= self.created_at {
            return Err(ContextMemoryDomainError::InvalidRetention);
        }
        if self.updated_at < self.created_at {
            return Err(ContextMemoryDomainError::InvalidTimestamp);
        }
        if self.memory_ids.iter().collect::<HashSet<_>>().len() != self.memory_ids.len()
            || self
                .knowledge_reference_ids
                .iter()
                .collect::<HashSet<_>>()
                .len()
                != self.knowledge_reference_ids.len()
        {
            return Err(ContextMemoryDomainError::DuplicateReference);
        }
        for id in &self.memory_ids {
            id.validate()?;
        }
        for id in &self.knowledge_reference_ids {
            id.validate()?;
        }
        Ok(())
    }

    fn ensure_revision(&self, expected: u64) -> Result<(), ContextMemoryDomainError> {
        if self.revision != expected {
            return Err(ContextMemoryDomainError::RevisionConflict {
                expected,
                current: self.revision,
            });
        }
        Ok(())
    }
    fn transition_from(
        &self,
        from: ContextPackageLifecycle,
        to: ContextPackageLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        if self.lifecycle != from {
            return Err(ContextMemoryDomainError::InvalidLifecycle {
                record: "Context package",
                from: self.lifecycle.as_str(),
                to: to.as_str(),
            });
        }
        self.transition_to(to, expected_revision, updated_at)
    }
    fn transition_to(
        &self,
        target: ContextPackageLifecycle,
        expected_revision: u64,
        updated_at: i64,
    ) -> Result<Self, ContextMemoryDomainError> {
        self.ensure_revision(expected_revision)?;
        if updated_at < self.updated_at {
            return Err(ContextMemoryDomainError::InvalidTimestamp);
        }
        let mut next = self.clone();
        next.lifecycle = target;
        next.revision += 1;
        next.updated_at = updated_at;
        next.validate()?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_memory_requires_opaque_reference_and_bounded_retention() {
        let secret_text = MemoryEntry::new(
            MemoryEntryId::new("memory:secret").unwrap(),
            "agent:one",
            MemoryKind::Observation,
            MemoryContent::Text("raw credential".to_string()),
            MemorySensitivity::OpaqueSecret,
            None,
            10,
            20,
        );
        assert!(matches!(
            secret_text,
            Err(ContextMemoryDomainError::SecretTextForbidden)
        ));

        let opaque = MemoryEntry::new(
            MemoryEntryId::new("memory:opaque").unwrap(),
            "agent:one",
            MemoryKind::Observation,
            MemoryContent::OpaqueReference("secret-broker:key-one".to_string()),
            MemorySensitivity::OpaqueSecret,
            None,
            10,
            20,
        )
        .unwrap();
        assert!(opaque.is_available(19));
        assert!(!opaque.is_available(20));
    }

    #[test]
    fn memory_lifecycle_is_revisioned_and_cannot_reactivate() {
        let memory = MemoryEntry::new(
            MemoryEntryId::new("memory:decision").unwrap(),
            "agent:one",
            MemoryKind::Decision,
            MemoryContent::Text("Use the bounded execution path".to_string()),
            MemorySensitivity::Internal,
            None,
            10,
            100,
        )
        .unwrap();
        let archived = memory.transition(MemoryLifecycle::Archived, 1, 20).unwrap();
        assert_eq!(archived.revision(), 2);
        assert!(!archived.is_available(21));
        assert!(archived.transition(MemoryLifecycle::Active, 2, 22).is_err());
        assert!(matches!(
            memory.transition(MemoryLifecycle::Expired, 2, 20),
            Err(ContextMemoryDomainError::RevisionConflict { .. })
        ));
    }

    #[test]
    fn context_package_must_resolve_then_seal_before_producing_references() {
        let package = ContextPackage::draft(
            ContextPackageId::new("context:one").unwrap(),
            RuntimeExecutionId::new("execution:one").unwrap(),
            "agent:one",
            ContextPolicyId::new("policy:one").unwrap(),
            10,
            100,
        )
        .unwrap();
        assert!(package.execution_references(11).is_err());
        let resolved = package
            .resolve(
                vec![MemoryEntryId::new("memory:one").unwrap()],
                vec![KnowledgeReferenceId::new("knowledge:one").unwrap()],
                1,
                20,
            )
            .unwrap();
        let sealed = resolved.seal(2, 21).unwrap();
        assert_eq!(
            sealed.execution_references(22).unwrap(),
            vec![
                "context-package:context:one",
                "memory:memory:one",
                "knowledge:knowledge:one"
            ]
        );
        assert!(sealed.execution_references(100).is_err());
    }

    #[test]
    fn context_policy_limits_source_kind_trust_and_sensitivity() {
        let policy = ContextPolicy::new(
            ContextPolicyId::new("policy:least-privilege").unwrap(),
            vec![MemoryKind::Decision],
            vec![KnowledgeSourceKind::File],
            2,
            2,
            MemorySensitivity::Internal,
            true,
            60,
        )
        .unwrap();
        let memory = MemoryEntry::new(
            MemoryEntryId::new("memory:confidential").unwrap(),
            "agent:one",
            MemoryKind::Decision,
            MemoryContent::Text("decision".to_string()),
            MemorySensitivity::Confidential,
            None,
            1,
            100,
        )
        .unwrap();
        let knowledge = KnowledgeReference::new(
            KnowledgeReferenceId::new("knowledge:unverified").unwrap(),
            None,
            KnowledgeSourceKind::File,
            "repo://docs/design.md",
            KnowledgeTrust::Unverified,
            None,
            1,
            100,
        )
        .unwrap();
        assert!(!policy.allows_memory(&memory));
        assert!(!policy.allows_knowledge(&knowledge));
    }
}
