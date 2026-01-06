//! Action execution for editor documents.
//!
//! This module provides the `execute_action` function that applies `EditorAction`
//! operations to any type implementing `EditorDocument`. The logic is generic
//! and platform-agnostic.

use crate::SnapDirection;
use crate::actions::{EditorAction, FormatAction, Range};
use crate::document::EditorDocument;
use crate::platform::{ClipboardPlatform, clipboard_copy, clipboard_cut, clipboard_paste};
use crate::text_helpers::{
    ListContext, detect_list_context, find_line_end, find_line_start, find_word_boundary_backward,
    find_word_boundary_forward, is_list_item_empty,
};
use crate::types::Selection;

/// Determine the cursor snap direction hint for an action.
///
/// Forward means cursor should snap toward new/remaining content (insertions).
/// Backward means cursor should snap toward content before edit (deletions).
pub fn snap_direction_for_action(action: &EditorAction) -> Option<SnapDirection> {
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

/// Execute an editor action on a document.
///
/// This is the central dispatch point for all editor operations.
/// Sets the appropriate snap direction hint before executing.
/// Returns true if the action was handled and the document was modified.
///
/// Note: Clipboard operations (Cut, Copy, CopyAsHtml, Paste) return false here.
/// Use [`execute_action_with_clipboard`] if you have a clipboard platform available.
pub fn execute_action<D: EditorDocument>(doc: &mut D, action: &EditorAction) -> bool {
    // Set pending snap direction before executing action.
    if let Some(snap) = snap_direction_for_action(action) {
        doc.set_pending_snap(Some(snap));
    }

    match action {
        EditorAction::Insert { text, range } => execute_insert(doc, text, *range),
        EditorAction::InsertLineBreak { range } => execute_insert_line_break(doc, *range),
        EditorAction::InsertParagraph { range } => execute_insert_paragraph(doc, *range),
        EditorAction::DeleteBackward { range } => execute_delete_backward(doc, *range),
        EditorAction::DeleteForward { range } => execute_delete_forward(doc, *range),
        EditorAction::DeleteWordBackward { range } => execute_delete_word_backward(doc, *range),
        EditorAction::DeleteWordForward { range } => execute_delete_word_forward(doc, *range),
        EditorAction::DeleteToLineStart { range } => execute_delete_to_line_start(doc, *range),
        EditorAction::DeleteToLineEnd { range } => execute_delete_to_line_end(doc, *range),
        EditorAction::DeleteSoftLineBackward { range } => {
            execute_action(doc, &EditorAction::DeleteToLineStart { range: *range })
        }
        EditorAction::DeleteSoftLineForward { range } => {
            execute_action(doc, &EditorAction::DeleteToLineEnd { range: *range })
        }
        EditorAction::Undo => execute_undo(doc),
        EditorAction::Redo => execute_redo(doc),
        EditorAction::ToggleBold => execute_toggle_format(doc, "**"),
        EditorAction::ToggleItalic => execute_toggle_format(doc, "*"),
        EditorAction::ToggleCode => execute_toggle_format(doc, "`"),
        EditorAction::ToggleStrikethrough => execute_toggle_format(doc, "~~"),
        EditorAction::InsertLink => execute_insert_link(doc),
        EditorAction::Cut | EditorAction::Copy | EditorAction::CopyAsHtml => {
            // Clipboard operations need platform - use execute_action_with_clipboard.
            false
        }
        EditorAction::Paste { range: _ } => {
            // Paste needs platform - use execute_action_with_clipboard.
            false
        }
        EditorAction::SelectAll => execute_select_all(doc),
        EditorAction::MoveCursor { offset } => execute_move_cursor(doc, *offset),
        EditorAction::ExtendSelection { offset } => execute_extend_selection(doc, *offset),
    }
}

/// Execute an editor action with clipboard support.
///
/// Like [`execute_action`], but also handles clipboard operations (Cut, Copy, Paste, CopyAsHtml)
/// using the provided platform implementation.
pub fn execute_action_with_clipboard<D, P>(doc: &mut D, action: &EditorAction, clipboard: &P) -> bool
where
    D: EditorDocument,
    P: ClipboardPlatform,
{
    match action {
        EditorAction::Copy => clipboard_copy(doc, clipboard),
        EditorAction::Cut => clipboard_cut(doc, clipboard),
        EditorAction::Paste { range: _ } => clipboard_paste(doc, clipboard),
        EditorAction::CopyAsHtml => crate::platform::clipboard_copy_as_html(doc, clipboard),
        // Delegate everything else to the regular execute_action.
        _ => execute_action(doc, action),
    }
}

fn execute_insert<D: EditorDocument>(doc: &mut D, text: &str, range: Range) -> bool {
    let range = range.normalize();

    // Clean up any preceding zero-width chars.
    let mut delete_start = range.start;
    while delete_start > 0 {
        match doc.char_at(delete_start - 1) {
            Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
            _ => break,
        }
    }

    let zw_count = range.start - delete_start;

    if range.is_caret() {
        if zw_count > 0 {
            doc.replace(delete_start..range.start, text);
        } else if range.start == doc.len_chars() {
            doc.insert(range.start, text);
        } else {
            doc.insert(range.start, text);
        }
    } else {
        // Replace selection.
        if zw_count > 0 {
            // Delete zero-width chars before selection start too.
            doc.replace(delete_start..range.end, text);
        } else {
            doc.replace(range.start..range.end, text);
        }
    }

    doc.set_selection(None);
    true
}

fn execute_insert_line_break<D: EditorDocument>(doc: &mut D, range: Range) -> bool {
    let range = range.normalize();
    let offset = range.start;

    // Delete selection if any.
    if !range.is_caret() {
        doc.delete(offset..range.end);
    }

    // Check if we're right after a soft break (newline + zero-width char).
    let is_double_enter = if offset >= 2 {
        let prev_char = doc.char_at(offset - 1);
        let prev_prev_char = doc.char_at(offset - 2);
        prev_char == Some('\u{200C}') && prev_prev_char == Some('\n')
    } else {
        false
    };

    if !is_double_enter {
        // Check for list context.
        if let Some(ctx) = detect_list_context(doc, offset) {
            if is_list_item_empty(doc, offset, &ctx) {
                // Empty item - exit list.
                let line_start = find_line_start(doc, offset);
                let line_end = find_line_end(doc, offset);
                let delete_end = (line_end + 1).min(doc.len_chars());
                doc.replace(line_start..delete_end, "\n\n\u{200C}\n");
                doc.set_cursor_offset(line_start + 2);
            } else {
                // Continue list.
                let continuation = list_continuation(&ctx);
                let len = continuation.chars().count();
                doc.insert(offset, &continuation);
                doc.set_cursor_offset(offset + len);
            }
        } else {
            // Normal soft break: insert newline + zero-width char.
            doc.insert(offset, "\n\u{200C}");
            doc.set_cursor_offset(offset + 2);
        }
    } else {
        // Replace zero-width char with newline.
        doc.replace(offset - 1..offset, "\n");
    }

    doc.set_selection(None);
    true
}

fn execute_insert_paragraph<D: EditorDocument>(doc: &mut D, range: Range) -> bool {
    let range = range.normalize();
    let cursor_offset = range.start;

    // Delete selection if any.
    if !range.is_caret() {
        doc.delete(cursor_offset..range.end);
    }

    // Check for list context.
    if let Some(ctx) = detect_list_context(doc, cursor_offset) {
        if is_list_item_empty(doc, cursor_offset, &ctx) {
            // Empty item - exit list.
            let line_start = find_line_start(doc, cursor_offset);
            let line_end = find_line_end(doc, cursor_offset);
            let delete_end = (line_end + 1).min(doc.len_chars());
            doc.replace(line_start..delete_end, "\n\n\u{200C}\n");
            doc.set_cursor_offset(line_start + 2);
        } else {
            // Continue list.
            let continuation = list_continuation(&ctx);
            let len = continuation.chars().count();
            doc.insert(cursor_offset, &continuation);
            doc.set_cursor_offset(cursor_offset + len);
        }
    } else {
        // Normal paragraph break.
        doc.insert(cursor_offset, "\n\n");
        doc.set_cursor_offset(cursor_offset + 2);
    }

    doc.set_selection(None);
    true
}

fn execute_delete_backward<D: EditorDocument>(doc: &mut D, range: Range) -> bool {
    let range = range.normalize();

    if !range.is_caret() {
        // Delete selection.
        doc.delete(range.start..range.end);
        return true;
    }

    if range.start == 0 {
        return false;
    }

    let cursor_offset = range.start;
    let prev_char = doc.char_at(cursor_offset - 1);

    if prev_char == Some('\n') {
        // Deleting a newline - handle paragraph merging.
        let newline_pos = cursor_offset - 1;
        let mut delete_start = newline_pos;
        let mut delete_end = cursor_offset;

        // Check for empty paragraph (double newline).
        if newline_pos > 0 && doc.char_at(newline_pos - 1) == Some('\n') {
            delete_start = newline_pos - 1;
        }

        // Check for trailing zero-width char.
        if let Some(ch) = doc.char_at(delete_end) {
            if ch == '\u{200C}' || ch == '\u{200B}' {
                delete_end += 1;
            }
        }

        // Scan backwards through zero-width chars.
        while delete_start > 0 {
            match doc.char_at(delete_start - 1) {
                Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                Some('\n') | _ => break,
            }
        }

        doc.delete(delete_start..delete_end);
    } else {
        // Normal single char delete.
        doc.delete(cursor_offset - 1..cursor_offset);
    }

    doc.set_selection(None);
    true
}

fn execute_delete_forward<D: EditorDocument>(doc: &mut D, range: Range) -> bool {
    let range = range.normalize();

    if !range.is_caret() {
        doc.delete(range.start..range.end);
        return true;
    }

    if range.start >= doc.len_chars() {
        return false;
    }

    doc.delete(range.start..range.start + 1);
    doc.set_selection(None);
    true
}

fn execute_delete_word_backward<D: EditorDocument>(doc: &mut D, range: Range) -> bool {
    let range = range.normalize();

    if !range.is_caret() {
        doc.delete(range.start..range.end);
        return true;
    }

    let cursor = range.start;
    let word_start = find_word_boundary_backward(doc, cursor);
    if word_start < cursor {
        doc.delete(word_start..cursor);
    }

    doc.set_selection(None);
    true
}

fn execute_delete_word_forward<D: EditorDocument>(doc: &mut D, range: Range) -> bool {
    let range = range.normalize();

    if !range.is_caret() {
        doc.delete(range.start..range.end);
        return true;
    }

    let cursor = range.start;
    let word_end = find_word_boundary_forward(doc, cursor);
    if word_end > cursor {
        doc.delete(cursor..word_end);
    }

    doc.set_selection(None);
    true
}

fn execute_delete_to_line_start<D: EditorDocument>(doc: &mut D, range: Range) -> bool {
    let range = range.normalize();
    let cursor = range.start;
    let line_start = find_line_start(doc, cursor);

    if line_start < cursor {
        doc.delete(line_start..cursor);
    }

    doc.set_selection(None);
    true
}

fn execute_delete_to_line_end<D: EditorDocument>(doc: &mut D, range: Range) -> bool {
    let range = range.normalize();
    let cursor = if range.is_caret() {
        range.start
    } else {
        range.end
    };
    let line_end = find_line_end(doc, cursor);

    if cursor < line_end {
        doc.delete(cursor..line_end);
    }

    doc.set_selection(None);
    true
}

fn execute_undo<D: EditorDocument>(doc: &mut D) -> bool {
    if doc.undo() {
        let max = doc.len_chars();
        let cursor = doc.cursor();
        if cursor.offset > max {
            doc.set_cursor_offset(max);
        }
        doc.set_selection(None);
        true
    } else {
        false
    }
}

fn execute_redo<D: EditorDocument>(doc: &mut D) -> bool {
    if doc.redo() {
        let max = doc.len_chars();
        let cursor = doc.cursor();
        if cursor.offset > max {
            doc.set_cursor_offset(max);
        }
        doc.set_selection(None);
        true
    } else {
        false
    }
}

fn execute_toggle_format<D: EditorDocument>(doc: &mut D, marker: &str) -> bool {
    let cursor_offset = doc.cursor_offset();
    let (start, end) = if let Some(sel) = doc.selection() {
        (sel.start(), sel.end())
    } else {
        find_word_boundaries(doc, cursor_offset)
    };

    // Insert end marker first so start position stays valid.
    doc.insert(end, marker);
    doc.insert(start, marker);
    doc.set_cursor_offset(end + marker.len() * 2);
    doc.set_selection(None);
    true
}

fn execute_insert_link<D: EditorDocument>(doc: &mut D) -> bool {
    let cursor_offset = doc.cursor_offset();
    let (start, end) = if let Some(sel) = doc.selection() {
        (sel.start(), sel.end())
    } else {
        find_word_boundaries(doc, cursor_offset)
    };

    // Insert [selected text](url)
    doc.insert(end, "](url)");
    doc.insert(start, "[");
    doc.set_cursor_offset(end + 8);
    doc.set_selection(None);
    true
}

/// Apply a formatting action to the document.
///
/// Handles markdown formatting operations like bold, italic, headings, lists, etc.
/// If there's a selection, formatting wraps it. Otherwise, behavior depends on the action:
/// - Inline formats (Bold, Italic, etc.) expand to word boundaries
/// - Block formats (Heading, Quote, List) operate on the current line
pub fn apply_formatting<D: EditorDocument>(doc: &mut D, action: FormatAction) -> bool {
    let cursor_offset = doc.cursor_offset();
    let (start, end) = if let Some(sel) = doc.selection() {
        (sel.start(), sel.end())
    } else {
        find_word_boundaries(doc, cursor_offset)
    };

    match action {
        FormatAction::Bold => {
            doc.insert(end, "**");
            doc.insert(start, "**");
            doc.set_cursor_offset(end + 4);
            doc.set_selection(None);
            true
        }
        FormatAction::Italic => {
            doc.insert(end, "*");
            doc.insert(start, "*");
            doc.set_cursor_offset(end + 2);
            doc.set_selection(None);
            true
        }
        FormatAction::Strikethrough => {
            doc.insert(end, "~~");
            doc.insert(start, "~~");
            doc.set_cursor_offset(end + 4);
            doc.set_selection(None);
            true
        }
        FormatAction::Code => {
            doc.insert(end, "`");
            doc.insert(start, "`");
            doc.set_cursor_offset(end + 2);
            doc.set_selection(None);
            true
        }
        FormatAction::Link => {
            doc.insert(end, "](url)");
            doc.insert(start, "[");
            doc.set_cursor_offset(end + 8);
            doc.set_selection(None);
            true
        }
        FormatAction::Image => {
            doc.insert(end, "](url)");
            doc.insert(start, "![");
            doc.set_cursor_offset(end + 9);
            doc.set_selection(None);
            true
        }
        FormatAction::Heading(level) => {
            let line_start = find_line_start(doc, cursor_offset);
            let prefix = "#".repeat(level as usize) + " ";
            let prefix_len = prefix.chars().count();
            doc.insert(line_start, &prefix);
            doc.set_cursor_offset(cursor_offset + prefix_len);
            doc.set_selection(None);
            true
        }
        FormatAction::BulletList => {
            if let Some(ctx) = detect_list_context(doc, cursor_offset) {
                let continuation = match ctx {
                    ListContext::Unordered { indent, marker } => {
                        format!("\n{}{} ", indent, marker)
                    }
                    ListContext::Ordered { .. } => "\n\n - ".to_string(),
                };
                let len = continuation.chars().count();
                doc.insert(cursor_offset, &continuation);
                doc.set_cursor_offset(cursor_offset + len);
            } else {
                let line_start = find_line_start(doc, cursor_offset);
                doc.insert(line_start, " - ");
                doc.set_cursor_offset(cursor_offset + 3);
            }
            doc.set_selection(None);
            true
        }
        FormatAction::NumberedList => {
            if let Some(ctx) = detect_list_context(doc, cursor_offset) {
                let continuation = match ctx {
                    ListContext::Unordered { .. } => "\n\n1. ".to_string(),
                    ListContext::Ordered { indent, number } => {
                        format!("\n{}{}. ", indent, number + 1)
                    }
                };
                let len = continuation.chars().count();
                doc.insert(cursor_offset, &continuation);
                doc.set_cursor_offset(cursor_offset + len);
            } else {
                let line_start = find_line_start(doc, cursor_offset);
                doc.insert(line_start, "1. ");
                doc.set_cursor_offset(cursor_offset + 3);
            }
            doc.set_selection(None);
            true
        }
        FormatAction::Quote => {
            let line_start = find_line_start(doc, cursor_offset);
            doc.insert(line_start, "> ");
            doc.set_cursor_offset(cursor_offset + 2);
            doc.set_selection(None);
            true
        }
    }
}

fn execute_select_all<D: EditorDocument>(doc: &mut D) -> bool {
    let len = doc.len_chars();
    doc.set_selection(Some(Selection::new(0, len)));
    doc.set_cursor_offset(len);
    true
}

fn execute_move_cursor<D: EditorDocument>(doc: &mut D, offset: usize) -> bool {
    let offset = offset.min(doc.len_chars());
    doc.set_cursor_offset(offset);
    doc.set_selection(None);
    true
}

fn execute_extend_selection<D: EditorDocument>(doc: &mut D, offset: usize) -> bool {
    let offset = offset.min(doc.len_chars());
    let anchor = doc
        .selection()
        .map(|s| s.anchor)
        .unwrap_or_else(|| doc.cursor_offset());
    doc.set_selection(Some(Selection::new(anchor, offset)));
    doc.set_cursor_offset(offset);
    true
}

/// Find word boundaries around cursor position.
fn find_word_boundaries<D: EditorDocument>(doc: &D, offset: usize) -> (usize, usize) {
    let len = doc.len_chars();

    // Find start by scanning backwards.
    let mut start = 0;
    for i in (0..offset).rev() {
        match doc.char_at(i) {
            Some(c) if c.is_whitespace() => {
                start = i + 1;
                break;
            }
            Some(_) => continue,
            None => break,
        }
    }

    // Find end by scanning forwards.
    let mut end = len;
    for i in offset..len {
        match doc.char_at(i) {
            Some(c) if c.is_whitespace() => {
                end = i;
                break;
            }
            Some(_) => continue,
            None => break,
        }
    }

    (start, end)
}

/// Generate list continuation text.
fn list_continuation(ctx: &ListContext) -> String {
    match ctx {
        ListContext::Unordered { indent, marker } => {
            format!("\n{}{} ", indent, marker)
        }
        ListContext::Ordered { indent, number } => {
            format!("\n{}{}. ", indent, number + 1)
        }
    }
}

// === Keydown handling ===

use crate::actions::{KeyCombo, KeybindingConfig, KeydownResult};

/// Handle a keydown event using the keybinding configuration.
///
/// This handles keyboard shortcuts only. Text input and deletion
/// are handled by beforeinput. Navigation (arrows, etc.) is passed
/// through to the browser/platform.
///
/// For clipboard operations, use [`handle_keydown_with_clipboard`] instead.
pub fn handle_keydown<D: EditorDocument>(
    doc: &mut D,
    config: &KeybindingConfig,
    combo: KeyCombo,
    range: Range,
) -> KeydownResult {
    // Look up keybinding (range is applied by lookup).
    if let Some(action) = config.lookup(&combo, range) {
        execute_action(doc, &action);
        return KeydownResult::Handled;
    }

    check_passthrough(&combo)
}

/// Handle a keydown event with clipboard support.
///
/// Like [`handle_keydown`], but uses the provided clipboard platform
/// for clipboard operations (Cut, Copy, Paste, CopyAsHtml).
pub fn handle_keydown_with_clipboard<D, P>(
    doc: &mut D,
    config: &KeybindingConfig,
    combo: KeyCombo,
    range: Range,
    clipboard: &P,
) -> KeydownResult
where
    D: EditorDocument,
    P: ClipboardPlatform,
{
    // Look up keybinding (range is applied by lookup).
    if let Some(action) = config.lookup(&combo, range) {
        execute_action_with_clipboard(doc, &action, clipboard);
        return KeydownResult::Handled;
    }

    check_passthrough(&combo)
}

/// Check if a key combo should pass through to the platform.
fn check_passthrough(combo: &KeyCombo) -> KeydownResult {
    // Navigation keys should pass through.
    if combo.key.is_navigation() {
        return KeydownResult::PassThrough;
    }

    // Modifier-only keypresses should pass through.
    if combo.key.is_modifier() {
        return KeydownResult::PassThrough;
    }

    // Content keys (typing, backspace, etc.) - let beforeinput handle.
    KeydownResult::NotHandled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditorRope, PlainEditor, UndoableBuffer};

    type TestEditor = PlainEditor<UndoableBuffer<EditorRope>>;

    fn make_editor(content: &str) -> TestEditor {
        let rope = EditorRope::from_str(content);
        let buf = UndoableBuffer::new(rope, 100);
        PlainEditor::new(buf)
    }

    #[test]
    fn test_insert() {
        let mut editor = make_editor("hello");
        let action = EditorAction::Insert {
            text: " world".to_string(),
            range: Range::caret(5),
        };
        assert!(execute_action(&mut editor, &action));
        assert_eq!(editor.content_string(), "hello world");
    }

    #[test]
    fn test_delete_backward() {
        let mut editor = make_editor("hello");
        editor.set_cursor_offset(5);
        let action = EditorAction::DeleteBackward {
            range: Range::caret(5),
        };
        assert!(execute_action(&mut editor, &action));
        assert_eq!(editor.content_string(), "hell");
    }

    #[test]
    fn test_delete_selection() {
        let mut editor = make_editor("hello world");
        editor.set_selection(Some(Selection::new(5, 11)));
        let action = EditorAction::DeleteBackward {
            range: Range::new(5, 11),
        };
        assert!(execute_action(&mut editor, &action));
        assert_eq!(editor.content_string(), "hello");
    }

    #[test]
    fn test_undo_redo() {
        let mut editor = make_editor("hello");

        let action = EditorAction::Insert {
            text: " world".to_string(),
            range: Range::caret(5),
        };
        execute_action(&mut editor, &action);
        assert_eq!(editor.content_string(), "hello world");

        assert!(execute_action(&mut editor, &EditorAction::Undo));
        assert_eq!(editor.content_string(), "hello");

        assert!(execute_action(&mut editor, &EditorAction::Redo));
        assert_eq!(editor.content_string(), "hello world");
    }

    #[test]
    fn test_select_all() {
        let mut editor = make_editor("hello world");
        assert!(execute_action(&mut editor, &EditorAction::SelectAll));
        let sel = editor.selection().unwrap();
        assert_eq!(sel.start(), 0);
        assert_eq!(sel.end(), 11);
    }

    #[test]
    fn test_toggle_bold() {
        let mut editor = make_editor("hello");
        editor.set_selection(Some(Selection::new(0, 5)));
        assert!(execute_action(&mut editor, &EditorAction::ToggleBold));
        assert_eq!(editor.content_string(), "**hello**");
    }
}
