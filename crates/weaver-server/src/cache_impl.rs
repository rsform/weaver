//! Platform-specific cache implementations
//! Native uses sync cache (thread-safe, no mutex needed)
//! WASM uses unsync cache wrapped in Arc<Mutex<>> (no threads, but need interior mutability)

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::time::Duration;

    pub type Cache<K, V> = mini_moka::sync::Cache<K, V>;

    pub fn new_cache<K, V>(max_capacity: u64, ttl: Duration) -> Cache<K, V>
    where
        K: std::hash::Hash + Eq + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
    {
        mini_moka::sync::Cache::builder()
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

    pub type Cache<K, V> = Arc<Mutex<mini_moka::unsync::Cache<K, V>>>;

    pub fn new_cache<K, V>(max_capacity: u64, ttl: Duration) -> Cache<K, V>
    where
        K: std::hash::Hash + Eq + 'static,
        V: Clone + 'static,
    {
        Arc::new(Mutex::new(
            mini_moka::unsync::Cache::builder()
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

    pub fn iter<K, V>(cache: &Cache<K, V>) -> Vec<V>
    where
        K: std::hash::Hash + Eq + 'static,
        V: Clone + 'static,
    {
        cache.lock().unwrap().iter().map(|(_, v)| v.clone()).collect()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
pub use wasm::*;
