//! Append-only, ordered, tamper-evident enterprise governance audit evidence.
//!
//! Audit evidence is deliberately separate from mutable operational state,
//! Execution History, and Memory. Metadata is bounded and rejects secret-like
//! fields before an event can be constructed or persisted.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    database::{lock_conn, Database},
    error::AppError,
    governance_time::{TrustedClock, TrustedClockError},
};

const MAX_ID_LENGTH: usize = 192;
const MAX_REFERENCE_LENGTH: usize = 256;
const MAX_METADATA_ENTRIES: usize = 32;
const MAX_METADATA_KEY_LENGTH: usize = 64;
const MAX_METADATA_VALUE_LENGTH: usize = 256;
const MAX_METADATA_BYTES: usize = 4_096;
const MAX_QUERY_LIMIT: usize = 1_000;
const MAX_APPEND_ATTEMPTS: usize = 64;
const ALLOWED_METADATA_KEYS: &[&str] = &[
    "actual_revision",
    "adapter_id",
    "binding_revision",
    "expected_revision",
    "lifecycle",
    "new_lifecycle",
    "organization_revision",
    "policy_binding_revision",
    "policy_layer",
    "policy_revision",
    "policy_version",
    "previous_lifecycle",
    "reason_code",
    "reason_count",
    "record_count",
    "revision",
    "scope_kind",
    "selection_count",
    "team_binding_revision",
];
const ALLOWED_REFERENCE_PREFIXES: &[&str] = &[
    "actor",
    "agent",
    "audit",
    "audit-stream",
    "authorization",
    "capability-snapshot",
    "environment",
    "execution",
    "isolation",
    "organization",
    "organization-boundary",
    "organization-policy-binding",
    "organization-team-binding",
    "permission-grant",
    "permission-policy",
    "policy-binding",
    "policy-record",
    "policy-selection",
    "provider",
    "provider-adapter",
    "provider-instance",
    "resolution",
    "role-assignment",
    "runtime",
    "runtime-adapter",
    "runtime-instance",
    "team",
    "workflow",
];
const ALLOWED_SUBJECT_TYPES: &[&str] = &[
    "controlled_environment",
    "cross_organization_request",
    "organization",
    "organization_boundary",
    "organization_policy_binding",
    "organization_team_binding",
    "permission_policy_record",
    "permission_policy_scope_binding",
    "permission_policy_selection",
    "provider_instance",
    "runtime_instance",
];
const ALLOWED_REASON_CODES: &[&str] = &[
    "activation_evidence",
    "active_binding_exists",
    "activated_boundary_mismatch",
    "ambiguous_policy_selection",
    "archived_read_only",
    "audit_failure",
    "audit_validation",
    "boundary_repository",
    "cross_organization_reference",
    "domain_validation",
    "environment_repository",
    "inactive_organization",
    "invalid_lifecycle",
    "isolation_rejected",
    "no_policy",
    "organization_not_found",
    "out_of_scope",
    "policy_not_found",
    "policy_not_published",
    "policy_binding_inactive",
    "policy_binding_not_found",
    "provider_adapter_mismatch",
    "provider_instance_missing",
    "provider_unavailable",
    "resolution_missing",
    "resolution_repository",
    "retired_policy",
    "runtime_adapter_mismatch",
    "runtime_instance_missing",
    "runtime_unavailable",
    "scope_mismatch",
    "stale_revision",
    "team_binding_inactive",
    "team_binding_not_found",
    "team_not_found",
    "team_owned_by_another_organization",
    "membership_not_effective",
    "query_scope_mismatch",
    "trusted_clock",
];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GovernanceAuditDomainError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains whitespace or control characters")]
    InvalidIdentifier { field: &'static str },
    #[error("Audit sequence must be positive")]
    InvalidSequence,
    #[error("Audit timestamp must not be negative")]
    InvalidTimestamp,
    #[error("Audit metadata exceeds {0} entries")]
    TooManyMetadataEntries(usize),
    #[error("Audit metadata contains a forbidden secret or payload field: {0}")]
    ForbiddenMetadata(String),
    #[error("{field} resembles forbidden secret or payload content")]
    ForbiddenReference { field: &'static str },
    #[error("Audit digest chain shape is invalid")]
    InvalidChain,
    #[error("Audit event digest does not match its canonical evidence")]
    DigestMismatch,
}

macro_rules! typed_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, GovernanceAuditDomainError> {
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

typed_id!(GovernanceAuditEventId, "Governance audit event ID");
typed_id!(GovernanceAuditStreamId, "Governance audit stream ID");

fn identifier(field: &'static str, value: String) -> Result<String, GovernanceAuditDomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(GovernanceAuditDomainError::Empty { field });
    }
    if value.chars().count() > MAX_ID_LENGTH {
        return Err(GovernanceAuditDomainError::TooLong {
            field,
            max: MAX_ID_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(GovernanceAuditDomainError::InvalidIdentifier { field });
    }
    Ok(value.to_string())
}

fn bounded_reference(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, GovernanceAuditDomainError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(GovernanceAuditDomainError::Empty { field });
    }
    if value.chars().count() > MAX_REFERENCE_LENGTH {
        return Err(GovernanceAuditDomainError::TooLong {
            field,
            max: MAX_REFERENCE_LENGTH,
        });
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err(GovernanceAuditDomainError::InvalidIdentifier { field });
    }
    let Some((prefix, suffix)) = value.split_once(':') else {
        return Err(GovernanceAuditDomainError::InvalidIdentifier { field });
    };
    if !ALLOWED_REFERENCE_PREFIXES.contains(&prefix)
        || suffix.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':' | '.')
        })
        || contains_forbidden_payload_hint(value)
    {
        return Err(GovernanceAuditDomainError::ForbiddenReference { field });
    }
    Ok(value.to_string())
}

