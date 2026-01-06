//! Action execution for editor documents.
//!
//! This module provides the `execute_action` function that applies `EditorAction`
//! operations to any type implementing `EditorDocument`. The logic is generic
//! and platform-agnostic.

use crate::actions::{EditorAction, Range};
use crate::document::EditorDocument;
use crate::text_helpers::{
    ListContext, detect_list_context, find_line_end, find_line_start, find_word_boundary_backward,
    find_word_boundary_forward, is_list_item_empty,
};
use crate::types::Selection;

/// Execute an editor action on a document.
///
/// This is the central dispatch point for all editor operations.
/// Returns true if the action was handled and the document was modified.
pub fn execute_action<D: EditorDocument>(doc: &mut D, action: &EditorAction) -> bool {
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
            // Clipboard operations are handled by platform layer.
            false
        }
        EditorAction::Paste { range: _ } => {
            // Paste is handled by platform layer with clipboard access.
            false
        }
        EditorAction::SelectAll => execute_select_all(doc),
        EditorAction::MoveCursor { offset } => execute_move_cursor(doc, *offset),
        EditorAction::ExtendSelection { offset } => execute_extend_selection(doc, *offset),
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
