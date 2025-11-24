//! StrWrite wrapper for JumpRopeBuf to enable efficient HTML rendering.

use jumprope::JumpRopeBuf;
use markdown_weaver_escape::StrWrite;

/// Wrapper around JumpRopeBuf that implements StrWrite.
///
/// This allows rendering HTML directly into a rope structure, enabling:
/// - O(log n) insertions instead of O(n) string reallocation
/// - Efficient splicing for incremental rendering
/// - Fast paragraph replacement in cached output
pub struct RopeWriter {
    rope: JumpRopeBuf,
}

impl RopeWriter {
    pub fn new() -> Self {
        Self {
            rope: JumpRopeBuf::new(),
        }
    }

    pub fn from_rope(rope: JumpRopeBuf) -> Self {
        Self { rope }
    }

    pub fn into_rope(self) -> JumpRopeBuf {
        self.rope
    }

    pub fn as_rope(&self) -> &JumpRopeBuf {
        &self.rope
    }

    pub fn to_string(&self) -> String {
        self.rope.to_string()
    }
}

impl Default for RopeWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl StrWrite for RopeWriter {
    type Error = std::convert::Infallible;

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        let offset = self.rope.len_chars();
        self.rope.insert(offset, s);
        Ok(())
    }

    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> Result<(), Self::Error> {
        let mut temp = String::new();
        std::fmt::Write::write_fmt(&mut temp, args).unwrap();
        self.write_str(&temp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rope_writer_basic() {
        let mut writer = RopeWriter::new();
        writer.write_str("hello ").unwrap();
        writer.write_str("world").unwrap();
        assert_eq!(writer.to_string(), "hello world");
    }

    #[test]
    fn test_rope_writer_fmt() {
        use std::fmt::Write;
        let mut writer = RopeWriter::new();
        write!(&mut writer, "number: {}", 42).unwrap();
        assert_eq!(writer.to_string(), "number: 42");
    }
}
