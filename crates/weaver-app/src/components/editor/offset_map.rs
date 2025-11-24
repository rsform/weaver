//! Offset mapping between source text and rendered DOM.
//!
//! When rendering markdown to HTML, some characters disappear (table pipes)
//! and content gets split across nodes (syntax highlighting). Offset maps
//! track how source byte positions map to DOM node positions.

use std::ops::Range;

/// Result of rendering markdown with offset tracking.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderResult {
    /// Rendered HTML string
    pub html: String,

    /// Mappings from source bytes to DOM positions
    pub offset_map: Vec<OffsetMapping>,
}

/// Maps a source range to a position in the rendered DOM.
///
/// # Example
///
/// Source: `| foo | bar |`
/// Bytes:   0  2-5  7-10 12
/// Chars:   0  2-5  7-10 12 (ASCII, so same)
///
/// Rendered:
/// ```html
/// <table id="t0">
///   <tr><td id="t0-c0">foo</td><td id="t0-c1">bar</td></tr>
/// </table>
/// ```
///
/// Mappings:
/// - `{ byte_range: 0..2, char_range: 0..2, node_id: "t0-c0", char_offset_in_node: 0, utf16_len: 0 }` - "| " invisible
/// - `{ byte_range: 2..5, char_range: 2..5, node_id: "t0-c0", char_offset_in_node: 0, utf16_len: 3 }` - "foo" visible
/// - `{ byte_range: 5..7, char_range: 5..7, node_id: "t0-c0", char_offset_in_node: 3, utf16_len: 0 }` - " |" invisible
/// - etc.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetMapping {
    /// Source byte range (UTF-8 bytes, from parser)
    pub byte_range: Range<usize>,

    /// Source char range (Unicode scalar values, for rope indexing)
    pub char_range: Range<usize>,

    /// DOM node ID containing this content
    /// For invisible content, this is the nearest visible container
    pub node_id: String,

    /// Position within the node
    /// - If child_index is Some: cursor at that child index in the element
    /// - If child_index is None: UTF-16 offset in text content
    pub char_offset_in_node: usize,

    /// If Some, position cursor at this child index in the element (not in text)
    /// Used for positions after <br /> or at empty lines
    pub child_index: Option<usize>,

    /// Length of this mapping in UTF-16 chars in DOM
    /// If 0, these source bytes aren't rendered (table pipes, etc)
    pub utf16_len: usize,
}

impl OffsetMapping {
    /// Check if this mapping contains the given byte offset
    pub fn contains_byte(&self, byte_offset: usize) -> bool {
        self.byte_range.contains(&byte_offset)
    }

    /// Check if this mapping contains the given char offset
    pub fn contains_char(&self, char_offset: usize) -> bool {
        self.char_range.contains(&char_offset)
    }

    /// Check if this mapping represents invisible content
    pub fn is_invisible(&self) -> bool {
        self.utf16_len == 0
    }
}

