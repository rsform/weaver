//! Platform-specific TTL cache implementations.
//!
//! Provides a unified API over mini-moka-wasm's sync (native) and unsync (WASM) caches.
//! Native uses the sync cache (thread-safe).
//! WASM uses the unsync cache wrapped in Arc<Mutex<>> (single-threaded but needs interior mutability).

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::time::Duration;

    pub type Cache<K, V> = mini_moka_wasm::sync::Cache<K, V>;

    pub fn new_cache<K, V>(max_capacity: u64, ttl: Duration) -> Cache<K, V>
    where
        K: std::hash::Hash + Eq + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
    {
        mini_moka_wasm::sync::Cache::builder()
            .max_capacity(max_capacity)
            .time_to_live(ttl)
            .build()
    }

    pub fn get<K, V>(cache: &Cache<K, V>, key: &K) -> Option<V>
    where
        K: std::hash::Hash + Eq + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
    {
        cache.get(key)
    }

    pub fn insert<K, V>(cache: &Cache<K, V>, key: K, value: V)
    where
        K: std::hash::Hash + Eq + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
    {
        cache.insert(key, value);
    }

    #[allow(dead_code)]
    pub fn iter<K, V>(cache: &Cache<K, V>) -> Vec<V>
    where
        K: std::hash::Hash + Eq + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
    {
        cache.iter().map(|entry| entry.value().clone()).collect()
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub type Cache<K, V> = Arc<Mutex<mini_moka_wasm::unsync::Cache<K, V>>>;

    pub fn new_cache<K, V>(max_capacity: u64, ttl: Duration) -> Cache<K, V>
    where
        K: std::hash::Hash + Eq + 'static,
        V: Clone + 'static,
    {
        Arc::new(Mutex::new(
            mini_moka_wasm::unsync::Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(ttl)
                .build(),
        ))
    }

    pub fn get<K, V>(cache: &Cache<K, V>, key: &K) -> Option<V>
    where
        K: std::hash::Hash + Eq + 'static,
        V: Clone + 'static,
    {
        cache.lock().unwrap().get(key).cloned()
    }

    pub fn insert<K, V>(cache: &Cache<K, V>, key: K, value: V)
    where
        K: std::hash::Hash + Eq + 'static,
        V: Clone + 'static,
    {
        cache.lock().unwrap().insert(key, value);
    }

    #[allow(dead_code)]
    pub fn iter<K, V>(cache: &Cache<K, V>) -> Vec<V>
    where
        K: std::hash::Hash + Eq + 'static,
        V: Clone + 'static,
    {
        cache
            .lock()
            .unwrap()
            .iter()
            .map(|(_, v)| v.clone())
            .collect()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

/// Create a new cache with the given capacity and TTL.
///
/// This is a convenience re-export of `new_cache` for documentation purposes.
/// The actual implementation is platform-specific.
///
/// # Example
///
/// ```ignore
/// use weaver_common::cache;
/// use std::time::Duration;
///
/// let cache = cache::new_cache::<String, String>(100, Duration::from_secs(3600));
/// cache::insert(&cache, "key".to_string(), "value".to_string());
/// assert_eq!(cache::get(&cache, &"key".to_string()), Some("value".to_string()));
/// ```
#[doc(hidden)]
pub fn _doc_example() {}
