use std::collections::{BTreeMap, HashMap};

use crate::storage::TagWrite;

const AUTO_TAG_LIMIT: usize = 8;
const MIN_AUTO_TAG_LENGTH: usize = 3;
const MAX_AUTO_TAG_LENGTH: usize = 32;
const STOPWORDS: &[&str] = &[
    "about", "after", "again", "also", "and", "before", "being", "between", "brief", "but", "can",
    "content", "default", "from", "have", "into", "just", "main", "more", "only", "other", "over",
    "same", "some", "that", "their", "there", "these", "they", "this", "those", "with", "without",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedTag {
    pub(crate) name: String,
    pub(crate) normalized_name: String,
    pub(crate) source: &'static str,
}

impl ResolvedTag {
    pub(crate) fn to_tag_write(&self, namespace: &str) -> TagWrite {
        let mut tag = TagWrite::new(self.name.clone(), self.normalized_name.clone());
        tag.namespace = namespace.to_owned();
        tag
    }
}

pub(crate) fn resolve_tags(
    explicit_tags: &[String],
    content: &str,
    source_uri: Option<&str>,
    auto_tag: bool,
) -> Vec<ResolvedTag> {
    let mut resolved = normalize_explicit_tags(explicit_tags);
    if !auto_tag {
        return resolved.into_values().collect();
    }

    let remaining = AUTO_TAG_LIMIT.saturating_sub(resolved.len());
    if remaining == 0 {
        return resolved.into_values().collect();
    }

    for candidate in extract_auto_tag_candidates(content, source_uri)
        .into_iter()
        .take(remaining)
    {
        resolved.entry(candidate.clone()).or_insert(ResolvedTag {
            name: candidate.clone(),
            normalized_name: candidate,
            source: "auto",
        });
    }

    resolved.into_values().collect()
}

fn normalize_explicit_tags(tags: &[String]) -> BTreeMap<String, ResolvedTag> {
    let mut normalized = BTreeMap::new();

    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }

        let key = trimmed.to_ascii_lowercase();
        normalized.entry(key.clone()).or_insert(ResolvedTag {
            name: trimmed.to_owned(),
            normalized_name: key,
            source: "explicit",
        });
    }

    normalized
}

fn extract_auto_tag_candidates(content: &str, source_uri: Option<&str>) -> Vec<String> {
    let mut scores = HashMap::<String, usize>::new();
    accumulate_tokens(&mut scores, content, 1);

    if let Some(source_uri) = source_uri {
        accumulate_tokens(&mut scores, source_uri, 2);
        if source_uri.contains("github.com") {
            scores.entry("github".to_owned()).or_insert(3);
        }
    }

    if content.contains("apiVersion:") && content.contains("kind:") {
        scores.entry("kubernetes".to_owned()).or_insert(4);
    }

    let mut candidates = scores.into_iter().collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    candidates.into_iter().map(|(token, _)| token).collect()
}

fn accumulate_tokens(scores: &mut HashMap<String, usize>, input: &str, weight: usize) {
    for token in tokenize(input) {
        if !should_keep_auto_tag(&token) {
            continue;
        }

        let entry = scores.entry(token).or_insert(0);
        *entry = entry.saturating_add(weight);
    }
}

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
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

fn should_keep_auto_tag(token: &str) -> bool {
    if token.len() < MIN_AUTO_TAG_LENGTH || token.len() > MAX_AUTO_TAG_LENGTH {
        return false;
    }

    if token.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }

    !STOPWORDS.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::resolve_tags;

    #[test]
    fn keeps_explicit_tags_and_adds_ranked_auto_tags() {
        let tags = resolve_tags(
            &["Project".to_owned()],
            "project atlas atlas orbit signal signal signal timeline",
            Some("https://github.com/acme/seahorse"),
            true,
        );

        let normalized = tags
            .iter()
            .map(|tag| (tag.normalized_name.as_str(), tag.source))
            .collect::<Vec<_>>();

        assert!(normalized.contains(&("project", "explicit")));
        assert!(normalized.contains(&("github", "auto")));
        assert!(normalized.contains(&("signal", "auto")));
    }

    #[test]
    fn does_not_duplicate_auto_tag_that_matches_explicit_tag() {
        let tags = resolve_tags(
            &["Atlas".to_owned()],
            "atlas atlas orbit orbit signal",
            None,
            true,
        );

        let atlas = tags
            .iter()
            .filter(|tag| tag.normalized_name == "atlas")
            .collect::<Vec<_>>();
        assert_eq!(atlas.len(), 1);
        assert_eq!(atlas[0].source, "explicit");
    }
}
