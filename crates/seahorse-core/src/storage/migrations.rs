use rusqlite::{Connection, OptionalExtension};

use super::{StorageError, StorageResult};

const INITIAL_MIGRATION: &str = include_str!("../../../../migrations/0001_init.sql");
const RELAX_DEDUP_CONSTRAINTS_MIGRATION: &str =
    include_str!("../../../../migrations/0002_relax_dedup_constraints.sql");
const DESIGN_ALL_PHASE1_MIGRATION: &str =
    include_str!("../../../../migrations/0003_design_all_phase1.sql");
const CONNECTOME_MIGRATION: &str = include_str!("../../../../migrations/0004_connectome.sql");

pub const LATEST_SCHEMA_VERSION: &str = "4";

pub fn apply_sqlite_migrations(connection: &Connection) -> StorageResult<()> {
    connection.execute_batch(INITIAL_MIGRATION)?;

    let mut schema_version = load_schema_version(connection)?;

    if schema_version.as_deref() == Some("1") {
        connection.execute_batch(RELAX_DEDUP_CONSTRAINTS_MIGRATION)?;
        schema_version = load_schema_version(connection)?;
    }

    if schema_version.as_deref() == Some("2") {
        connection.execute_batch(DESIGN_ALL_PHASE1_MIGRATION)?;
        schema_version = load_schema_version(connection)?;
    }

    if schema_version.as_deref() == Some("3") {
        connection.execute_batch(CONNECTOME_MIGRATION)?;
        schema_version = load_schema_version(connection)?;
    }

    match schema_version.as_deref() {
        Some(LATEST_SCHEMA_VERSION) => Ok(()),
        Some(actual) => Err(StorageError::InvalidSchemaMeta {
            key: "schema_version",
            expected: "1, 2, 3, or 4".to_owned(),
            actual: Some(actual.to_owned()),
        }),
        None => Err(StorageError::MissingSchemaMeta {
            key: "schema_version",
        }),
    }
}

fn load_schema_version(connection: &Connection) -> StorageResult<Option<String>> {
    connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(Into::into)
}
