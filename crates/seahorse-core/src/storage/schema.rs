use std::collections::HashMap;

use rusqlite::Connection;

use super::{StorageError, StorageResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaExpectation {
    pub schema_version: String,
    pub index_version: String,
    pub embedding_model_id: String,
    pub embedding_dimension: usize,
}

impl SchemaExpectation {
    pub fn new(
        schema_version: impl Into<String>,
        index_version: impl Into<String>,
        embedding_model_id: impl Into<String>,
        embedding_dimension: usize,
    ) -> Self {
        Self {
            schema_version: schema_version.into(),
            index_version: index_version.into(),
            embedding_model_id: embedding_model_id.into(),
            embedding_dimension,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaMetaSnapshot {
    pub schema_version: String,
    pub index_version: String,
    pub embedding_model_id: String,
    pub embedding_dimension: usize,
    pub index_state: Option<String>,
}

pub fn read_schema_meta(connection: &Connection) -> StorageResult<SchemaMetaSnapshot> {
    let values = load_schema_meta_map(connection)?;

    Ok(SchemaMetaSnapshot {
        schema_version: required_value(&values, "schema_version")?,
        index_version: required_value(&values, "index_version")?,
        embedding_model_id: required_value(&values, "embedding_model_id")?,
        embedding_dimension: parse_dimension(&values, "embedding_dimension")?,
        index_state: values.get("index_state").cloned(),
    })
}

pub fn validate_schema_meta(
    connection: &Connection,
    expected: &SchemaExpectation,
) -> StorageResult<SchemaMetaSnapshot> {
    let snapshot = read_schema_meta(connection)?;

    ensure_exact_match(
        "schema_version",
        &expected.schema_version,
        &snapshot.schema_version,
    )?;
    ensure_exact_match("index_version", &expected.index_version, &snapshot.index_version)?;
    ensure_exact_match(
        "embedding_model_id",
        &expected.embedding_model_id,
        &snapshot.embedding_model_id,
    )?;

    if snapshot.embedding_dimension != expected.embedding_dimension {
        return Err(StorageError::InvalidSchemaMeta {
            key: "embedding_dimension",
            expected: expected.embedding_dimension.to_string(),
            actual: Some(snapshot.embedding_dimension.to_string()),
        });
    }

    Ok(snapshot)
}

fn load_schema_meta_map(connection: &Connection) -> StorageResult<HashMap<String, String>> {
    let mut statement = connection.prepare("SELECT key, value FROM schema_meta")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut values = HashMap::new();
    for row in rows {
        let (key, value) = row?;
        values.insert(key, value);
    }
    Ok(values)
}

fn required_value(values: &HashMap<String, String>, key: &'static str) -> StorageResult<String> {
    values
        .get(key)
        .cloned()
        .ok_or(StorageError::MissingSchemaMeta { key })
}

fn parse_dimension(values: &HashMap<String, String>, key: &'static str) -> StorageResult<usize> {
    let value = required_value(values, key)?;
    value
        .parse::<usize>()
        .map_err(|_| StorageError::InvalidSchemaMeta {
            key,
            expected: "usize".to_owned(),
            actual: Some(value),
        })
}

fn ensure_exact_match(key: &'static str, expected: &str, actual: &str) -> StorageResult<()> {
    if expected == actual {
        return Ok(());
    }

    Err(StorageError::InvalidSchemaMeta {
        key,
        expected: expected.to_owned(),
        actual: Some(actual.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{read_schema_meta, validate_schema_meta, SchemaExpectation};
    use crate::storage::{apply_sqlite_migrations, StorageError, LATEST_SCHEMA_VERSION};

    fn migrated_connection() -> Connection {
        let connection = Connection::open_in_memory().expect("in-memory sqlite");
        apply_sqlite_migrations(&connection).expect("apply migration");
        connection
    }

    #[test]
    fn reads_seeded_schema_meta() {
        let connection = migrated_connection();
        let snapshot = read_schema_meta(&connection).expect("read schema_meta");

        assert_eq!(snapshot.schema_version, LATEST_SCHEMA_VERSION);
        assert_eq!(snapshot.index_version, "1");
        assert_eq!(snapshot.embedding_model_id, "unknown");
        assert_eq!(snapshot.embedding_dimension, 0);
        assert_eq!(snapshot.index_state, None);
    }

    #[test]
    fn validates_schema_meta_against_expected_values() {
        let connection = migrated_connection();
        let expected = SchemaExpectation::new(LATEST_SCHEMA_VERSION, "1", "unknown", 0);

        let snapshot = validate_schema_meta(&connection, &expected).expect("schema valid");
        assert_eq!(snapshot.embedding_dimension, 0);
    }

    #[test]
    fn reports_schema_mismatch_clearly() {
        let connection = migrated_connection();
        connection
            .execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                ["2"],
            )
            .expect("update schema version");

        let error =
            validate_schema_meta(
                &connection,
                &SchemaExpectation::new(LATEST_SCHEMA_VERSION, "1", "unknown", 0),
            )
                .expect_err("schema should mismatch");

        match error {
            StorageError::InvalidSchemaMeta {
                key,
                expected,
                actual,
            } => {
                assert_eq!(key, "schema_version");
                assert_eq!(expected, "1");
                assert_eq!(actual.as_deref(), Some("2"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
