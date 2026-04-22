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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CortexArchiveSnapshot {
    pub header: CortexArchiveHeader,
    pub entry_count: usize,
}

impl CortexArchiveSnapshot {
    pub fn new(dimension: usize, entry_count: usize) -> Self {
        Self {
            header: CortexArchiveHeader::new(dimension),
            entry_count,
        }
    }
}
