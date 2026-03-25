use std::collections::VecDeque;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub last_modified: Option<String>,
    #[serde(default)]
    seen: VecDeque<String>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read state: {}", path.display()))?;
        let state = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse state: {}", path.display()))?;
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
        self.seen.iter().any(|seen| seen == key)
    }

    pub fn mark_seen(&mut self, key: String, max_seen: usize) {
        if self.has_seen(&key) {
            return;
        }

        self.seen.push_back(key);
        while self.seen.len() > max_seen {
            self.seen.pop_front();
        }
    }
}