fn validated_actor_reference(
    value: impl Into<String>,
) -> Result<String, GovernanceAuditDomainError> {
    let value = bounded_reference("Audit actor reference", value)?;
    if !value.starts_with("actor:") {
        return Err(GovernanceAuditDomainError::ForbiddenReference {
            field: "Audit actor reference",
        });
    }
    Ok(value)
}

fn validated_subject_type(value: impl Into<String>) -> Result<String, GovernanceAuditDomainError> {
    let value = identifier("Audit subject type", value.into())?;
    if !ALLOWED_SUBJECT_TYPES.contains(&value.as_str()) {
        return Err(GovernanceAuditDomainError::ForbiddenReference {
            field: "Audit subject type",
        });
    }
    Ok(value)
}

fn contains_forbidden_payload_hint(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    [
        "apikey",
        "api_key",
        "bearer",
        "credential",
        "filecontent",
        "memorycontent",
        "modeloutput",
        "password",
        "promptcontent",
        "refreshtoken",
        "secret",
        "sk-",
    ]
    .iter()
    .any(|forbidden| normalized.contains(forbidden))
}

fn validate_metadata_value(key: &str, value: &str) -> Result<(), GovernanceAuditDomainError> {
    let invalid = || GovernanceAuditDomainError::ForbiddenMetadata(key.to_string());
    match key {
        "actual_revision"
        | "binding_revision"
        | "expected_revision"
        | "organization_revision"
        | "policy_binding_revision"
        | "policy_revision"
        | "policy_version"
        | "revision"
        | "team_binding_revision" => {
            if value.parse::<u64>().ok().is_none_or(|value| value == 0) {
                return Err(invalid());
            }
        }
        "reason_count" | "record_count" | "selection_count" => {
            if value.parse::<u64>().is_err() {
                return Err(invalid());
            }
        }
        "adapter_id" => {
            let reference = bounded_reference("Audit adapter reference", value.to_string())?;
            if !reference.starts_with("runtime-adapter:")
                && !reference.starts_with("provider-adapter:")
            {
                return Err(invalid());
            }
        }
        "reason_code" => {
            if !ALLOWED_REASON_CODES.contains(&value) {
                return Err(invalid());
            }
        }
        "lifecycle" | "new_lifecycle" | "previous_lifecycle" => {
            if ![
                "active",
                "activating",
                "archived",
                "degraded",
                "draft",
                "ended",
                "failed",
                "published",
                "ready",
                "registered",
                "retired",
                "stopped",
                "stopping",
                "suspended",
            ]
            .contains(&value)
            {
                return Err(invalid());
            }
        }
        "policy_layer" => {
            if ![
                "environment",
                "human_owner",
                "repository",
                "role_assignment",
                "team",
                "workflow",
                "workspace",
            ]
            .contains(&value)
            {
                return Err(invalid());
            }
        }
        "scope_kind" => {
            if ![
                "agent",
                "environment",
                "organization",
                "repository",
                "team",
                "workflow",
                "workspace",
            ]
            .contains(&value)
            {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceAuditEventKind {
    ControlledEnvironmentPreparationRequested,
    RuntimeSnapshotCaptured,
    ProviderSnapshotCaptured,
    ControlledEnvironmentPreparationAccepted,
    ControlledEnvironmentPreparationRejected,
    ControlledEnvironmentRevalidationAccepted,
    ControlledEnvironmentRevalidationRejected,
    PermissionPolicyDraftCreated,
    PermissionPolicyScopeBindingCreated,
    PermissionPolicyPublished,
    PermissionPolicyScopeBindingActivated,
    PermissionPolicyVersionReplaced,
    PermissionPolicyScopeBindingEnded,
    PermissionPolicyRetired,
    PermissionPolicySelectionAccepted,
    PermissionPolicySelectionRejected,
    PermissionPolicyOperationRejected,
    OrganizationCreated,
    OrganizationLifecycleChanged,
    OrganizationLifecycleChangeRejected,
    OrganizationTeamBindingCreated,
    OrganizationTeamBindingActivated,
    OrganizationTeamBindingEnded,
    OrganizationTeamBindingRejected,
    OrganizationPolicyBindingCreated,
    OrganizationPolicyBindingActivated,
    OrganizationPolicyBindingEnded,
    OrganizationPolicyBindingRejected,
    OrganizationBoundaryResolutionAccepted,
    OrganizationBoundaryResolutionRejected,
    CrossOrganizationAccessDenied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceAuditOutcome {
    Accepted,
    Rejected,
    Created,
    Updated,
    Stale,
    Denied,
    NoPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "AuditCorrelationReferencesDto")]
pub struct AuditCorrelationReferences {
    execution_id: Option<String>,
    environment_id: Option<String>,
    model_resolution_id: Option<String>,
    authorization_decision_id: Option<String>,
    permission_grant_id: Option<String>,
    policy_record_id: Option<String>,
    organization_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditCorrelationReferencesDto {
    execution_id: Option<String>,
    environment_id: Option<String>,
    model_resolution_id: Option<String>,
    authorization_decision_id: Option<String>,
    permission_grant_id: Option<String>,
    policy_record_id: Option<String>,
    organization_id: Option<String>,
}

impl TryFrom<AuditCorrelationReferencesDto> for AuditCorrelationReferences {
    type Error = GovernanceAuditDomainError;

    fn try_from(value: AuditCorrelationReferencesDto) -> Result<Self, Self::Error> {
        Self::new(
            value.execution_id,
            value.environment_id,
            value.model_resolution_id,
            value.authorization_decision_id,
            value.permission_grant_id,
            value.policy_record_id,
            value.organization_id,
        )
    }
}

impl AuditCorrelationReferences {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: Option<String>,
        environment_id: Option<String>,
        model_resolution_id: Option<String>,
        authorization_decision_id: Option<String>,
        permission_grant_id: Option<String>,
        policy_record_id: Option<String>,
        organization_id: Option<String>,
    ) -> Result<Self, GovernanceAuditDomainError> {
        let references = Self {
            execution_id,
            environment_id,
            model_resolution_id,
            authorization_decision_id,
            permission_grant_id,
            policy_record_id,
            organization_id,
        };
        references.validate()?;
        Ok(references)
    }

    pub fn for_environment(
        execution_id: impl Into<String>,
        environment_id: impl Into<String>,
        model_resolution_id: impl Into<String>,
    ) -> Result<Self, GovernanceAuditDomainError> {
        Self::new(
            Some(execution_id.into()),
            Some(environment_id.into()),
            Some(model_resolution_id.into()),
            None,
            None,
            None,
            None,
        )
    }

    pub fn execution_id(&self) -> Option<&str> {
        self.execution_id.as_deref()
    }

    pub fn environment_id(&self) -> Option<&str> {
        self.environment_id.as_deref()
    }

    pub fn model_resolution_id(&self) -> Option<&str> {
        self.model_resolution_id.as_deref()
    }

    pub fn authorization_decision_id(&self) -> Option<&str> {
        self.authorization_decision_id.as_deref()
    }

    pub fn permission_grant_id(&self) -> Option<&str> {
        self.permission_grant_id.as_deref()
    }

    pub fn policy_record_id(&self) -> Option<&str> {
        self.policy_record_id.as_deref()
    }

    pub fn organization_id(&self) -> Option<&str> {
        self.organization_id.as_deref()
    }

    pub fn validate(&self) -> Result<(), GovernanceAuditDomainError> {
        for (field, value) in [
            ("Execution ID", self.execution_id.as_ref()),
            ("Environment ID", self.environment_id.as_ref()),
            ("Model resolution ID", self.model_resolution_id.as_ref()),
            (
                "Authorization decision ID",
                self.authorization_decision_id.as_ref(),
            ),
            ("Permission grant ID", self.permission_grant_id.as_ref()),
            ("Policy record ID", self.policy_record_id.as_ref()),
            ("Organization ID", self.organization_id.as_ref()),
        ] {
            if let Some(value) = value {
                bounded_reference(field, value.clone())?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(transparent)]
pub struct SanitizedAuditMetadata(BTreeMap<String, String>);

impl<'de> Deserialize<'de> for SanitizedAuditMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let values = BTreeMap::<String, String>::deserialize(deserializer)?;
        Self::new(values).map_err(serde::de::Error::custom)
    }
}

impl SanitizedAuditMetadata {
    pub fn new(values: BTreeMap<String, String>) -> Result<Self, GovernanceAuditDomainError> {
        let metadata = Self(values);
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn empty() -> Self {
        Self(BTreeMap::new())
    }

    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    pub fn validate(&self) -> Result<(), GovernanceAuditDomainError> {
        if self.0.len() > MAX_METADATA_ENTRIES {
            return Err(GovernanceAuditDomainError::TooManyMetadataEntries(
                MAX_METADATA_ENTRIES,
            ));
        }
        if self
            .0
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>()
            > MAX_METADATA_BYTES
        {
            return Err(GovernanceAuditDomainError::TooLong {
                field: "Audit metadata",
                max: MAX_METADATA_BYTES,
            });
        }
        for (key, value) in &self.0 {
            let normalized_key = key
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>();
            if [
                "apikey",
                "accesstoken",
                "refreshtoken",
                "bearertoken",
                "credential",
                "secret",
                "password",
                "authorizationheader",
                "providersettings",
                "environmentvariable",
                "memorycontent",
                "fullprompt",
                "promptcontent",
                "modeloutput",
                "filecontent",
            ]
            .iter()
            .any(|forbidden| normalized_key.contains(forbidden))
            {
                return Err(GovernanceAuditDomainError::ForbiddenMetadata(key.clone()));
            }
            if key.trim().is_empty()
                || key.chars().count() > MAX_METADATA_KEY_LENGTH
                || key.chars().any(char::is_whitespace)
                || key.chars().any(char::is_control)
            {
                return Err(GovernanceAuditDomainError::InvalidIdentifier {
                    field: "Audit metadata key",
                });
            }
            if !ALLOWED_METADATA_KEYS.contains(&key.as_str()) {
                return Err(GovernanceAuditDomainError::ForbiddenMetadata(key.clone()));
            }
            if value.trim().is_empty()
                || value.chars().count() > MAX_METADATA_VALUE_LENGTH
                || !value.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':' | '.')
                })
            {
                return Err(GovernanceAuditDomainError::InvalidIdentifier {
                    field: "Audit metadata value",
                });
            }
            if contains_forbidden_payload_hint(value) {
                return Err(GovernanceAuditDomainError::ForbiddenMetadata(key.clone()));
            }
            validate_metadata_value(key, value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "GovernanceAuditEventDto")]
pub struct GovernanceAuditEvent {
    event_id: GovernanceAuditEventId,
    stream_id: GovernanceAuditStreamId,
    sequence: u64,
    kind: GovernanceAuditEventKind,
    outcome: GovernanceAuditOutcome,
    actor_reference: String,
    subject_type: String,
    subject_reference: String,
    correlations: AuditCorrelationReferences,
    occurred_at: i64,
    previous_digest: Option<String>,
    digest: String,
    metadata: SanitizedAuditMetadata,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GovernanceAuditEventDto {
    event_id: GovernanceAuditEventId,
    stream_id: GovernanceAuditStreamId,
    sequence: u64,
    kind: GovernanceAuditEventKind,
    outcome: GovernanceAuditOutcome,
    actor_reference: String,
    subject_type: String,
    subject_reference: String,
    correlations: AuditCorrelationReferences,
    occurred_at: i64,
    previous_digest: Option<String>,
    digest: String,
    metadata: SanitizedAuditMetadata,
}

impl TryFrom<GovernanceAuditEventDto> for GovernanceAuditEvent {
    type Error = GovernanceAuditDomainError;

    fn try_from(value: GovernanceAuditEventDto) -> Result<Self, Self::Error> {
        let event = Self {
            event_id: value.event_id,
            stream_id: value.stream_id,
            sequence: value.sequence,
            kind: value.kind,
            outcome: value.outcome,
            actor_reference: value.actor_reference,
            subject_type: value.subject_type,
            subject_reference: value.subject_reference,
            correlations: value.correlations,
            occurred_at: value.occurred_at,
            previous_digest: value.previous_digest,
            digest: value.digest,
            metadata: value.metadata,
        };
        event.validate()?;
        Ok(event)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditDigestPayload<'a> {
    event_id: &'a GovernanceAuditEventId,
    stream_id: &'a GovernanceAuditStreamId,
    sequence: u64,
    kind: GovernanceAuditEventKind,
    outcome: GovernanceAuditOutcome,
    actor_reference: &'a str,
    subject_type: &'a str,
    subject_reference: &'a str,
    correlations: &'a AuditCorrelationReferences,
    occurred_at: i64,
    previous_digest: &'a Option<String>,
    metadata: &'a SanitizedAuditMetadata,
}

impl GovernanceAuditEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_id: GovernanceAuditEventId,
        stream_id: GovernanceAuditStreamId,
        sequence: u64,
        kind: GovernanceAuditEventKind,
        outcome: GovernanceAuditOutcome,
        actor_reference: impl Into<String>,
        subject_type: impl Into<String>,
        subject_reference: impl Into<String>,
        correlations: AuditCorrelationReferences,
        occurred_at: i64,
        previous_digest: Option<String>,
        metadata: SanitizedAuditMetadata,
    ) -> Result<Self, GovernanceAuditDomainError> {
        let mut event = Self {
            event_id,
            stream_id,
            sequence,
            kind,
            outcome,
            actor_reference: validated_actor_reference(actor_reference)?,
            subject_type: validated_subject_type(subject_type)?,
            subject_reference: bounded_reference("Audit subject reference", subject_reference)?,
            correlations,
            occurred_at,
            previous_digest,
            digest: String::new(),
            metadata,
        };
        event.digest = event.canonical_digest()?;
        event.validate()?;
        Ok(event)
    }

    pub fn event_id(&self) -> &GovernanceAuditEventId {
        &self.event_id
    }
    pub fn stream_id(&self) -> &GovernanceAuditStreamId {
        &self.stream_id
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn kind(&self) -> GovernanceAuditEventKind {
        self.kind
    }
    pub fn outcome(&self) -> GovernanceAuditOutcome {
        self.outcome
    }
    pub fn actor_reference(&self) -> &str {
        &self.actor_reference
    }
    pub fn subject_type(&self) -> &str {
        &self.subject_type
    }
    pub fn subject_reference(&self) -> &str {
        &self.subject_reference
    }
    pub fn correlations(&self) -> &AuditCorrelationReferences {
        &self.correlations
    }
    pub fn occurred_at(&self) -> i64 {
        self.occurred_at
    }
    pub fn previous_digest(&self) -> Option<&str> {
        self.previous_digest.as_deref()
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn metadata(&self) -> &SanitizedAuditMetadata {
        &self.metadata
    }

    pub fn validate(&self) -> Result<(), GovernanceAuditDomainError> {
        GovernanceAuditEventId::new(self.event_id.as_str())?;
        GovernanceAuditStreamId::new(self.stream_id.as_str())?;
        if self.sequence == 0 {
            return Err(GovernanceAuditDomainError::InvalidSequence);
        }
        if self.occurred_at < 0 {
            return Err(GovernanceAuditDomainError::InvalidTimestamp);
        }
        validated_actor_reference(self.actor_reference.clone())?;
        validated_subject_type(self.subject_type.clone())?;
        bounded_reference("Audit subject reference", self.subject_reference.clone())?;
        self.correlations.validate()?;
        self.metadata.validate()?;
        if (self.sequence == 1) != self.previous_digest.is_none() {
            return Err(GovernanceAuditDomainError::InvalidChain);
        }
        if let Some(previous_digest) = &self.previous_digest {
            validate_digest(previous_digest)?;
        }
        validate_digest(&self.digest)?;
        if self.canonical_digest()? != self.digest {
            return Err(GovernanceAuditDomainError::DigestMismatch);
        }
        Ok(())
    }

    fn canonical_digest(&self) -> Result<String, GovernanceAuditDomainError> {
        let payload = AuditDigestPayload {
            event_id: &self.event_id,
            stream_id: &self.stream_id,
            sequence: self.sequence,
            kind: self.kind,
            outcome: self.outcome,
            actor_reference: &self.actor_reference,
            subject_type: &self.subject_type,
            subject_reference: &self.subject_reference,
            correlations: &self.correlations,
            occurred_at: self.occurred_at,
            previous_digest: &self.previous_digest,
            metadata: &self.metadata,
        };
        let bytes = serde_json::to_vec(&payload).map_err(|_| {
            GovernanceAuditDomainError::InvalidIdentifier {
                field: "Audit canonical evidence",
            }
        })?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

fn validate_digest(value: &str) -> Result<(), GovernanceAuditDomainError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GovernanceAuditDomainError::InvalidChain);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum GovernanceAuditRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] GovernanceAuditDomainError),
    #[error("Governance audit event already exists: {0}")]
    DuplicateEvent(GovernanceAuditEventId),
    #[error("Governance audit stream sequence already exists: {stream_id}#{sequence}")]
    DuplicateSequence {
        stream_id: GovernanceAuditStreamId,
        sequence: u64,
    },
    #[error("Governance audit digest chain is broken for stream: {0}")]
    BrokenChain(GovernanceAuditStreamId),
    #[error("Governance audit query limit must be between 1 and {MAX_QUERY_LIMIT}")]
    InvalidQueryLimit,
    #[error("Governance audit repository lock failed: {0}")]
    RegistryLock(String),
    #[error("Governance audit persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for GovernanceAuditRepositoryError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait GovernanceAuditRepository: Send + Sync {
    fn append(&self, event: GovernanceAuditEvent) -> Result<(), GovernanceAuditRepositoryError>;
    fn get(
        &self,
        event_id: &GovernanceAuditEventId,
    ) -> Result<Option<GovernanceAuditEvent>, GovernanceAuditRepositoryError>;
    fn list_stream(
        &self,
        stream_id: &GovernanceAuditStreamId,
        limit: usize,
    ) -> Result<Vec<GovernanceAuditEvent>, GovernanceAuditRepositoryError>;
    fn last_stream_event(
        &self,
        stream_id: &GovernanceAuditStreamId,
    ) -> Result<Option<GovernanceAuditEvent>, GovernanceAuditRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryGovernanceAuditRepository {
    events: Arc<RwLock<HashMap<GovernanceAuditEventId, GovernanceAuditEvent>>>,
}

fn validate_append(
    events: impl Iterator<Item = GovernanceAuditEvent>,
    candidate: &GovernanceAuditEvent,
) -> Result<(), GovernanceAuditRepositoryError> {
    candidate.validate()?;
    let mut stream = events
        .filter(|event| event.stream_id() == candidate.stream_id())
        .collect::<Vec<_>>();
    stream.sort_by_key(GovernanceAuditEvent::sequence);
    if stream
        .iter()
        .any(|event| event.sequence() == candidate.sequence())
    {
        return Err(GovernanceAuditRepositoryError::DuplicateSequence {
            stream_id: candidate.stream_id().clone(),
            sequence: candidate.sequence(),
        });
    }
    let expected_sequence = stream.last().map_or(1, |event| event.sequence() + 1);
    let expected_digest = stream.last().map(|event| event.digest());
    let time_regressed = stream
        .last()
        .is_some_and(|event| candidate.occurred_at() < event.occurred_at());
    if candidate.sequence() != expected_sequence
        || candidate.previous_digest() != expected_digest
        || time_regressed
    {
        return Err(GovernanceAuditRepositoryError::BrokenChain(
            candidate.stream_id().clone(),
        ));
    }
    Ok(())
}

impl GovernanceAuditRepository for InMemoryGovernanceAuditRepository {
    fn append(&self, event: GovernanceAuditEvent) -> Result<(), GovernanceAuditRepositoryError> {
        let mut events = self
            .events
            .write()
            .map_err(|error| GovernanceAuditRepositoryError::RegistryLock(error.to_string()))?;
        if events.contains_key(event.event_id()) {
            return Err(GovernanceAuditRepositoryError::DuplicateEvent(
                event.event_id().clone(),
            ));
        }
        validate_append(events.values().cloned(), &event)?;
        events.insert(event.event_id().clone(), event);
        Ok(())
    }

    fn get(
        &self,
        event_id: &GovernanceAuditEventId,
    ) -> Result<Option<GovernanceAuditEvent>, GovernanceAuditRepositoryError> {
        let events = self
            .events
            .read()
            .map_err(|error| GovernanceAuditRepositoryError::RegistryLock(error.to_string()))?;
        Ok(events.get(event_id).cloned())
    }

    fn list_stream(
        &self,
        stream_id: &GovernanceAuditStreamId,
        limit: usize,
    ) -> Result<Vec<GovernanceAuditEvent>, GovernanceAuditRepositoryError> {
        validate_limit(limit)?;
        let events = self
            .events
            .read()
            .map_err(|error| GovernanceAuditRepositoryError::RegistryLock(error.to_string()))?;
        let mut stream = events
            .values()
            .filter(|event| event.stream_id() == stream_id)
            .cloned()
            .collect::<Vec<_>>();
        stream.sort_by_key(GovernanceAuditEvent::sequence);
        stream.truncate(limit);
        validate_loaded_stream(&stream)?;
        Ok(stream)
    }

    fn last_stream_event(
        &self,
        stream_id: &GovernanceAuditStreamId,
    ) -> Result<Option<GovernanceAuditEvent>, GovernanceAuditRepositoryError> {
        let events = self
            .events
            .read()
            .map_err(|error| GovernanceAuditRepositoryError::RegistryLock(error.to_string()))?;
        let event = events
            .values()
            .filter(|event| event.stream_id() == stream_id)
            .max_by_key(|event| event.sequence())
            .cloned();
        if let Some(event) = &event {
            event.validate()?;
        }
        Ok(event)
    }
}

#[derive(Clone)]
pub struct SqliteGovernanceAuditRepository {
    database: Arc<Database>,
}

struct StoredGovernanceAuditEvent {
    event_id: String,
    stream_id: String,
    sequence: i64,
    event_json: String,
    occurred_at: i64,
    previous_digest: Option<String>,
    digest: String,
}

impl StoredGovernanceAuditEvent {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            event_id: row.get(0)?,
            stream_id: row.get(1)?,
            sequence: row.get(2)?,
            event_json: row.get(3)?,
            occurred_at: row.get(4)?,
            previous_digest: row.get(5)?,
            digest: row.get(6)?,
        })
    }
}

impl SqliteGovernanceAuditRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    fn decode(
        stored: StoredGovernanceAuditEvent,
    ) -> Result<GovernanceAuditEvent, GovernanceAuditRepositoryError> {
        let event = serde_json::from_str::<GovernanceAuditEvent>(&stored.event_json)
            .map_err(|error| GovernanceAuditRepositoryError::Persistence(error.to_string()))?;
        let stored_sequence = u64::try_from(stored.sequence).map_err(|_| {
            GovernanceAuditRepositoryError::Persistence(
                "Governance audit row sequence is invalid".into(),
            )
        })?;
        if event.event_id().as_str() != stored.event_id
            || event.stream_id().as_str() != stored.stream_id
            || event.sequence() != stored_sequence
            || event.occurred_at() != stored.occurred_at
            || event.previous_digest() != stored.previous_digest.as_deref()
            || event.digest() != stored.digest
        {
            return Err(GovernanceAuditRepositoryError::Persistence(
                "Governance audit indexed columns do not match immutable event evidence".into(),
            ));
        }
        event.validate()?;
        Ok(event)
    }
}

impl GovernanceAuditRepository for SqliteGovernanceAuditRepository {
    fn append(&self, event: GovernanceAuditEvent) -> Result<(), GovernanceAuditRepositoryError> {
        event.validate()?;
        let encoded = serde_json::to_string(&event)
            .map_err(|error| GovernanceAuditRepositoryError::Persistence(error.to_string()))?;
        let conn = lock_conn!(self.database.conn);
        let tail = conn
            .query_row(
                "SELECT audit_event_id,stream_id,sequence,event_json,occurred_at,
                        previous_digest,digest
                 FROM agent_os_governance_audit_events
                 WHERE stream_id=?1 ORDER BY sequence DESC LIMIT 1",
                [event.stream_id().as_str()],
                StoredGovernanceAuditEvent::from_row,
            )
            .optional()
            .map_err(|error| GovernanceAuditRepositoryError::Persistence(error.to_string()))?;
        let tail = tail.map(Self::decode).transpose()?;
        validate_append(tail.into_iter(), &event)?;
        conn.execute(
            "INSERT INTO agent_os_governance_audit_events
             (audit_event_id,stream_id,sequence,event_json,occurred_at,previous_digest,digest)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                event.event_id().as_str(),
                event.stream_id().as_str(),
                event.sequence() as i64,
                encoded,
                event.occurred_at(),
                event.previous_digest(),
                event.digest(),
            ],
        )
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("audit_event_id") {
                GovernanceAuditRepositoryError::DuplicateEvent(event.event_id().clone())
            } else if message.contains("stream_id") && message.contains("sequence") {
                GovernanceAuditRepositoryError::DuplicateSequence {
                    stream_id: event.stream_id().clone(),
                    sequence: event.sequence(),
                }
            } else {
                GovernanceAuditRepositoryError::Persistence(message)
            }
        })?;
        Ok(())
    }

    fn get(
        &self,
        event_id: &GovernanceAuditEventId,
    ) -> Result<Option<GovernanceAuditEvent>, GovernanceAuditRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT audit_event_id,stream_id,sequence,event_json,occurred_at,
                        previous_digest,digest
                 FROM agent_os_governance_audit_events WHERE audit_event_id=?1",
                [event_id.as_str()],
                StoredGovernanceAuditEvent::from_row,
            )
            .optional()
            .map_err(|error| GovernanceAuditRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode).transpose()
    }

    fn list_stream(
        &self,
        stream_id: &GovernanceAuditStreamId,
        limit: usize,
    ) -> Result<Vec<GovernanceAuditEvent>, GovernanceAuditRepositoryError> {
        validate_limit(limit)?;
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn
            .prepare(
                "SELECT audit_event_id,stream_id,sequence,event_json,occurred_at,
                        previous_digest,digest
                 FROM agent_os_governance_audit_events
                 WHERE stream_id=?1 ORDER BY sequence LIMIT ?2",
            )
            .map_err(|error| GovernanceAuditRepositoryError::Persistence(error.to_string()))?;
        let rows = statement
            .query_map(
                params![stream_id.as_str(), limit as i64],
                StoredGovernanceAuditEvent::from_row,
            )
            .map_err(|error| GovernanceAuditRepositoryError::Persistence(error.to_string()))?;
        let mut events = Vec::new();
        for row in rows {
            events.push(Self::decode(row.map_err(|error| {
                GovernanceAuditRepositoryError::Persistence(error.to_string())
            })?)?);
        }
        validate_loaded_stream(&events)?;
        Ok(events)
    }

    fn last_stream_event(
        &self,
        stream_id: &GovernanceAuditStreamId,
    ) -> Result<Option<GovernanceAuditEvent>, GovernanceAuditRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT audit_event_id,stream_id,sequence,event_json,occurred_at,
                        previous_digest,digest
                 FROM agent_os_governance_audit_events
                 WHERE stream_id=?1 ORDER BY sequence DESC LIMIT 1",
                [stream_id.as_str()],
                StoredGovernanceAuditEvent::from_row,
            )
            .optional()
            .map_err(|error| GovernanceAuditRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode).transpose()
    }
}

