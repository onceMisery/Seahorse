use crate::cerebellum::Cerebellum;
use crate::cortex::Cortex;
use crate::hippocampus::Hippocampus;
use crate::synapse::Synapse;
use crate::thalamus::Thalamus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeahorseEngineConfig {
    pub default_namespace: String,
}

impl Default for SeahorseEngineConfig {
    fn default() -> Self {
        Self {
            default_namespace: "default".to_owned(),
        }
    }
}

#[derive(Debug)]
pub struct SeahorseEngine {
    pub config: SeahorseEngineConfig,
    pub cortex: Cortex,
    pub synapse: Synapse,
    pub thalamus: Thalamus,
    pub hippocampus: Hippocampus,
    pub cerebellum: Cerebellum,
}

impl SeahorseEngine {
    pub fn from_parts(
        config: SeahorseEngineConfig,
        cortex: Cortex,
        synapse: Synapse,
        thalamus: Thalamus,
        hippocampus: Hippocampus,
        cerebellum: Cerebellum,
    ) -> Self {
        Self {
            config,
            cortex,
            synapse,
            thalamus,
            hippocampus,
            cerebellum,
        }
    }
}
