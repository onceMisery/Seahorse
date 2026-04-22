use super::hnsw::{BootstrapHnswEntry, BootstrapHnswIndex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CortexArchiveHeader {
    pub version: u32,
    pub dimension: usize,
}

impl CortexArchiveHeader {
    pub fn new(dimension: usize) -> Self {
        Self {
            version: 1,
            dimension: dimension.max(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CortexArchiveSnapshot {
    pub header: CortexArchiveHeader,
    pub entries: Vec<BootstrapHnswEntry>,
}

impl CortexArchiveSnapshot {
    pub fn new(dimension: usize, entries: Vec<BootstrapHnswEntry>) -> Self {
        Self {
            header: CortexArchiveHeader::new(dimension),
            entries,
        }
    }

    pub fn from_index(index: &BootstrapHnswIndex) -> Self {
        Self::new(index.dimension(), index.snapshot_entries())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut lines = vec![format!(
            "v1|{}|{}",
            self.header.dimension,
            self.entries.len()
        )];
        for entry in &self.entries {
            let vector = entry
                .vector
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",");
            lines.push(format!("{}|{}|{}", entry.chunk_id, entry.namespace, vector));
        }
        lines.join("\n").into_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CortexArchiveError> {
        let text =
            String::from_utf8(bytes.to_vec()).map_err(|_| CortexArchiveError::InvalidFormat {
                message: "archive is not valid utf-8".to_owned(),
            })?;
        let mut lines = text.lines();
        let header_line = lines
            .next()
            .ok_or_else(|| CortexArchiveError::InvalidFormat {
                message: "archive header is missing".to_owned(),
            })?;
        let header_parts = header_line.split('|').collect::<Vec<_>>();
        if header_parts.len() != 3 || header_parts[0] != "v1" {
            return Err(CortexArchiveError::InvalidFormat {
                message: "archive header is invalid".to_owned(),
            });
        }

        let dimension =
            header_parts[1]
                .parse::<usize>()
                .map_err(|_| CortexArchiveError::InvalidFormat {
                    message: "archive dimension is invalid".to_owned(),
                })?;
        let expected_entries =
            header_parts[2]
                .parse::<usize>()
                .map_err(|_| CortexArchiveError::InvalidFormat {
                    message: "archive entry count is invalid".to_owned(),
                })?;

        let mut entries = Vec::with_capacity(expected_entries);
        for line in lines {
            let parts = line.split('|').collect::<Vec<_>>();
            if parts.len() != 3 {
                return Err(CortexArchiveError::InvalidFormat {
                    message: "archive entry is invalid".to_owned(),
                });
            }

            let chunk_id =
                parts[0]
                    .parse::<i64>()
                    .map_err(|_| CortexArchiveError::InvalidFormat {
                        message: "archive chunk id is invalid".to_owned(),
                    })?;
            let vector = if parts[2].is_empty() {
                Vec::new()
            } else {
                parts[2]
                    .split(',')
                    .map(|value| {
                        value
                            .parse::<f32>()
                            .map_err(|_| CortexArchiveError::InvalidFormat {
                                message: "archive vector value is invalid".to_owned(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            if vector.len() != dimension {
                return Err(CortexArchiveError::InvalidFormat {
                    message: format!(
                        "archive vector dimension mismatch: expected {}, actual {}",
                        dimension,
                        vector.len()
                    ),
                });
            }

            entries.push(BootstrapHnswEntry {
                chunk_id,
                namespace: parts[1].to_owned(),
                vector,
            });
        }

        if entries.len() != expected_entries {
            return Err(CortexArchiveError::InvalidFormat {
                message: format!(
                    "archive entry count mismatch: expected {}, actual {}",
                    expected_entries,
                    entries.len()
                ),
            });
        }

        Ok(Self::new(dimension, entries))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CortexArchiveError {
    InvalidFormat { message: String },
}

impl std::fmt::Display for CortexArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat { message } => write!(f, "invalid cortex archive: {message}"),
        }
    }
}

impl std::error::Error for CortexArchiveError {}

#[cfg(test)]
mod tests {
    use super::CortexArchiveSnapshot;
    use crate::cortex::hnsw::{BootstrapHnswConfig, BootstrapHnswIndex};
    use crate::index::{IndexEntry, SearchRequest};

    #[test]
    fn round_trips_cortex_archive_snapshot() {
        let mut index = BootstrapHnswIndex::new(BootstrapHnswConfig::new(3));
        index
            .insert(&[
                IndexEntry::new(10, "default", vec![1.0, 0.0, 0.0]),
                IndexEntry::new(11, "default", vec![0.0, 1.0, 0.0]),
            ])
            .expect("insert bootstrap hnsw entries");

        let snapshot = CortexArchiveSnapshot::from_index(&index);
        let encoded = snapshot.to_bytes();
        let restored_snapshot =
            CortexArchiveSnapshot::from_bytes(&encoded).expect("decode archive snapshot");
        let mut restored =
            BootstrapHnswIndex::new(BootstrapHnswConfig::new(restored_snapshot.header.dimension));
        restored
            .rebuild_from_snapshot(&restored_snapshot.entries)
            .expect("restore snapshot into index");

        let hits = restored
            .search(&SearchRequest::new("default", vec![1.0, 0.0, 0.0], 2))
            .expect("search restored bootstrap hnsw entries");

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_id, 10);
    }
}
