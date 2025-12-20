use core::fmt;

use markdown_weaver_escape::StrWrite;

/// Writer that segments output by paragraph boundaries.
///
/// Each paragraph's HTML is written to a separate String in the segments Vec.
/// Call `new_segment()` at paragraph boundaries to start a new segment.
#[derive(Debug, Clone, Default)]
pub struct SegmentedWriter {
    pub segments: Vec<String>,
}

#[allow(dead_code)]
impl SegmentedWriter {
    pub fn new() -> Self {
        Self {
            segments: vec![String::new()],
        }
    }

    /// Start a new segment for the next paragraph.
    pub fn new_segment(&mut self) {
        self.segments.push(String::new());
    }

    /// Get the completed segments.
    pub fn into_segments(self) -> Vec<String> {
        self.segments
    }

    /// Get current segment count.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
}

impl StrWrite for SegmentedWriter {
    type Error = fmt::Error;

    #[inline]
    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        if let Some(segment) = self.segments.last_mut() {
            segment.push_str(s);
        }
        Ok(())
    }

    #[inline]
    fn write_fmt(&mut self, args: fmt::Arguments) -> Result<(), Self::Error> {
        if let Some(segment) = self.segments.last_mut() {
            fmt::Write::write_fmt(segment, args)?;
        }
        Ok(())
    }
}

impl fmt::Write for SegmentedWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        <Self as StrWrite>::write_str(self, s)
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        <Self as StrWrite>::write_fmt(self, args)
    }
}
