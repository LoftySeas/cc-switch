//! Capability discovery and execution-scoped validation service.

use crate::{
    capability_domain::{
        CapabilityDefinition, CapabilityEvidence, CapabilityId, CapabilityRequirement,
        CapabilityRequirementLevel, CapabilityResolutionEntry, CapabilityResolutionStatus,
        CapabilitySnapshot, CapabilitySnapshotId, CapabilitySupportState,
    },
    capability_registry::{
        CapabilityRegistry, CapabilityRegistryError, CapabilitySnapshotRepository,
    },
    runtime_domain::RuntimeExecutionId,
};

pub struct CapabilityGovernanceService<R> {
    registry: R,
}

impl<R> CapabilityGovernanceService<R>
where
    R: CapabilityRegistry + CapabilitySnapshotRepository,
{
    pub fn new(registry: R) -> Self {
        Self { registry }
    }

    pub fn register_definition(
        &self,
        definition: CapabilityDefinition,
    ) -> Result<(), CapabilityRegistryError> {
        self.registry.register_definition(definition)
    }

    pub fn record_evidence(
        &self,
        evidence: CapabilityEvidence,
    ) -> Result<(), CapabilityRegistryError> {
        self.registry.register_evidence(evidence)
    }

    pub fn discover(
        &self,
        capability_id: &CapabilityId,
        subject_references: &[String],
    ) -> Result<Vec<CapabilityEvidence>, CapabilityRegistryError> {
        self.registry
            .discover_evidence(capability_id, subject_references)
    }

    pub fn get_snapshot(
        &self,
        snapshot_id: &CapabilitySnapshotId,
    ) -> Result<Option<CapabilitySnapshot>, CapabilityRegistryError> {
        self.registry.get_snapshot(snapshot_id)
    }

    pub fn resolve(
        &self,
        execution_id: RuntimeExecutionId,
        requirements: Vec<CapabilityRequirement>,
        subject_references: Vec<String>,
        resolved_at: i64,
    ) -> Result<CapabilitySnapshot, CapabilityRegistryError> {
        let entries = requirements
            .into_iter()
            .map(|requirement| {
                self.resolve_requirement(requirement, &subject_references, resolved_at)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let snapshot = CapabilitySnapshot::new(
            CapabilitySnapshotId::new(uuid::Uuid::new_v4().to_string())?,
            execution_id,
            subject_references,
            entries,
            resolved_at,
        )?;
        self.registry.store_snapshot(snapshot.clone())?;
        Ok(snapshot)
    }

    fn resolve_requirement(
        &self,
        requirement: CapabilityRequirement,
        subject_references: &[String],
        resolved_at: i64,
    ) -> Result<CapabilityResolutionEntry, CapabilityRegistryError> {
        let definition = self
            .registry
            .latest_definition(requirement.capability_id())?;
        let Some(definition) =
            definition.filter(|definition| definition.version() >= requirement.minimum_version())
        else {
            return self.unsatisfied(
                requirement,
                None,
                CapabilityResolutionStatus::MissingDefinition,
                "No compatible Capability definition is registered",
            );
        };

        let evidence = self
            .registry
            .discover_evidence(requirement.capability_id(), subject_references)?;
        if let Some(candidate) = evidence.iter().find(|candidate| {
            candidate.support_state() == CapabilitySupportState::Supported
                && candidate.supported_version() >= requirement.minimum_version()
                && candidate.supported_version() <= definition.version()
                && candidate.confidence_percent() > 0
                && is_fresh(candidate, &requirement, resolved_at)
                && constraints_satisfy(candidate, &requirement)
        }) {
            return Ok(CapabilityResolutionEntry::new(
                requirement,
                Some(candidate.id().clone()),
                CapabilityResolutionStatus::Satisfied,
                "Compatible, fresh Capability evidence satisfies the requirement",
            )?);
        }

        let (evidence_id, status, reason) =
            classify_unsatisfied(&requirement, &evidence, resolved_at);
        self.unsatisfied(requirement, evidence_id, status, reason)
    }

    fn unsatisfied(
        &self,
        requirement: CapabilityRequirement,
        evidence_id: Option<crate::capability_domain::CapabilityEvidenceId>,
        status: CapabilityResolutionStatus,
        reason: &str,
    ) -> Result<CapabilityResolutionEntry, CapabilityRegistryError> {
        if requirement.level() == CapabilityRequirementLevel::Optional {
            let fallback = requirement
                .fallback_ref()
                .expect("optional requirements are validated with a fallback")
                .to_string();
            return Ok(CapabilityResolutionEntry::new(
                requirement,
                evidence_id,
                CapabilityResolutionStatus::OptionalFallback,
                format!("{reason}; use declared fallback {fallback}"),
            )?);
        }
        Ok(CapabilityResolutionEntry::new(
            requirement,
            evidence_id,
            status,
            reason,
        )?)
    }
}

fn is_fresh(
    evidence: &CapabilityEvidence,
    requirement: &CapabilityRequirement,
    resolved_at: i64,
) -> bool {
    requirement
        .max_evidence_age_ms()
        .is_none_or(|max_age| resolved_at.saturating_sub(evidence.observed_at()) <= max_age)
}

fn constraints_satisfy(evidence: &CapabilityEvidence, requirement: &CapabilityRequirement) -> bool {
    requirement
        .required_constraints()
        .iter()
        .all(|(key, expected)| evidence.constraints().get(key) == Some(expected))
}

fn classify_unsatisfied(
    requirement: &CapabilityRequirement,
    evidence: &[CapabilityEvidence],
    resolved_at: i64,
) -> (
    Option<crate::capability_domain::CapabilityEvidenceId>,
    CapabilityResolutionStatus,
    &'static str,
) {
    if evidence.is_empty() {
        return (
            None,
            CapabilityResolutionStatus::MissingEvidence,
            "No evidence exists for an eligible subject",
        );
    }
    let candidate = &evidence[0];
    if evidence
        .iter()
        .any(|item| item.support_state() == CapabilitySupportState::RequiresConfiguration)
    {
        return (
            Some(candidate.id().clone()),
            CapabilityResolutionStatus::RequiresConfiguration,
            "Capability requires configuration",
        );
    }
    if evidence
        .iter()
        .all(|item| item.support_state() != CapabilitySupportState::Supported)
    {
        return (
            Some(candidate.id().clone()),
            CapabilityResolutionStatus::Unsupported,
            "Capability is unsupported or unknown",
        );
    }
    if evidence
        .iter()
        .filter(|item| item.support_state() == CapabilitySupportState::Supported)
        .all(|item| !is_fresh(item, requirement, resolved_at))
    {
        return (
            Some(candidate.id().clone()),
            CapabilityResolutionStatus::Stale,
            "Capability evidence is stale",
        );
    }
    if evidence.iter().any(|item| {
        item.support_state() == CapabilitySupportState::Supported
            && is_fresh(item, requirement, resolved_at)
            && constraints_satisfy(item, requirement)
            && item.confidence_percent() == 0
    }) {
        return (
            Some(candidate.id().clone()),
            CapabilityResolutionStatus::InsufficientConfidence,
            "Capability evidence has no usable confidence",
        );
    }
    (
        Some(candidate.id().clone()),
        CapabilityResolutionStatus::ConstraintMismatch,
        "Capability evidence does not meet version or constraint requirements",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        capability_domain::{
            CapabilityDefinition, CapabilityEvidenceId, CapabilityEvidenceSourceKind, CapabilityId,
        },
        capability_registry::{CapabilityRegistry, InMemoryCapabilityRegistry},
    };

    fn registry_with_patch_evidence(
        observed_at: i64,
        confidence_percent: u8,
    ) -> InMemoryCapabilityRegistry {
        let registry = InMemoryCapabilityRegistry::default();
        let id = CapabilityId::new("workspace.patch").unwrap();
        registry
            .register_definition(
                CapabilityDefinition::new(
                    id.clone(),
                    2,
                    "Patch workspace",
                    "Apply bounded patches",
                    BTreeMap::new(),
                )
                .unwrap(),
            )
            .unwrap();
        registry
            .register_evidence(
                CapabilityEvidence::new(
                    CapabilityEvidenceId::new("evidence:patch").unwrap(),
                    id,
                    "runtime:one",
                    CapabilityEvidenceSourceKind::Runtime,
                    2,
                    CapabilitySupportState::Supported,
                    BTreeMap::from([("mode".into(), "unified".into())]),
                    observed_at,
                    confidence_percent,
                    "probe:runtime-one",
                )
                .unwrap(),
            )
            .unwrap();
        registry
    }

    #[test]
    fn resolution_discovers_compatible_evidence_and_stores_snapshot() {
        let registry = registry_with_patch_evidence(90, 100);
        let service = CapabilityGovernanceService::new(registry.clone());
        let snapshot = service
            .resolve(
                RuntimeExecutionId::new("execution:one").unwrap(),
                vec![CapabilityRequirement::new(
                    CapabilityId::new("workspace.patch").unwrap(),
                    2,
                    CapabilityRequirementLevel::Required,
                    BTreeMap::from([("mode".into(), "unified".into())]),
                    Some(20),
                    None,
                )
                .unwrap()],
                vec!["runtime:one".into()],
                100,
            )
            .unwrap();

        assert!(snapshot.is_satisfied());
        assert!(registry.get_snapshot(snapshot.id()).unwrap().is_some());
    }

    #[test]
    fn required_capability_fails_closed_for_stale_evidence() {
        let service = CapabilityGovernanceService::new(registry_with_patch_evidence(10, 100));
        let snapshot = service
            .resolve(
                RuntimeExecutionId::new("execution:one").unwrap(),
                vec![CapabilityRequirement::new(
                    CapabilityId::new("workspace.patch").unwrap(),
                    1,
                    CapabilityRequirementLevel::Required,
                    BTreeMap::new(),
                    Some(20),
                    None,
                )
                .unwrap()],
                vec!["runtime:one".into()],
                100,
            )
            .unwrap();

        assert!(!snapshot.is_satisfied());
        assert_eq!(
            snapshot.entries()[0].status(),
            CapabilityResolutionStatus::Stale
        );
    }

    #[test]
    fn optional_capability_uses_only_an_explicit_fallback() {
        let service = CapabilityGovernanceService::new(InMemoryCapabilityRegistry::default());
        let snapshot = service
            .resolve(
                RuntimeExecutionId::new("execution:one").unwrap(),
                vec![CapabilityRequirement::new(
                    CapabilityId::new("output.streaming").unwrap(),
                    1,
                    CapabilityRequirementLevel::Optional,
                    BTreeMap::new(),
                    None,
                    Some("fallback:buffered-output".into()),
                )
                .unwrap()],
                vec!["runtime:one".into()],
                100,
            )
            .unwrap();

        assert!(snapshot.is_satisfied());
        assert_eq!(
            snapshot.entries()[0].status(),
            CapabilityResolutionStatus::OptionalFallback
        );
    }

    #[test]
    fn zero_confidence_evidence_cannot_satisfy_required_capability() {
        let service = CapabilityGovernanceService::new(registry_with_patch_evidence(90, 0));
        let snapshot = service
            .resolve(
                RuntimeExecutionId::new("execution:one").unwrap(),
                vec![CapabilityRequirement::new(
                    CapabilityId::new("workspace.patch").unwrap(),
                    1,
                    CapabilityRequirementLevel::Required,
                    BTreeMap::new(),
                    Some(20),
                    None,
                )
                .unwrap()],
                vec!["runtime:one".into()],
                100,
            )
            .unwrap();

        assert!(!snapshot.is_satisfied());
        assert_eq!(
            snapshot.entries()[0].status(),
            CapabilityResolutionStatus::InsufficientConfidence
        );
    }
}
