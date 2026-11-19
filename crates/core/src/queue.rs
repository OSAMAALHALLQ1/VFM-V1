//! Playback queue: ordering, shuffle, repeat and reordering.
//!
//! This is deliberately separate from saved playlists: a queue is transient
//! playback state (though it is persisted so a restart can restore it).

use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::RepeatMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    items: Vec<String>,
    /// Indices into `items`, in the order they will be played.
    order: Vec<usize>,
    cursor: usize,
    repeat: RepeatMode,
    shuffle: bool,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

impl Queue {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            order: Vec::new(),
            cursor: 0,
            repeat: RepeatMode::All,
            shuffle: false,
        }
    }

    pub fn from_saved(
        items: Vec<String>,
        current_index: usize,
        shuffle: bool,
        repeat: RepeatMode,
    ) -> Self {
        let n = items.len();
        let current = current_index.min(n.saturating_sub(1));
        let mut q = Self {
            items,
            order: (0..n).collect(),
            cursor: current,
            repeat,
            shuffle: false,
        };
        q.set_shuffle(shuffle);
        q
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Index into [`Self::items`] of the currently selected entry.
    pub fn current_index(&self) -> Option<usize> {
        self.order.get(self.cursor).copied()
    }

    pub fn current(&self) -> Option<&str> {
        self.current_index()
            .and_then(|i| self.items.get(i).map(String::as_str))
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        if shuffle == self.shuffle {
            return;
        }
        let current = self.current_index();
        self.shuffle = shuffle;
        let n = self.items.len();
        self.order = (0..n).collect();
        if shuffle {
            let mut seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
                | 1;
            let others: Vec<usize> = self
                .order
                .iter()
                .copied()
                .filter(|i| Some(*i) != current)
                .collect();
            let mut shuffled = Vec::with_capacity(others.len() + usize::from(current.is_some()));
            if let Some(c) = current {
                shuffled.push(c);
            }
            let mut pool = others;
            for i in (1..pool.len()).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let j = (seed >> 33) as usize % (i + 1);
                pool.swap(i, j);
            }
            shuffled.extend(pool);
            self.order = shuffled;
            self.cursor = 0;
        } else {
            self.cursor = current.unwrap_or(0).min(n.saturating_sub(1));
        }
    }

    pub fn set_items(&mut self, items: Vec<String>, current_index: usize) {
        let shuffle = self.shuffle;
        self.items = items;
        self.order = (0..self.items.len()).collect();
        self.cursor = current_index.min(self.items.len().saturating_sub(1));
        self.set_shuffle(shuffle);
    }

    pub fn push(&mut self, uri: impl Into<String>) {
        let index = self.items.len();
        self.items.push(uri.into());
        if self.shuffle {
            self.order.push(index);
        } else {
            self.order = (0..self.items.len()).collect();
            self.cursor = index;
        }
    }

    pub fn insert(&mut self, index: usize, uri: impl Into<String>) {
        let index = index.min(self.items.len());
        // Rebuild against the new indices; the selected URI, not its old index,
        // is what must survive an insert.
        let selected = self.current().map(str::to_owned);
        self.items.insert(index, uri.into());
        self.order = (0..self.items.len()).collect();
        self.cursor = index.min(self.items.len().saturating_sub(1));
        if let Some(uri) = selected {
            self.select_uri(&uri);
        }
        if self.shuffle {
            let shuffle = self.shuffle;
            self.shuffle = false;
            self.set_shuffle(shuffle);
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        let selected = self.current().map(str::to_owned);
        self.items.remove(index);
        self.order = (0..self.items.len()).collect();
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
        if let Some(uri) = selected {
            self.select_uri(&uri);
        }
        if self.shuffle {
            let shuffle = self.shuffle;
            self.shuffle = false;
            self.set_shuffle(shuffle);
        }
    }

    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let selected = self.current().map(str::to_owned);
        let item = self.items.remove(from);
        self.items.insert(to, item);
        self.order = (0..self.items.len()).collect();
        if let Some(uri) = selected {
            self.select_uri(&uri);
        }
        if self.shuffle {
            let shuffle = self.shuffle;
            self.shuffle = false;
            self.set_shuffle(shuffle);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.cursor = 0;
    }

    pub fn select(&mut self, index: usize) -> Option<&str> {
        let position = self.order.iter().position(|i| *i == index)?;
        self.cursor = position;
        self.current()
    }

    pub fn select_uri(&mut self, uri: &str) {
        if let Some(i) = self.items.iter().position(|s| s == uri) {
            self.cursor = self.order.iter().position(|o| *o == i).unwrap_or(0);
        }
    }

    /// Explicit Next: repeat `One` still advances on a user action.
    pub fn next_uri(&mut self) -> Option<&str> {
        if self.order.is_empty() {
            return None;
        }
        self.cursor += 1;
        if self.cursor >= self.order.len() {
            if self.repeat == RepeatMode::All {
                self.cursor = 0;
            } else {
                self.cursor = self.order.len() - 1;
                return self.current();
            }
        }
        self.current()
    }

    /// Automatic advance when playback ends: `One` replays the current item,
    /// `Off` refuses to advance past the last one.
    pub fn on_end_reached(&mut self, autoplay: bool) -> Option<&str> {
        if !autoplay || self.order.is_empty() {
            return None;
        }
        if self.repeat == RepeatMode::One {
            return self.current();
        }
        if self.cursor + 1 >= self.order.len() && self.repeat == RepeatMode::Off {
            return None;
        }
        self.next_uri()
    }

    pub fn previous(&mut self) -> Option<&str> {
        if self.order.is_empty() {
            return None;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        } else if self.repeat == RepeatMode::All {
            self.cursor = self.order.len() - 1;
        }
        self.current()
    }

    /// `(uris, current index into items)` for [`crate::store::Store::save_queue`].
    pub fn snapshot(&self) -> (Vec<String>, usize) {
        (self.items.clone(), self.current_index().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(items: &[&str]) -> Queue {
        Queue::from_saved(
            items.iter().map(|s| (*s).to_owned()).collect(),
            0,
            false,
            RepeatMode::All,
        )
    }

    #[test]
    fn next_and_previous_respect_repeat() {
        let mut queue = q(&["a", "b"]);
        assert_eq!(queue.current(), Some("a"));
        assert_eq!(queue.next_uri(), Some("b"));
        assert_eq!(queue.next_uri(), Some("a"));
        assert_eq!(queue.previous(), Some("b"));

        queue.set_repeat(RepeatMode::Off);
        assert_eq!(queue.next_uri(), Some("b"));
        assert_eq!(queue.on_end_reached(true), None);

        queue.set_repeat(RepeatMode::One);
        assert_eq!(queue.on_end_reached(true), Some("b"));

        queue.set_repeat(RepeatMode::All);
        assert_eq!(queue.next_uri(), Some("a"));
    }

    #[test]
    fn autoplay_off_never_advances() {
        let mut queue = q(&["a", "b"]);
        assert_eq!(queue.on_end_reached(false), None);
        assert_eq!(queue.current(), Some("a"));
    }

    #[test]
    fn shuffle_keeps_current_and_is_a_permutation() {
        let mut queue = q(&["a", "b", "c", "d"]);
        queue.select(2);
        queue.set_shuffle(true);
        assert_eq!(queue.current(), Some("c"));
        let mut played: Vec<String> = Vec::new();
        for _ in 0..4 {
            played.push(queue.current().unwrap().to_owned());
            queue.next_uri();
        }
        played.sort_unstable();
        assert_eq!(played, vec!["a", "b", "c", "d"]);
        queue.set_shuffle(false);
        assert_eq!(queue.current(), Some("c"));
    }

    #[test]
    fn remove_and_move_keep_the_selected_uri() {
        let mut queue = q(&["a", "b", "c"]);
        queue.select(2);
        queue.remove(0);
        assert_eq!(queue.current(), Some("c"));
        assert_eq!(queue.items(), &["b".to_owned(), "c".to_owned()]);
        queue.move_item(1, 0);
        assert_eq!(queue.current(), Some("c"));
        assert_eq!(queue.items(), &["c".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn insert_preserves_selection() {
        let mut queue = q(&["a", "b"]);
        queue.select(1);
        queue.insert(0, "x");
        assert_eq!(queue.current(), Some("b"));
        assert_eq!(
            queue.items(),
            &["x".to_owned(), "a".to_owned(), "b".to_owned()]
        );
    }

    #[test]
    fn snapshot_round_trips_through_store_shape() {
        let mut queue = q(&["a", "b", "c"]);
        queue.select(1);
        let (uris, current) = queue.snapshot();
        let restored = Queue::from_saved(uris, current, false, RepeatMode::All);
        assert_eq!(restored.current(), Some("b"));
    }
}
