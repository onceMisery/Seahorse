#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThalamusConfig {
    pub default_worldview: String,
}

impl Default for ThalamusConfig {
    fn default() -> Self {
        Self {
            default_worldview: "default".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThalamicAnalysis {
    pub worldview: String,
    pub entropy: f32,
}

impl ThalamicAnalysis {
    pub fn open(worldview: &str, entropy: f32) -> Self {
        Self {
            worldview: worldview.to_owned(),
            entropy,
        }
    }
}

#[derive(Debug)]
pub struct Thalamus {
    config: ThalamusConfig,
}

impl Thalamus {
    pub fn new(config: ThalamusConfig) -> Self {
        Self { config }
    }

    pub fn analyze(&self, _query: &str, depth: usize) -> ThalamicAnalysis {
        let bounded_depth = depth.max(1) as f32;
        ThalamicAnalysis::open(&self.config.default_worldview, 1.0 / (bounded_depth + 1.0))
    }
}
