//! Formatting actions and utilities for applying markdown formatting.

use super::document::EditorDocument;

/// Formatting actions available in the editor.
#[derive(Clone, Debug, PartialEq)]
pub enum FormatAction {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Link,
    Image,
    Heading(u8), // 1-6
    BulletList,
    NumberedList,
    Quote,
}

/// Find word boundaries around cursor position.
///
/// Expands to whitespace boundaries. Used when applying formatting
/// without a selection.
pub fn find_word_boundaries(rope: &jumprope::JumpRopeBuf, offset: usize) -> (usize, usize) {
    let rope = rope.borrow();
    let mut start = 0;
    let mut end = rope.len_chars();

    // Find start by scanning backwards
    let mut char_pos = 0;
    for substr in rope.slice_substrings(0..offset) {
        for c in substr.chars() {
            if c.is_whitespace() {
                start = char_pos + 1;
            }
            char_pos += 1;
        }
    }

    // Find end by scanning forwards
    char_pos = offset;
    let byte_len = rope.len_bytes();
    for substr in rope.slice_substrings(offset..byte_len) {
        for c in substr.chars() {
            if c.is_whitespace() {
                end = char_pos;
                return (start, end);
            }
            char_pos += 1;
        }
    }

    (start, end)
}

/// Apply formatting to document.
///
/// If there's a selection, wrap it. Otherwise, expand to word boundaries and wrap.
pub fn apply_formatting(doc: &mut EditorDocument, action: FormatAction) {
    let (start, end) = if let Some(sel) = doc.selection {
        // Use selection
        (sel.anchor.min(sel.head), sel.anchor.max(sel.head))
    } else {
        // Expand to word
        find_word_boundaries(&doc.rope, doc.cursor.offset)
    };

    match action {
        FormatAction::Bold => {
            doc.rope.insert(end, "**");
            doc.rope.insert(start, "**");
            doc.cursor.offset = end + 4;
            doc.selection = None;
        }
        FormatAction::Italic => {
            doc.rope.insert(end, "*");
            doc.rope.insert(start, "*");
            doc.cursor.offset = end + 2;
            doc.selection = None;
        }
        FormatAction::Strikethrough => {
            doc.rope.insert(end, "~~");
            doc.rope.insert(start, "~~");
            doc.cursor.offset = end + 4;
            doc.selection = None;
        }
        FormatAction::Code => {
            doc.rope.insert(end, "`");
            doc.rope.insert(start, "`");
            doc.cursor.offset = end + 2;
            doc.selection = None;
        }
        FormatAction::Link => {
            // Insert [selected text](url)
            doc.rope.insert(end, "](url)");
            doc.rope.insert(start, "[");
            doc.cursor.offset = end + 8; // Position cursor after ](url)
            doc.selection = None;
        }
        FormatAction::Image => {
            // Insert ![alt text](url)
            doc.rope.insert(end, "](url)");
            doc.rope.insert(start, "![");
            doc.cursor.offset = end + 9;
            doc.selection = None;
        }
        FormatAction::Heading(level) => {
            // Find start of current line
            let line_start = find_line_start(&doc.rope, doc.cursor.offset);
            let prefix = "#".repeat(level as usize) + " ";
            doc.rope.insert(line_start, &prefix);
            doc.cursor.offset += prefix.len();
            doc.selection = None;
        }
        FormatAction::BulletList => {
            let line_start = find_line_start(&doc.rope, doc.cursor.offset);
            doc.rope.insert(line_start, "- ");
            doc.cursor.offset += 2;
            doc.selection = None;
        }
        FormatAction::NumberedList => {
            let line_start = find_line_start(&doc.rope, doc.cursor.offset);
            doc.rope.insert(line_start, "1. ");
            doc.cursor.offset += 3;
            doc.selection = None;
        }
        FormatAction::Quote => {
            let line_start = find_line_start(&doc.rope, doc.cursor.offset);
            doc.rope.insert(line_start, "> ");
            doc.cursor.offset += 2;
            doc.selection = None;
        }
    }
}

/// Find start of line containing offset (same as in mod.rs)
fn find_line_start(rope: &jumprope::JumpRopeBuf, offset: usize) -> usize {
    let mut char_pos = 0;
    let mut last_newline_pos = None;

    let rope = rope.borrow();
    for substr in rope.slice_substrings(0..offset) {
        for c in substr.chars() {
            if c == '\n' {
                last_newline_pos = Some(char_pos);
            }
            char_pos += 1;
        }
    }

    last_newline_pos.map(|pos| pos + 1).unwrap_or(0)
}
