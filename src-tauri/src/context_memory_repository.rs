//! Persistence boundary for governed Context, Memory, and Knowledge records.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Transaction};
use thiserror::Error;

use crate::{
    context_memory_domain::{
        ContextMemoryDomainError, ContextPackage, ContextPackageId, KnowledgeLifecycle,
        KnowledgeReference, KnowledgeReferenceId, MemoryEntry, MemoryEntryId, MemoryLifecycle,
    },
    database::{lock_conn, Database},
    error::AppError,
    runtime_domain::RuntimeExecutionId,
};

#[derive(Debug, Error)]
pub enum ContextMemoryRepositoryError {
    #[error(transparent)]
    InvalidDomain(#[from] ContextMemoryDomainError),
    #[error("{record} already exists: {id}")]
    AlreadyExists { record: &'static str, id: String },
    #[error("{record} was not found: {id}")]
    NotFound { record: &'static str, id: String },
    #[error("{record} revision conflict for {id}: expected {expected}, current {current}")]
    RevisionConflict {
        record: &'static str,
        id: String,
        expected: u64,
        current: u64,
    },
    #[error("Context and memory persistence failed: {0}")]
    Persistence(String),
}

impl From<AppError> for ContextMemoryRepositoryError {
    fn from(error: AppError) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait ContextMemoryRepository: Send + Sync {
    fn create_memory(
        &self,
        memory: MemoryEntry,
    ) -> Result<MemoryEntry, ContextMemoryRepositoryError>;
    fn get_memory(
        &self,
        id: &MemoryEntryId,
    ) -> Result<Option<MemoryEntry>, ContextMemoryRepositoryError>;
    fn list_agent_memories(
        &self,
        agent_id: &str,
    ) -> Result<Vec<MemoryEntry>, ContextMemoryRepositoryError>;
    fn transition_memory(
        &self,
        id: &MemoryEntryId,
        target: MemoryLifecycle,
        expected_revision: u64,
        now: i64,
    ) -> Result<MemoryEntry, ContextMemoryRepositoryError>;

    fn create_knowledge(
        &self,
        knowledge: KnowledgeReference,
    ) -> Result<KnowledgeReference, ContextMemoryRepositoryError>;
    fn get_knowledge(
        &self,
        id: &KnowledgeReferenceId,
    ) -> Result<Option<KnowledgeReference>, ContextMemoryRepositoryError>;
    fn list_knowledge(
        &self,
        agent_scope: Option<&str>,
    ) -> Result<Vec<KnowledgeReference>, ContextMemoryRepositoryError>;
    fn transition_knowledge(
        &self,
        id: &KnowledgeReferenceId,
        target: KnowledgeLifecycle,
        expected_revision: u64,
        now: i64,
    ) -> Result<KnowledgeReference, ContextMemoryRepositoryError>;

    fn create_context(
        &self,
        package: ContextPackage,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError>;
    fn get_context(
        &self,
        id: &ContextPackageId,
    ) -> Result<Option<ContextPackage>, ContextMemoryRepositoryError>;
    fn get_execution_context(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Option<ContextPackage>, ContextMemoryRepositoryError>;
    fn resolve_context(
        &self,
        id: &ContextPackageId,
        memory_ids: Vec<MemoryEntryId>,
        knowledge_ids: Vec<KnowledgeReferenceId>,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError>;
    fn seal_context(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError>;
    fn expire_context(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError>;
    fn revoke_context(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError>;
}

#[derive(Clone)]
pub struct SqliteContextMemoryRepository {
    database: Arc<Database>,
}

impl SqliteContextMemoryRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    fn encode<T: serde::Serialize>(value: &T) -> Result<String, ContextMemoryRepositoryError> {
        serde_json::to_string(value)
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))
    }

    fn decode_memory(value: String) -> Result<MemoryEntry, ContextMemoryRepositoryError> {
        let memory: MemoryEntry = serde_json::from_str(&value)
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        memory.validate()?;
        Ok(memory)
    }

    fn decode_knowledge(value: String) -> Result<KnowledgeReference, ContextMemoryRepositoryError> {
        let knowledge: KnowledgeReference = serde_json::from_str(&value)
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        knowledge.validate()?;
        Ok(knowledge)
    }

    fn decode_context(value: String) -> Result<ContextPackage, ContextMemoryRepositoryError> {
        let package: ContextPackage = serde_json::from_str(&value)
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        package.validate()?;
        Ok(package)
    }

    fn duplicate_or_persistence(
        error: rusqlite::Error,
        record: &'static str,
        id: &str,
    ) -> ContextMemoryRepositoryError {
        if error.to_string().contains("UNIQUE constraint failed") {
            ContextMemoryRepositoryError::AlreadyExists {
                record,
                id: id.to_string(),
            }
        } else {
            ContextMemoryRepositoryError::Persistence(error.to_string())
        }
    }

    fn load_in_transaction<T>(
        transaction: &Transaction<'_>,
        table: &str,
        id_column: &str,
        id: &str,
        record: &'static str,
        decode: fn(String) -> Result<T, ContextMemoryRepositoryError>,
    ) -> Result<T, ContextMemoryRepositoryError> {
        let sql = format!("SELECT record_json FROM {table} WHERE {id_column}=?1");
        let value = transaction
            .query_row(&sql, [id], |row| row.get::<_, String>(0))
            .optional()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?
            .ok_or_else(|| ContextMemoryRepositoryError::NotFound {
                record,
                id: id.to_string(),
            })?;
        decode(value)
    }

    #[allow(clippy::too_many_arguments)]
    fn update_record<T: serde::Serialize>(
        transaction: &Transaction<'_>,
        table: &str,
        id_column: &str,
        id: &str,
        record: &'static str,
        value: &T,
        lifecycle: &str,
        next_revision: u64,
        expected_revision: u64,
    ) -> Result<(), ContextMemoryRepositoryError> {
        let sql = format!(
            "UPDATE {table} SET record_json=?1,lifecycle_state=?2,revision=?3
             WHERE {id_column}=?4 AND revision=?5"
        );
        let changed = transaction
            .execute(
                &sql,
                params![
                    Self::encode(value)?,
                    lifecycle,
                    next_revision as i64,
                    id,
                    expected_revision as i64
                ],
            )
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        if changed != 1 {
            return Err(ContextMemoryRepositoryError::RevisionConflict {
                record,
                id: id.to_string(),
                expected: expected_revision,
                current: expected_revision + 1,
            });
        }
        Ok(())
    }
}

impl ContextMemoryRepository for SqliteContextMemoryRepository {
    fn create_memory(
        &self,
        memory: MemoryEntry,
    ) -> Result<MemoryEntry, ContextMemoryRepositoryError> {
        memory.validate()?;
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_memory_entries
             (memory_id,agent_id,record_json,lifecycle_state,revision,expires_at,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                memory.id().as_str(),
                memory.agent_id(),
                Self::encode(&memory)?,
                memory.lifecycle().as_str(),
                memory.revision() as i64,
                memory.expires_at(),
                memory.created_at()
            ],
        )
        .map_err(|error| Self::duplicate_or_persistence(error, "Memory", memory.id().as_str()))?;
        Ok(memory)
    }

    fn get_memory(
        &self,
        id: &MemoryEntryId,
    ) -> Result<Option<MemoryEntry>, ContextMemoryRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT record_json FROM agent_os_memory_entries WHERE memory_id=?1",
                [id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode_memory).transpose()
    }

    fn list_agent_memories(
        &self,
        agent_id: &str,
    ) -> Result<Vec<MemoryEntry>, ContextMemoryRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let mut statement = conn.prepare("SELECT record_json FROM agent_os_memory_entries WHERE agent_id=?1 ORDER BY created_at,memory_id").map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        let rows = statement
            .query_map([agent_id], |row| row.get::<_, String>(0))
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        rows.map(|row| {
            row.map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))
                .and_then(Self::decode_memory)
        })
        .collect()
    }

    fn transition_memory(
        &self,
        id: &MemoryEntryId,
        target: MemoryLifecycle,
        expected_revision: u64,
        now: i64,
    ) -> Result<MemoryEntry, ContextMemoryRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        let current = Self::load_in_transaction(
            &transaction,
            "agent_os_memory_entries",
            "memory_id",
            id.as_str(),
            "Memory",
            Self::decode_memory,
        )?;
        if current.revision() != expected_revision {
            return Err(ContextMemoryRepositoryError::RevisionConflict {
                record: "Memory",
                id: id.to_string(),
                expected: expected_revision,
                current: current.revision(),
            });
        }
        let next = current.transition(target, expected_revision, now)?;
        Self::update_record(
            &transaction,
            "agent_os_memory_entries",
            "memory_id",
            id.as_str(),
            "Memory",
            &next,
            next.lifecycle().as_str(),
            next.revision(),
            expected_revision,
        )?;
        transaction
            .commit()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        Ok(next)
    }

    fn create_knowledge(
        &self,
        knowledge: KnowledgeReference,
    ) -> Result<KnowledgeReference, ContextMemoryRepositoryError> {
        knowledge.validate()?;
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_knowledge_references
             (knowledge_id,agent_scope,record_json,lifecycle_state,revision,expires_at,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                knowledge.id().as_str(),
                knowledge.agent_scope(),
                Self::encode(&knowledge)?,
                knowledge.lifecycle().as_str(),
                knowledge.revision() as i64,
                knowledge.expires_at(),
                knowledge.created_at()
            ],
        )
        .map_err(|error| {
            Self::duplicate_or_persistence(error, "Knowledge", knowledge.id().as_str())
        })?;
        Ok(knowledge)
    }

