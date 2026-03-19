/// Stable, deterministic hashing helpers used by the pipeline.
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Compute FNV-1a 64-bit hash from UTF-8 bytes.
pub fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Return the lowercase hex encoding of the stable hash.
pub fn stable_content_hash(text: &str) -> String {
    format!("{:016x}", fnv1a_hash(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{fnv1a_hash, stable_content_hash};

    #[test]
    fn deterministic_hash() {
        let a = "chunk";
        let b = "chunk";
        assert_eq!(fnv1a_hash(a.as_bytes()), fnv1a_hash(b.as_bytes()));
        assert_eq!(stable_content_hash(a), stable_content_hash(b));
    }
}
