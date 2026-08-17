//! Immutable communication and revisioned Handoff repository boundary.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use thiserror::Error;

use crate::{
    collaboration_domain::{
        CollaborationDomainError, CollaborationMessage, CollaborationMessageId, Handoff, HandoffId,
        HandoffLifecycle,
    },
    workflow_domain::WorkflowRunId,
};

#[derive(Debug, Error)]
pub enum CollaborationRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] CollaborationDomainError),
    #[error("Collaboration Message is already recorded: {0}")]
    MessageAlreadyRecorded(CollaborationMessageId),
    #[error("Collaboration Message was not found: {0}")]
    MessageNotFound(CollaborationMessageId),
    #[error("Handoff is already recorded: {0}")]
    HandoffAlreadyRecorded(HandoffId),
    #[error("Handoff was not found: {0}")]
    HandoffNotFound(HandoffId),
    #[error("Handoff identity changed during update")]
    HandoffIdentityChanged,
    #[error("Handoff must be proposed at revision 1")]
    InvalidInitialHandoff,
    #[error("Handoff update is not one legal lifecycle transition")]
    InvalidHandoffUpdate,
    #[error("Collaboration repository lock failed: {0}")]
    RegistryLock(String),
}

pub trait CollaborationRepository: Send + Sync {
    fn record_message(
        &self,
        message: CollaborationMessage,
    ) -> Result<(), CollaborationRepositoryError>;
    fn get_message(
        &self,
        message_id: &CollaborationMessageId,
    ) -> Result<Option<CollaborationMessage>, CollaborationRepositoryError>;
    fn list_messages(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Vec<CollaborationMessage>, CollaborationRepositoryError>;
    fn insert_handoff(&self, handoff: Handoff) -> Result<(), CollaborationRepositoryError>;
    fn get_handoff(
        &self,
        handoff_id: &HandoffId,
    ) -> Result<Option<Handoff>, CollaborationRepositoryError>;
    fn list_handoffs(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Vec<Handoff>, CollaborationRepositoryError>;
    fn update_handoff(
        &self,
        handoff: Handoff,
        expected_revision: u64,
    ) -> Result<(), CollaborationRepositoryError>;
}

#[derive(Clone, Default)]
pub struct InMemoryCollaborationRepository {
    messages: Arc<RwLock<HashMap<CollaborationMessageId, CollaborationMessage>>>,
    handoffs: Arc<RwLock<HashMap<HandoffId, Handoff>>>,
}

impl CollaborationRepository for InMemoryCollaborationRepository {
    fn record_message(
        &self,
        message: CollaborationMessage,
    ) -> Result<(), CollaborationRepositoryError> {
        let mut messages = self
            .messages
            .write()
            .map_err(|error| CollaborationRepositoryError::RegistryLock(error.to_string()))?;
        if messages.contains_key(message.id()) {
            return Err(CollaborationRepositoryError::MessageAlreadyRecorded(
                message.id().clone(),
            ));
        }
        messages.insert(message.id().clone(), message);
        Ok(())
    }

    fn get_message(
        &self,
        message_id: &CollaborationMessageId,
    ) -> Result<Option<CollaborationMessage>, CollaborationRepositoryError> {
        let messages = self
            .messages
            .read()
            .map_err(|error| CollaborationRepositoryError::RegistryLock(error.to_string()))?;
        Ok(messages.get(message_id).cloned())
    }

    fn list_messages(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Vec<CollaborationMessage>, CollaborationRepositoryError> {
        let messages = self
            .messages
            .read()
            .map_err(|error| CollaborationRepositoryError::RegistryLock(error.to_string()))?;
        let mut result = messages
            .values()
            .filter(|message| message.run_id() == run_id)
            .cloned()
            .collect::<Vec<_>>();
        result.sort_by(|left, right| {
            left.created_at()
                .cmp(&right.created_at())
                .then_with(|| left.id().cmp(right.id()))
        });
        Ok(result)
    }

    fn insert_handoff(&self, handoff: Handoff) -> Result<(), CollaborationRepositoryError> {
        if handoff.lifecycle() != HandoffLifecycle::Proposed || handoff.revision() != 1 {
            return Err(CollaborationRepositoryError::InvalidInitialHandoff);
        }
        if self.get_message(handoff.proposal_message_id())?.is_none() {
            return Err(CollaborationRepositoryError::MessageNotFound(
                handoff.proposal_message_id().clone(),
            ));
        }
        let mut handoffs = self
            .handoffs
            .write()
            .map_err(|error| CollaborationRepositoryError::RegistryLock(error.to_string()))?;
        if handoffs.contains_key(handoff.id()) {
            return Err(CollaborationRepositoryError::HandoffAlreadyRecorded(
                handoff.id().clone(),
            ));
        }
        handoffs.insert(handoff.id().clone(), handoff);
        Ok(())
    }

    fn get_handoff(
        &self,
        handoff_id: &HandoffId,
    ) -> Result<Option<Handoff>, CollaborationRepositoryError> {
        let handoffs = self
            .handoffs
            .read()
            .map_err(|error| CollaborationRepositoryError::RegistryLock(error.to_string()))?;
        Ok(handoffs.get(handoff_id).cloned())
    }

    fn list_handoffs(
        &self,
        run_id: &WorkflowRunId,
    ) -> Result<Vec<Handoff>, CollaborationRepositoryError> {
        let handoffs = self
            .handoffs
            .read()
            .map_err(|error| CollaborationRepositoryError::RegistryLock(error.to_string()))?;
        let mut result = handoffs
            .values()
            .filter(|handoff| handoff.run_id() == run_id)
            .cloned()
            .collect::<Vec<_>>();
        result.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(result)
    }

    fn update_handoff(
        &self,
        handoff: Handoff,
        expected_revision: u64,
    ) -> Result<(), CollaborationRepositoryError> {
        if let Some(message_id) = handoff.resolution_message_id() {
            if self.get_message(message_id)?.is_none() {
                return Err(CollaborationRepositoryError::MessageNotFound(
                    message_id.clone(),
                ));
            }
        }
        let mut handoffs = self
            .handoffs
            .write()
            .map_err(|error| CollaborationRepositoryError::RegistryLock(error.to_string()))?;
        let current = handoffs
            .get(handoff.id())
            .ok_or_else(|| CollaborationRepositoryError::HandoffNotFound(handoff.id().clone()))?;
        if current.team_id() != handoff.team_id()
            || current.run_id() != handoff.run_id()
            || current.source_task_id() != handoff.source_task_id()
            || current.target_step_id() != handoff.target_step_id()
            || current.source_membership_id() != handoff.source_membership_id()
            || current.target_membership_id() != handoff.target_membership_id()
            || current.proposal_message_id() != handoff.proposal_message_id()
            || current.created_at() != handoff.created_at()
        {
            return Err(CollaborationRepositoryError::HandoffIdentityChanged);
        }
        if current.revision() != expected_revision
            || handoff.revision() != expected_revision + 1
            || !current.lifecycle().can_transition_to(handoff.lifecycle())
        {
            return Err(CollaborationRepositoryError::InvalidHandoffUpdate);
        }
        handoffs.insert(handoff.id().clone(), handoff);
        Ok(())
    }
}
