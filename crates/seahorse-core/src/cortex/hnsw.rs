#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapHnswConfig {
    pub dimension: usize,
}

impl BootstrapHnswConfig {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension: dimension.max(1),
        }
    }
}

#[derive(Debug)]
pub struct BootstrapHnswIndex {
    config: BootstrapHnswConfig,
}

impl BootstrapHnswIndex {
    pub fn new(config: BootstrapHnswConfig) -> Self {
        Self { config }
    }

    pub fn dimension(&self) -> usize {
        self.config.dimension
    }
}
