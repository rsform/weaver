//! CRDT document trait and sync state tracking.

use loro::VersionVector;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;

/// Sync state for a CRDT document.
///
/// Tracks the edit root, last diff, and version at last sync.
#[derive(Clone, Debug, Default)]
pub struct SyncState {
    /// StrongRef to the sh.weaver.edit.root record.
    pub edit_root: Option<StrongRef<'static>>,

    /// StrongRef to the most recent sh.weaver.edit.diff record.
    pub last_diff: Option<StrongRef<'static>>,

    /// Version vector at the time of last sync.
    pub last_synced_version: Option<VersionVector>,
}

impl SyncState {
    /// Create new empty sync state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if we have an edit root (i.e., have synced at least once).
    pub fn has_root(&self) -> bool {
        self.edit_root.is_some()
    }
}

/// Trait for CRDT documents that can be synced to AT Protocol PDS.
///
/// Implementors provide access to the underlying CRDT operations
/// and sync state tracking.
pub trait CrdtDocument {
    /// Export full snapshot bytes.
    fn export_snapshot(&self) -> Vec<u8>;

    /// Export updates since the last synced version.
    /// Returns None if no changes since last sync.
    fn export_updates_since_sync(&self) -> Option<Vec<u8>>;

    /// Import remote changes.
    fn import(&mut self, data: &[u8]) -> Result<(), crate::CrdtError>;

    /// Get current version vector.
    fn version(&self) -> VersionVector;

    /// Get the edit root StrongRef.
    fn edit_root(&self) -> Option<StrongRef<'static>>;

    /// Set the edit root StrongRef.
    fn set_edit_root(&mut self, root: Option<StrongRef<'static>>);

    /// Get the last diff StrongRef.
    fn last_diff(&self) -> Option<StrongRef<'static>>;

    /// Set the last diff StrongRef.
    fn set_last_diff(&mut self, diff: Option<StrongRef<'static>>);

    /// Mark current version as synced.
    fn mark_synced(&mut self);

    /// Check if there are changes since last sync.
    fn has_unsynced_changes(&self) -> bool;
}

// Blanket implementation for LoroTextBuffer with embedded SyncState
// (Concrete types can provide their own implementations)

/// A simple CRDT document wrapping LoroTextBuffer with sync state.
pub struct SimpleCrdtDocument {
    buffer: crate::LoroTextBuffer,
    sync_state: SyncState,
}

impl SimpleCrdtDocument {
    /// Create a new empty document.
    pub fn new() -> Self {
        Self {
            buffer: crate::LoroTextBuffer::new(),
            sync_state: SyncState::new(),
        }
    }

    /// Create from snapshot.
    pub fn from_snapshot(snapshot: &[u8]) -> Result<Self, crate::CrdtError> {
        Ok(Self {
            buffer: crate::LoroTextBuffer::from_snapshot(snapshot)?,
            sync_state: SyncState::new(),
        })
    }

    /// Get the underlying buffer.
    pub fn buffer(&self) -> &crate::LoroTextBuffer {
        &self.buffer
    }

    /// Get mutable access to the buffer.
    pub fn buffer_mut(&mut self) -> &mut crate::LoroTextBuffer {
        &mut self.buffer
    }
}

impl Default for SimpleCrdtDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl CrdtDocument for SimpleCrdtDocument {
    fn export_snapshot(&self) -> Vec<u8> {
        self.buffer.export_snapshot()
    }

    fn export_updates_since_sync(&self) -> Option<Vec<u8>> {
        self.sync_state
            .last_synced_version
            .as_ref()
            .and_then(|v| self.buffer.export_updates_since(v))
    }

    fn import(&mut self, data: &[u8]) -> Result<(), crate::CrdtError> {
        self.buffer.import(data)
    }

    fn version(&self) -> VersionVector {
        self.buffer.version()
    }

    fn edit_root(&self) -> Option<StrongRef<'static>> {
        self.sync_state.edit_root.clone()
    }

    fn set_edit_root(&mut self, root: Option<StrongRef<'static>>) {
        self.sync_state.edit_root = root;
    }

    fn last_diff(&self) -> Option<StrongRef<'static>> {
        self.sync_state.last_diff.clone()
    }

    fn set_last_diff(&mut self, diff: Option<StrongRef<'static>>) {
        self.sync_state.last_diff = diff;
    }

    fn mark_synced(&mut self) {
        self.sync_state.last_synced_version = Some(self.buffer.version());
    }

    fn has_unsynced_changes(&self) -> bool {
        match &self.sync_state.last_synced_version {
            None => true, // Never synced
            Some(last) => self.buffer.version() != *last,
        }
    }
}
