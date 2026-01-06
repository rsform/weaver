//! PDS synchronization for editor edit state.
//!
//! This module provides app-specific sync functionality built on top of
//! `weaver_editor_crdt::sync`. It adds:
//! - Fetcher-based API (wrapping the generic client)
//! - Embed prefetching and blob caching
//! - localStorage integration for document loading
//! - Dioxus UI components for sync status

use std::collections::HashMap;

use super::document::{EditorDocument, LoadedDocState};
use crate::fetch::Fetcher;
use jacquard::IntoStatic;
use jacquard::prelude::*;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::{AtUri, Cid, Did};
use loro::LoroDoc;
use loro::ToJson;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::edit::draft::Draft;
use weaver_api::sh_weaver::edit::{DocRef, DocRefValue};
use weaver_common::{WeaverError, WeaverExt};

// Re-export crdt sync types for convenience.
pub use weaver_editor_crdt::{
    CreateRootResult, PdsEditState, RemoteDraft, SyncResult, build_draft_uri, find_all_edit_roots,
    find_diffs_for_root, find_edit_root_for_draft, list_drafts, load_all_edit_states,
    load_edit_state_from_draft, load_edit_state_from_entry,
};

/// Extract record embeds from a LoroDoc and pre-fetch their rendered content.
///
/// Reads the embeds.records list from the document, extracts RecordEmbed entries,
/// and fetches/renders each one to populate a ResolvedContent map.
/// Also pre-warms the blob cache for images if `owner_ident` is provided.
async fn prefetch_embeds_from_doc(
    doc: &LoroDoc,
    fetcher: &Fetcher,
    owner_ident: Option<&str>,
) -> weaver_common::ResolvedContent {
    use weaver_api::sh_weaver::embed::images::Image;
    use weaver_api::sh_weaver::embed::records::RecordEmbed;

    let mut resolved = weaver_common::ResolvedContent::default();

    let embeds_map = doc.get_map("embeds");

    // Pre-warm blob cache for images
    #[cfg(feature = "fullstack-server")]
    if let Some(ident) = owner_ident {
        if let Ok(images_container) =
            embeds_map.get_or_create_container("images", loro::LoroList::new())
        {
            for i in 0..images_container.len() {
                let Some(value) = images_container.get(i) else {
                    continue;
                };
                let Some(loro_value) = value.as_value() else {
                    continue;
                };
                let json = loro_value.to_json_value();
                let Ok(image) = jacquard::from_json_value::<Image>(json) else {
                    continue;
                };

                let cid = image.image.blob().cid();
                let name = image.name.as_ref().map(|n| n.as_ref());
                if let Err(e) = crate::data::cache_blob(
                    ident.into(),
                    cid.as_ref().into(),
                    name.map(|n| n.into()),
                )
                .await
                {
                    tracing::warn!("Failed to pre-warm blob cache for {}: {}", cid, e);
                }
            }
        }
    }
    #[cfg(not(feature = "fullstack-server"))]
    let _ = owner_ident;

    // Strategy 1: Get embeds from Loro embeds map -> records list

    if let Ok(records_container) =
        embeds_map.get_or_create_container("records", loro::LoroList::new())
    {
        for i in 0..records_container.len() {
            let Some(value) = records_container.get(i) else {
                continue;
            };
            let Some(loro_value) = value.as_value() else {
                continue;
            };
            let json = loro_value.to_json_value();
            let Ok(record_embed) = jacquard::from_json_value::<RecordEmbed>(json) else {
                continue;
            };

            // name is the key used in markdown, fallback to record.uri
            let key_uri = if let Some(ref name) = record_embed.name {
                match AtUri::new(name.as_ref()) {
                    Ok(uri) => uri.into_static(),
                    Err(_) => continue,
                }
            } else {
                record_embed.record.uri.clone().into_static()
            };

            // Fetch and render
            match weaver_renderer::atproto::fetch_and_render(&record_embed.record.uri, fetcher)
                .await
            {
                Ok(html) => {
                    resolved.add_embed(key_uri, html, None);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to pre-fetch embed {}: {}",
                        record_embed.record.uri,
                        e
                    );
                }
            }
        }
    }

    // Strategy 2: If no embeds found in Loro map, parse markdown text
    if resolved.embed_content.is_empty() {
        use weaver_common::{ExtractedRef, collect_refs_from_markdown};

        let text = doc.get_text("content");
        let markdown = text.to_string();

        if !markdown.is_empty() {
            let refs = collect_refs_from_markdown(&markdown);

            for extracted in refs {
                if let ExtractedRef::AtEmbed { uri, .. } = extracted {
                    let key_uri = match AtUri::new(&uri) {
                        Ok(u) => u.into_static(),
                        Err(_) => continue,
                    };

                    match weaver_renderer::atproto::fetch_and_render(&key_uri, fetcher).await {
                        Ok(html) => {
                            resolved.add_embed(key_uri, html, None);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to pre-fetch embed {}: {}", uri, e);
                        }
                    }
                }
            }
        }
    }

    resolved
}

