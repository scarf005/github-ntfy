use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const MAX_MERGED_MESSAGES: usize = 4;
const MAX_MERGED_MESSAGE_BYTES: usize = 3900;
const MERGED_MESSAGE_SEPARATOR: &str = "\n";
const TRUNCATION_SUFFIX: &str = "…";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationMerge {
    pub blocks: Vec<String>,
    pub had_existing: bool,
    pub inserted_new_block: bool,
}

impl NotificationMerge {
    pub fn message(&self) -> String {
        self.blocks.join(MERGED_MESSAGE_SEPARATOR)
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
    #[serde(default)]
    auto_watch_initialized: bool,
    #[serde(default)]
    auto_watched_repositories: VecDeque<String>,
    #[serde(skip)]
    seen_index: HashSet<String>,
    #[serde(skip)]
    notification_index: HashMap<String, Vec<String>>,
    #[serde(skip)]
    auto_watched_repository_index: HashSet<String>,
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
        state.rebuild_auto_watched_repository_index();
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

    pub fn has_notification(&self, sequence_id: &str) -> bool {
        self.notification_index.contains_key(sequence_id)
    }

    pub fn merge_notification(
        &self,
        sequence_id: &str,
        incoming_message: &str,
    ) -> NotificationMerge {
        let incoming_message = truncate_to_bytes(incoming_message.trim());
        let Some(existing) = self.notification_index.get(sequence_id) else {
            return NotificationMerge {
                blocks: fit_notification_blocks(vec![incoming_message]),
                had_existing: false,
                inserted_new_block: true,
            };
        };

        if existing
            .first()
            .is_some_and(|block| block == &incoming_message)
        {
            return NotificationMerge {
                blocks: fit_notification_blocks(existing.clone()),
                had_existing: true,
                inserted_new_block: false,
            };
        }

        let mut blocks = Vec::with_capacity((existing.len() + 1).min(MAX_MERGED_MESSAGES));
        blocks.push(incoming_message.clone());
        blocks.extend(
            existing
                .iter()
                .filter(|block| *block != &incoming_message)
                .take(MAX_MERGED_MESSAGES - 1)
                .cloned(),
        );

        NotificationMerge {
            blocks: fit_notification_blocks(blocks),
            had_existing: true,
            inserted_new_block: true,
        }
    }

    pub fn is_auto_watch_initialized(&self) -> bool {
        self.auto_watch_initialized
    }

    pub fn mark_auto_watch_initialized(&mut self) {
        self.auto_watch_initialized = true;
    }

    pub fn has_auto_watched_repository(&self, full_name: &str) -> bool {
        self.auto_watched_repository_index.contains(full_name)
    }

    pub fn remember_auto_watched_repository(&mut self, full_name: String) {
        if self.has_auto_watched_repository(&full_name) {
            return;
        }

        self.auto_watched_repository_index.insert(full_name.clone());
        self.auto_watched_repositories.push_back(full_name);
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
        rebuild_string_index(&mut self.seen, &mut self.seen_index);
    }

    fn rebuild_auto_watched_repository_index(&mut self) {
        rebuild_string_index(
            &mut self.auto_watched_repositories,
            &mut self.auto_watched_repository_index,
        );
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

fn rebuild_string_index(queue: &mut VecDeque<String>, index: &mut HashSet<String>) {
    *index = HashSet::with_capacity(queue.len());
    queue.retain(|value| index.insert(value.clone()));
}

fn fit_notification_blocks(blocks: Vec<String>) -> Vec<String> {
    let mut fitted = Vec::with_capacity(blocks.len().min(MAX_MERGED_MESSAGES));
    let mut omitted_count = 0usize;

    for block in blocks.into_iter().map(|block| truncate_to_bytes(&block)) {
        if let Some(count) = omitted_marker_count(&block) {
            omitted_count += count;
        } else if message_len_with(&fitted, &block) <= MAX_MERGED_MESSAGE_BYTES {
            fitted.push(block);
        } else {
            omitted_count += 1;
        }
    }

    if omitted_count > 0 {
        append_omitted_history_marker(&mut fitted, omitted_count);
    }

    fitted
}

fn append_omitted_history_marker(blocks: &mut Vec<String>, omitted_count: usize) {
    let marker = format!(
        "[{omitted_count} older update{} omitted]",
        if omitted_count == 1 { "" } else { "s" }
    );

    while blocks.len() > 1 && message_len_with(blocks, &marker) > MAX_MERGED_MESSAGE_BYTES {
        blocks.pop();
    }

    if message_len_with(blocks, &marker) <= MAX_MERGED_MESSAGE_BYTES {
        blocks.push(marker);
    }
}

fn omitted_marker_count(block: &str) -> Option<usize> {
    let count = block.strip_prefix('[')?.strip_suffix(" omitted]")?;
    count
        .strip_suffix(" older updates")
        .or_else(|| count.strip_suffix(" older update"))?
        .parse()
        .ok()
}

fn message_len_with(blocks: &[String], block: &str) -> usize {
    let separator_len = if blocks.is_empty() {
        0
    } else {
        MERGED_MESSAGE_SEPARATOR.len()
    };
    NotificationMerge {
        blocks: blocks.to_vec(),
        had_existing: false,
        inserted_new_block: false,
    }
    .message()
    .len()
        + separator_len
        + block.len()
}

fn truncate_to_bytes(text: &str) -> String {
    if text.len() <= MAX_MERGED_MESSAGE_BYTES {
        return String::from(text);
    }

    let limit = MAX_MERGED_MESSAGE_BYTES.saturating_sub(TRUNCATION_SUFFIX.len());
    let end = text
        .char_indices()
        .map(|(index, character)| index + character.len_utf8())
        .take_while(|end| *end <= limit)
        .last()
        .unwrap_or(0);
    format!("{}{}", &text[..end], TRUNCATION_SUFFIX)
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
    fn tracks_auto_watch_initialization() {
        let mut state = State::default();

        assert!(!state.is_auto_watch_initialized());

        state.mark_auto_watch_initialized();

        assert!(state.is_auto_watch_initialized());
    }

    #[test]
    fn remembers_auto_watched_repositories() {
        let mut state = State::default();

        state.remember_auto_watched_repository(String::from("alice/app"));
        state.remember_auto_watched_repository(String::from("alice/app"));
        state.remember_auto_watched_repository(String::from("alice/cli"));

        assert!(state.has_auto_watched_repository("alice/app"));
        assert!(state.has_auto_watched_repository("alice/cli"));
    }

    #[test]
    fn rebuilds_auto_watch_index_when_loading_from_disk() {
        let path = unique_test_path();
        fs::write(
            &path,
            r#"{
  "auto_watch_initialized": true,
  "auto_watched_repositories": ["alice/app", "alice/app", "alice/cli"]
}"#,
        )
        .expect("write state");

        let state = State::load(&path).expect("state loaded");

        assert!(state.is_auto_watch_initialized());
        assert!(state.has_auto_watched_repository("alice/app"));
        assert!(state.has_auto_watched_repository("alice/cli"));
        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn preserves_existing_notification_history_after_latest_block() {
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
        assert_eq!(merged.message(), "newest\nolder\noldest");
    }

    #[test]
    fn sends_repeated_message_when_it_is_not_the_latest_block() {
        let mut state = State::default();
        state.remember_notification(
            String::from("github-thread-1"),
            vec![String::from("newer"), String::from("same")],
            10,
        );

        let merged = state.merge_notification("github-thread-1", "same");

        assert!(merged.had_existing);
        assert!(merged.inserted_new_block);
        assert_eq!(
            merged.blocks,
            vec![String::from("same"), String::from("newer")]
        );
    }

    #[test]
    fn folds_stored_omitted_marker_into_new_marker() {
        let mut state = State::default();
        state.remember_notification(
            String::from("github-thread-1"),
            vec!["a".repeat(2000), String::from("[1 older update omitted]")],
            10,
        );

        let merged = state.merge_notification("github-thread-1", &"n".repeat(2000));

        assert_eq!(
            merged.blocks,
            vec!["n".repeat(2000), String::from("[2 older updates omitted]")]
        );
    }

    #[test]
    fn keeps_initial_notification_body_under_ntfy_inline_limit() {
        let state = State::default();

        let merged = state.merge_notification("github-thread-1", &"🦀".repeat(2000));

        assert!(!merged.had_existing);
        assert!(merged.message().len() <= 4096);
    }

    #[test]
    fn keeps_merged_notification_body_under_ntfy_inline_limit() {
        let mut state = State::default();
        state.remember_notification(
            String::from("github-thread-1"),
            vec!["older".repeat(1000), "oldest".repeat(1000)],
            10,
        );

        let merged = state.merge_notification("github-thread-1", &"newest".repeat(1000));

        assert!(merged.message().len() <= 4096);
    }

    #[test]
    fn skips_duplicate_truncated_latest_block() {
        let state = State::default();
        let original = "🦀".repeat(2000);
        let first = state.merge_notification("github-thread-1", &original);
        let mut state = State::default();
        state.remember_notification(String::from("github-thread-1"), first.blocks, 10);

        let repeated = state.merge_notification("github-thread-1", &original);

        assert!(!repeated.inserted_new_block);
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
