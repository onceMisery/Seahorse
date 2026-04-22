use std::collections::BTreeMap;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThalamicRoute {
    pub allow_association: bool,
    pub association_reason: String,
    pub allow_weak_signal: bool,
    pub weak_signal_reason: String,
}

impl ThalamicRoute {
    pub fn new(
        allow_association: bool,
        association_reason: impl Into<String>,
        allow_weak_signal: bool,
        weak_signal_reason: impl Into<String>,
    ) -> Self {
        Self {
            allow_association,
            association_reason: association_reason.into(),
            allow_weak_signal,
            weak_signal_reason: weak_signal_reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThalamicAnalysis {
    pub worldview: String,
    pub entropy: f32,
    pub route: ThalamicRoute,
}

impl ThalamicAnalysis {
    pub fn open(worldview: &str, entropy: f32) -> Self {
        Self {
            worldview: worldview.to_owned(),
            entropy,
            route: decide_route(worldview, entropy),
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

    pub fn analyze(&self, query: &str, depth: usize) -> ThalamicAnalysis {
        let normalized_tokens = tokenize(query);
        let worldview = classify_worldview(&self.config.default_worldview, &normalized_tokens);
        let entropy = estimate_entropy(&normalized_tokens, depth);

        ThalamicAnalysis::open(&worldview, entropy)
    }
}

fn classify_worldview(default_worldview: &str, normalized_tokens: &[String]) -> String {
    if normalized_tokens.is_empty() {
        return default_worldview.to_owned();
    }

    if normalized_tokens
        .iter()
        .any(|token| TECHNICAL_KEYWORDS.contains(&token.as_str()))
    {
        return "technical".to_owned();
    }

    if normalized_tokens
        .iter()
        .any(|token| CREATIVE_KEYWORDS.contains(&token.as_str()))
    {
        return "creative".to_owned();
    }

    if normalized_tokens
        .iter()
        .any(|token| EMOTIONAL_KEYWORDS.contains(&token.as_str()))
    {
        return "emotional".to_owned();
    }

    default_worldview.to_owned()
}

fn estimate_entropy(normalized_tokens: &[String], depth: usize) -> f32 {
    if normalized_tokens.is_empty() {
        return 0.0;
    }

    let mut counts = BTreeMap::<&str, usize>::new();
    for token in normalized_tokens {
        *counts.entry(token.as_str()).or_insert(0) += 1;
    }

    let total = normalized_tokens.len() as f32;
    let unique = counts.len() as f32;
    let mut entropy = 0.0_f32;
    for count in counts.into_values() {
        let probability = count as f32 / total;
        entropy -= probability * probability.ln();
    }

    let normalized_entropy = if unique <= 1.0 {
        0.0
    } else {
        entropy / unique.ln()
    };
    let depth_factor = 1.0 / (depth.max(1) as f32).sqrt();

    (normalized_entropy * depth_factor).clamp(0.0, 1.0)
}

fn tokenize(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in query.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
            continue;
        }

        flush_token(&mut tokens, &mut current);
    }

    flush_token(&mut tokens, &mut current);
    tokens
}

fn flush_token(tokens: &mut Vec<String>, current: &mut String) {
    if current.is_empty() {
        return;
    }

    tokens.push(std::mem::take(current));
}

const TECHNICAL_KEYWORDS: &[&str] = &[
    "api",
    "database",
    "embedding",
    "index",
    "memory",
    "pipeline",
    "rust",
    "schema",
    "sql",
    "vector",
];

const CREATIVE_KEYWORDS: &[&str] = &[
    "brainstorm",
    "concept",
    "create",
    "creative",
    "design",
    "idea",
    "invent",
    "story",
];

const EMOTIONAL_KEYWORDS: &[&str] = &[
    "care", "emotion", "empathy", "feel", "feeling", "grief", "happy", "sad",
];
const ASSOCIATION_ENTROPY_THRESHOLD: f32 = 0.85;

fn decide_route(worldview: &str, entropy: f32) -> ThalamicRoute {
    let (allow_association, association_reason) = if worldview == "emotional" {
        (false, "worldview_emotional_blocks_association".to_owned())
    } else if entropy >= ASSOCIATION_ENTROPY_THRESHOLD {
        (false, "entropy_above_association_threshold".to_owned())
    } else {
        (true, "tagmemo_allowed".to_owned())
    };

    ThalamicRoute::new(
        allow_association,
        association_reason,
        false,
        "tide_not_implemented",
    )
}

#[cfg(test)]
mod tests {
    use super::{ThalamicAnalysis, ThalamicRoute, Thalamus, ThalamusConfig};

    #[test]
    fn classifies_technical_queries() {
        let thalamus = Thalamus::new(ThalamusConfig::default());

        let analysis = thalamus.analyze("rust vector index pipeline", 2);

        assert_eq!(analysis.worldview, "technical");
        assert!(analysis.entropy > 0.0);
        assert!(analysis.entropy <= 1.0);
        assert_eq!(
            analysis.route,
            ThalamicRoute::new(true, "tagmemo_allowed", false, "tide_not_implemented")
        );
    }

    #[test]
    fn classifies_creative_queries() {
        let thalamus = Thalamus::new(ThalamusConfig::default());

        let analysis = thalamus.analyze("story idea design concept", 2);

        assert_eq!(analysis.worldview, "creative");
        assert!(analysis.entropy > 0.0);
        assert!(analysis.route.allow_association);
    }

    #[test]
    fn falls_back_to_default_worldview() {
        let thalamus = Thalamus::new(ThalamusConfig::default());

        let analysis = thalamus.analyze("alpha recall log", 2);

        assert_eq!(
            analysis,
            ThalamicAnalysis::open("default", analysis.entropy)
        );
    }

    #[test]
    fn blocks_association_for_emotional_queries() {
        let thalamus = Thalamus::new(ThalamusConfig::default());

        let analysis = thalamus.analyze("feel grief and care", 2);

        assert_eq!(analysis.worldview, "emotional");
        assert!(!analysis.route.allow_association);
        assert_eq!(
            analysis.route.association_reason,
            "worldview_emotional_blocks_association"
        );
        assert!(!analysis.route.allow_weak_signal);
        assert_eq!(analysis.route.weak_signal_reason, "tide_not_implemented");
    }

    #[test]
    fn blocks_association_when_entropy_is_high() {
        let thalamus = Thalamus::new(ThalamusConfig::default());

        let analysis = thalamus.analyze("alpha beta gamma delta epsilon", 1);

        assert_eq!(analysis.worldview, "default");
        assert!(analysis.entropy >= 0.85);
        assert!(!analysis.route.allow_association);
        assert_eq!(
            analysis.route.association_reason,
            "entropy_above_association_threshold"
        );
    }
}