/// Convert a DocRef to an entry_ref StrongRef.
///
/// For EntryRef: returns the entry's StrongRef directly
/// For DraftRef: parses the draft_key as AT-URI, fetches the draft record to get CID, builds StrongRef
/// For NotebookRef: returns the notebook's StrongRef
async fn doc_ref_to_entry_ref(
    fetcher: &Fetcher,
    doc_ref: &DocRef<'_>,
) -> Option<StrongRef<'static>> {
    match &doc_ref.value {
        DocRefValue::EntryRef(entry_ref) => Some(entry_ref.entry.clone().into_static()),
        DocRefValue::DraftRef(draft_ref) => {
            // draft_key contains the canonical AT-URI: at://{did}/sh.weaver.edit.draft/{rkey}
            let draft_uri = AtUri::new(&draft_ref.draft_key).ok()?.into_static();

            // Fetch the draft record to get its CID
            match fetcher.client.get_record::<Draft>(&draft_uri).await {
                Ok(response) => {
                    let output = response.into_output().ok()?;
                    let cid = output.cid?.into_static();
                    Some(StrongRef::new().uri(draft_uri).cid(cid).build())
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch draft record for entry_ref: {}", e);
                    None
                }
            }
        }
        DocRefValue::NotebookRef(notebook_ref) => Some(notebook_ref.notebook.clone().into_static()),
        DocRefValue::Unknown(_) => {
            tracing::warn!("Unknown DocRefValue variant, cannot convert to entry_ref");
            None
        }
    }
}

/// List all drafts from PDS for the current user.
///
/// Wraps the crdt crate's list_drafts function with Fetcher support.
pub async fn list_drafts_from_pds(fetcher: &Fetcher) -> Result<Vec<RemoteDraft>, WeaverError> {
    let did = fetcher
        .current_did()
        .await
        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

    list_drafts(fetcher.get_client().as_ref(), &did)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))
}

/// Create the edit root record for an entry.
///
/// Wraps the crdt crate's create_edit_root with Fetcher support.
pub async fn create_edit_root(
    fetcher: &Fetcher,
    doc: &EditorDocument,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> Result<CreateRootResult, WeaverError> {
    weaver_editor_crdt::create_edit_root(
        fetcher.get_client().as_ref(),
        doc,
        draft_key,
        entry_uri,
        entry_cid,
    )
    .await
    .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))
}

/// Create a diff record with updates since the last sync.
///
/// Wraps the crdt crate's create_diff with Fetcher support.
pub async fn create_diff(
    fetcher: &Fetcher,
    doc: &EditorDocument,
    root_uri: &AtUri<'_>,
    root_cid: &Cid<'_>,
    prev_diff: Option<(&AtUri<'_>, &Cid<'_>)>,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> Result<Option<(AtUri<'static>, Cid<'static>)>, WeaverError> {
    weaver_editor_crdt::create_diff(
        fetcher.get_client().as_ref(),
        doc,
        root_uri,
        root_cid,
        prev_diff,
        draft_key,
        entry_uri,
        entry_cid,
    )
    .await
    .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))
}

