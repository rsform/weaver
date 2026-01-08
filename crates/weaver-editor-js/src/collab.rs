//! JsCollabEditor - collaborative editor with Loro CRDT.
//!
//! Only available with the `collab` feature.

use wasm_bindgen::prelude::*;

use weaver_editor_crdt::LoroTextBuffer;

/// Collaborative editor with CRDT sync.
///
/// Wraps LoroTextBuffer for collaborative editing with iroh P2P transport.
#[wasm_bindgen]
pub struct JsCollabEditor {
    // TODO: Implement collab editor
    // - LoroTextBuffer for CRDT-backed text
    // - iroh node for P2P transport
    // - Session management callbacks
    _marker: std::marker::PhantomData<LoroTextBuffer>,
}

#[wasm_bindgen]
impl JsCollabEditor {
    /// Create a new collaborative editor.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<JsCollabEditor, JsError> {
        Err(JsError::new("CollabEditor not yet implemented"))
    }
}

impl Default for JsCollabEditor {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

// TODO: Implement these when ready:
// - from_snapshot / from_loro_doc
// - export_updates / import_updates
// - get_version
// - add_peer / remove_peer / get_connected_peers
// - Session callbacks (onSessionNeeded, onSessionRefresh, onSessionEnd, onPeersNeeded)
