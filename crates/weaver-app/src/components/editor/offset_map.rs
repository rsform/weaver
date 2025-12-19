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
#[allow(dead_code)]
pub fn find_mapping_for_char(
    offset_map: &[OffsetMapping],
    char_offset: usize,
) -> Option<(&OffsetMapping, bool)> {
    // Binary search for the mapping
    // Rust ranges are end-exclusive, so range 0..10 covers positions 0-9.
    // When cursor is exactly at a boundary (e.g., position 10 between 0..10 and 10..20),
    // prefer the NEXT mapping so cursor goes "down" to new content.
    let result = offset_map.binary_search_by(|mapping| {
        if mapping.char_range.end <= char_offset {
            // Cursor is at or after end of this mapping - look forward
            std::cmp::Ordering::Less
        } else if mapping.char_range.start > char_offset {
            // Cursor is before this mapping
            std::cmp::Ordering::Greater
        } else {
            // Cursor is within [start, end)
            std::cmp::Ordering::Equal
        }
    });

    let mapping = match result {
        Ok(idx) => &offset_map[idx],
        Err(idx) => {
            // No exact match - cursor is at boundary between mappings (or past end)
            // If cursor is exactly at end of previous mapping, return that mapping
            // This handles cursor at end of document or end of last mapping
            if idx > 0 && offset_map[idx - 1].char_range.end == char_offset {
                &offset_map[idx - 1]
            } else {
                return None;
            }
        }
    };

    let should_snap = mapping.is_invisible();
    Some((mapping, should_snap))
}

/// Direction hint for cursor snapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapDirection {
    Backward,
    Forward,
}

/// Result of finding a valid cursor position.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SnappedPosition<'a> {
    pub mapping: &'a OffsetMapping,
    pub offset_in_mapping: usize,
    pub snapped: Option<SnapDirection>,
}

#[allow(dead_code)]
impl SnappedPosition<'_> {
    /// Get the absolute char offset for this position.
    pub fn char_offset(&self) -> usize {
        self.mapping.char_range.start + self.offset_in_mapping
    }
}

/// Find the nearest valid cursor position to a char offset.
///
/// A valid position is one that maps to visible content (utf16_len > 0).
/// If the position is already valid, returns it directly. Otherwise,
/// searches in the preferred direction first, falling back to the other
/// direction if needed.
#[allow(dead_code)]
pub fn find_nearest_valid_position(
    offset_map: &[OffsetMapping],
    char_offset: usize,
    preferred_direction: Option<SnapDirection>,
) -> Option<SnappedPosition<'_>> {
    if offset_map.is_empty() {
        return None;
    }

    // Try exact match first
    if let Some((mapping, should_snap)) = find_mapping_for_char(offset_map, char_offset) {
        if !should_snap {
            // Position is valid, return it directly
            let offset_in_mapping = char_offset.saturating_sub(mapping.char_range.start);
            return Some(SnappedPosition {
                mapping,
                offset_in_mapping,
                snapped: None,
            });
        }
    }

    // Position is invalid or not found - search for nearest valid
    let search_order = match preferred_direction {
        Some(SnapDirection::Backward) => [SnapDirection::Backward, SnapDirection::Forward],
        Some(SnapDirection::Forward) | None => [SnapDirection::Forward, SnapDirection::Backward],
    };

    for direction in search_order {
        if let Some(pos) = find_valid_in_direction(offset_map, char_offset, direction) {
            return Some(pos);
        }
    }

    None
}

/// Search for a valid position in a specific direction.
#[allow(dead_code)]
fn find_valid_in_direction(
    offset_map: &[OffsetMapping],
    char_offset: usize,
    direction: SnapDirection,
) -> Option<SnappedPosition<'_>> {
    match direction {
        SnapDirection::Forward => {
            // Find first visible mapping at or after char_offset
            for mapping in offset_map {
                if mapping.char_range.start >= char_offset && !mapping.is_invisible() {
                    return Some(SnappedPosition {
                        mapping,
                        offset_in_mapping: 0,
                        snapped: Some(SnapDirection::Forward),
                    });
                }
                // Also check if char_offset falls within this visible mapping
                if mapping.char_range.contains(&char_offset) && !mapping.is_invisible() {
                    let offset_in_mapping = char_offset - mapping.char_range.start;
                    return Some(SnappedPosition {
                        mapping,
                        offset_in_mapping,
                        snapped: Some(SnapDirection::Forward),
                    });
                }
            }
            None
        }
        SnapDirection::Backward => {
            // Find last visible mapping at or before char_offset
            for mapping in offset_map.iter().rev() {
                if mapping.char_range.end <= char_offset && !mapping.is_invisible() {
                    // Snap to end of this mapping
                    let offset_in_mapping = mapping.char_range.len();
                    return Some(SnappedPosition {
                        mapping,
                        offset_in_mapping,
                        snapped: Some(SnapDirection::Backward),
                    });
                }
                // Also check if char_offset falls within this visible mapping
                if mapping.char_range.contains(&char_offset) && !mapping.is_invisible() {
                    let offset_in_mapping = char_offset - mapping.char_range.start;
                    return Some(SnappedPosition {
                        mapping,
                        offset_in_mapping,
                        snapped: Some(SnapDirection::Backward),
                    });
                }
            }
            None
        }
    }
}

