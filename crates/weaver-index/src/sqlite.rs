use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use dashmap::DashMap;
use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};
use smol_str::SmolStr;

use crate::error::{IndexError, SqliteError};

/// Key for shard routing - (collection, rkey) tuple
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ShardKey {
    pub collection: SmolStr,
    pub rkey: SmolStr,
}

impl ShardKey {
    pub fn new(collection: impl Into<SmolStr>, rkey: impl Into<SmolStr>) -> Self {
        Self {
            collection: collection.into(),
            rkey: rkey.into(),
        }
    }

    fn hash_prefix(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        let hash = hasher.finish();
        format!("{:02x}", (hash & 0xFF) as u8)
    }

    /// Directory path: {base}/{hash(collection,rkey)[0..2]}/{rkey}/
    fn dir_path(&self, base: &Path) -> PathBuf {
        base.join(self.hash_prefix()).join(self.rkey.as_str())
    }

    pub fn collection(&self) -> &str {
        &self.collection
    }

    pub fn rkey(&self) -> &str {
        &self.rkey
    }
}

/// A single SQLite shard for a resource
pub struct SqliteShard {
    conn: Mutex<Connection>,
    path: PathBuf,
    last_accessed: Mutex<Instant>,
}

impl SqliteShard {
    const DB_FILENAME: &'static str = "store.sqlite";

    fn open(dir: &Path) -> Result<Self, IndexError> {
        fs::create_dir_all(dir).map_err(|e| SqliteError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;

        let db_path = dir.join(Self::DB_FILENAME);
        let mut conn = Connection::open(&db_path).map_err(|e| SqliteError::Open {
            path: db_path.clone(),
            source: e,
        })?;

        // Enable WAL mode for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| SqliteError::Pragma {
                pragma: "journal_mode",
                source: e,
            })?;

        // Run migrations
        // PERF: rusqlite_migration checks user_version pragma, which is fast when
        // no migrations needed. If shard open becomes a bottleneck, consider adding
        // a signal file (e.g., .schema_v{N}) to skip migration check entirely.
        Self::migrations()
            .to_latest(&mut conn)
            .map_err(|e| SqliteError::Migration {
                message: e.to_string(),
            })?;

        Ok(Self {
            conn: Mutex::new(conn),
            path: db_path,
            last_accessed: Mutex::new(Instant::now()),
        })
    }

    fn migrations() -> Migrations<'static> {
        Migrations::new(vec![
            M::up(include_str!("sqlite/migrations/001_edit_graph.sql")),
            M::up(include_str!("sqlite/migrations/002_collaboration.sql")),
            M::up(include_str!("sqlite/migrations/003_permissions.sql")),
        ])
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn touch(&self) {
        if let Ok(mut last) = self.last_accessed.lock() {
            *last = Instant::now();
        }
    }

    pub fn last_accessed(&self) -> Instant {
        self.last_accessed
            .lock()
            .map(|t| *t)
            .unwrap_or_else(|_| Instant::now())
    }

    /// Execute a read operation on the shard
    pub fn read<F, T>(&self, f: F) -> Result<T, IndexError>
    where
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    {
        self.touch();
        let conn = self.conn.lock().map_err(|_| SqliteError::LockPoisoned)?;
        f(&conn).map_err(|e| {
            SqliteError::Query {
                message: e.to_string(),
            }
            .into()
        })
    }

    /// Execute a write operation on the shard
    pub fn write<F, T>(&self, f: F) -> Result<T, IndexError>
    where
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    {
        self.touch();
        let conn = self.conn.lock().map_err(|_| SqliteError::LockPoisoned)?;
        f(&conn).map_err(|e| {
            SqliteError::Query {
                message: e.to_string(),
            }
            .into()
        })
    }
}

/// Routes resources to their SQLite shards
pub struct ShardRouter {
    base_path: PathBuf,
    shards: DashMap<ShardKey, std::sync::Arc<SqliteShard>>,
}

impl ShardRouter {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
            shards: DashMap::new(),
        }
    }

    /// Get or create a shard for the given key
    pub fn get_or_create(&self, key: &ShardKey) -> Result<std::sync::Arc<SqliteShard>, IndexError> {
        // Fast path: already cached
        if let Some(shard) = self.shards.get(key) {
            shard.touch();
            return Ok(shard.clone());
        }

        // Slow path: create new shard
        let dir = key.dir_path(&self.base_path);
        let shard = std::sync::Arc::new(SqliteShard::open(&dir)?);
        self.shards.insert(key.clone(), shard.clone());

        Ok(shard)
    }

    /// Get an existing shard without creating
    pub fn get(&self, key: &ShardKey) -> Option<std::sync::Arc<SqliteShard>> {
        self.shards.get(key).map(|s| {
            s.touch();
            s.clone()
        })
    }

    /// Number of active shards
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    /// Iterate over shards that haven't been accessed since the given instant
    pub fn idle_shards(&self, since: Instant) -> Vec<ShardKey> {
        self.shards
            .iter()
            .filter(|entry| entry.value().last_accessed() < since)
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Remove a shard from the cache (for eviction)
    pub fn evict(&self, key: &ShardKey) -> Option<std::sync::Arc<SqliteShard>> {
        self.shards.remove(key).map(|(_, shard)| shard)
    }
}
