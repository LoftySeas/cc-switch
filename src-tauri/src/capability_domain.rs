//! Agent OS Capability governance domain.
//!
//! Capability states whether behavior is evidenced as possible. Nothing in this
//! module grants authority or makes an authorization decision.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::runtime_domain::RuntimeExecutionId;

const MAX_ID_LENGTH: usize = 160;
const MAX_TEXT_LENGTH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CapabilityDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Capability version must be positive")]
    InvalidVersion,
    #[error("Capability evidence timestamp must not be negative")]
    InvalidTimestamp,
    #[error("Capability evidence confidence must be between 0 and 100")]
    InvalidConfidence,
    #[error("Optional Capability requirement must declare an acceptable fallback")]
    MissingOptionalFallback,
    #[error("Duplicate Capability requirement: {0}")]
    DuplicateRequirement(CapabilityId),
    #[error("Capability snapshot contains an invalid satisfied entry")]
    InvalidSatisfiedEntry,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, CapabilityDomainError> {
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

typed_id!(CapabilityId, "Capability ID");
typed_id!(CapabilityEvidenceId, "Capability evidence ID");
typed_id!(CapabilitySnapshotId, "Capability snapshot ID");

fn identifier(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, CapabilityDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(CapabilityDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(CapabilityDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(CapabilityDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn text(field: &'static str, value: impl Into<String>) -> Result<String, CapabilityDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(CapabilityDomainError::Empty { field });
    }
    if value.chars().count() > MAX_TEXT_LENGTH {
        return Err(CapabilityDomainError::TooLong {
            field,
            max: MAX_TEXT_LENGTH,
        });
    }
    Ok(value.to_string())
}

fn validate_metadata(metadata: &BTreeMap<String, String>) -> Result<(), CapabilityDomainError> {
    for (key, value) in metadata {
        identifier("Capability metadata key", key.clone())?;
        text("Capability metadata value", value.clone())?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDefinition {
    id: CapabilityId,
    version: u16,
    display_name: String,
    description: String,
    constraint_vocabulary: BTreeMap<String, String>,
}

impl CapabilityDefinition {
    pub fn new(
        id: CapabilityId,
        version: u16,
        display_name: impl Into<String>,
        description: impl Into<String>,
        constraint_vocabulary: BTreeMap<String, String>,
    ) -> Result<Self, CapabilityDomainError> {
        if version == 0 {
            return Err(CapabilityDomainError::InvalidVersion);
        }
        validate_metadata(&constraint_vocabulary)?;
        Ok(Self {
            id,
            version,
            display_name: text("Capability display name", display_name)?,
            description: text("Capability description", description)?,
            constraint_vocabulary,
        })
    }

    pub fn id(&self) -> &CapabilityId {
        &self.id
    }
    pub fn version(&self) -> u16 {
        self.version
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn constraint_vocabulary(&self) -> &BTreeMap<String, String> {
        &self.constraint_vocabulary
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRequirementLevel {
    Required,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRequirement {
    capability_id: CapabilityId,
    minimum_version: u16,
    level: CapabilityRequirementLevel,
    required_constraints: BTreeMap<String, String>,
    max_evidence_age_ms: Option<i64>,
    fallback_ref: Option<String>,
}

impl CapabilityRequirement {
    pub fn new(
        capability_id: CapabilityId,
        minimum_version: u16,
        level: CapabilityRequirementLevel,
        required_constraints: BTreeMap<String, String>,
        max_evidence_age_ms: Option<i64>,
        fallback_ref: Option<String>,
    ) -> Result<Self, CapabilityDomainError> {
        if minimum_version == 0 {
            return Err(CapabilityDomainError::InvalidVersion);
        }
        if max_evidence_age_ms.is_some_and(|age| age < 0) {
            return Err(CapabilityDomainError::InvalidTimestamp);
        }
        validate_metadata(&required_constraints)?;
        let fallback_ref = fallback_ref
            .map(|value| identifier("Capability fallback reference", value))
            .transpose()?;
        if level == CapabilityRequirementLevel::Optional && fallback_ref.is_none() {
            return Err(CapabilityDomainError::MissingOptionalFallback);
        }
        Ok(Self {
            capability_id,
            minimum_version,
            level,
            required_constraints,
            max_evidence_age_ms,
            fallback_ref,
        })
    }

    pub fn capability_id(&self) -> &CapabilityId {
        &self.capability_id
    }
    pub fn minimum_version(&self) -> u16 {
        self.minimum_version
    }
    pub fn level(&self) -> CapabilityRequirementLevel {
        self.level
    }
    pub fn required_constraints(&self) -> &BTreeMap<String, String> {
        &self.required_constraints
    }
    pub fn max_evidence_age_ms(&self) -> Option<i64> {
        self.max_evidence_age_ms
    }
    pub fn fallback_ref(&self) -> Option<&str> {
        self.fallback_ref.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityEvidenceSourceKind {
    Runtime,
    Provider,
    Model,
    Tool,
    Configuration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupportState {
    Supported,
    Unsupported,
    Unknown,
    RequiresConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEvidence {
    id: CapabilityEvidenceId,
    capability_id: CapabilityId,
    subject_ref: String,
    source_kind: CapabilityEvidenceSourceKind,
    supported_version: u16,
    support_state: CapabilitySupportState,
    constraints: BTreeMap<String, String>,
    observed_at: i64,
    confidence_percent: u8,
    source_ref: String,
}

impl CapabilityEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: CapabilityEvidenceId,
        capability_id: CapabilityId,
        subject_ref: impl Into<String>,
        source_kind: CapabilityEvidenceSourceKind,
        supported_version: u16,
        support_state: CapabilitySupportState,
        constraints: BTreeMap<String, String>,
        observed_at: i64,
        confidence_percent: u8,
        source_ref: impl Into<String>,
    ) -> Result<Self, CapabilityDomainError> {
        if supported_version == 0 {
            return Err(CapabilityDomainError::InvalidVersion);
        }
        if observed_at < 0 {
            return Err(CapabilityDomainError::InvalidTimestamp);
        }
        if confidence_percent > 100 {
            return Err(CapabilityDomainError::InvalidConfidence);
        }
        validate_metadata(&constraints)?;
        Ok(Self {
            id,
            capability_id,
            subject_ref: identifier("Capability subject reference", subject_ref)?,
            source_kind,
            supported_version,
            support_state,
            constraints,
            observed_at,
            confidence_percent,
            source_ref: identifier("Capability evidence source reference", source_ref)?,
        })
    }

    pub fn id(&self) -> &CapabilityEvidenceId {
        &self.id
    }
    pub fn capability_id(&self) -> &CapabilityId {
        &self.capability_id
    }
    pub fn subject_ref(&self) -> &str {
        &self.subject_ref
    }
    pub fn source_kind(&self) -> CapabilityEvidenceSourceKind {
        self.source_kind
    }
    pub fn supported_version(&self) -> u16 {
        self.supported_version
    }
    pub fn support_state(&self) -> CapabilitySupportState {
        self.support_state
    }
    pub fn constraints(&self) -> &BTreeMap<String, String> {
        &self.constraints
    }
    pub fn observed_at(&self) -> i64 {
        self.observed_at
    }
    pub fn confidence_percent(&self) -> u8 {
        self.confidence_percent
    }
    pub fn source_ref(&self) -> &str {
        &self.source_ref
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityResolutionStatus {
    Satisfied,
    MissingDefinition,
    MissingEvidence,
    Unsupported,
    RequiresConfiguration,
    Stale,
    InsufficientConfidence,
    ConstraintMismatch,
    OptionalFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityResolutionEntry {
    requirement: CapabilityRequirement,
    evidence_id: Option<CapabilityEvidenceId>,
    status: CapabilityResolutionStatus,
    reason: String,
}

impl CapabilityResolutionEntry {
    pub(crate) fn new(
        requirement: CapabilityRequirement,
        evidence_id: Option<CapabilityEvidenceId>,
        status: CapabilityResolutionStatus,
        reason: impl Into<String>,
    ) -> Result<Self, CapabilityDomainError> {
        if status == CapabilityResolutionStatus::Satisfied && evidence_id.is_none() {
            return Err(CapabilityDomainError::InvalidSatisfiedEntry);
        }
        Ok(Self {
            requirement,
            evidence_id,
            status,
            reason: text("Capability resolution reason", reason)?,
        })
    }

    pub fn requirement(&self) -> &CapabilityRequirement {
        &self.requirement
    }
    pub fn evidence_id(&self) -> Option<&CapabilityEvidenceId> {
        self.evidence_id.as_ref()
    }
    pub fn status(&self) -> CapabilityResolutionStatus {
        self.status
    }
    pub fn reason(&self) -> &str {
        &self.reason
    }
    pub fn is_satisfied(&self) -> bool {
        matches!(
            self.status,
            CapabilityResolutionStatus::Satisfied | CapabilityResolutionStatus::OptionalFallback
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySnapshot {
    id: CapabilitySnapshotId,
    execution_id: RuntimeExecutionId,
    subject_references: Vec<String>,
    entries: Vec<CapabilityResolutionEntry>,
    resolved_at: i64,
}

impl CapabilitySnapshot {
    pub(crate) fn new(
        id: CapabilitySnapshotId,
        execution_id: RuntimeExecutionId,
        subject_references: Vec<String>,
        entries: Vec<CapabilityResolutionEntry>,
        resolved_at: i64,
    ) -> Result<Self, CapabilityDomainError> {
        if resolved_at < 0 {
            return Err(CapabilityDomainError::InvalidTimestamp);
        }
        let subject_references = subject_references
            .into_iter()
            .map(|value| identifier("Capability subject reference", value))
            .collect::<Result<Vec<_>, _>>()?;
        let mut requirement_ids = HashSet::new();
        for entry in &entries {
            if !requirement_ids.insert(entry.requirement().capability_id().clone()) {
                return Err(CapabilityDomainError::DuplicateRequirement(
                    entry.requirement().capability_id().clone(),
                ));
            }
        }
        Ok(Self {
            id,
            execution_id,
            subject_references,
            entries,
            resolved_at,
        })
    }

    pub fn id(&self) -> &CapabilitySnapshotId {
        &self.id
    }
    pub fn execution_id(&self) -> &RuntimeExecutionId {
        &self.execution_id
    }
    pub fn subject_references(&self) -> &[String] {
        &self.subject_references
    }
    pub fn entries(&self) -> &[CapabilityResolutionEntry] {
        &self.entries
    }
    pub fn resolved_at(&self) -> i64 {
        self.resolved_at
    }
    pub fn is_satisfied(&self) -> bool {
        self.entries
            .iter()
            .all(CapabilityResolutionEntry::is_satisfied)
    }
    pub fn satisfies(&self, capability_id: &CapabilityId) -> bool {
        self.entries.iter().any(|entry| {
            entry.requirement().capability_id() == capability_id && entry.is_satisfied()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_requirement_requires_explicit_fallback() {
        assert!(matches!(
            CapabilityRequirement::new(
                CapabilityId::new("workspace.patch").unwrap(),
                1,
                CapabilityRequirementLevel::Optional,
                BTreeMap::new(),
                None,
                None,
            ),
            Err(CapabilityDomainError::MissingOptionalFallback)
        ));
    }

    #[test]
    fn capability_definition_and_evidence_contain_no_authority() {
        let definition = CapabilityDefinition::new(
            CapabilityId::new("workspace.patch").unwrap(),
            1,
            "Patch workspace",
            "Can apply a bounded patch",
            BTreeMap::from([("mode".into(), "Supported patch mode".into())]),
        )
        .unwrap();
        let evidence = CapabilityEvidence::new(
            CapabilityEvidenceId::new("evidence:one").unwrap(),
            definition.id().clone(),
            "runtime:one",
            CapabilityEvidenceSourceKind::Runtime,
            1,
            CapabilitySupportState::Supported,
            BTreeMap::from([("mode".into(), "unified".into())]),
            10,
            100,
            "probe:one",
        )
        .unwrap();

        assert_eq!(definition.id(), evidence.capability_id());
        assert_eq!(evidence.subject_ref(), "runtime:one");
        assert_eq!(evidence.support_state(), CapabilitySupportState::Supported);
    }
}
