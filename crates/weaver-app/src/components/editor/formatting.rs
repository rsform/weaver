//! Formatting actions and utilities for applying markdown formatting.

use super::document::EditorDocument;
use super::input::{ListContext, detect_list_context, find_line_end};
use dioxus::prelude::*;

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
pub fn find_word_boundaries(text: &loro::LoroText, offset: usize) -> (usize, usize) {
    let len = text.len_unicode();

    // Find start by scanning backwards using char_at
    let mut start = 0;
    for i in (0..offset).rev() {
        match text.char_at(i) {
            Ok(c) if c.is_whitespace() => {
                start = i + 1;
                break;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }

    // Find end by scanning forwards using char_at
    let mut end = len;
    for i in offset..len {
        match text.char_at(i) {
            Ok(c) if c.is_whitespace() => {
                end = i;
                break;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }

    (start, end)
}

/// Apply formatting to document.
///
/// If there's a selection, wrap it. Otherwise, expand to word boundaries and wrap.
pub fn apply_formatting(doc: &mut EditorDocument, action: FormatAction) {
    let cursor_offset = doc.cursor.read().offset;
    let (start, end) = if let Some(sel) = *doc.selection.read() {
        // Use selection
        (sel.anchor.min(sel.head), sel.anchor.max(sel.head))
    } else {
        // Expand to word
        find_word_boundaries(doc.loro_text(), cursor_offset)
    };

    match action {
        FormatAction::Bold => {
            // Insert end marker first so start position stays valid
            let _ = doc.insert_tracked(end, "**");
            let _ = doc.insert_tracked(start, "**");
            doc.cursor.write().offset = end + 4;
            doc.selection.set(None);
        }
        FormatAction::Italic => {
            let _ = doc.insert_tracked(end, "*");
            let _ = doc.insert_tracked(start, "*");
            doc.cursor.write().offset = end + 2;
            doc.selection.set(None);
        }
        FormatAction::Strikethrough => {
            let _ = doc.insert_tracked(end, "~~");
            let _ = doc.insert_tracked(start, "~~");
            doc.cursor.write().offset = end + 4;
            doc.selection.set(None);
        }
        FormatAction::Code => {
            let _ = doc.insert_tracked(end, "`");
            let _ = doc.insert_tracked(start, "`");
            doc.cursor.write().offset = end + 2;
            doc.selection.set(None);
        }
        FormatAction::Link => {
            // Insert [selected text](url)
            let _ = doc.insert_tracked(end, "](url)");
            let _ = doc.insert_tracked(start, "[");
            doc.cursor.write().offset = end + 8; // Position cursor after ](url)
            doc.selection.set(None);
        }
        FormatAction::Image => {
            // Insert ![alt text](url)
            let _ = doc.insert_tracked(end, "](url)");
            let _ = doc.insert_tracked(start, "![");
            doc.cursor.write().offset = end + 9;
            doc.selection.set(None);
        }
        FormatAction::Heading(level) => {
            // Find start of current line
            let line_start = find_line_start(doc.loro_text(), cursor_offset);
            let prefix = "#".repeat(level as usize) + " ";
            let _ = doc.insert_tracked(line_start, &prefix);
            doc.cursor.write().offset = cursor_offset + prefix.len();
            doc.selection.set(None);
        }
        FormatAction::BulletList => {
            if let Some(ctx) = detect_list_context(doc.loro_text(), cursor_offset) {
                let continuation = match ctx {
                    ListContext::Unordered { indent, marker } => {
                        format!("\n{}{} ", indent, marker)
                    }
                    ListContext::Ordered { .. } => {
                        format!("\n\n - ")
                    }
                };
                let len = continuation.chars().count();
                let _ = doc.insert_tracked(cursor_offset, &continuation);
                doc.cursor.write().offset = cursor_offset + len;
                doc.selection.set(None);
            } else {
                let line_start = find_line_start(doc.loro_text(), cursor_offset);
                let _ = doc.insert_tracked(line_start, " - ");
                doc.cursor.write().offset = cursor_offset + 3;
                doc.selection.set(None);
            }
        }
        FormatAction::NumberedList => {
            if let Some(ctx) = detect_list_context(doc.loro_text(), cursor_offset) {
                let continuation = match ctx {
                    ListContext::Unordered { .. } => {
                        format!("\n\n1. ")
                    }
                    ListContext::Ordered { indent, number } => {
                        format!("\n{}{}. ", indent, number + 1)
                    }
                };
                let len = continuation.chars().count();
                let _ = doc.insert_tracked(cursor_offset, &continuation);
                doc.cursor.write().offset = cursor_offset + len;
                doc.selection.set(None);
            } else {
                let line_start = find_line_start(doc.loro_text(), cursor_offset);
                let _ = doc.insert_tracked(line_start, "1. ");
                doc.cursor.write().offset = cursor_offset + 3;
                doc.selection.set(None);
            }
        }
        FormatAction::Quote => {
            let line_start = find_line_start(doc.loro_text(), cursor_offset);
            let _ = doc.insert_tracked(line_start, "> ");
            doc.cursor.write().offset = cursor_offset + 2;
            doc.selection.set(None);
        }
    }
}

/// Find start of line containing offset
fn find_line_start(text: &loro::LoroText, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }

    // Get text up to offset
    let prefix = match text.slice(0, offset) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    // Find last newline
    prefix
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == '\n')
        .last()
        .map(|(pos, _)| pos + 1)
        .unwrap_or(0)
}
