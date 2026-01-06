//! Text navigation and analysis helpers.
//!
//! These functions work with the `EditorDocument` trait to provide
//! common text operations like finding line boundaries and word boundaries.

use crate::document::EditorDocument;

/// Find start of line containing offset.
pub fn find_line_start<D: EditorDocument>(doc: &D, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }

    let mut pos = offset;
    while pos > 0 {
        if let Some('\n') = doc.char_at(pos - 1) {
            return pos;
        }
        pos -= 1;
    }
    0
}

/// Find end of line containing offset (position of newline or end of doc).
pub fn find_line_end<D: EditorDocument>(doc: &D, offset: usize) -> usize {
    let len = doc.len_chars();
    if offset >= len {
        return len;
    }

    let mut pos = offset;
    while pos < len {
        if let Some('\n') = doc.char_at(pos) {
            return pos;
        }
        pos += 1;
    }
    len
}

/// Find word boundary backward from cursor.
pub fn find_word_boundary_backward<D: EditorDocument>(doc: &D, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }

    let mut pos = cursor;

    // Skip any whitespace/punctuation immediately before cursor.
    while pos > 0 {
        match doc.char_at(pos - 1) {
            Some(c) if c.is_alphanumeric() || c == '_' => break,
            Some(_) => pos -= 1,
            None => break,
        }
    }

    // Skip the word characters.
    while pos > 0 {
        match doc.char_at(pos - 1) {
            Some(c) if c.is_alphanumeric() || c == '_' => pos -= 1,
            _ => break,
        }
    }

    pos
}

/// Find word boundary forward from cursor.
pub fn find_word_boundary_forward<D: EditorDocument>(doc: &D, cursor: usize) -> usize {
    let len = doc.len_chars();
    if cursor >= len {
        return len;
    }

    let mut pos = cursor;

    // Skip word characters first.
    while pos < len {
        match doc.char_at(pos) {
            Some(c) if c.is_alphanumeric() || c == '_' => pos += 1,
            _ => break,
        }
    }

    // Then skip whitespace/punctuation.
    while pos < len {
        match doc.char_at(pos) {
            Some(c) if c.is_alphanumeric() || c == '_' => break,
            Some(_) => pos += 1,
            None => break,
        }
    }

    pos
}

/// Describes what kind of list item the cursor is in, if any.
#[derive(Debug, Clone)]
pub enum ListContext {
    /// Unordered list with the given marker char ('-' or '*') and indentation.
    Unordered { indent: String, marker: char },
    /// Ordered list with the current number and indentation.
    Ordered { indent: String, number: usize },
}

/// Detect if cursor is in a list item and return context for continuation.
pub fn detect_list_context<D: EditorDocument>(doc: &D, cursor_offset: usize) -> Option<ListContext> {
    let line_start = find_line_start(doc, cursor_offset);
    let line_end = find_line_end(doc, cursor_offset);

    if line_start >= line_end {
        return None;
    }

    let line = doc.slice(line_start..line_end)?;

    // Parse indentation.
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let trimmed = &line[indent.len()..];

    // Check for unordered list marker: "- " or "* ".
    if trimmed.starts_with("- ") {
        return Some(ListContext::Unordered {
            indent,
            marker: '-',
        });
    }
    if trimmed.starts_with("* ") {
        return Some(ListContext::Unordered {
            indent,
            marker: '*',
        });
    }

    // Check for ordered list marker: "1. ", "2. ", etc.
    if let Some(dot_pos) = trimmed.find(". ") {
        let num_part = &trimmed[..dot_pos];
        if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(number) = num_part.parse::<usize>() {
                return Some(ListContext::Ordered { indent, number });
            }
        }
    }

    None
}

