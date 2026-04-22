use std::fmt;

#[derive(Debug)]
pub enum StorageError {
    Sqlite(rusqlite::Error),
    MissingSchemaMeta {
        key: &'static str,
    },
    InvalidSchemaMeta {
        key: &'static str,
        expected: String,
        actual: Option<String>,
    },
    InvalidBatchReference {
        message: String,
    },
}

pub type StorageResult<T> = Result<T, StorageError>;

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(source) => write!(f, "sqlite error: {source}"),
            Self::MissingSchemaMeta { key } => write!(f, "missing schema_meta entry: key={key}"),
            Self::InvalidSchemaMeta {
                key,
                expected,
                actual,
            } => write!(
                f,
                "invalid schema_meta entry: key={key}, expected={expected}, actual={}",
                actual.as_deref().unwrap_or("<missing>")
            ),
            Self::InvalidBatchReference { message } => {
                write!(f, "invalid batch reference: {message}")
            }
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(source) => Some(source),
            Self::MissingSchemaMeta { .. }
            | Self::InvalidSchemaMeta { .. }
            | Self::InvalidBatchReference { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}
