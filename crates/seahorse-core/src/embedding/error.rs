use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingError {
    ProviderTimeout {
        provider: &'static str,
        timeout_ms: u64,
    },
    ProviderFailure {
        provider: &'static str,
        message: String,
    },
    DimensionMismatch {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderTimeout {
                provider,
                timeout_ms,
            } => write!(
                f,
                "embedding provider timed out: provider={provider}, timeout_ms={timeout_ms}"
            ),
            Self::ProviderFailure { provider, message } => {
                write!(f, "embedding provider failed: provider={provider}, message={message}")
            }
            Self::DimensionMismatch { expected, actual } => {
                write!(f, "embedding dimension mismatch: expected={expected}, actual={actual}")
            }
        }
    }
}

impl std::error::Error for EmbeddingError {}