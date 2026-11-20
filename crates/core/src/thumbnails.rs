//! Seek-thumbnail cache and request arbitration for the headless grabber.

use std::collections::{HashMap, VecDeque};

/// Small LRU cache keyed by rounded seek position in milliseconds.
#[derive(Debug)]
pub struct ThumbnailCache {
    images: HashMap<u64, Vec<u8>>,
    order: VecDeque<u64>,
    capacity: usize,
}

impl ThumbnailCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            images: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn get(&self, time_ms: u64) -> Option<&[u8]> {
        self.images.get(&time_ms).map(Vec::as_slice)
    }

    pub fn insert(&mut self, time_ms: u64, data: Vec<u8>) {
        if self.images.contains_key(&time_ms) {
            return;
        }
        while self.order.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.images.remove(&oldest);
            }
        }
        self.order.push_back(time_ms);
        self.images.insert(time_ms, data);
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

/// Latest-wins request token: stale grab results are discarded by the worker.
#[derive(Debug, Default, Clone, Copy)]
pub struct ThumbnailRequests {
    latest: u64,
}

impl ThumbnailRequests {
    pub fn request(&mut self) -> u64 {
        self.latest = self.latest.wrapping_add(1);
        self.latest
    }

    pub fn is_latest(&self, token: u64) -> bool {
        self.latest == token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keeps_most_recent_entries() {
        let mut cache = ThumbnailCache::new(2);
        cache.insert(100, vec![1]);
        cache.insert(200, vec![2]);
        assert_eq!(cache.get(100), Some(&[1][..]));
        cache.insert(300, vec![3]);
        assert_eq!(cache.get(100), None);
        assert_eq!(cache.get(200), Some(&[2][..]));
        assert_eq!(cache.get(300), Some(&[3][..]));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn duplicate_insert_does_not_evict() {
        let mut cache = ThumbnailCache::new(2);
        cache.insert(100, vec![1]);
        cache.insert(100, vec![9]);
        cache.insert(200, vec![2]);
        assert_eq!(cache.get(100), Some(&[1][..]));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn stale_requests_are_rejected() {
        let mut requests = ThumbnailRequests::default();
        let first = requests.request();
        let second = requests.request();
        assert!(!requests.is_latest(first));
        assert!(requests.is_latest(second));
    }
}