/// Sync the document to the PDS.
///
/// If no edit root exists, creates one with a full snapshot.
/// If a root exists, creates a diff with updates since last sync.
/// Updates the document's sync state on success.
pub async fn sync_to_pds(
    fetcher: &Fetcher,
    doc: &mut EditorDocument,
    draft_key: &str,
) -> Result<SyncResult, WeaverError> {
    let fn_start = crate::perf::now();

    // Check if we have changes to sync
    if !doc.has_unsynced_changes() {
        return Ok(SyncResult::NoChanges);
    }

    // Get entry info if published
    let entry_ref = doc.entry_ref();

    if doc.edit_root().is_none() {
        // First sync - create root
        let create_start = crate::perf::now();
        let result = create_edit_root(
            fetcher,
            doc,
            draft_key,
            entry_ref.as_ref().map(|r| &r.uri),
            entry_ref.as_ref().map(|r| &r.cid),
        )
        .await?;
        let create_ms = crate::perf::now() - create_start;

        // Build StrongRef for the root
        let root_ref = StrongRef::new()
            .uri(result.root_uri.clone())
            .cid(result.root_cid.clone())
            .build();

        // Update document state
        doc.set_edit_root(Some(root_ref));
        doc.set_last_diff(None);
        doc.mark_synced();

        // For drafts: set entry_ref to the draft record (enables draft discovery/recovery)
        if let Some(draft_ref) = result.draft_ref {
            if doc.entry_ref().is_none() {
                tracing::debug!("Setting entry_ref to draft: {}", draft_ref.uri);
                doc.set_entry_ref(Some(draft_ref));
            }
        }

        let total_ms = crate::perf::now() - fn_start;
        tracing::debug!(total_ms, create_ms, "sync_to_pds: created root");

        Ok(SyncResult::CreatedRoot {
            uri: result.root_uri,
            cid: result.root_cid,
        })
    } else {
        // Subsequent sync - create diff
        let root_ref = doc.edit_root().unwrap();
        let prev_diff = doc.last_diff();

        let create_start = crate::perf::now();
        let result = create_diff(
            fetcher,
            doc,
            &root_ref.uri,
            &root_ref.cid,
            prev_diff.as_ref().map(|d| (&d.uri, &d.cid)),
            draft_key,
            entry_ref.as_ref().map(|r| &r.uri),
            entry_ref.as_ref().map(|r| &r.cid),
        )
        .await?;
        let create_ms = crate::perf::now() - create_start;

        match result {
            Some((diff_uri, diff_cid)) => {
                // Build StrongRef for the diff
                let diff_ref = StrongRef::new()
                    .uri(diff_uri.clone())
                    .cid(diff_cid.clone())
                    .build();

                doc.set_last_diff(Some(diff_ref));
                doc.mark_synced();

                let total_ms = crate::perf::now() - fn_start;
                tracing::debug!(total_ms, create_ms, "sync_to_pds: created diff");

                Ok(SyncResult::CreatedDiff {
                    uri: diff_uri,
                    cid: diff_cid,
                })
            }
            None => {
                let total_ms = crate::perf::now() - fn_start;
                tracing::debug!(total_ms, create_ms, "sync_to_pds: no changes in diff");
                Ok(SyncResult::NoChanges)
            }
        }
    }
}

/// Load edit state from the PDS for an entry.
///
/// Wraps the crdt crate's load_edit_state_from_entry with Fetcher support.
pub async fn load_edit_state_from_pds(
    fetcher: &Fetcher,
    entry_uri: &AtUri<'_>,
) -> Result<Option<PdsEditState>, WeaverError> {
    let client = fetcher.get_client();
    // Get collaborators for this resource.
    let collaborators = client
        .find_collaborators_for_resource(entry_uri)
        .await
        .unwrap_or_default();

    load_edit_state_from_entry(client.as_ref(), entry_uri, collaborators)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))
}

/// Load edit state from ALL collaborator repos for an entry, returning merged state.
///
/// Wraps the crdt crate's load_all_edit_states with Fetcher support.
pub async fn load_all_edit_states_from_pds(
    fetcher: &Fetcher,
    entry_uri: &AtUri<'_>,
    last_seen_diffs: &HashMap<AtUri<'static>, AtUri<'static>>,
) -> Result<Option<PdsEditState>, WeaverError> {
    let client = fetcher.get_client();

    // Get collaborators for this resource.
    let collaborators = client
        .find_collaborators_for_resource(entry_uri)
        .await
        .unwrap_or_default();

    let current_did = fetcher.current_did().await;

    load_all_edit_states(
        client.as_ref(),
        entry_uri,
        collaborators,
        current_did.as_ref(),
        last_seen_diffs,
    )
    .await
    .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))
}

