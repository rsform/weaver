//! Remote collaborator cursor overlays.
//!
//! Renders cursor indicators for each remote collaborator in a real-time
//! editing session. Uses the same offset mapping as local cursor restoration.

use dioxus::prelude::*;

use super::document::SignalEditorDocument;

/// Remote collaborator cursors overlay.
///
/// Renders cursor indicators for each remote collaborator.
/// Uses the same offset mapping as local cursor restoration.
#[component]
pub fn RemoteCursors(
    presence: Signal<weaver_common::transport::PresenceSnapshot>,
    document: SignalEditorDocument,
    render_cache: Signal<weaver_editor_browser::RenderCache>,
) -> Element {
    let presence_read = presence.read();
    let cursor_count = presence_read.collaborators.len();
    let cursors: Vec<_> = presence_read
        .collaborators
        .iter()
        .filter_map(|c| {
            c.cursor_position
                .map(|pos| (c.display_name.clone(), c.color, pos, c.selection))
        })
        .collect();

    if cursor_count > 0 {
        tracing::debug!(
            "RemoteCursors: {} collaborators, {} with cursors",
            cursor_count,
            cursors.len()
        );
    }

    if cursors.is_empty() {
        return rsx! {};
    }

    // Get flattened offset map from all paragraphs.
    let cache = render_cache.read();
    let offset_map: Vec<_> = cache
        .paragraphs
        .iter()
        .flat_map(|p| p.offset_map.iter().cloned())
        .collect();

    rsx! {
        div { class: "remote-cursors-overlay",
            for (display_name, color, position, selection) in cursors {
                RemoteCursorIndicator {
                    key: "{display_name}-{position}",
                    display_name,
                    position,
                    selection,
                    color,
                    offset_map: offset_map.clone(),
                }
            }
        }
    }
}

/// Single remote cursor indicator with DOM-based positioning.
#[component]
fn RemoteCursorIndicator(
    display_name: String,
    position: usize,
    selection: Option<(usize, usize)>,
    color: u32,
    offset_map: Vec<weaver_editor_core::OffsetMapping>,
) -> Element {
    use weaver_editor_browser::{
        get_cursor_rect_relative, get_selection_rects_relative, rgba_u32_to_css,
        rgba_u32_to_css_alpha,
    };

    let color_css = rgba_u32_to_css(color);
    let selection_color_css = rgba_u32_to_css_alpha(color, 0.25);

    // Get cursor position relative to editor.
    let rect = get_cursor_rect_relative(position, &offset_map, "markdown-editor");

    // Get selection rectangles if there's a selection.
    let selection_rects = if let Some((start, end)) = selection {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        get_selection_rects_relative(start, end, &offset_map, "markdown-editor")
    } else {
        vec![]
    };

    let Some(rect) = rect else {
        tracing::debug!(
            "RemoteCursorIndicator: no rect for position {} (offset_map len: {})",
            position,
            offset_map.len()
        );
        return rsx! {};
    };

    tracing::trace!(
        "RemoteCursorIndicator: {} at ({}, {}) h={}, selection_rects={}",
        display_name,
        rect.x,
        rect.y,
        rect.height,
        selection_rects.len()
    );

    let style = format!(
        "left: {}px; top: {}px; --cursor-height: {}px; --cursor-color: {};",
        rect.x, rect.y, rect.height, color_css
    );

    rsx! {
        // Selection highlight rectangles (rendered behind cursor).
        for (i, sel_rect) in selection_rects.iter().enumerate() {
            div {
                key: "sel-{i}",
                class: "remote-selection",
                style: "left: {sel_rect.x}px; top: {sel_rect.y}px; width: {sel_rect.width}px; height: {sel_rect.height}px; background-color: {selection_color_css};",
            }
        }

        div {
            class: "remote-cursor",
            style: "{style}",

            // Cursor caret line.
            div { class: "remote-cursor-caret" }

            // Name label.
            div { class: "remote-cursor-label",
                "{display_name}"
            }
        }
    }
}
