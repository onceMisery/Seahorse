use rusqlite::{Connection, OptionalExtension};

use super::{StorageError, StorageResult};

const INITIAL_MIGRATION: &str = include_str!("../../../../migrations/0001_init.sql");
const RELAX_DEDUP_CONSTRAINTS_MIGRATION: &str =
    include_str!("../../../../migrations/0002_relax_dedup_constraints.sql");

pub const LATEST_SCHEMA_VERSION: &str = "2";

pub fn apply_sqlite_migrations(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(INITIAL_MIGRATION)?;

    let schema_version = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    match schema_version.as_deref() {
        Some("1") => {
            connection.execute_batch(RELAX_DEDUP_CONSTRAINTS_MIGRATION)?;
            Ok(())
        }
        Some(LATEST_SCHEMA_VERSION) => Ok(()),
        Some(actual) => Err(StorageError::InvalidSchemaMeta {
            key: "schema_version",
            expected: "1 or 2".to_owned(),
            actual: Some(actual.to_owned()),
        }),
        None => Err(StorageError::MissingSchemaMeta {
            key: "schema_version",
        }),
    }
}
