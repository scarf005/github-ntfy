use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub last_modified: Option<String>,
    #[serde(default)]
    seen: VecDeque<String>,
    #[serde(skip)]
    seen_index: HashSet<String>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read state: {}", path.display()))?;
        let mut state: Self = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse state: {}", path.display()))?;
        state.rebuild_seen_index();
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create state directory: {}", parent.display())
            })?;
        }

        let data = serde_json::to_vec_pretty(self).context("failed to encode state")?;
        let temp_path = path.with_extension("json.tmp");
        fs::write(&temp_path, data)
            .with_context(|| format!("failed to write temporary state: {}", temp_path.display()))?;
        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "failed to replace state file: {} -> {}",
                temp_path.display(),
                path.display()
            )
        })?;
        Ok(())
    }

    pub fn has_seen(&self, key: &str) -> bool {
        self.seen_index.contains(key)
    }

    pub fn mark_seen(&mut self, key: String, max_seen: usize) {
        if self.has_seen(&key) {
            return;
        }

        self.seen_index.insert(key.clone());
        self.seen.push_back(key);
        while self.seen.len() > max_seen {
            if let Some(removed) = self.seen.pop_front() {
                self.seen_index.remove(&removed);
            }
        }
    }

    fn rebuild_seen_index(&mut self) {
        let mut seen_index = HashSet::with_capacity(self.seen.len());
        let mut deduped = VecDeque::with_capacity(self.seen.len());
        for key in self.seen.drain(..) {
            if seen_index.insert(key.clone()) {
                deduped.push_back(key);
            }
        }
        self.seen = deduped;
        self.seen_index = seen_index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_path() -> std::path::PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("github-ntfy-agent-state-{suffix}.json"))
    }

    #[test]
    fn marks_seen_in_constant_time_index() {
        let mut state = State::default();

        state.mark_seen(String::from("one"), 3);
        state.mark_seen(String::from("two"), 3);

        assert!(state.has_seen("one"));
        assert!(state.has_seen("two"));
        assert!(!state.has_seen("three"));
    }

    #[test]
    fn evicts_oldest_seen_key() {
        let mut state = State::default();

        state.mark_seen(String::from("one"), 2);
        state.mark_seen(String::from("two"), 2);
        state.mark_seen(String::from("three"), 2);

        assert!(!state.has_seen("one"));
        assert!(state.has_seen("two"));
        assert!(state.has_seen("three"));
    }

    #[test]
    fn rebuilds_seen_index_when_loading_from_disk() {
        let path = unique_test_path();
        fs::write(
            &path,
            r#"{
  "last_modified": "today",
  "seen": ["one", "two"]
}"#,
        )
        .expect("write state");

        let state = State::load(&path).expect("state loaded");

        assert!(state.has_seen("one"));
        assert!(state.has_seen("two"));
        fs::remove_file(path).expect("cleanup");
    }
}