/// Load document state by merging local storage and PDS state.
///
/// Loads from localStorage and PDS (if available), then merges both using Loro's
/// CRDT merge. The result is a pre-merged LoroDoc that can be converted to an
/// EditorDocument inside a reactive context using `use_hook`.
///
/// For unpublished drafts, attempts to discover edit state via Constellation
/// using the synthetic draft URI.
pub async fn load_and_merge_document(
    fetcher: &Fetcher,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
) -> Result<Option<LoadedDocState>, WeaverError> {
    use super::storage::load_snapshot_from_storage;

    // Load snapshot + entry_ref from localStorage
    let local_data = load_snapshot_from_storage(draft_key);

    // Load from PDS - for entries use multi-repo loading (all collaborators),
    // for drafts use single-repo loading (draft sharing requires knowing the URI)
    let pds_state = if let Some(uri) = entry_uri {
        // Published entry: load from ALL collaborators (multi-repo CRDT merge)
        let empty_last_seen = HashMap::new();
        load_all_edit_states_from_pds(fetcher, uri, &empty_last_seen).await?
    } else if let Some(did) = fetcher.current_did().await {
        // Unpublished draft: single-repo for now
        // (draft sharing would require collaborator to know the draft URI)
        let draft_uri = build_draft_uri(&did, draft_key);
        load_edit_state_from_draft(fetcher.get_client().as_ref(), &draft_uri)
            .await
            .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?
    } else {
        // Not authenticated, can't query PDS
        None
    };

    // Extract owner identity from entry URI for blob cache warming
    let owner_ident: Option<String> = entry_uri.map(|uri| match uri.authority() {
        AtIdentifier::Did(d) => d.as_ref().to_string(),
        AtIdentifier::Handle(h) => h.as_ref().to_string(),
    });

    match (local_data, pds_state) {
        (None, None) => Ok(None),

        (Some(local), None) => {
            // Only local state exists - build LoroDoc from snapshot
            tracing::debug!("Loaded document from localStorage only");
            let doc = LoroDoc::new();
            if let Err(e) = doc.import(&local.snapshot) {
                tracing::warn!("Failed to import local snapshot: {:?}", e);
            }

            let resolved_content =
                prefetch_embeds_from_doc(&doc, fetcher, owner_ident.as_deref()).await;

            Ok(Some(LoadedDocState {
                doc,
                entry_ref: local.entry_ref, // Restored from localStorage
                edit_root: None,
                last_diff: None,
                synced_version: None, // Local-only, never synced to PDS
                last_seen_diffs: std::collections::HashMap::new(),
                resolved_content,
                notebook_uri: local.notebook_uri, // Restored from localStorage
            }))
        }

        (None, Some(pds)) => {
            // Only PDS state exists - reconstruct from snapshot + diffs
            tracing::debug!("Loaded document from PDS only");
            let doc = LoroDoc::new();

            // Import root snapshot
            if let Err(e) = doc.import(&pds.root_snapshot) {
                tracing::warn!("Failed to import PDS root snapshot: {:?}", e);
            }

            // Apply all diffs in order
            for updates in &pds.diff_updates {
                if let Err(e) = doc.import(updates) {
                    tracing::warn!("Failed to apply diff update: {:?}", e);
                }
            }

            // Capture the version after loading all PDS state - this is our sync baseline
            let synced_version = Some(doc.oplog_vv());

            // Reconstruct entry_ref from the DocRef stored in edit.root
            let entry_ref = doc_ref_to_entry_ref(fetcher, &pds.doc_ref).await;
            if entry_ref.is_some() {
                tracing::debug!("Reconstructed entry_ref from PDS DocRef");
            }

            let resolved_content =
                prefetch_embeds_from_doc(&doc, fetcher, owner_ident.as_deref()).await;

            Ok(Some(LoadedDocState {
                doc,
                entry_ref,
                edit_root: Some(pds.root_ref),
                last_diff: pds.last_diff_ref,
                synced_version, // Just loaded from PDS, fully synced
                last_seen_diffs: pds.last_seen_diffs,
                resolved_content,
                notebook_uri: None, // PDS-only, notebook context comes from target_notebook
            }))
        }

        (Some(local), Some(pds)) => {
            // Both exist - merge using CRDT
            tracing::debug!("Merging document from localStorage and PDS");

            // First, reconstruct the PDS state to get its version vector
            let pds_doc = LoroDoc::new();
            if let Err(e) = pds_doc.import(&pds.root_snapshot) {
                tracing::warn!("Failed to import PDS root snapshot for VV: {:?}", e);
            }
            for updates in &pds.diff_updates {
                if let Err(e) = pds_doc.import(updates) {
                    tracing::warn!("Failed to apply PDS diff for VV: {:?}", e);
                }
            }
            let pds_version = pds_doc.oplog_vv();

            // Now create the merged doc
            let doc = LoroDoc::new();

            // Import local snapshot first
            if let Err(e) = doc.import(&local.snapshot) {
                tracing::warn!("Failed to import local snapshot: {:?}", e);
            }

            // Import PDS root snapshot - Loro will merge
            if let Err(e) = doc.import(&pds.root_snapshot) {
                tracing::warn!("Failed to merge PDS root snapshot: {:?}", e);
            }

            // Import all diffs
            for updates in &pds.diff_updates {
                if let Err(e) = doc.import(updates) {
                    tracing::warn!("Failed to merge PDS diff: {:?}", e);
                }
            }

            // Use the PDS version as our sync baseline - any local changes
            // beyond this will be detected as unsynced
            let resolved_content =
                prefetch_embeds_from_doc(&doc, fetcher, owner_ident.as_deref()).await;

            Ok(Some(LoadedDocState {
                doc,
                entry_ref: local.entry_ref, // Restored from localStorage
                edit_root: Some(pds.root_ref),
                last_diff: pds.last_diff_ref,
                synced_version: Some(pds_version),
                last_seen_diffs: pds.last_seen_diffs,
                resolved_content,
                notebook_uri: local.notebook_uri, // Restored from localStorage
            }))
        }
    }
}

