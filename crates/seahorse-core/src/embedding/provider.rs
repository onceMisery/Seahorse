use crate::embedding::error::EmbeddingError;

pub type EmbeddingResult<T> = Result<T, EmbeddingError>;

pub trait EmbeddingProvider: Send + Sync {
    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>>;

    fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>>;

    fn model_id(&self) -> &str;

    fn dimension(&self) -> usize;

    fn max_batch_size(&self) -> usize;
}
