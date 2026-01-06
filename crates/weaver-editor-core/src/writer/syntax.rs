//! Syntax span emission methods for EditorWriter.

use core::fmt;
use std::ops::Range;

use markdown_weaver::Event;
use markdown_weaver_escape::{StrWrite, escape_html};

use crate::render::{EmbedContentProvider, ImageResolver, WikilinkValidator};
use crate::syntax::{SyntaxSpanInfo, SyntaxType, classify_syntax};

use super::EditorWriter;

impl<'a, I, E, R, W> EditorWriter<'a, I, E, R, W>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
    E: EmbedContentProvider,
    R: ImageResolver,
    W: WikilinkValidator,
{
    /// Emit syntax span for a given range and record offset mapping.
    pub(crate) fn emit_syntax(&mut self, range: Range<usize>) -> Result<(), fmt::Error> {
        if range.start < range.end {
            let syntax = &self.source[range.clone()];
            if !syntax.is_empty() {
                let char_start = self.last_char_offset;
                let syntax_char_len = syntax.chars().count();
                let char_end = char_start + syntax_char_len;

                tracing::trace!(
                    target: "weaver::writer",
                    byte_range = ?range,
                    char_range = ?(char_start..char_end),
                    syntax = %syntax.escape_debug(),
                    "emit_syntax"
                );

                // Whitespace-only content (trailing spaces, newlines) should be emitted
                // as plain text, not wrapped in a hideable syntax span
                let is_whitespace_only = syntax.trim().is_empty();

                if is_whitespace_only {
                    // Emit as plain text with tracking span (not hideable)
                    let created_node = if self.current_node.id.is_none() {
                        let node_id = self.gen_node_id();
                        write!(&mut self.writer, "<span id=\"{}\">", node_id)?;
                        self.begin_node(node_id);
                        true
                    } else {
                        false
                    };

                    escape_html(&mut self.writer, syntax)?;

                    // Record offset mapping BEFORE end_node (which clears current_node.id)
                    self.record_mapping(range.clone(), char_start..char_end);
                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;

                    if created_node {
                        self.write("</span>")?;
                        self.end_node();
                    }
                } else {
                    // Real syntax - wrap in hideable span
                    let syntax_type = classify_syntax(syntax);
                    let class = match syntax_type {
                        SyntaxType::Inline => "md-syntax-inline",
                        SyntaxType::Block => "md-syntax-block",
                    };

                    // Generate unique ID for this syntax span
                    let syn_id = self.gen_syn_id();

                    // If we're outside any node, create a wrapper span for tracking
                    let created_node = if self.current_node.id.is_none() {
                        let node_id = self.gen_node_id();
                        write!(
                            &mut self.writer,
                            "<span id=\"{}\" class=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                            node_id, class, syn_id, char_start, char_end
                        )?;
                        self.begin_node(node_id);
                        true
                    } else {
                        write!(
                            &mut self.writer,
                            "<span class=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                            class, syn_id, char_start, char_end
                        )?;
                        false
                    };

                    escape_html(&mut self.writer, syntax)?;
                    self.write("</span>")?;

                    // Record syntax span info for visibility toggling
                    self.current_para.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type,
                        formatted_range: None,
                    });

                    // Record offset mapping for this syntax
                    self.record_mapping(range.clone(), char_start..char_end);
                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;

                    // Close wrapper if we created one
                    if created_node {
                        self.write("</span>")?;
                        self.end_node();
                    }
                }
            }
        }
        Ok(())
    }

    /// Emit syntax span inside current node with full offset tracking.
    ///
    /// Use this for syntax markers that appear inside block elements (headings, lists,
    /// blockquotes, code fences). Unlike `emit_syntax` which is for gaps and creates
    /// wrapper nodes, this assumes we're already inside a tracked node.
    ///
    /// - Writes `<span class="md-syntax-{class}">{syntax}</span>`
    /// - Records offset mapping (for cursor positioning)
    /// - Updates both `last_char_offset` and `last_byte_offset`
    pub(crate) fn emit_inner_syntax(
        &mut self,
        syntax: &str,
        byte_start: usize,
        syntax_type: SyntaxType,
    ) -> Result<(), fmt::Error> {
        if syntax.is_empty() {
            return Ok(());
        }

        let char_start = self.last_char_offset;
        let syntax_char_len = syntax.chars().count();
        let char_end = char_start + syntax_char_len;
        let byte_end = byte_start + syntax.len();

        let class_str = match syntax_type {
            SyntaxType::Inline => "md-syntax-inline",
            SyntaxType::Block => "md-syntax-block",
        };

        // Generate unique ID for this syntax span
        let syn_id = self.gen_syn_id();

        write!(
            &mut self.writer,
            "<span class=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
            class_str, syn_id, char_start, char_end
        )?;
        escape_html(&mut self.writer, syntax)?;
        self.write("</span>")?;

        // Record syntax span info for visibility toggling
        self.current_para.syntax_spans.push(SyntaxSpanInfo {
            syn_id,
            char_range: char_start..char_end,
            syntax_type,
            formatted_range: None,
        });

        // Record offset mapping for cursor positioning
        self.record_mapping(byte_start..byte_end, char_start..char_end);

        self.last_char_offset = char_end;
        self.last_byte_offset = byte_end;

        Ok(())
    }

    /// Emit any gap between last position and next offset.
    pub(crate) fn emit_gap_before(&mut self, next_offset: usize) -> Result<(), fmt::Error> {
        // Skip gap emission if we're inside a table being rendered as markdown
        if self.table.start_offset.is_some() && self.table.render_as_markdown {
            return Ok(());
        }

        // Skip gap emission if we're buffering code block content
        // The code block handler manages its own syntax emission
        if self.code_block.is_active() {
            return Ok(());
        }

        if next_offset > self.last_byte_offset {
            self.emit_syntax(self.last_byte_offset..next_offset)?;
        }
        Ok(())
    }
}
