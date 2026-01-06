//! BeforeInput event handling for the editor.
//!
//! This module provides the primary input handling via the `beforeinput` event,
//! which gives us semantic information about what the browser wants to do
//! (insert text, delete backward, etc.) rather than raw key codes.
//!
//! The core logic is in `weaver_editor_browser::handle_beforeinput`. This module
//! adds app-specific concerns like `pending_snap` for cursor snapping direction.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use super::document::SignalEditorDocument;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use dioxus::prelude::*;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use weaver_editor_core::SnapDirection;

// Re-export types from extracted crates.
pub use weaver_editor_browser::{BeforeInputContext, BeforeInputResult};
pub use weaver_editor_core::InputType;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use weaver_editor_browser::StaticRange;

/// Determine the cursor snap direction hint for an input type.
///
/// This is used to hint `dom_sync` which direction to snap the cursor if it
/// lands on invisible content after an edit.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn snap_direction_for_input_type(input_type: &InputType) -> Option<SnapDirection> {
    match input_type {
        // Forward: cursor should snap toward new/remaining content after the edit.
        InputType::InsertLineBreak
        | InputType::InsertParagraph
        | InputType::DeleteContentForward
        | InputType::DeleteWordForward
        | InputType::DeleteEntireWordForward
        | InputType::DeleteSoftLineForward
        | InputType::DeleteHardLineForward => Some(SnapDirection::Forward),

        // Backward: cursor should snap toward content before the deleted range.
        InputType::DeleteContentBackward
        | InputType::DeleteWordBackward
        | InputType::DeleteEntireWordBackward
        | InputType::DeleteSoftLineBackward
        | InputType::DeleteHardLineBackward => Some(SnapDirection::Backward),

        // No snap hint for other operations.
        _ => None,
    }
}

/// Handle a beforeinput event.
///
/// This is the main entry point for beforeinput-based input handling.
/// Sets `pending_snap` for cursor snapping, then delegates to the browser crate.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn handle_beforeinput(
    doc: &mut SignalEditorDocument,
    ctx: BeforeInputContext<'_>,
) -> BeforeInputResult {
    // Set pending_snap hint before executing the action.
    if let Some(snap) = snap_direction_for_input_type(&ctx.input_type) {
        doc.pending_snap.set(Some(snap));
    }

    // Get current range for the browser handler.
    let current_range = weaver_editor_browser::get_current_range(doc);

    // Delegate to browser crate's generic handler.
    weaver_editor_browser::handle_beforeinput(doc, &ctx, current_range)
}