/// Check if the current list item is empty (just the marker, no content).
pub fn is_list_item_empty<D: EditorDocument>(
    doc: &D,
    cursor_offset: usize,
    ctx: &ListContext,
) -> bool {
    let line_start = find_line_start(doc, cursor_offset);
    let line_end = find_line_end(doc, cursor_offset);

    let line = match doc.slice(line_start..line_end) {
        Some(s) => s,
        None => return false,
    };

    // Calculate expected marker length.
    let marker_len = match ctx {
        ListContext::Unordered { indent, .. } => indent.len() + 2, // "- "
        ListContext::Ordered { indent, number } => {
            indent.len() + number.to_string().len() + 2 // "1. "
        }
    };

    line.len() <= marker_len
}

/// Count leading zero-width characters before offset.
pub fn count_leading_zero_width<D: EditorDocument>(doc: &D, offset: usize) -> usize {
    let mut count = 0;
    let mut pos = offset;

    while pos > 0 {
        match doc.char_at(pos - 1) {
            Some('\u{200C}') | Some('\u{200B}') => {
                count += 1;
                pos -= 1;
            }
            _ => break,
        }
    }

    count
}

/// Check if character at offset is a zero-width character.
pub fn is_zero_width_char<D: EditorDocument>(doc: &D, offset: usize) -> bool {
    matches!(doc.char_at(offset), Some('\u{200C}') | Some('\u{200B}'))
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
    fn test_find_line_start() {
        let editor = make_editor("hello\nworld\ntest");

        assert_eq!(find_line_start(&editor, 0), 0);
        assert_eq!(find_line_start(&editor, 3), 0);
        assert_eq!(find_line_start(&editor, 5), 0); // at newline
        assert_eq!(find_line_start(&editor, 6), 6); // start of "world"
        assert_eq!(find_line_start(&editor, 8), 6);
        assert_eq!(find_line_start(&editor, 12), 12); // start of "test"
    }

    #[test]
    fn test_find_line_end() {
        let editor = make_editor("hello\nworld\ntest");

        assert_eq!(find_line_end(&editor, 0), 5);
        assert_eq!(find_line_end(&editor, 3), 5);
        assert_eq!(find_line_end(&editor, 6), 11);
        assert_eq!(find_line_end(&editor, 12), 16);
    }

    #[test]
    fn test_find_word_boundary_backward() {
        let editor = make_editor("hello world test");

        assert_eq!(find_word_boundary_backward(&editor, 16), 12); // from end
        assert_eq!(find_word_boundary_backward(&editor, 12), 6); // from "test"
        assert_eq!(find_word_boundary_backward(&editor, 11), 6); // from space before "test"
        assert_eq!(find_word_boundary_backward(&editor, 5), 0); // from end of "hello"
    }

    #[test]
    fn test_find_word_boundary_forward() {
        let editor = make_editor("hello world test");

        assert_eq!(find_word_boundary_forward(&editor, 0), 6); // from start
        assert_eq!(find_word_boundary_forward(&editor, 6), 12); // from space
        assert_eq!(find_word_boundary_forward(&editor, 12), 16); // from "test"
    }

    #[test]
    fn test_detect_list_context_unordered() {
        let editor = make_editor("- item one\n- item two");

        let ctx = detect_list_context(&editor, 5);
        assert!(matches!(ctx, Some(ListContext::Unordered { marker: '-', .. })));

        let ctx = detect_list_context(&editor, 15);
        assert!(matches!(ctx, Some(ListContext::Unordered { marker: '-', .. })));
    }

    #[test]
    fn test_detect_list_context_ordered() {
        let editor = make_editor("1. first\n2. second");

        let ctx = detect_list_context(&editor, 5);
        assert!(matches!(ctx, Some(ListContext::Ordered { number: 1, .. })));

        let ctx = detect_list_context(&editor, 12);
        assert!(matches!(ctx, Some(ListContext::Ordered { number: 2, .. })));
    }

    #[test]
    fn test_is_list_item_empty() {
        let editor = make_editor("- \n- item");

        let ctx = detect_list_context(&editor, 1).unwrap();
        assert!(is_list_item_empty(&editor, 1, &ctx));

        let ctx = detect_list_context(&editor, 5).unwrap();
        assert!(!is_list_item_empty(&editor, 5, &ctx));
    }
}
