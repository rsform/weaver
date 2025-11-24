//! Offset conversion utilities for converting between different offset systems.
//!
//! The editor deals with multiple offset systems:
//! 1. **JumpRope**: Unicode scalar values (Rust `char` count)
//! 2. **markdown-weaver**: UTF-8 byte offsets
//! 3. **Rust strings**: UTF-8 byte indexing
//! 4. **JavaScript DOM**: UTF-16 code units (Phase 2+)
//!
//! # Performance Notes
//!
//! **Prefer JumpRope's built-in methods:**
//! - `rope.len_chars()` - O(1) character count
//! - `rope.len_bytes()` - O(1) byte count
//! - `rope.len_wchars()` - O(1) UTF-16 code unit count (Phase 2 with wchar_conversion)
//!
//! **Only use these conversion functions when:**
//! - Converting markdown-weaver byte offsets to char offsets
//! - Converting char offsets to byte offsets for markdown parsing
//!
//! For Phase 2+, use JumpRope's O(log n) UTF-16 conversions via the helpers below:
//! - `char_to_utf16()` - O(log n)
//! - `utf16_to_char()` - O(log n)

/// Convert JumpRope char offset to UTF-8 byte offset.
///
/// This is O(n) but acceptable for Phase 1 since we only render once per keystroke.
/// For Phase 2+, we can optimize by caching or using string-offsets crate.
///
/// # Example
/// ```
/// let text = "Hello 🐻‍❄️ World";
/// // "Hello " = 6 chars, 6 bytes
/// // "🐻‍❄️" = 4 chars, 13 bytes
/// // Total at char 6 = byte 6
/// assert_eq!(char_to_byte(text, 6), 6);
/// // Total at char 10 (after emoji) = byte 19
/// assert_eq!(char_to_byte(text, 10), 19);
/// ```
pub fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(text.len())
}

/// Convert UTF-8 byte offset to JumpRope char offset.
///
/// Used when we need to map markdown-weaver byte offsets back to rope positions.
///
/// # Example
/// ```
/// let text = "Hello 🐻‍❄️ World";
/// assert_eq!(byte_to_char(text, 6), 6);
/// assert_eq!(byte_to_char(text, 19), 10);
/// ```
pub fn byte_to_char(text: &str, byte_offset: usize) -> usize {
    text.char_indices()
        .take_while(|(idx, _)| *idx < byte_offset)
        .count()
}

/// Convert JumpRope char offset to UTF-16 code units (for DOM Selection API).
///
/// O(log n) - uses JumpRope's internal index.
///
/// # Example
/// ```
/// let rope = JumpRopeBuf::from("🐻‍❄️");
/// // Polar bear is 4 chars, 5 UTF-16 code units
/// assert_eq!(char_to_utf16(&rope, 0), 0);
/// assert_eq!(char_to_utf16(&rope, 4), 5);
/// ```
pub fn char_to_utf16(rope: &jumprope::JumpRopeBuf, char_offset: usize) -> usize {
    rope.borrow().chars_to_wchars(char_offset)
}

/// Convert UTF-16 code units (from DOM) to JumpRope char offset.
///
/// O(log n) - uses JumpRope's internal index.
///
/// # Example
/// ```
/// let rope = JumpRopeBuf::from("🐻‍❄️");
/// assert_eq!(utf16_to_char(&rope, 0), 0);
/// assert_eq!(utf16_to_char(&rope, 5), 4);
/// ```
pub fn utf16_to_char(rope: &jumprope::JumpRopeBuf, utf16_offset: usize) -> usize {
    rope.borrow().wchars_to_chars(utf16_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii() {
        let text = "hello";
        assert_eq!(char_to_byte(text, 0), 0);
        assert_eq!(char_to_byte(text, 2), 2);
        assert_eq!(byte_to_char(text, 0), 0);
        assert_eq!(byte_to_char(text, 2), 2);
    }

    #[test]
    fn test_emoji() {
        // Polar bear: 4 chars, 13 bytes
        let text = "🐻‍❄️";
        assert_eq!(text.chars().count(), 4);
        assert_eq!(text.len(), 13);

        assert_eq!(char_to_byte(text, 0), 0);
        assert_eq!(char_to_byte(text, 4), 13);

        assert_eq!(byte_to_char(text, 0), 0);
        assert_eq!(byte_to_char(text, 13), 4);
    }

    #[test]
    fn test_mixed() {
        let text = "Hello 🐻‍❄️ World";
        // "Hello " = 6 chars, 6 bytes
        // "🐻‍❄️" = 4 chars, 13 bytes
        // " World" = 6 chars, 6 bytes
        // Total: 16 chars, 25 bytes

        assert_eq!(text.chars().count(), 16);
        assert_eq!(text.len(), 25);

        // Char 6 is start of emoji (byte 6)
        assert_eq!(char_to_byte(text, 6), 6);
        // Char 10 is after emoji (byte 19)
        assert_eq!(char_to_byte(text, 10), 19);
    }
}
