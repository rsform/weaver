//! Editor actions and keybinding system.
//!
//! This module re-exports core types and provides Dioxus-specific conversions.
//! Action execution delegates to `weaver_editor_core::execute_action`.

use dioxus::prelude::*;

use super::document::SignalEditorDocument;
use weaver_editor_browser::Platform;
use weaver_editor_core::SnapDirection;

// Re-export core types.
pub use weaver_editor_core::{
    EditorAction, FormatAction, Key, KeyCombo, KeybindingConfig, KeydownResult, Modifiers, Range,
    apply_formatting,
};

/// Determine the cursor snap direction hint for an action.
fn snap_direction_for_action(action: &EditorAction) -> Option<SnapDirection> {
    match action {
        // Forward: cursor should snap toward new/remaining content.
        EditorAction::InsertLineBreak { .. }
        | EditorAction::InsertParagraph { .. }
        | EditorAction::DeleteForward { .. }
        | EditorAction::DeleteWordForward { .. }
        | EditorAction::DeleteToLineEnd { .. }
        | EditorAction::DeleteSoftLineForward { .. } => Some(SnapDirection::Forward),

        // Backward: cursor should snap toward content before edit.
        EditorAction::DeleteBackward { .. }
        | EditorAction::DeleteWordBackward { .. }
        | EditorAction::DeleteToLineStart { .. }
        | EditorAction::DeleteSoftLineBackward { .. } => Some(SnapDirection::Backward),

        _ => None,
    }
}

// === Dioxus conversion helpers ===

/// Convert a dioxus keyboard_types::Key to our Key type.
pub fn key_from_dioxus(key: dioxus::prelude::keyboard_types::Key) -> Key {
    use dioxus::prelude::keyboard_types::Key as KT;

    match key {
        KT::Character(s) => Key::character(s.as_str()),
        KT::Unidentified => Key::Unidentified,
        KT::Backspace => Key::Backspace,
        KT::Delete => Key::Delete,
        KT::Enter => Key::Enter,
        KT::Tab => Key::Tab,
        KT::Escape => Key::Escape,
        KT::Insert => Key::Insert,
        KT::Clear => Key::Clear,
        KT::ArrowLeft => Key::ArrowLeft,
        KT::ArrowRight => Key::ArrowRight,
        KT::ArrowUp => Key::ArrowUp,
        KT::ArrowDown => Key::ArrowDown,
        KT::Home => Key::Home,
        KT::End => Key::End,
        KT::PageUp => Key::PageUp,
        KT::PageDown => Key::PageDown,
        KT::Alt => Key::Alt,
        KT::AltGraph => Key::AltGraph,
        KT::CapsLock => Key::CapsLock,
        KT::Control => Key::Control,
        KT::Fn => Key::Fn,
        KT::FnLock => Key::FnLock,
        KT::Meta => Key::Meta,
        KT::NumLock => Key::NumLock,
        KT::ScrollLock => Key::ScrollLock,
        KT::Shift => Key::Shift,
        KT::Symbol => Key::Symbol,
        KT::SymbolLock => Key::SymbolLock,
        KT::Hyper => Key::Hyper,
        KT::Super => Key::Super,
        KT::F1 => Key::F1,
        KT::F2 => Key::F2,
        KT::F3 => Key::F3,
        KT::F4 => Key::F4,
        KT::F5 => Key::F5,
        KT::F6 => Key::F6,
        KT::F7 => Key::F7,
        KT::F8 => Key::F8,
        KT::F9 => Key::F9,
        KT::F10 => Key::F10,
        KT::F11 => Key::F11,
        KT::F12 => Key::F12,
        KT::F13 => Key::F13,
        KT::F14 => Key::F14,
        KT::F15 => Key::F15,
        KT::F16 => Key::F16,
        KT::F17 => Key::F17,
        KT::F18 => Key::F18,
        KT::F19 => Key::F19,
        KT::F20 => Key::F20,
        KT::ContextMenu => Key::ContextMenu,
        KT::PrintScreen => Key::PrintScreen,
        KT::Pause => Key::Pause,
        KT::Help => Key::Help,
        KT::Copy => Key::Copy,
        KT::Cut => Key::Cut,
        KT::Paste => Key::Paste,
        KT::Undo => Key::Undo,
        KT::Redo => Key::Redo,
        KT::Find => Key::Find,
        KT::Select => Key::Select,
        KT::MediaPlayPause => Key::MediaPlayPause,
        KT::MediaStop => Key::MediaStop,
        KT::MediaTrackNext => Key::MediaTrackNext,
        KT::MediaTrackPrevious => Key::MediaTrackPrevious,
        KT::AudioVolumeDown => Key::AudioVolumeDown,
        KT::AudioVolumeUp => Key::AudioVolumeUp,
        KT::AudioVolumeMute => Key::AudioVolumeMute,
        KT::Compose => Key::Compose,
        KT::Convert => Key::Convert,
        KT::NonConvert => Key::NonConvert,
        KT::Dead => Key::Dead,
        KT::HangulMode => Key::HangulMode,
        KT::HanjaMode => Key::HanjaMode,
        KT::JunjaMode => Key::JunjaMode,
        KT::Eisu => Key::Eisu,
        KT::Hankaku => Key::Hankaku,
        KT::Hiragana => Key::Hiragana,
        KT::HiraganaKatakana => Key::HiraganaKatakana,
        KT::KanaMode => Key::KanaMode,
        KT::KanjiMode => Key::KanjiMode,
        KT::Katakana => Key::Katakana,
        KT::Romaji => Key::Romaji,
        KT::Zenkaku => Key::Zenkaku,
        KT::ZenkakuHankaku => Key::ZenkakuHankaku,
        _ => Key::Unidentified,
    }
}

