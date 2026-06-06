use anyhow::Result;

const FORCED_EXCLUDES: &[&str] = &["devsync.toml", ".devsync", ".git"];

#[derive(Debug, Clone)]
pub struct ExcludeMatcher {
    patterns: Vec<String>,
}

impl ExcludeMatcher {
    pub fn new(patterns: Vec<String>) -> Result<Self> {
        Ok(Self { patterns })
    }

    pub fn is_excluded(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        FORCED_EXCLUDES.iter().any(|pattern| matches_pattern(&path, pattern))
            || self.patterns.iter().any(|pattern| matches_pattern(&path, pattern))
    }
}

fn matches_pattern(path: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_matches('/').replace('\\', "/");
    path == pattern || path.starts_with(&format!("{pattern}/")) || path.split('/').any(|part| part == pattern)
}