/// Check if a char offset is at a valid (non-invisible) cursor position.
#[allow(dead_code)]
pub fn is_valid_cursor_position(offset_map: &[OffsetMapping], char_offset: usize) -> bool {
    find_mapping_for_char(offset_map, char_offset)
        .map(|(m, should_snap)| !should_snap && m.utf16_len > 0)
        .unwrap_or(false)
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

    fn make_test_mappings() -> Vec<OffsetMapping> {
        vec![
            OffsetMapping {
                byte_range: 0..2,
                char_range: 0..2,
                node_id: "n0".to_string(),
                char_offset_in_node: 0,
                child_index: None,
                utf16_len: 0, // invisible: "!["
            },
            OffsetMapping {
                byte_range: 2..5,
                char_range: 2..5,
                node_id: "n0".to_string(),
                char_offset_in_node: 0,
                child_index: None,
                utf16_len: 3, // visible: "alt"
            },
            OffsetMapping {
                byte_range: 5..15,
                char_range: 5..15,
                node_id: "n0".to_string(),
                char_offset_in_node: 3,
                child_index: None,
                utf16_len: 0, // invisible: "](url.png)"
            },
            OffsetMapping {
                byte_range: 15..20,
                char_range: 15..20,
                node_id: "n0".to_string(),
                char_offset_in_node: 3,
                child_index: None,
                utf16_len: 5, // visible: " text"
            },
        ]
    }

    #[test]
    fn test_find_nearest_valid_position_exact_match() {
        let mappings = make_test_mappings();

        // Position 3 is in visible mapping (2..5)
        let pos = find_nearest_valid_position(&mappings, 3, None).unwrap();
        assert_eq!(pos.char_offset(), 3);
        assert!(pos.snapped.is_none());
    }

    #[test]
    fn test_find_nearest_valid_position_snap_forward() {
        let mappings = make_test_mappings();

        // Position 0 is invisible, should snap forward to 2
        let pos = find_nearest_valid_position(&mappings, 0, Some(SnapDirection::Forward)).unwrap();
        assert_eq!(pos.char_offset(), 2);
        assert_eq!(pos.snapped, Some(SnapDirection::Forward));
    }

    #[test]
    fn test_find_nearest_valid_position_snap_backward() {
        let mappings = make_test_mappings();

        // Position 10 is invisible (in 5..15), prefer backward to end of "alt" (position 5)
        let pos =
            find_nearest_valid_position(&mappings, 10, Some(SnapDirection::Backward)).unwrap();
        assert_eq!(pos.char_offset(), 5); // end of "alt" mapping
        assert_eq!(pos.snapped, Some(SnapDirection::Backward));
    }

    #[test]
    fn test_find_nearest_valid_position_default_forward() {
        let mappings = make_test_mappings();

        // Position 0 is invisible, None direction defaults to forward
        let pos = find_nearest_valid_position(&mappings, 0, None).unwrap();
        assert_eq!(pos.char_offset(), 2);
        assert_eq!(pos.snapped, Some(SnapDirection::Forward));
    }

    #[test]
    fn test_find_nearest_valid_position_snap_forward_from_invisible() {
        let mappings = make_test_mappings();

        // Position 10 is in invisible range (5..15), forward finds visible (15..20)
        let pos = find_nearest_valid_position(&mappings, 10, Some(SnapDirection::Forward)).unwrap();
        assert_eq!(pos.char_offset(), 15);
        assert_eq!(pos.snapped, Some(SnapDirection::Forward));
    }

    #[test]
    fn test_is_valid_cursor_position() {
        let mappings = make_test_mappings();

        // Invisible positions
        assert!(!is_valid_cursor_position(&mappings, 0));
        assert!(!is_valid_cursor_position(&mappings, 1));
        assert!(!is_valid_cursor_position(&mappings, 10));

        // Visible positions
        assert!(is_valid_cursor_position(&mappings, 2));
        assert!(is_valid_cursor_position(&mappings, 3));
        assert!(is_valid_cursor_position(&mappings, 4));
        assert!(is_valid_cursor_position(&mappings, 15));
        assert!(is_valid_cursor_position(&mappings, 17));
    }

    #[test]
    fn test_find_nearest_valid_position_empty() {
        let mappings: Vec<OffsetMapping> = vec![];
        assert!(find_nearest_valid_position(&mappings, 0, None).is_none());
    }
}