/// Find the offset mapping containing the given byte offset.
///
/// Returns the mapping and whether the cursor should snap to the next
/// visible position (for invisible content).
pub fn find_mapping_for_byte(
    offset_map: &[OffsetMapping],
    byte_offset: usize,
) -> Option<(&OffsetMapping, bool)> {
    // Binary search for the mapping
    // Note: We allow cursor at the end boundary of a mapping (cursor after text)
    let idx = offset_map
        .binary_search_by(|mapping| {
            if mapping.byte_range.end < byte_offset {
                std::cmp::Ordering::Less
            } else if mapping.byte_range.start > byte_offset {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .ok()?;

    let mapping = &offset_map[idx];
    let should_snap = mapping.is_invisible();

    Some((mapping, should_snap))
}

/// Find the offset mapping containing the given char offset.
///
/// This is the primary lookup method for cursor restoration, since
/// cursor positions are tracked as char offsets in the rope.
///
/// Returns the mapping and whether the cursor should snap to the next
/// visible position (for invisible content).
pub fn find_mapping_for_char(
    offset_map: &[OffsetMapping],
    char_offset: usize,
) -> Option<(&OffsetMapping, bool)> {
    // Binary search for the mapping
    // Note: We allow cursor at the end boundary of a mapping (cursor after text)
    // This makes ranges END-INCLUSIVE for cursor positioning
    let idx = offset_map
        .binary_search_by(|mapping| {
            if mapping.char_range.end < char_offset {
                // Cursor is after this mapping
                std::cmp::Ordering::Less
            } else if mapping.char_range.start > char_offset {
                // Cursor is before this mapping
                std::cmp::Ordering::Greater
            } else {
                // Cursor is within [start, end] OR exactly at end (inclusive)
                // This handles cursor at position N matching range N-1..N
                std::cmp::Ordering::Equal
            }
        })
        .ok()?;

    let mapping = &offset_map[idx];
    let should_snap = mapping.is_invisible();

    Some((mapping, should_snap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_mapping_by_byte() {
        let mappings = vec![
            OffsetMapping {
                byte_range: 0..2,
                char_range: 0..2,
                node_id: "n0".to_string(),
                char_offset_in_node: 0,
                child_index: None,
                utf16_len: 0, // invisible
            },
            OffsetMapping {
                byte_range: 2..5,
                char_range: 2..5,
                node_id: "n0".to_string(),
                char_offset_in_node: 0,
                child_index: None,
                utf16_len: 3,
            },
            OffsetMapping {
                byte_range: 5..7,
                char_range: 5..7,
                node_id: "n0".to_string(),
                char_offset_in_node: 3,
                child_index: None,
                utf16_len: 0, // invisible
            },
        ];

        // Byte 0 (invisible)
        let (mapping, should_snap) = find_mapping_for_byte(&mappings, 0).unwrap();
        assert_eq!(mapping.byte_range, 0..2);
        assert!(should_snap);

        // Byte 3 (visible)
        let (mapping, should_snap) = find_mapping_for_byte(&mappings, 3).unwrap();
        assert_eq!(mapping.byte_range, 2..5);
        assert!(!should_snap);

        // Byte 6 (invisible)
        let (mapping, should_snap) = find_mapping_for_byte(&mappings, 6).unwrap();
        assert_eq!(mapping.byte_range, 5..7);
        assert!(should_snap);
    }

    #[test]
    fn test_find_mapping_by_char() {
        let mappings = vec![
            OffsetMapping {
                byte_range: 0..2,
                char_range: 0..2,
                node_id: "n0".to_string(),
                char_offset_in_node: 0,
                child_index: None,
                utf16_len: 0, // invisible
            },
            OffsetMapping {
                byte_range: 2..5,
                char_range: 2..5,
                node_id: "n0".to_string(),
                char_offset_in_node: 0,
                child_index: None,
                utf16_len: 3,
            },
            OffsetMapping {
                byte_range: 5..7,
                char_range: 5..7,
                node_id: "n0".to_string(),
                char_offset_in_node: 3,
                child_index: None,
                utf16_len: 0, // invisible
            },
        ];

        // Char 0 (invisible)
        let (mapping, should_snap) = find_mapping_for_char(&mappings, 0).unwrap();
        assert_eq!(mapping.char_range, 0..2);
        assert!(should_snap);

        // Char 3 (visible)
        let (mapping, should_snap) = find_mapping_for_char(&mappings, 3).unwrap();
        assert_eq!(mapping.char_range, 2..5);
        assert!(!should_snap);

        // Char 6 (invisible)
        let (mapping, should_snap) = find_mapping_for_char(&mappings, 6).unwrap();
        assert_eq!(mapping.char_range, 5..7);
        assert!(should_snap);
    }

    #[test]
    fn test_contains_byte() {
        let mapping = OffsetMapping {
            byte_range: 10..20,
            char_range: 10..20,
            node_id: "test".to_string(),
            char_offset_in_node: 0,
            child_index: None,
            utf16_len: 5,
        };

        assert!(!mapping.contains_byte(9));
        assert!(mapping.contains_byte(10));
        assert!(mapping.contains_byte(15));
        assert!(mapping.contains_byte(19));
        assert!(!mapping.contains_byte(20));
    }

    #[test]
    fn test_contains_char() {
        let mapping = OffsetMapping {
            byte_range: 10..20,
            char_range: 8..15, // emoji example: fewer chars than bytes
            node_id: "test".to_string(),
            char_offset_in_node: 0,
            child_index: None,
            utf16_len: 5,
        };

        assert!(!mapping.contains_char(7));
        assert!(mapping.contains_char(8));
        assert!(mapping.contains_char(12));
        assert!(mapping.contains_char(14));
        assert!(!mapping.contains_char(15));
    }
}