    fn get_knowledge(
        &self,
        id: &KnowledgeReferenceId,
    ) -> Result<Option<KnowledgeReference>, ContextMemoryRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT record_json FROM agent_os_knowledge_references WHERE knowledge_id=?1",
                [id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode_knowledge).transpose()
    }

    fn list_knowledge(
        &self,
        agent_scope: Option<&str>,
    ) -> Result<Vec<KnowledgeReference>, ContextMemoryRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let (sql, parameter) = match agent_scope {
            Some(scope) => ("SELECT record_json FROM agent_os_knowledge_references WHERE agent_scope IS NULL OR agent_scope=?1 ORDER BY created_at,knowledge_id", Some(scope)),
            None => ("SELECT record_json FROM agent_os_knowledge_references WHERE agent_scope IS NULL ORDER BY created_at,knowledge_id", None),
        };
        let mut statement = conn
            .prepare(sql)
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        let values = match parameter {
            Some(scope) => statement
                .query_map([scope], |row| row.get::<_, String>(0))
                .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?
                .collect::<Result<Vec<_>, _>>(),
            None => statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?
                .collect::<Result<Vec<_>, _>>(),
        }
        .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        values.into_iter().map(Self::decode_knowledge).collect()
    }

    fn transition_knowledge(
        &self,
        id: &KnowledgeReferenceId,
        target: KnowledgeLifecycle,
        expected_revision: u64,
        now: i64,
    ) -> Result<KnowledgeReference, ContextMemoryRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        let current = Self::load_in_transaction(
            &transaction,
            "agent_os_knowledge_references",
            "knowledge_id",
            id.as_str(),
            "Knowledge",
            Self::decode_knowledge,
        )?;
        if current.revision() != expected_revision {
            return Err(ContextMemoryRepositoryError::RevisionConflict {
                record: "Knowledge",
                id: id.to_string(),
                expected: expected_revision,
                current: current.revision(),
            });
        }
        let next = current.transition(target, expected_revision, now)?;
        Self::update_record(
            &transaction,
            "agent_os_knowledge_references",
            "knowledge_id",
            id.as_str(),
            "Knowledge",
            &next,
            next.lifecycle().as_str(),
            next.revision(),
            expected_revision,
        )?;
        transaction
            .commit()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        Ok(next)
    }

    fn create_context(
        &self,
        package: ContextPackage,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError> {
        package.validate()?;
        let conn = lock_conn!(self.database.conn);
        conn.execute(
            "INSERT INTO agent_os_context_packages
             (context_package_id,execution_id,agent_id,record_json,lifecycle_state,revision,expires_at,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![package.id().as_str(), package.execution_id().as_str(), package.agent_id(), Self::encode(&package)?, package.lifecycle().as_str(), package.revision() as i64, package.expires_at(), package.created_at()],
        ).map_err(|error| Self::duplicate_or_persistence(error, "Context package", package.id().as_str()))?;
        Ok(package)
    }

    fn get_context(
        &self,
        id: &ContextPackageId,
    ) -> Result<Option<ContextPackage>, ContextMemoryRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT record_json FROM agent_os_context_packages WHERE context_package_id=?1",
                [id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode_context).transpose()
    }

    fn get_execution_context(
        &self,
        execution_id: &RuntimeExecutionId,
    ) -> Result<Option<ContextPackage>, ContextMemoryRepositoryError> {
        let conn = lock_conn!(self.database.conn);
        let value = conn
            .query_row(
                "SELECT record_json FROM agent_os_context_packages WHERE execution_id=?1",
                [execution_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        value.map(Self::decode_context).transpose()
    }

    fn resolve_context(
        &self,
        id: &ContextPackageId,
        memory_ids: Vec<MemoryEntryId>,
        knowledge_ids: Vec<KnowledgeReferenceId>,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError> {
        self.update_context(id, expected_revision, |current| {
            current.resolve(memory_ids, knowledge_ids, expected_revision, now)
        })
    }

    fn seal_context(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError> {
        self.update_context(id, expected_revision, |current| {
            current.seal(expected_revision, now)
        })
    }

    fn expire_context(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError> {
        self.update_context(id, expected_revision, |current| {
            current.expire(expected_revision, now)
        })
    }

    fn revoke_context(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        now: i64,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError> {
        self.update_context(id, expected_revision, |current| {
            current.revoke(expected_revision, now)
        })
    }
}

impl SqliteContextMemoryRepository {
    fn update_context<F>(
        &self,
        id: &ContextPackageId,
        expected_revision: u64,
        transition: F,
    ) -> Result<ContextPackage, ContextMemoryRepositoryError>
    where
        F: FnOnce(&ContextPackage) -> Result<ContextPackage, ContextMemoryDomainError>,
    {
        let conn = lock_conn!(self.database.conn);
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        let current = Self::load_in_transaction(
            &transaction,
            "agent_os_context_packages",
            "context_package_id",
            id.as_str(),
            "Context package",
            Self::decode_context,
        )?;
        if current.revision() != expected_revision {
            return Err(ContextMemoryRepositoryError::RevisionConflict {
                record: "Context package",
                id: id.to_string(),
                expected: expected_revision,
                current: current.revision(),
            });
        }
        let next = transition(&current)?;
        Self::update_record(
            &transaction,
            "agent_os_context_packages",
            "context_package_id",
            id.as_str(),
            "Context package",
            &next,
            next.lifecycle().as_str(),
            next.revision(),
            expected_revision,
        )?;
        transaction
            .commit()
            .map_err(|error| ContextMemoryRepositoryError::Persistence(error.to_string()))?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_memory_domain::{
        ContextPolicyId, KnowledgeSourceKind, KnowledgeTrust, MemoryContent, MemoryKind,
        MemorySensitivity,
    };

    fn memory(id: &str) -> MemoryEntry {
        MemoryEntry::new(
            MemoryEntryId::new(id).unwrap(),
            "agent:one",
            MemoryKind::Decision,
            MemoryContent::Text("Use explicit context".to_string()),
            MemorySensitivity::Internal,
            Some(RuntimeExecutionId::new("execution:source").unwrap()),
            10,
            100,
        )
        .unwrap()
    }

    fn knowledge(id: &str) -> KnowledgeReference {
        KnowledgeReference::new(
            KnowledgeReferenceId::new(id).unwrap(),
            Some("agent:one".to_string()),
            KnowledgeSourceKind::File,
            "repo://docs/architecture.md",
            KnowledgeTrust::Verified,
            None,
            10,
            100,
        )
        .unwrap()
    }

    #[test]
    fn sqlite_records_survive_repository_recreation_and_enforce_revisions() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteContextMemoryRepository::new(database.clone());
        let memory = repository.create_memory(memory("memory:one")).unwrap();
        repository
            .create_knowledge(knowledge("knowledge:one"))
            .unwrap();
        let package = repository
            .create_context(
                ContextPackage::draft(
                    ContextPackageId::new("context:one").unwrap(),
                    RuntimeExecutionId::new("execution:one").unwrap(),
                    "agent:one",
                    ContextPolicyId::new("policy:one").unwrap(),
                    10,
                    100,
                )
                .unwrap(),
            )
            .unwrap();
        drop(repository);

        let reopened = SqliteContextMemoryRepository::new(database);
        assert_eq!(reopened.get_memory(memory.id()).unwrap().unwrap(), memory);
        let resolved = reopened
            .resolve_context(
                package.id(),
                vec![MemoryEntryId::new("memory:one").unwrap()],
                vec![KnowledgeReferenceId::new("knowledge:one").unwrap()],
                package.revision(),
                20,
            )
            .unwrap();
        assert!(matches!(
            reopened.seal_context(package.id(), package.revision(), 21),
            Err(ContextMemoryRepositoryError::RevisionConflict { .. })
        ));
        assert_eq!(
            reopened
                .seal_context(package.id(), resolved.revision(), 21)
                .unwrap()
                .lifecycle(),
            crate::context_memory_domain::ContextPackageLifecycle::Sealed
        );
    }

    #[test]
    fn repository_never_physically_deletes_context_memory_records() {
        let database = Arc::new(Database::memory().unwrap());
        let repository = SqliteContextMemoryRepository::new(database.clone());
        repository.create_memory(memory("memory:audit")).unwrap();
        repository
            .create_knowledge(knowledge("knowledge:audit"))
            .unwrap();
        drop(repository);
        let conn = database.conn.lock().unwrap();
        assert!(conn
            .execute(
                "DELETE FROM agent_os_memory_entries WHERE memory_id='memory:audit'",
                [],
            )
            .is_err());
        assert!(conn
            .execute(
                "DELETE FROM agent_os_knowledge_references WHERE knowledge_id='knowledge:audit'",
                [],
            )
            .is_err());
    }
}