// ============================================================================
// Sync UI Components
// ============================================================================

use crate::auth::AuthState;
use dioxus::prelude::*;

/// Sync status states for UI display.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncState {
    /// All local changes have been synced to PDS
    Synced,
    /// Currently syncing to PDS
    Syncing,
    /// Has local changes not yet synced
    Unsynced,
    /// Remote collaborator changes available
    RemoteChanges,
    /// Last sync failed
    Error,
    /// Not authenticated or sync disabled
    Disabled,
}

/// Props for the SyncStatus component.
#[derive(Props, Clone, PartialEq)]
pub struct SyncStatusProps {
    /// The editor document to sync
    pub document: EditorDocument,
    /// Draft key for this document
    pub draft_key: String,
    /// Auto-sync interval in milliseconds (0 to disable, default disabled)
    #[props(default = 0)]
    pub auto_sync_interval_ms: u32,
    /// Callback to refresh/reload document from collaborators
    #[props(default)]
    pub on_refresh: Option<EventHandler<()>>,
    /// Whether this is a collaborative document (has collaborators)
    #[props(default = false)]
    pub is_collaborative: bool,
}

/// Sync status indicator with auto-sync functionality.
///
/// Displays the current sync state and automatically syncs to PDS periodically.
/// Initially shows "Start syncing" until user activates sync, then auto-syncs.
#[component]
pub fn SyncStatus(props: SyncStatusProps) -> Element {
    let fetcher = use_context::<Fetcher>();
    let auth_state = use_context::<Signal<AuthState>>();

    let doc = props.document.clone();
    let draft_key = props.draft_key.clone();

    // Sync activated - true if sync has been started (either manually or doc already has edit_root)
    // Once activated, auto-sync is enabled
    let mut sync_activated = use_signal(|| {
        // If document already has an edit_root, syncing is already active
        props.document.edit_root().is_some()
    });

    // Sync state management
    let mut sync_state = use_signal(|| {
        if props.document.has_unsynced_changes() {
            SyncState::Unsynced
        } else {
            SyncState::Synced
        }
    });
    let mut last_error: Signal<Option<String>> = use_signal(|| None);

    // Check if we're authenticated (drafts can sync via DraftRef even without entry)
    let is_authenticated = auth_state.read().is_authenticated();

    // Auto-sync trigger signal - set to true to trigger a sync
    let mut trigger_sync = use_signal(|| false);

    // Auto-sync timer - only triggers after sync has been activated
    {
        let doc_for_check = doc.clone();

        // Use 30s interval for auto-sync once activated
        dioxus_sdk::time::use_interval(std::time::Duration::from_secs(30), move |_| {
            // Only auto-sync if activated
            if !*sync_activated.peek() {
                return;
            }
            // Only trigger if there are unsynced changes
            if doc_for_check.has_unsynced_changes() {
                trigger_sync.set(true);
            }
        });
    }

    // Collaborator poll timer - checks for collaborator updates periodically
    // For collaborative documents, poll every 60s
    // - If user has been idle ≥30s: auto-trigger refresh
    // - If user is actively editing: show RemoteChanges state
    {
        let is_collaborative = props.is_collaborative;
        let on_refresh = props.on_refresh.clone();
        let doc_for_idle = doc.clone();

        dioxus_sdk::time::use_interval(std::time::Duration::from_secs(60), move |_| {
            if !is_collaborative {
                return;
            }

            let idle_threshold = std::time::Duration::from_secs(30);

            // Check time since last edit
            let is_idle = match doc_for_idle.last_edit() {
                Some(edit_info) => edit_info.timestamp.elapsed() >= idle_threshold,
                None => true, // No edits yet = idle
            };

            if is_idle {
                // User is idle - safe to auto-refresh
                if let Some(ref handler) = on_refresh {
                    handler.call(());
                }
            } else {
                // User is actively editing - show remote changes indicator
                sync_state.set(SyncState::RemoteChanges);
            }
        });
    }

    // Update sync state when document changes
    // Note: We use peek() to avoid creating a reactive dependency on sync_state
    let doc_for_effect = doc.clone();
    use_effect(move || {
        // Check for unsynced changes (reads last_edit signal for reactivity)
        let _edit = doc_for_effect.last_edit();

        // Use peek to avoid reactive loop
        let current_state = *sync_state.peek();
        if current_state != SyncState::Syncing {
            if doc_for_effect.has_unsynced_changes() && current_state != SyncState::Unsynced {
                sync_state.set(SyncState::Unsynced);
            }
        }
    });

    // Sync effect - watches trigger_sync and performs sync when triggered
    let doc_for_sync = doc.clone();
    let draft_key_for_sync = draft_key.clone();
    let fetcher_for_sync = fetcher.clone();

    let doc_for_check = doc.clone();
    use_effect(move || {
        // Read trigger to create reactive dependency
        let should_sync = *trigger_sync.read();

        if !should_sync {
            return;
        }

        // Reset trigger immediately
        trigger_sync.set(false);

        // Check if already syncing
        if *sync_state.peek() == SyncState::Syncing {
            return;
        }

        // Check if authenticated (drafts can sync too via DraftRef)
        if !is_authenticated {
            return;
        }

        // Check if there are actually changes to sync
        if !doc_for_check.has_unsynced_changes() {
            // Already synced, just update state
            sync_state.set(SyncState::Synced);
            return;
        }

        sync_state.set(SyncState::Syncing);

        let mut doc = doc_for_sync.clone();
        let draft_key = draft_key_for_sync.clone();
        let fetcher = fetcher_for_sync.clone();

        // Spawn the async work
        spawn(async move {
            match sync_to_pds(&fetcher, &mut doc, &draft_key).await {
                Ok(SyncResult::NoChanges) => {
                    // No changes to sync - already up to date
                    sync_state.set(SyncState::Synced);
                    last_error.set(None);
                    tracing::debug!("No changes to sync");
                }
                Ok(_) => {
                    sync_state.set(SyncState::Synced);
                    last_error.set(None);
                    // Activate auto-sync after first successful sync
                    if !*sync_activated.peek() {
                        sync_activated.set(true);
                        tracing::debug!("Sync activated - auto-sync enabled");
                    }
                    tracing::debug!("Sync completed successfully");
                }
                Err(e) => {
                    sync_state.set(SyncState::Error);
                    last_error.set(Some(e.to_string()));
                    tracing::warn!("Sync failed: {}", e);
                }
            }
        });
    });

    // Determine display state (drafts can sync too via DraftRef)
    let is_activated = *sync_activated.read();
    let display_state = if !is_authenticated {
        SyncState::Disabled
    } else {
        *sync_state.read()
    };

    // Before activation: show "Start syncing" button
    // After activation: show normal sync states
    let (icon, label, class) = if !is_activated && is_authenticated {
        ("▶", "Start syncing", "sync-status start-sync")
    } else {
        match display_state {
            SyncState::Synced => ("✓", "Synced", "sync-status synced"),
            SyncState::Syncing => ("◌", "Syncing...", "sync-status syncing"),
            SyncState::Unsynced => ("●", "Unsynced", "sync-status unsynced"),
            SyncState::RemoteChanges => ("↓", "Updates", "sync-status remote-changes"),
            SyncState::Error => ("✕", "Sync error", "sync-status error"),
            SyncState::Disabled => ("○", "Sync disabled", "sync-status disabled"),
        }
    };

    // Long-press detection for deactivating sync
    let mut long_press_active = use_signal(|| false);
    #[cfg(target_arch = "wasm32")]
    let mut long_press_timeout: Signal<Option<gloo_timers::callback::Timeout>> =
        use_signal(|| None);

    let on_pointer_down = move |_: dioxus::events::PointerEvent| {
        // Only allow deactivation if sync is currently activated
        if !*sync_activated.peek() {
            return;
        }

        long_press_active.set(true);

        // Start 1 second timer for long press
        #[cfg(target_arch = "wasm32")]
        let timeout = gloo_timers::callback::Timeout::new(1000, move || {
            if *long_press_active.peek() {
                sync_activated.set(false);
                long_press_active.set(false);
                tracing::debug!("Sync deactivated via long press");
            }
        });
        #[cfg(target_arch = "wasm32")]
        long_press_timeout.set(Some(timeout));
    };

    let on_pointer_up = move |_: dioxus::events::PointerEvent| {
        long_press_active.set(false);
        // Cancel the timeout by dropping it
        #[cfg(target_arch = "wasm32")]
        long_press_timeout.set(None);
    };

    let on_pointer_leave = move |_: dioxus::events::PointerEvent| {
        long_press_active.set(false);
        #[cfg(target_arch = "wasm32")]
        long_press_timeout.set(None);
    };

    // Combined sync handler - pulls remote changes first if needed, then pushes local
    let doc_for_sync = doc.clone();
    let on_sync_click = {
        let on_refresh = props.on_refresh.clone();
        let current_state = display_state;
        move |_: dioxus::events::MouseEvent| {
            // Don't trigger click if long press just fired
            if !*sync_activated.peek() && *long_press_active.peek() {
                return;
            }

            if *sync_state.peek() == SyncState::Syncing {
                return; // Already syncing
            }
            // If there are remote changes, pull them first
            if current_state == SyncState::RemoteChanges {
                if let Some(ref handler) = on_refresh {
                    handler.call(());
                }
            }
            // Trigger sync if there are local changes
            if doc_for_sync.has_unsynced_changes() {
                trigger_sync.set(true);
            } else if current_state != SyncState::RemoteChanges {
                sync_state.set(SyncState::Synced);
            }
        }
    };

    // Show tooltip hint about long-press when sync is active
    let title = if is_activated {
        if let Some(ref err) = *last_error.read() {
            err.clone()
        } else {
            format!("{} (hold to stop syncing)", label)
        }
    } else {
        label.to_string()
    };

    rsx! {
        div {
            class: "{class}",
            title: "{title}",
            role: "status",
            aria_live: "polite",
            onclick: on_sync_click,
            onpointerdown: on_pointer_down,
            onpointerup: on_pointer_up,
            onpointerleave: on_pointer_leave,

            span { class: "sync-icon", "{icon}" }
            span { class: "sync-label", "{label}" }
        }
    }
}
