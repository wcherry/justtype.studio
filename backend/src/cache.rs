//! Snapshot scene bodies, kept in memory.
//!
//! A snapshot never changes once taken — that is the whole design — so a scene
//! body read for one reader is the same bytes for the next, and caching it is
//! free of the usual problem: there is nothing to invalidate. The version's
//! own row (its label, its state) does change, so only the bodies are cached,
//! keyed by version and scene, never the assembled response.
//!
//! Bounded by an estimate of size, and the least recently used body goes
//! first. In process, like the rate limiter.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

struct Entry {
    body: Arc<Value>,
    bytes: usize,
    used: u64,
}

pub struct SceneCache {
    budget: usize,
    inner: Mutex<Inner>,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<(String, String), Entry>,
    bytes: usize,
    clock: u64,
}

impl SceneCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget: budget_bytes,
            inner: Mutex::new(Inner::default()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, version_id: &str, scene_id: &str) -> Option<Arc<Value>> {
        let mut inner = self.lock();
        inner.clock += 1;
        let now = inner.clock;
        let found = inner
            .entries
            .get_mut(&(version_id.to_string(), scene_id.to_string()))
            .map(|entry| {
                entry.used = now;
                entry.body.clone()
            });
        match &found {
            Some(_) => self.hits.fetch_add(1, Ordering::Relaxed),
            None => self.misses.fetch_add(1, Ordering::Relaxed),
        };
        found
    }

    pub fn put(&self, version_id: &str, scene_id: &str, body: Value) -> Arc<Value> {
        // Serialised length is a fair stand-in for what the tree costs in
        // memory, and it is cheap next to the disk read it replaces.
        let bytes = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        let body = Arc::new(body);
        if bytes > self.budget {
            return body;
        }
        let mut inner = self.lock();
        inner.clock += 1;
        let used = inner.clock;
        let key = (version_id.to_string(), scene_id.to_string());
        if let Some(old) = inner.entries.insert(
            key,
            Entry {
                body: body.clone(),
                bytes,
                used,
            },
        ) {
            inner.bytes -= old.bytes;
        }
        inner.bytes += bytes;
        while inner.bytes > self.budget {
            let Some(oldest) = inner
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(gone) = inner.entries.remove(&oldest) {
                inner.bytes -= gone.bytes;
            }
        }
        body
    }

    pub fn bytes(&self) -> usize {
        self.lock().bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Value {
        serde_json::json!({ "type": "doc", "text": text })
    }

    #[test]
    fn a_body_is_read_once_and_served_from_memory_after() {
        let cache = SceneCache::new(10_000);
        assert!(cache.get("v1", "s1").is_none());
        cache.put("v1", "s1", doc("words"));
        assert_eq!(cache.get("v1", "s1").unwrap()["text"], "words");
        assert_eq!(cache.hits.load(Ordering::Relaxed), 1);
        assert_eq!(cache.misses.load(Ordering::Relaxed), 1);
        // The same scene in another snapshot is another body.
        assert!(cache.get("v2", "s1").is_none());
    }

    #[test]
    fn the_least_recently_read_goes_first_when_over_budget() {
        let one = serde_json::to_string(&doc("aaaa")).unwrap().len();
        let cache = SceneCache::new(one * 2);
        cache.put("v", "a", doc("aaaa"));
        cache.put("v", "b", doc("bbbb"));
        cache.get("v", "a");
        cache.put("v", "c", doc("cccc"));
        assert!(cache.get("v", "a").is_some(), "read recently, kept");
        assert!(cache.get("v", "b").is_none(), "oldest, evicted");
        assert!(cache.get("v", "c").is_some());
        assert!(cache.bytes() <= one * 2);
    }
}
