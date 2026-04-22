use super::hashing;
use super::preprocessor;

/// A single chunk produced from the chunker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub index: usize,
    pub text: String,
    pub content_hash: String,
}

/// Configuration for chunk generation.
#[derive(Debug, Clone, Copy)]
pub struct ChunkerConfig {
    /// Maximum number of characters per chunk.
    pub max_chars: usize,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self { max_chars: 512 }
    }
}

impl ChunkerConfig {
    fn effective_size(&self) -> usize {
        if self.max_chars == 0 {
            1
        } else {
            self.max_chars
        }
    }
}

/// Produce deterministic chunks from normalized text.
pub fn chunk_text(input: &str, config: ChunkerConfig) -> Vec<Chunk> {
    let normalized = preprocessor::normalize_text(input);
    let mut chunks = Vec::new();
    let mut buffer = String::with_capacity(config.effective_size());
    let mut current_chars = 0;
    let chunk_limit = config.effective_size();

    for ch in normalized.chars() {
        buffer.push(ch);
        current_chars += 1;

        if current_chars >= chunk_limit {
            emit_chunk(&mut chunks, &buffer);
            buffer.clear();
            current_chars = 0;
        }
    }

    if !buffer.is_empty() {
        emit_chunk(&mut chunks, &buffer);
    }

    chunks
}

fn emit_chunk(chunks: &mut Vec<Chunk>, text: &str) {
    let index = chunks.len();
    let content_hash = hashing::stable_content_hash(text);
    chunks.push(Chunk {
        index,
        text: text.to_owned(),
        content_hash,
    });
}

#[cfg(test)]
mod tests {
    use super::{chunk_text, ChunkerConfig};

    #[test]
    fn chunks_are_ordered_and_stable() {
        let config = ChunkerConfig { max_chars: 3 };
        let result = chunk_text("abcdef", config);
        let hashes: Vec<_> = result
            .iter()
            .map(|chunk| chunk.content_hash.clone())
            .collect();
        assert_eq!(hashes, hashes.clone());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "abc");
        assert_eq!(result[1].text, "def");
    }
}
