use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const MAX_MERGED_MESSAGES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationMerge {
    pub blocks: Vec<String>,
    pub had_existing: bool,
    pub inserted_new_block: bool,
}

impl NotificationMerge {
    pub fn message(&self) -> String {
        self.blocks.join("\n\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotificationEntry {
    sequence_id: String,
    messages: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub last_modified: Option<String>,
    #[serde(default)]
    seen: VecDeque<String>,
    #[serde(default)]
    notifications: VecDeque<NotificationEntry>,
    #[serde(skip)]
    seen_index: HashSet<String>,
    #[serde(skip)]
    notification_index: HashMap<String, Vec<String>>,
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
        state.rebuild_notification_index();
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

    pub fn merge_notification(
        &self,
        sequence_id: &str,
        incoming_message: &str,
    ) -> NotificationMerge {
        let incoming_message = incoming_message.trim();
        let Some(existing) = self.notification_index.get(sequence_id) else {
            return NotificationMerge {
                blocks: vec![String::from(incoming_message)],
                had_existing: false,
                inserted_new_block: true,
            };
        };

        if existing.iter().any(|block| block == incoming_message) {
            return NotificationMerge {
                blocks: existing.clone(),
                had_existing: true,
                inserted_new_block: false,
            };
        }

        let mut blocks = Vec::with_capacity((existing.len() + 1).min(MAX_MERGED_MESSAGES));
        blocks.push(String::from(incoming_message));
        blocks.extend(existing.iter().take(MAX_MERGED_MESSAGES - 1).cloned());

        NotificationMerge {
            blocks,
            had_existing: true,
            inserted_new_block: true,
        }
    }

    pub fn remember_notification(
        &mut self,
        sequence_id: String,
        messages: Vec<String>,
        max_seen: usize,
    ) {
        if let Some(entry) = self
            .notifications
            .iter_mut()
            .find(|entry| entry.sequence_id == sequence_id)
        {
            entry.messages = messages.clone();
            self.notification_index.insert(sequence_id, messages);
            return;
        }

        self.notifications.push_back(NotificationEntry {
            sequence_id: sequence_id.clone(),
            messages: messages.clone(),
        });
        self.notification_index.insert(sequence_id, messages);
        while self.notifications.len() > max_seen {
            if let Some(removed) = self.notifications.pop_front() {
                self.notification_index.remove(&removed.sequence_id);
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

    fn rebuild_notification_index(&mut self) {
        let mut notification_index = HashMap::with_capacity(self.notifications.len());
        let mut deduped = VecDeque::with_capacity(self.notifications.len());
        for entry in self.notifications.drain(..) {
            if notification_index.contains_key(&entry.sequence_id) {
                continue;
            }

            let messages = entry
                .messages
                .into_iter()
                .take(MAX_MERGED_MESSAGES)
                .collect::<Vec<_>>();
            notification_index.insert(entry.sequence_id.clone(), messages.clone());
            deduped.push_back(NotificationEntry {
                sequence_id: entry.sequence_id,
                messages,
            });
        }
        self.notifications = deduped;
        self.notification_index = notification_index;
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

    #[test]
    fn merges_new_notification_blocks_newest_first() {
        let mut state = State::default();
        state.remember_notification(
            String::from("github-thread-1"),
            vec![String::from("older"), String::from("oldest")],
            10,
        );

        let merged = state.merge_notification("github-thread-1", "newest");

        assert!(merged.had_existing);
        assert!(merged.inserted_new_block);
        assert_eq!(
            merged.blocks,
            vec![
                String::from("newest"),
                String::from("older"),
                String::from("oldest")
            ]
        );
        assert_eq!(merged.message(), "newest\n\nolder\n\noldest");
    }

    #[test]
    fn skips_duplicate_notification_blocks() {
        let mut state = State::default();
        state.remember_notification(
            String::from("github-thread-1"),
            vec![String::from("same"), String::from("older")],
            10,
        );

        let merged = state.merge_notification("github-thread-1", "same");

        assert!(merged.had_existing);
        assert!(!merged.inserted_new_block);
        assert_eq!(
            merged.blocks,
            vec![String::from("same"), String::from("older")]
        );
    }
}