fn validate_limit(limit: usize) -> Result<(), GovernanceAuditRepositoryError> {
    if limit == 0 || limit > MAX_QUERY_LIMIT {
        return Err(GovernanceAuditRepositoryError::InvalidQueryLimit);
    }
    Ok(())
}

fn validate_loaded_stream(
    events: &[GovernanceAuditEvent],
) -> Result<(), GovernanceAuditRepositoryError> {
    for (index, event) in events.iter().enumerate() {
        event.validate()?;
        let expected_sequence = index as u64 + 1;
        let expected_previous = index
            .checked_sub(1)
            .map(|previous| events[previous].digest());
        let time_regressed = index
            .checked_sub(1)
            .is_some_and(|previous| event.occurred_at() < events[previous].occurred_at());
        if event.sequence() != expected_sequence
            || event.previous_digest() != expected_previous
            || time_regressed
        {
            return Err(GovernanceAuditRepositoryError::BrokenChain(
                event.stream_id().clone(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct GovernanceAuditRecordRequest {
    pub stream_id: GovernanceAuditStreamId,
    pub kind: GovernanceAuditEventKind,
    pub outcome: GovernanceAuditOutcome,
    pub actor_reference: String,
    pub subject_type: String,
    pub subject_reference: String,
    pub correlations: AuditCorrelationReferences,
    pub metadata: SanitizedAuditMetadata,
    pub not_before: i64,
}

#[derive(Debug, Error)]
pub enum GovernanceAuditServiceError {
    #[error(transparent)]
    Domain(#[from] GovernanceAuditDomainError),
    #[error(transparent)]
    Repository(#[from] GovernanceAuditRepositoryError),
    #[error(transparent)]
    Clock(#[from] TrustedClockError),
    #[error("Trusted audit time precedes the evidence it records")]
    TrustedTimeBeforeEvidence,
    #[error("Trusted audit time precedes the current audit stream tail")]
    TrustedTimeBeforeStream,
    #[error("Governance audit stream sequence is exhausted")]
    SequenceExhausted,
    #[error("Governance audit stream remained contended after bounded retries")]
    AppendContention,
}

pub trait GovernanceAuditSink: Send + Sync {
    fn record(
        &self,
        request: GovernanceAuditRecordRequest,
    ) -> Result<GovernanceAuditEvent, GovernanceAuditServiceError>;
}

#[derive(Clone)]
pub struct GovernanceAuditService<R, C> {
    repository: R,
    clock: C,
}

impl<R, C> GovernanceAuditService<R, C> {
    pub fn new(repository: R, clock: C) -> Self {
        Self { repository, clock }
    }
}

impl<R, C> GovernanceAuditSink for GovernanceAuditService<R, C>
where
    R: GovernanceAuditRepository,
    C: TrustedClock,
{
    fn record(
        &self,
        request: GovernanceAuditRecordRequest,
    ) -> Result<GovernanceAuditEvent, GovernanceAuditServiceError> {
        for _ in 0..MAX_APPEND_ATTEMPTS {
            let tail = self.repository.last_stream_event(&request.stream_id)?;
            let occurred_at = self.clock.now()?;
            if occurred_at < request.not_before {
                return Err(GovernanceAuditServiceError::TrustedTimeBeforeEvidence);
            }
            if tail
                .as_ref()
                .is_some_and(|event| occurred_at < event.occurred_at())
            {
                return Err(GovernanceAuditServiceError::TrustedTimeBeforeStream);
            }
            let sequence = match tail.as_ref() {
                Some(event) => event
                    .sequence()
                    .checked_add(1)
                    .ok_or(GovernanceAuditServiceError::SequenceExhausted)?,
                None => 1,
            };
            let previous_digest = tail.as_ref().map(|event| event.digest().to_string());
            let id_seed = serde_json::to_vec(&(
                request.stream_id.as_str(),
                sequence,
                request.kind,
                occurred_at,
            ))
            .map_err(|_| GovernanceAuditDomainError::InvalidIdentifier {
                field: "Audit event identity seed",
            })?;
            let id_digest = format!("{:x}", Sha256::digest(id_seed));
            let event = GovernanceAuditEvent::new(
                GovernanceAuditEventId::new(format!("audit:{}", &id_digest[..32]))?,
                request.stream_id.clone(),
                sequence,
                request.kind,
                request.outcome,
                request.actor_reference.clone(),
                request.subject_type.clone(),
                request.subject_reference.clone(),
                request.correlations.clone(),
                occurred_at,
                previous_digest,
                request.metadata.clone(),
            )?;
            match self.repository.append(event.clone()) {
                Ok(()) => return Ok(event),
                Err(GovernanceAuditRepositoryError::DuplicateSequence { .. })
                | Err(GovernanceAuditRepositoryError::DuplicateEvent(_))
                | Err(GovernanceAuditRepositoryError::BrokenChain(_)) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(GovernanceAuditServiceError::AppendContention)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;
    use crate::governance_time::FixedTrustedClock;

    #[derive(Clone)]
    struct IncrementingTrustedClock {
        next: Arc<AtomicI64>,
    }

    impl IncrementingTrustedClock {
        fn new(first: i64) -> Self {
            Self {
                next: Arc::new(AtomicI64::new(first)),
            }
        }
    }

    impl TrustedClock for IncrementingTrustedClock {
        fn now(&self) -> Result<i64, TrustedClockError> {
            Ok(self.next.fetch_add(1, Ordering::SeqCst))
        }
    }

    fn request(stream: &str, kind: GovernanceAuditEventKind) -> GovernanceAuditRecordRequest {
        GovernanceAuditRecordRequest {
            stream_id: GovernanceAuditStreamId::new(stream).unwrap(),
            kind,
            outcome: GovernanceAuditOutcome::Accepted,
            actor_reference: "actor:test".into(),
            subject_type: "controlled_environment".into(),
            subject_reference: "environment:test".into(),
            correlations: AuditCorrelationReferences::default(),
            metadata: SanitizedAuditMetadata::empty(),
            not_before: 10,
        }
    }

    #[test]
    fn stream_is_ordered_append_only_and_tamper_evident() {
        let repository = InMemoryGovernanceAuditRepository::default();
        let service =
            GovernanceAuditService::new(repository.clone(), FixedTrustedClock::new(10).unwrap());
        let first = service
            .record(request(
                "audit-stream:test",
                GovernanceAuditEventKind::ControlledEnvironmentPreparationRequested,
            ))
            .unwrap();
        let second = service
            .record(request(
                "audit-stream:test",
                GovernanceAuditEventKind::ControlledEnvironmentPreparationAccepted,
            ))
            .unwrap();
        assert_eq!(second.sequence(), 2);
        assert_eq!(second.previous_digest(), Some(first.digest()));
        assert_eq!(
            repository
                .list_stream(
                    &GovernanceAuditStreamId::new("audit-stream:test").unwrap(),
                    10,
                )
                .unwrap(),
            vec![first, second]
        );
    }

    #[test]
    fn secret_like_metadata_is_rejected_before_serialization() {
        let mut values = BTreeMap::new();
        values.insert("api_key".into(), "must-not-appear".into());
        assert!(matches!(
            SanitizedAuditMetadata::new(values),
            Err(GovernanceAuditDomainError::ForbiddenMetadata(_))
        ));
        let mut values = BTreeMap::new();
        values.insert("reason_code".into(), "sk-live-secret".into());
        assert!(matches!(
            SanitizedAuditMetadata::new(values),
            Err(GovernanceAuditDomainError::ForbiddenMetadata(_))
        ));
        let mut values = BTreeMap::new();
        values.insert("reason_code".into(), "AKIAIOSFODNN7EXAMPLE".into());
        assert!(matches!(
            SanitizedAuditMetadata::new(values),
            Err(GovernanceAuditDomainError::ForbiddenMetadata(_))
        ));
        let mut values = BTreeMap::new();
        values.insert("detail".into(), "apparently_safe".into());
        assert!(matches!(
            SanitizedAuditMetadata::new(values),
            Err(GovernanceAuditDomainError::ForbiddenMetadata(_))
        ));
        assert!(GovernanceAuditEvent::new(
            GovernanceAuditEventId::new("audit:unsafe-actor").unwrap(),
            GovernanceAuditStreamId::new("audit-stream:unsafe-actor").unwrap(),
            1,
            GovernanceAuditEventKind::ControlledEnvironmentPreparationRejected,
            GovernanceAuditOutcome::Rejected,
            "sk-live-secret",
            "controlled_environment",
            "environment:test",
            AuditCorrelationReferences::default(),
            10,
            None,
            SanitizedAuditMetadata::empty(),
        )
        .is_err());
        let event = GovernanceAuditService::new(
            InMemoryGovernanceAuditRepository::default(),
            FixedTrustedClock::new(10).unwrap(),
        )
        .record(request(
            "audit-stream:safe",
            GovernanceAuditEventKind::ControlledEnvironmentPreparationAccepted,
        ))
        .unwrap();
        let serialized = serde_json::to_string(&event).unwrap();
        for forbidden in ["apiKey", "credential", "fullPrompt", "modelOutput"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn sqlite_audit_is_ordered_and_database_guards_forbid_mutation() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteGovernanceAuditRepository::new(database.clone());
        let service =
            GovernanceAuditService::new(repository.clone(), FixedTrustedClock::new(20).unwrap());
        service
            .record(request(
                "audit-stream:sqlite",
                GovernanceAuditEventKind::ControlledEnvironmentPreparationRequested,
            ))
            .unwrap();
        service
            .record(request(
                "audit-stream:sqlite",
                GovernanceAuditEventKind::ControlledEnvironmentPreparationAccepted,
            ))
            .unwrap();
        let stream_id = GovernanceAuditStreamId::new("audit-stream:sqlite").unwrap();
        let events = repository.list_stream(&stream_id, 10).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].previous_digest(), Some(events[0].digest()));

        let conn = database.conn.lock().unwrap();
        assert!(conn
            .execute(
                "UPDATE agent_os_governance_audit_events SET occurred_at=21
                 WHERE stream_id='audit-stream:sqlite' AND sequence=1",
                [],
            )
            .is_err());
        assert!(conn
            .execute(
                "DELETE FROM agent_os_governance_audit_events
                 WHERE stream_id='audit-stream:sqlite' AND sequence=1",
                [],
            )
            .is_err());
        conn.execute_batch(
            "DROP TRIGGER trg_agent_os_governance_audit_update_forbidden;
             UPDATE agent_os_governance_audit_events
             SET stream_id='audit-stream:tampered'
             WHERE stream_id='audit-stream:sqlite' AND sequence=1;",
        )
        .unwrap();
        drop(conn);
        assert!(repository.get(&events[0].event_id().clone()).is_err());
    }

    #[test]
    fn broken_digest_chain_is_rejected() {
        let repository = InMemoryGovernanceAuditRepository::default();
        let event = GovernanceAuditEvent::new(
            GovernanceAuditEventId::new("audit:broken").unwrap(),
            GovernanceAuditStreamId::new("audit-stream:broken").unwrap(),
            2,
            GovernanceAuditEventKind::ControlledEnvironmentPreparationRejected,
            GovernanceAuditOutcome::Rejected,
            "actor:test",
            "controlled_environment",
            "environment:test",
            AuditCorrelationReferences::default(),
            20,
            Some("0".repeat(64)),
            SanitizedAuditMetadata::empty(),
        )
        .unwrap();
        assert!(matches!(
            repository.append(event),
            Err(GovernanceAuditRepositoryError::BrokenChain(_))
        ));
    }

    #[test]
    fn audit_stream_rejects_time_regression() {
        let repository = InMemoryGovernanceAuditRepository::default();
        let first =
            GovernanceAuditService::new(repository.clone(), FixedTrustedClock::new(20).unwrap())
                .record(request(
                    "audit-stream:time",
                    GovernanceAuditEventKind::ControlledEnvironmentPreparationRequested,
                ))
                .unwrap();
        let second = GovernanceAuditEvent::new(
            GovernanceAuditEventId::new("audit:time-regression").unwrap(),
            first.stream_id().clone(),
            2,
            GovernanceAuditEventKind::ControlledEnvironmentPreparationRejected,
            GovernanceAuditOutcome::Rejected,
            "actor:test",
            "controlled_environment",
            "environment:test",
            AuditCorrelationReferences::default(),
            19,
            Some(first.digest().into()),
            SanitizedAuditMetadata::empty(),
        )
        .unwrap();
        assert!(matches!(
            repository.append(second),
            Err(GovernanceAuditRepositoryError::BrokenChain(_))
        ));
    }

    #[test]
    fn concurrent_audit_appends_are_retried_into_one_contiguous_stream() {
        let repository = InMemoryGovernanceAuditRepository::default();
        let service = Arc::new(GovernanceAuditService::new(
            repository.clone(),
            IncrementingTrustedClock::new(20),
        ));
        let mut workers = Vec::new();
        for _ in 0..24 {
            let service = service.clone();
            workers.push(std::thread::spawn(move || {
                service.record(request(
                    "audit-stream:concurrent",
                    GovernanceAuditEventKind::ControlledEnvironmentRevalidationAccepted,
                ))
            }));
        }
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        let events = repository
            .list_stream(
                &GovernanceAuditStreamId::new("audit-stream:concurrent").unwrap(),
                24,
            )
            .unwrap();
        assert_eq!(events.len(), 24);
        assert_eq!(events.last().unwrap().sequence(), 24);
        assert!(events
            .windows(2)
            .all(|pair| pair[0].occurred_at() <= pair[1].occurred_at()));
    }
}
