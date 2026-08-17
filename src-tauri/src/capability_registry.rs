//! Replaceable repositories for Capability definitions, evidence, and snapshots.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::capability_domain::{
    CapabilityDefinition, CapabilityDomainError, CapabilityEvidence, CapabilityEvidenceId,
    CapabilityId, CapabilitySnapshot, CapabilitySnapshotId,
};

#[derive(Debug, Error)]
pub enum CapabilityRegistryError {
    #[error(transparent)]
    InvalidDomain(#[from] CapabilityDomainError),
    #[error("Capability definition is already registered: {id} v{version}")]
    DefinitionAlreadyRegistered { id: CapabilityId, version: u16 },
    #[error("Capability evidence is already registered: {0}")]
    EvidenceAlreadyRegistered(CapabilityEvidenceId),
    #[error("Capability snapshot is already registered: {0}")]
    SnapshotAlreadyRegistered(CapabilitySnapshotId),
    #[error("Capability registry lock failed: {0}")]
    RegistryLock(String),
}

pub trait CapabilityRegistry: Send + Sync {
    fn register_definition(
        &self,
        definition: CapabilityDefinition,
    ) -> Result<(), CapabilityRegistryError>;
    fn get_definition(
        &self,
        capability_id: &CapabilityId,
        version: u16,
    ) -> Result<Option<CapabilityDefinition>, CapabilityRegistryError>;
    fn latest_definition(
        &self,
        capability_id: &CapabilityId,
    ) -> Result<Option<CapabilityDefinition>, CapabilityRegistryError>;
    fn list_definitions(&self) -> Result<Vec<CapabilityDefinition>, CapabilityRegistryError>;
    fn register_evidence(
        &self,
        evidence: CapabilityEvidence,
    ) -> Result<(), CapabilityRegistryError>;
    fn get_evidence(
        &self,
        evidence_id: &CapabilityEvidenceId,
    ) -> Result<Option<CapabilityEvidence>, CapabilityRegistryError>;
    fn discover_evidence(
        &self,
        capability_id: &CapabilityId,
        subject_references: &[String],
    ) -> Result<Vec<CapabilityEvidence>, CapabilityRegistryError>;
}

pub trait CapabilitySnapshotRepository: Send + Sync {
    fn store_snapshot(&self, snapshot: CapabilitySnapshot) -> Result<(), CapabilityRegistryError>;
    fn get_snapshot(
        &self,
        snapshot_id: &CapabilitySnapshotId,
    ) -> Result<Option<CapabilitySnapshot>, CapabilityRegistryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryCapabilityRegistry {
    definitions: Arc<RwLock<HashMap<(CapabilityId, u16), CapabilityDefinition>>>,
    evidence: Arc<RwLock<HashMap<CapabilityEvidenceId, CapabilityEvidence>>>,
    snapshots: Arc<RwLock<HashMap<CapabilitySnapshotId, CapabilitySnapshot>>>,
}

impl CapabilityRegistry for InMemoryCapabilityRegistry {
    fn register_definition(
        &self,
        definition: CapabilityDefinition,
    ) -> Result<(), CapabilityRegistryError> {
        let key = (definition.id().clone(), definition.version());
        let mut definitions = self
            .definitions
            .write()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        if definitions.contains_key(&key) {
            return Err(CapabilityRegistryError::DefinitionAlreadyRegistered {
                id: key.0,
                version: key.1,
            });
        }
        definitions.insert(key, definition);
        Ok(())
    }

    fn get_definition(
        &self,
        capability_id: &CapabilityId,
        version: u16,
    ) -> Result<Option<CapabilityDefinition>, CapabilityRegistryError> {
        let definitions = self
            .definitions
            .read()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        Ok(definitions.get(&(capability_id.clone(), version)).cloned())
    }

    fn latest_definition(
        &self,
        capability_id: &CapabilityId,
    ) -> Result<Option<CapabilityDefinition>, CapabilityRegistryError> {
        let definitions = self
            .definitions
            .read()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        Ok(definitions
            .values()
            .filter(|definition| definition.id() == capability_id)
            .max_by_key(|definition| definition.version())
            .cloned())
    }

    fn list_definitions(&self) -> Result<Vec<CapabilityDefinition>, CapabilityRegistryError> {
        let definitions = self
            .definitions
            .read()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        let mut definitions = definitions.values().cloned().collect::<Vec<_>>();
        definitions.sort_by(|left, right| {
            left.id()
                .cmp(right.id())
                .then_with(|| left.version().cmp(&right.version()))
        });
        Ok(definitions)
    }

    fn register_evidence(
        &self,
        evidence: CapabilityEvidence,
    ) -> Result<(), CapabilityRegistryError> {
        let mut registry = self
            .evidence
            .write()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        if registry.contains_key(evidence.id()) {
            return Err(CapabilityRegistryError::EvidenceAlreadyRegistered(
                evidence.id().clone(),
            ));
        }
        registry.insert(evidence.id().clone(), evidence);
        Ok(())
    }

    fn get_evidence(
        &self,
        evidence_id: &CapabilityEvidenceId,
    ) -> Result<Option<CapabilityEvidence>, CapabilityRegistryError> {
        let evidence = self
            .evidence
            .read()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        Ok(evidence.get(evidence_id).cloned())
    }

    fn discover_evidence(
        &self,
        capability_id: &CapabilityId,
        subject_references: &[String],
    ) -> Result<Vec<CapabilityEvidence>, CapabilityRegistryError> {
        let evidence = self
            .evidence
            .read()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        let mut evidence = evidence
            .values()
            .filter(|item| {
                item.capability_id() == capability_id
                    && subject_references
                        .iter()
                        .any(|subject| subject == item.subject_ref())
            })
            .cloned()
            .collect::<Vec<_>>();
        evidence.sort_by(|left, right| {
            right
                .supported_version()
                .cmp(&left.supported_version())
                .then_with(|| right.observed_at().cmp(&left.observed_at()))
                .then_with(|| left.id().as_str().cmp(right.id().as_str()))
        });
        Ok(evidence)
    }
}

impl CapabilitySnapshotRepository for InMemoryCapabilityRegistry {
    fn store_snapshot(&self, snapshot: CapabilitySnapshot) -> Result<(), CapabilityRegistryError> {
        let mut snapshots = self
            .snapshots
            .write()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        if snapshots.contains_key(snapshot.id()) {
            return Err(CapabilityRegistryError::SnapshotAlreadyRegistered(
                snapshot.id().clone(),
            ));
        }
        snapshots.insert(snapshot.id().clone(), snapshot);
        Ok(())
    }

    fn get_snapshot(
        &self,
        snapshot_id: &CapabilitySnapshotId,
    ) -> Result<Option<CapabilitySnapshot>, CapabilityRegistryError> {
        let snapshots = self
            .snapshots
            .read()
            .map_err(|error| CapabilityRegistryError::RegistryLock(error.to_string()))?;
        Ok(snapshots.get(snapshot_id).cloned())
    }
}
