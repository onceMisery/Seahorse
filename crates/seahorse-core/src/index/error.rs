use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    DimensionMismatch {
        expected: usize,
        actual: usize,
    },
    InvalidTopK {
        top_k: usize,
    },
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionMismatch { expected, actual } => {
                write!(f, "index dimension mismatch: expected={expected}, actual={actual}")
            }
            Self::InvalidTopK { top_k } => write!(f, "invalid top_k: {top_k}"),
        }
    }
}

impl std::error::Error for IndexError {}
