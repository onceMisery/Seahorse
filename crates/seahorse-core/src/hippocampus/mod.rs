use rusqlite::Connection;

use crate::storage::{apply_sqlite_migrations, SqliteRepository, StorageResult};

#[derive(Debug)]
pub struct Hippocampus {
    repository: SqliteRepository,
}

impl Hippocampus {
    pub fn new(repository: SqliteRepository) -> Self {
        Self { repository }
    }

    pub fn open_in_memory() -> StorageResult<Self> {
        let connection = Connection::open_in_memory()?;
        apply_sqlite_migrations(&connection)?;
        let repository = SqliteRepository::new(connection)?;
        Ok(Self::new(repository))
    }

    pub fn repository(&self) -> &SqliteRepository {
        &self.repository
    }

    pub fn repository_mut(&mut self) -> &mut SqliteRepository {
        &mut self.repository
    }

    pub fn into_repository(self) -> SqliteRepository {
        self.repository
    }
}
