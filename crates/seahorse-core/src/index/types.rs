#[derive(Debug, Clone, PartialEq)]
pub struct IndexEntry {
    pub chunk_id: i64,
    pub namespace: String,
    pub vector: Vec<f32>,
}

impl IndexEntry {
    pub fn new(chunk_id: i64, namespace: impl Into<String>, vector: Vec<f32>) -> Self {
        Self {
            chunk_id,
            namespace: namespace.into(),
            vector,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchRequest {
    pub namespace: String,
    pub query_vector: Vec<f32>,
    pub top_k: usize,
}

impl SearchRequest {
    pub fn new(namespace: impl Into<String>, query_vector: Vec<f32>, top_k: usize) -> Self {
        Self {
            namespace: namespace.into(),
            query_vector,
            top_k,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub chunk_id: i64,
    pub namespace: String,
    pub score: f32,
}
