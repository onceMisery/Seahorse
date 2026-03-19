pub mod error;
pub mod provider;
pub mod stub;

pub use error::EmbeddingError;
pub use provider::{EmbeddingProvider, EmbeddingResult};
pub use stub::StubEmbeddingProvider;
