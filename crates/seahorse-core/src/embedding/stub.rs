use super::{EmbeddingError, EmbeddingProvider, EmbeddingResult};

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Debug, Clone)]
pub enum StubFailureMode {
    Timeout { timeout_ms: u64 },
    Failure { message: String },
    DimensionMismatch { actual: usize },
}

#[derive(Debug, Clone)]
pub struct StubEmbeddingProvider {
    model_id: String,
    dimension: usize,
    max_batch_size: usize,
    failure_mode: Option<StubFailureMode>,
}

impl StubEmbeddingProvider {
    pub fn new(
        model_id: impl Into<String>,
        dimension: usize,
        max_batch_size: usize,
    ) -> EmbeddingResult<Self> {
        if dimension == 0 {
            return Err(EmbeddingError::ProviderFailure {
                provider: "stub",
                message: "dimension must be greater than zero".to_owned(),
            });
        }

        Ok(Self {
            model_id: model_id.into(),
            dimension,
            max_batch_size: max_batch_size.max(1),
            failure_mode: None,
        })
    }

    pub fn from_dimension(dimension: usize) -> EmbeddingResult<Self> {
        Self::new(format!("stub-{dimension}d"), dimension, 32)
    }

    pub fn with_failure_mode(mut self, failure_mode: StubFailureMode) -> Self {
        self.failure_mode = Some(failure_mode);
        self
    }

    fn check_failure_mode(&self) -> EmbeddingResult<()> {
        match &self.failure_mode {
            Some(StubFailureMode::Timeout { timeout_ms }) => {
                Err(EmbeddingError::ProviderTimeout {
                    provider: "stub",
                    timeout_ms: *timeout_ms,
                })
            }
            Some(StubFailureMode::Failure { message }) => Err(EmbeddingError::ProviderFailure {
                provider: "stub",
                message: message.clone(),
            }),
            Some(StubFailureMode::DimensionMismatch { actual }) => {
                Err(EmbeddingError::DimensionMismatch {
                    expected: self.dimension,
                    actual: *actual,
                })
            }
            None => Ok(()),
        }
    }

    fn embed_internal(&self, text: &str) -> Vec<f32> {
        let mut values = Vec::with_capacity(self.dimension);
        let normalized = if text.is_empty() { "<empty>" } else { text };

        for index in 0..self.dimension {
            let seed = format!("{}:{index}", normalized);
            values.push(hash_to_unit_f32(seed.as_bytes()));
        }

        values
    }
}

impl Default for StubEmbeddingProvider {
    fn default() -> Self {
        Self {
            model_id: "stub-8d".to_owned(),
            dimension: 8,
            max_batch_size: 32,
            failure_mode: None,
        }
    }
}

impl EmbeddingProvider for StubEmbeddingProvider {
    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        self.check_failure_mode()?;
        Ok(self.embed_internal(text))
    }

    fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        self.check_failure_mode()?;
        if texts.len() > self.max_batch_size {
            return Err(EmbeddingError::ProviderFailure {
                provider: "stub",
                message: format!(
                    "batch size {} exceeds max_batch_size {}",
                    texts.len(),
                    self.max_batch_size
                ),
            });
        }

        Ok(texts
            .iter()
            .map(|text| self.embed_internal(text))
            .collect::<Vec<_>>())
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn max_batch_size(&self) -> usize {
        self.max_batch_size
    }
}

fn hash_to_unit_f32(bytes: &[u8]) -> f32 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    let ratio = (hash as f64) / (u64::MAX as f64);
    (ratio as f32) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::StubEmbeddingProvider;
    use crate::embedding::EmbeddingProvider;

    #[test]
    fn stub_provider_is_deterministic() {
        let provider = StubEmbeddingProvider::from_dimension(4).expect("stub provider");
        let first = provider.embed("seahorse").expect("first embedding");
        let second = provider.embed("seahorse").expect("second embedding");

        assert_eq!(first, second);
        assert_eq!(first.len(), 4);
    }

    #[test]
    fn stub_provider_embeds_batches() {
        let provider = StubEmbeddingProvider::default();
        let batch = vec!["alpha".to_owned(), "beta".to_owned()];

        let embeddings = provider.embed_batch(&batch).expect("batch embedding");
        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0].len(), provider.dimension());
    }
}