/// Create a KeyCombo from a dioxus keyboard event.
pub fn keycombo_from_dioxus_event(event: &dioxus::events::KeyboardData) -> KeyCombo {
    let key = key_from_dioxus(event.key());
    let modifiers = Modifiers {
        ctrl: event.modifiers().ctrl(),
        alt: event.modifiers().alt(),
        shift: event.modifiers().shift(),
        meta: event.modifiers().meta(),
        hyper: false,
        super_: false,
    };
    KeyCombo::with_modifiers(key, modifiers)
}

/// Create a default KeybindingConfig for the given platform.
pub fn default_keybindings(platform: &Platform) -> KeybindingConfig {
    KeybindingConfig::default_for_platform(platform.mac)
}

/// Execute an editor action on a document.
///
/// This is the central dispatch point for all editor operations.
/// Returns true if the action was handled and the document was modified.
pub fn execute_action(doc: &mut SignalEditorDocument, action: &EditorAction) -> bool {
    use super::input::{
        detect_list_context, find_line_end, find_line_start, get_char_at, is_list_item_empty,
    };
    use weaver_editor_core::SnapDirection;

    match action {
        EditorAction::Insert { text, range } => {
            let range = range.normalize();
            if range.is_caret() {
                // Simple insert
                let offset = range.start;

                // Clean up any preceding zero-width chars
                let mut delete_start = offset;
                while delete_start > 0 {
                    match get_char_at(doc.loro_text(), delete_start - 1) {
                        Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                        _ => break,
                    }
                }

                let zw_count = offset - delete_start;
                if zw_count > 0 {
                    let _ = doc.replace_tracked(delete_start, zw_count, text);
                    doc.cursor.write().offset = delete_start + text.chars().count();
                } else if offset == doc.len_chars() {
                    let _ = doc.push_tracked(text);
                    doc.cursor.write().offset = offset + text.chars().count();
                } else {
                    let _ = doc.insert_tracked(offset, text);
                    doc.cursor.write().offset = offset + text.chars().count();
                }
            } else {
                // Replace range
                let _ = doc.replace_tracked(range.start, range.len(), text);
                doc.cursor.write().offset = range.start + text.chars().count();
            }
            doc.selection.set(None);
            true
        }

        EditorAction::InsertLineBreak { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            let offset = range.start;
            if !range.is_caret() {
                let _ = doc.remove_tracked(offset, range.len());
            }

            // Check if we're right after a soft break (newline + zero-width char).
            // If so, convert to paragraph break by replacing the zero-width char
            // with a newline.
            let is_double_enter = if offset >= 2 {
                let prev_char = get_char_at(doc.loro_text(), offset - 1);
                let prev_prev_char = get_char_at(doc.loro_text(), offset - 2);
                if prev_char == Some('\u{200C}') && prev_prev_char == Some('\n') {
                    true
                } else {
                    false
                }
            } else {
                false
            };

            if !is_double_enter {
                // Check for list context
                if let Some(ctx) = detect_list_context(doc.loro_text(), offset) {
                    tracing::debug!("List context detected: {:?}", ctx);
                    if is_list_item_empty(doc.loro_text(), offset, &ctx) {
                        // Empty item - exit list
                        let line_start = find_line_start(doc.loro_text(), offset);
                        let line_end = find_line_end(doc.loro_text(), offset);
                        let delete_end = (line_end + 1).min(doc.len_chars());

                        let _ = doc.replace_tracked(
                            line_start,
                            delete_end.saturating_sub(line_start),
                            "\n\n\u{200C}\n",
                        );
                        doc.cursor.write().offset = line_start + 2;
                        tracing::debug!("empty list");
                    } else {
                        // Continue list
                        let continuation = match ctx {
                            super::input::ListContext::Unordered { indent, marker } => {
                                format!("\n{}{} ", indent, marker)
                            }
                            super::input::ListContext::Ordered { indent, number } => {
                                format!("\n{}{}. ", indent, number + 1)
                            }
                        };
                        let len = continuation.chars().count();
                        let _ = doc.insert_tracked(offset, &continuation);
                        doc.cursor.write().offset = offset + len;
                        tracing::debug!("continuation {}", continuation);
                    }
                } else {
                    // Normal soft break: insert newline + zero-width char for cursor positioning.
                    let _ = doc.insert_tracked(offset, "\n\u{200C}");
                    doc.cursor.write().offset = offset + 2;
                }
            } else {
                // Replace zero-width char with newline
                let _ = doc.replace_tracked(offset - 1, 1, "\n");
                doc.cursor.write().offset = offset;
            }

            doc.selection.set(None);
            true
        }

        EditorAction::InsertParagraph { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            let cursor_offset = range.start;
            if !range.is_caret() {
                let _ = doc.remove_tracked(cursor_offset, range.len());
            }

            // Check for list context
            if let Some(ctx) = detect_list_context(doc.loro_text(), cursor_offset) {
                if is_list_item_empty(doc.loro_text(), cursor_offset, &ctx) {
                    // Empty item - exit list
                    let line_start = find_line_start(doc.loro_text(), cursor_offset);
                    let line_end = find_line_end(doc.loro_text(), cursor_offset);
                    let delete_end = (line_end + 1).min(doc.len_chars());

                    let _ = doc.replace_tracked(
                        line_start,
                        delete_end.saturating_sub(line_start),
                        "\n\n\u{200C}\n",
                    );
                    doc.cursor.write().offset = line_start + 2;
                } else {
                    // Continue list
                    let continuation = match ctx {
                        super::input::ListContext::Unordered { indent, marker } => {
                            format!("\n{}{} ", indent, marker)
                        }
                        super::input::ListContext::Ordered { indent, number } => {
                            format!("\n{}{}. ", indent, number + 1)
                        }
                    };
                    let len = continuation.chars().count();
                    let _ = doc.insert_tracked(cursor_offset, &continuation);
                    doc.cursor.write().offset = cursor_offset + len;
                }
            } else {
                // Normal paragraph break
                let _ = doc.insert_tracked(cursor_offset, "\n\n");
                doc.cursor.write().offset = cursor_offset + 2;
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteBackward { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Backward));

            if !range.is_caret() {
                // Delete selection
                let _ = doc.remove_tracked(range.start, range.len());
                doc.cursor.write().offset = range.start;
            } else if range.start > 0 {
                let cursor_offset = range.start;
                let prev_char = get_char_at(doc.loro_text(), cursor_offset - 1);

                if prev_char == Some('\n') {
                    // Deleting a newline - handle paragraph merging
                    let newline_pos = cursor_offset - 1;
                    let mut delete_start = newline_pos;
                    let mut delete_end = cursor_offset;

                    // Check for empty paragraph (double newline)
                    if newline_pos > 0 {
                        if get_char_at(doc.loro_text(), newline_pos - 1) == Some('\n') {
                            delete_start = newline_pos - 1;
                        }
                    }

                    // Check for trailing zero-width char
                    if let Some(ch) = get_char_at(doc.loro_text(), delete_end) {
                        if ch == '\u{200C}' || ch == '\u{200B}' {
                            delete_end += 1;
                        }
                    }

                    // Scan backwards through zero-width chars
                    while delete_start > 0 {
                        match get_char_at(doc.loro_text(), delete_start - 1) {
                            Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                            Some('\n') | _ => break,
                        }
                    }

                    let _ =
                        doc.remove_tracked(delete_start, delete_end.saturating_sub(delete_start));
                    doc.cursor.write().offset = delete_start;
                } else {
                    // Normal single char delete
                    let _ = doc.remove_tracked(cursor_offset - 1, 1);
                    doc.cursor.write().offset = cursor_offset - 1;
                }
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteForward { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            if !range.is_caret() {
                let _ = doc.remove_tracked(range.start, range.len());
                doc.cursor.write().offset = range.start;
            } else if range.start < doc.len_chars() {
                let _ = doc.remove_tracked(range.start, 1);
                // Cursor stays at same position
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteWordBackward { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Backward));

            if !range.is_caret() {
                let _ = doc.remove_tracked(range.start, range.len());
                doc.cursor.write().offset = range.start;
            } else {
                // Find word boundary backwards
                let cursor = range.start;
                let word_start = find_word_boundary_backward(doc, cursor);
                if word_start < cursor {
                    let _ = doc.remove_tracked(word_start, cursor - word_start);
                    doc.cursor.write().offset = word_start;
                }
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteWordForward { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            if !range.is_caret() {
                let _ = doc.remove_tracked(range.start, range.len());
                doc.cursor.write().offset = range.start;
            } else {
                // Find word boundary forward
                let cursor = range.start;
                let word_end = find_word_boundary_forward(doc, cursor);
                if word_end > cursor {
                    let _ = doc.remove_tracked(cursor, word_end - cursor);
                }
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteToLineStart { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Backward));

            let cursor = if range.is_caret() {
                range.start
            } else {
                range.start
            };
            let line_start = find_line_start(doc.loro_text(), cursor);

            if line_start < cursor {
                let _ = doc.remove_tracked(line_start, cursor - line_start);
                doc.cursor.write().offset = line_start;
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteToLineEnd { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            let cursor = if range.is_caret() {
                range.start
            } else {
                range.end
            };
            let line_end = find_line_end(doc.loro_text(), cursor);

            if cursor < line_end {
                let _ = doc.remove_tracked(cursor, line_end - cursor);
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteSoftLineBackward { range } => {
            // For now, treat same as DeleteToLineStart
            // TODO: Handle visual line wrapping if needed
            execute_action(doc, &EditorAction::DeleteToLineStart { range: *range })
        }

        EditorAction::DeleteSoftLineForward { range } => {
            // For now, treat same as DeleteToLineEnd
            execute_action(doc, &EditorAction::DeleteToLineEnd { range: *range })
        }

        EditorAction::Undo => {
            if let Ok(true) = doc.undo() {
                let max = doc.len_chars();
                doc.cursor.with_mut(|c| c.offset = c.offset.min(max));
                doc.selection.set(None);
                true
            } else {
                false
            }
        }

        EditorAction::Redo => {
            if let Ok(true) = doc.redo() {
                let max = doc.len_chars();
                doc.cursor.with_mut(|c| c.offset = c.offset.min(max));
                doc.selection.set(None);
                true
            } else {
                false
            }
        }

        EditorAction::ToggleBold => {
            apply_formatting(doc, FormatAction::Bold);
            true
        }

        EditorAction::ToggleItalic => {
            apply_formatting(doc, FormatAction::Italic);
            true
        }

        EditorAction::ToggleCode => {
            apply_formatting(doc, FormatAction::Code);
            true
        }

        EditorAction::ToggleStrikethrough => {
            apply_formatting(doc, FormatAction::Strikethrough);
            true
        }

        EditorAction::InsertLink => {
            apply_formatting(doc, FormatAction::Link);
            true
        }

        EditorAction::Cut => {
            // Handled separately via clipboard events
            false
        }

        EditorAction::Copy => {
            // Handled separately via clipboard events
            false
        }

        EditorAction::Paste { range: _ } => {
            // Handled separately via clipboard events (needs async clipboard access)
            false
        }

        EditorAction::CopyAsHtml => {
            // Handled in component with async clipboard access
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            {
                if let Some(sel) = *doc.selection.read() {
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    if start != end {
                        if let Some(markdown) = doc.slice(start, end) {
                            let clean_md = markdown.replace('\u{200C}', "").replace('\u{200B}', "");
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Err(e) = super::input::copy_as_html(&clean_md).await {
                                    tracing::warn!("[COPY HTML] Failed: {:?}", e);
                                }
                            });
                            return true;
                        }
                    }
                }
            }
            false
        }

        EditorAction::SelectAll => {
            let len = doc.len_chars();
            doc.selection.set(Some(super::document::Selection {
                anchor: 0,
                head: len,
            }));
            doc.cursor.write().offset = len;
            true
        }

        EditorAction::MoveCursor { offset } => {
            let offset = (*offset).min(doc.len_chars());
            doc.cursor.write().offset = offset;
            doc.selection.set(None);
            true
        }

        EditorAction::ExtendSelection { offset } => {
            let offset = (*offset).min(doc.len_chars());
            let current_sel = *doc.selection.read();
            let anchor = current_sel
                .map(|s| s.anchor)
                .unwrap_or(doc.cursor.read().offset);
            doc.selection.set(Some(super::document::Selection {
                anchor,
                head: offset,
            }));
            doc.cursor.write().offset = offset;
            true
        }
    }
}

/// Find word boundary backward from cursor.
fn find_word_boundary_backward(doc: &SignalEditorDocument, cursor: usize) -> usize {
    use super::input::get_char_at;

    if cursor == 0 {
        return 0;
    }

    let mut pos = cursor;

    // Skip any whitespace/punctuation immediately before cursor
    while pos > 0 {
        match get_char_at(doc.loro_text(), pos - 1) {
            Some(c) if c.is_alphanumeric() || c == '_' => break,
            Some(_) => pos -= 1,
            None => break,
        }
    }

    // Skip the word characters
    while pos > 0 {
        match get_char_at(doc.loro_text(), pos - 1) {
            Some(c) if c.is_alphanumeric() || c == '_' => pos -= 1,
            _ => break,
        }
    }

    pos
}

/// Find word boundary forward from cursor.
fn find_word_boundary_forward(doc: &SignalEditorDocument, cursor: usize) -> usize {
    use super::input::get_char_at;

    let len = doc.len_chars();
    if cursor >= len {
        return len;
    }

    let mut pos = cursor;

    // Skip word characters first
    while pos < len {
        match get_char_at(doc.loro_text(), pos) {
            Some(c) if c.is_alphanumeric() || c == '_' => pos += 1,
            _ => break,
        }
    }

    // Then skip whitespace/punctuation
    while pos < len {
        match get_char_at(doc.loro_text(), pos) {
            Some(c) if c.is_alphanumeric() || c == '_' => break,
            Some(_) => pos += 1,
            None => break,
        }
    }

    pos
}

/// Handle a keydown event using the keybinding configuration.
///
/// This handles keyboard shortcuts only. Text input and deletion
/// are handled by beforeinput. Navigation (arrows, etc.) is passed
/// through to the browser.
pub fn handle_keydown_with_bindings(
    doc: &mut SignalEditorDocument,
    config: &KeybindingConfig,
    combo: KeyCombo,
    range: Range,
) -> KeydownResult {
    // Look up keybinding (range is applied by lookup)
    if let Some(action) = config.lookup(&combo, range) {
        execute_action(doc, &action);
        return KeydownResult::Handled;
    }

    // No keybinding matched - check if this is navigation or content
    if combo.key.is_navigation() {
        return KeydownResult::PassThrough;
    }

    // Modifier-only keypresses should pass through
    if combo.key.is_modifier() {
        return KeydownResult::PassThrough;
    }

    // Content keys (typing, backspace, etc.) - let beforeinput handle
    KeydownResult::NotHandled
}
