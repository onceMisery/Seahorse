pub mod archive;
pub mod hnsw;

use archive::CortexArchiveHeader;
use hnsw::{BootstrapHnswConfig, BootstrapHnswIndex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CortexConfig {
    pub dimension: usize,
}

impl CortexConfig {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension: dimension.max(1),
        }
    }
}

#[derive(Debug)]
pub struct Cortex {
    config: CortexConfig,
    index: BootstrapHnswIndex,
}

impl Cortex {
    pub fn new(config: CortexConfig) -> Self {
        let index = BootstrapHnswIndex::new(BootstrapHnswConfig::new(config.dimension));
        Self { config, index }
    }

    pub fn config(&self) -> &CortexConfig {
        &self.config
    }

    pub fn backend_name(&self) -> &'static str {
        "bootstrap-hnsw"
    }

    pub fn archive_header(&self) -> CortexArchiveHeader {
        CortexArchiveHeader::new(self.index.dimension())
    }
}
