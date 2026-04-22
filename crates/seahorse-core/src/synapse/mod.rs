#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynapseConfig {
    pub max_signals: usize,
}

impl Default for SynapseConfig {
    fn default() -> Self {
        Self { max_signals: 128 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SynapticSignal {
    pub namespace: String,
    pub tag: String,
    pub potential: f32,
}

#[derive(Debug)]
pub struct Synapse {
    config: SynapseConfig,
    signals: Vec<SynapticSignal>,
}

impl Synapse {
    pub fn new(config: SynapseConfig) -> Self {
        Self {
            config,
            signals: Vec::new(),
        }
    }

    pub fn prime(&mut self, namespace: &str, tag: &str, potential: f32) {
        if self.signals.len() >= self.config.max_signals {
            return;
        }

        self.signals.push(SynapticSignal {
            namespace: namespace.to_owned(),
            tag: tag.to_owned(),
            potential,
        });
    }

    pub fn signals(&self) -> &[SynapticSignal] {
        &self.signals
    }
}
