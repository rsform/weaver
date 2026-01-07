//! Event processing for EditorWriter - the main run loop and event dispatch.

use core::fmt;
use std::fmt::Write as _;
use std::ops::Range;

use markdown_weaver::{Event, Tag, TagEnd};
use markdown_weaver_escape::{escape_html, escape_html_body_text_with_char_count};

use crate::offset_map::OffsetMapping;
use crate::render::{EmbedContentProvider, ImageResolver, WikilinkValidator};
use crate::syntax::{SyntaxSpanInfo, SyntaxType};

use super::{EditorWriter, WriterResult};

// Main run loop
impl<'a, T, I, E, R, W> EditorWriter<'a, T, I, E, R, W>
where
    T: crate::TextBuffer,
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
    E: EmbedContentProvider,
    R: ImageResolver,
    W: WikilinkValidator,
{
    /// Process markdown events and write HTML.
    ///
    /// Returns offset mappings and paragraph boundaries. The HTML is written
    /// to the writer passed in the constructor.
    pub fn run(mut self) -> Result<WriterResult, fmt::Error> {
        while let Some((event, range)) = self.events.next() {
            tracing::trace!(
                target: "weaver::writer",
                event = ?event,
                byte_range = ?range,
                "processing event"
            );

            // For End events, emit any trailing content within the event's range
            // BEFORE calling end_tag (which calls end_node and clears current_node_id)
            //
            // EXCEPTION: For inline formatting tags (Strong, Emphasis, Strikethrough),
            // the closing syntax must be emitted AFTER the closing HTML tag, not before.
            // Otherwise the closing `**` span ends up INSIDE the <strong> element.
            // These tags handle their own closing syntax in end_tag().
            // Image and Embed handle ALL their syntax in the Start event, so exclude them too.
            let is_self_handled_end = matches!(
                &event,
                Event::End(
                    TagEnd::Strong
                        | TagEnd::Emphasis
                        | TagEnd::Strikethrough
                        | TagEnd::Image
                        | TagEnd::Embed
                )
            );

            if matches!(&event, Event::End(_)) && !is_self_handled_end {
                // Emit gap from last_byte_offset to range.end
                self.emit_gap_before(range.end)?;
            } else if !matches!(&event, Event::End(_)) {
                // For paragraph-level start events, capture pre-gap position so the
                // paragraph's char_range includes leading whitespace/gap content.
                let is_para_start = matches!(
                    &event,
                    Event::Start(
                        Tag::Paragraph(_)
                            | Tag::Heading { .. }
                            | Tag::CodeBlock(_)
                            | Tag::List(_)
                            | Tag::BlockQuote(_)
                            | Tag::HtmlBlock
                    )
                );
                if is_para_start && self.paragraphs.should_track_boundaries() {
                    self.paragraphs.pre_gap_start =
                        Some((self.last_byte_offset, self.last_char_offset));
                }

                // For other events, emit any gap before range.start
                // (emit_syntax handles char offset tracking)
                self.emit_gap_before(range.start)?;
            }
            // For inline format End events, gap is emitted inside end_tag() AFTER the closing HTML

            // Store last_byte before processing
            let last_byte_before = self.last_byte_offset;

            // Process the event (passing range for tag syntax)
            self.process_event(event, range.clone())?;

            // Update tracking - but don't override if start_tag manually updated it
            // (for inline formatting tags that emit opening syntax)
            if self.last_byte_offset == last_byte_before {
                // Event didn't update offset, so we update it
                self.last_byte_offset = range.end;
            }
            // else: Event updated offset (e.g. start_tag emitted opening syntax), keep that value
        }

        // Check if document ends with a paragraph break (double newline) BEFORE emitting trailing.
        // If so, we'll reserve the final newline for a synthetic trailing paragraph.
        let ends_with_para_break = self.source.ends_with("\n\n")
            || self.source.ends_with("\n\u{200C}\n");

        // Determine where to stop emitting trailing syntax
        let trailing_emit_end = if ends_with_para_break {
            // Don't emit the final newline - save it for synthetic paragraph
            self.source.len().saturating_sub(1)
        } else {
            self.source.len()
        };

        // Emit trailing syntax up to the determined point
        self.emit_gap_before(trailing_emit_end)?;

        // Handle unmapped trailing content (stripped by parser)
        // This includes trailing spaces that markdown ignores
        let doc_byte_len = self.source.len();
        let doc_char_len = self.text_buffer.len_chars();

        if !ends_with_para_break
            && (self.last_byte_offset < doc_byte_len || self.last_char_offset < doc_char_len)
        {
            // Emit the trailing content as visible syntax (only if not creating synthetic para)
            if self.last_byte_offset < doc_byte_len {
                let trailing = &self.source[self.last_byte_offset..];
                if !trailing.is_empty() {
                    let char_start = self.last_char_offset;
                    let trailing_char_len = trailing.chars().count();

                    let char_end = char_start + trailing_char_len;
                    let syn_id = self.gen_syn_id();

                    write!(
                        &mut self.writer,
                        "<span class=\"md-placeholder\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                        syn_id, char_start, char_end
                    )?;
                    escape_html(&mut self.writer, trailing)?;
                    self.write("</span>")?;

                    // Record mapping if we have a node
                    if let Some(ref node_id) = self.current_node.id {
                        let mapping = OffsetMapping {
                            byte_range: self.last_byte_offset..doc_byte_len,
                            char_range: char_start..char_end,
                            node_id: node_id.clone(),
                            char_offset_in_node: self.current_node.char_offset,
                            child_index: None,
                            utf16_len: trailing_char_len, // visible
                        };
                        self.current_para.offset_maps.push(mapping);
                        self.current_node.char_offset += trailing_char_len;
                    }

                    self.last_char_offset = char_start + trailing_char_len;
                }
            }
        }

        // Add any remaining accumulated data for the last paragraph FIRST
        // (content that wasn't followed by a paragraph boundary)
        if !self.current_para.offset_maps.is_empty()
            || !self.current_para.syntax_spans.is_empty()
            || !self.ref_collector.refs.is_empty()
        {
            self.offset_maps_by_para
                .push(std::mem::take(&mut self.current_para.offset_maps));
            self.syntax_spans_by_para
                .push(std::mem::take(&mut self.current_para.syntax_spans));
            self.refs_by_para
                .push(std::mem::take(&mut self.ref_collector.refs));
        }

        // Now create a synthetic trailing paragraph if needed
        if ends_with_para_break {
            // Get the trailing content we reserved (the final newline)
            let trailing_content = &self.source[trailing_emit_end..];
            let trailing_char_len = trailing_content.chars().count();

            let trailing_start_char = self.last_char_offset;
            let trailing_start_byte = self.last_byte_offset;
            let trailing_end_char = trailing_start_char + trailing_char_len;
            let trailing_end_byte = self.source.len();

            // Create paragraph range that includes the trailing content
            self.paragraphs.ranges.push((
                trailing_start_byte..trailing_end_byte,
                trailing_start_char..trailing_end_char,
            ));

            // Start a new HTML segment for this trailing paragraph
            self.writer.new_segment();
            let node_id = self.gen_node_id();

            // Write the actual trailing content plus ZWSP for cursor positioning
            write!(&mut self.writer, "<span id=\"{}\">", node_id)?;
            escape_html(&mut self.writer, trailing_content)?;
            self.write("\u{200B}</span>")?;

            // Record offset mapping for the trailing content
            let mapping = OffsetMapping {
                byte_range: trailing_start_byte..trailing_end_byte,
                char_range: trailing_start_char..trailing_end_char,
                node_id,
                char_offset_in_node: 0,
                child_index: None,
                utf16_len: trailing_char_len + 1, // Content + ZWSP
            };

            // Create offset_maps/syntax_spans/refs for this trailing paragraph
            self.offset_maps_by_para.push(vec![mapping]);
            self.syntax_spans_by_para.push(vec![]);
            self.refs_by_para.push(vec![]);
        }

        // Get HTML segments from writer
        let html_segments = self.writer.into_segments();

        Ok(WriterResult {
            html_segments,
            offset_maps_by_paragraph: self.offset_maps_by_para,
            paragraph_ranges: self.paragraphs.ranges,
            syntax_spans_by_paragraph: self.syntax_spans_by_para,
            collected_refs_by_paragraph: self.refs_by_para,
        })
    }

    fn process_event(&mut self, event: Event<'_>, range: Range<usize>) -> Result<(), fmt::Error> {
        use Event::*;

        match event {
            Start(tag) => self.start_tag(tag, range)?,
            End(tag) => self.end_tag(tag, range)?,
            Text(text) => {
                // If buffering code, append to buffer instead of writing
                if let Some((_, ref mut content)) = self.code_block.buffer {
                    content.push_str(&text);

                    // Track byte and char ranges for code block content
                    let text_char_len = text.chars().count();
                    let text_byte_len = text.len();
                    if let Some(ref mut code_byte_range) = self.code_block.byte_range {
                        // Extend existing ranges
                        code_byte_range.end = range.end;
                        if let Some(ref mut code_char_range) = self.code_block.char_range {
                            code_char_range.end = self.last_char_offset + text_char_len;
                        }
                    } else {
                        // First text in code block - start tracking
                        self.code_block.byte_range = Some(range.clone());
                        self.code_block.char_range =
                            Some(self.last_char_offset..self.last_char_offset + text_char_len);
                    }
                    // Update offsets so paragraph boundary is correct
                    self.last_char_offset += text_char_len;
                    self.last_byte_offset += text_byte_len;
                } else if !self.in_non_writing_block {
                    // Escape HTML and count chars in one pass
                    let char_start = self.last_char_offset;
                    let text_char_len =
                        escape_html_body_text_with_char_count(&mut self.writer, &text)?;
                    let char_end = char_start + text_char_len;

                    // Text becomes a text node child of the current container
                    if text_char_len > 0 {
                        self.current_node.child_count += 1;
                    }

                    // Record offset mapping
                    self.record_mapping(range.clone(), char_start..char_end);

                    // Update char offset tracking
                    self.last_char_offset = char_end;
                    self.end_newline = text.ends_with('\n');
                }
            }
            Code(text) => {
                let format_start = self.last_char_offset;
                let raw_text = &self.source[range.clone()];

                // Track opening span index so we can set formatted_range later
                let opening_span_idx = if raw_text.starts_with('`') {
                    let syn_id = self.gen_syn_id();
                    let char_start = self.last_char_offset;
                    let backtick_char_end = char_start + 1;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">`</span>",
                        syn_id, char_start, backtick_char_end
                    )?;
                    self.current_para.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..backtick_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None, // Set after we know the full range
                    });
                    self.last_char_offset += 1;
                    Some(self.current_para.syntax_spans.len() - 1)
                } else {
                    None
                };

                self.write("<code>")?;

                // Track offset mapping for code content
                let content_char_start = self.last_char_offset;
                let text_char_len =
                    escape_html_body_text_with_char_count(&mut self.writer, &text)?;
                let content_char_end = content_char_start + text_char_len;

                // Record offset mapping (code content is visible)
                self.record_mapping(range.clone(), content_char_start..content_char_end);
                self.last_char_offset = content_char_end;

                self.write("</code>")?;

                // Emit closing backtick and track it
                if raw_text.ends_with('`') {
                    let syn_id = self.gen_syn_id();
                    let backtick_char_start = self.last_char_offset;
                    let backtick_char_end = backtick_char_start + 1;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">`</span>",
                        syn_id, backtick_char_start, backtick_char_end
                    )?;

                    // Now we know the full formatted range
                    let formatted_range = format_start..backtick_char_end;

                    self.current_para.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: backtick_char_start..backtick_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });

                    // Update opening span with formatted_range
                    if let Some(idx) = opening_span_idx {
                        self.current_para.syntax_spans[idx].formatted_range =
                            Some(formatted_range);
                    }

                    self.last_char_offset += 1;
                }
            }
            InlineMath(text) => {
                self.process_inline_math(&text, range)?;
            }
            DisplayMath(text) => {
                self.process_display_math(&text, range)?;
            }
            Html(html) => {
                // Track offset mapping for raw HTML
                let char_start = self.last_char_offset;
                let html_char_len = html.chars().count();
                let char_end = char_start + html_char_len;

                self.write(&html)?;

                // Record mapping for inline HTML
                self.record_mapping(range.clone(), char_start..char_end);
                self.last_char_offset = char_end;
            }
            InlineHtml(html) => {
                // Track offset mapping for raw HTML
                let char_start = self.last_char_offset;
                let html_char_len = html.chars().count();
                let char_end = char_start + html_char_len;
                self.write(r#"<span class="html-embed html-embed-inline">"#)?;
                self.write(&html)?;
                self.write("</span>")?;
                // Record mapping for inline HTML
                self.record_mapping(range.clone(), char_start..char_end);
                self.last_char_offset = char_end;
            }
            SoftBreak => {
                // Emit <br> for visual line break, plus a space for cursor positioning.
                // This space maps to the \n so the cursor can land here when navigating.
                let char_start = self.last_char_offset;

                // Emit <br>
                self.write("<br />")?;
                self.current_node.child_count += 1;

                // Emit space for cursor positioning - this gives the browser somewhere
                // to place the cursor when navigating to this line
                self.write("\u{200B}")?;
                self.current_node.child_count += 1;

                // Map the space to the newline position - cursor landing here means
                // we're at the end of the line (after the \n)
                if let Some(ref node_id) = self.current_node.id {
                    let mapping = OffsetMapping {
                        byte_range: range.clone(),
                        char_range: char_start..char_start + 1,
                        node_id: node_id.clone(),
                        char_offset_in_node: self.current_node.char_offset,
                        child_index: None,
                        utf16_len: 1, // the space we emitted
                    };
                    self.current_para.offset_maps.push(mapping);
                    self.current_node.char_offset += 1;
                }

                self.last_char_offset = char_start + 1; // +1 for the \n
            }
            HardBreak => {
                // Emit the two spaces as visible (dimmed) text, then <br>
                let gap = &self.source[range.clone()];
                if gap.ends_with('\n') {
                    let spaces = &gap[..gap.len() - 1]; // everything except the \n
                    let char_start = self.last_char_offset;
                    let spaces_char_len = spaces.chars().count();
                    let char_end = char_start + spaces_char_len;

                    // Emit and map the visible spaces
                    let syn_id = self.gen_syn_id();
                    write!(
                        &mut self.writer,
                        "<span class=\"md-placeholder\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                        syn_id, char_start, char_end
                    )?;
                    escape_html(&mut self.writer, spaces)?;
                    self.write("</span>")?;

                    // Count this span as a child
                    self.current_node.child_count += 1;

                    self.record_mapping(
                        range.start..range.start + spaces.len(),
                        char_start..char_end,
                    );

                    // Now the actual line break <br>
                    self.write("<br />")?;

                    // Count the <br> as a child
                    self.current_node.child_count += 1;

                    // After <br>, emit plain zero-width space for cursor positioning
                    self.write("\u{200B}")?;

                    // Count the zero-width space text node as a child
                    self.current_node.child_count += 1;

                    // Map the newline position to the zero-width space text node
                    if let Some(ref node_id) = self.current_node.id {
                        let newline_char_offset = char_start + spaces_char_len;
                        let mapping = OffsetMapping {
                            byte_range: range.start + spaces.len()..range.end,
                            char_range: newline_char_offset..newline_char_offset + 1,
                            node_id: node_id.clone(),
                            char_offset_in_node: self.current_node.char_offset,
                            child_index: None, // text node - TreeWalker will find it
                            utf16_len: 1,      // zero-width space is 1 UTF-16 unit
                        };
                        self.current_para.offset_maps.push(mapping);

                        // Increment char offset - TreeWalker will encounter this text node
                        self.current_node.char_offset += 1;
                    }

                    self.last_char_offset = char_start + spaces_char_len + 1; // +1 for \n
                } else {
                    // Fallback: just <br>
                    self.write("<br />")?;
                }
            }
            Rule => {
                if !self.end_newline {
                    self.write("\n")?;
                }

                // Emit syntax span before the rendered element
                if range.start < range.end {
                    let raw_text = &self.source[range];
                    let trimmed = raw_text.trim();
                    if !trimmed.is_empty() {
                        let syn_id = self.gen_syn_id();
                        let char_start = self.last_char_offset;
                        let char_len = trimmed.chars().count();
                        let char_end = char_start + char_len;

                        write!(
                            &mut self.writer,
                            "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                            syn_id, char_start, char_end
                        )?;
                        escape_html(&mut self.writer, trimmed)?;
                        self.write("</span>")?;

                        self.current_para.syntax_spans.push(SyntaxSpanInfo {
                            syn_id,
                            char_range: char_start..char_end,
                            syntax_type: SyntaxType::Block,
                            formatted_range: None,
                        });
                    }
                }

                // Wrap <hr /> in toggle-block for future cursor-based toggling
                self.write("<div class=\"toggle-block\"><hr /></div>")?;
            }
            FootnoteReference(name) => {
                // Emit [^name] as styled (but NOT hidden) inline span
                let raw_text = &self.source[range.clone()];
                let char_start = self.last_char_offset;
                let syntax_char_len = raw_text.chars().count();
                let char_end = char_start + syntax_char_len;

                // Use footnote-ref class for styling, not md-syntax-inline (which hides)
                write!(
                    &mut self.writer,
                    "<span class=\"footnote-ref\" data-char-start=\"{}\" data-char-end=\"{}\" data-footnote=\"{}\">",
                    char_start, char_end, name
                )?;
                escape_html(&mut self.writer, raw_text)?;
                self.write("</span>")?;

                // Record offset mapping
                self.record_mapping(range.clone(), char_start..char_end);

                // Count as child
                self.current_node.child_count += 1;

                // Update tracking
                self.last_char_offset = char_end;
                self.last_byte_offset = range.end;
            }
            TaskListMarker(checked) => {
                // Emit the [ ] or [x] syntax
                if range.start < range.end {
                    let raw_text = &self.source[range];
                    if let Some(bracket_pos) = raw_text.find('[') {
                        let end_pos = raw_text.find(']').map(|p| p + 1).unwrap_or(bracket_pos + 3);
                        let syntax = &raw_text[bracket_pos..end_pos.min(raw_text.len())];

                        let syn_id = self.gen_syn_id();
                        let char_start = self.last_char_offset;
                        let syntax_char_len = syntax.chars().count();
                        let char_end = char_start + syntax_char_len;

                        write!(
                            &mut self.writer,
                            "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                            syn_id, char_start, char_end
                        )?;
                        escape_html(&mut self.writer, syntax)?;
                        self.write("</span> ")?;

                        self.current_para.syntax_spans.push(SyntaxSpanInfo {
                            syn_id,
                            char_range: char_start..char_end,
                            syntax_type: SyntaxType::Inline,
                            formatted_range: None,
                        });
                    }
                }

                if checked {
                    self.write("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>")?;
                } else {
                    self.write("<input disabled=\"\" type=\"checkbox\"/>")?;
                }
            }
            WeaverBlock(text) => {
                // Buffer WeaverBlock content for parsing on End
                self.weaver_block.buffer.push_str(&text);
            }
        }
        Ok(())
    }

    /// Process inline math ($...$)
    fn process_inline_math(&mut self, text: &str, range: Range<usize>) -> Result<(), fmt::Error> {
        let raw_text = &self.source[range.clone()];
        let syn_id = self.gen_syn_id();
        let opening_char_start = self.last_char_offset;

        // Calculate char positions
        let text_char_len = text.chars().count();
        let opening_char_end = opening_char_start + 1; // "$"
        let content_char_start = opening_char_end;
        let content_char_end = content_char_start + text_char_len;
        let closing_char_start = content_char_end;
        let closing_char_end = closing_char_start + 1; // "$"
        let formatted_range = opening_char_start..closing_char_end;

        // 1. Emit opening $ syntax span
        if raw_text.starts_with('$') {
            write!(
                &mut self.writer,
                "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">$</span>",
                syn_id, opening_char_start, opening_char_end
            )?;
            self.current_para.syntax_spans.push(SyntaxSpanInfo {
                syn_id: syn_id.clone(),
                char_range: opening_char_start..opening_char_end,
                syntax_type: SyntaxType::Inline,
                formatted_range: Some(formatted_range.clone()),
            });
            self.record_mapping(
                range.start..range.start + 1,
                opening_char_start..opening_char_end,
            );
        }

        // 2. Emit raw LaTeX content (hidden with syntax when cursor outside)
        write!(
            &mut self.writer,
            "<span class=\"math-source\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
            syn_id, content_char_start, content_char_end
        )?;
        escape_html(&mut self.writer, text)?;
        self.write("</span>")?;
        self.current_para.syntax_spans.push(SyntaxSpanInfo {
            syn_id: syn_id.clone(),
            char_range: content_char_start..content_char_end,
            syntax_type: SyntaxType::Inline,
            formatted_range: Some(formatted_range.clone()),
        });
        self.record_mapping(
            range.start + 1..range.end - 1,
            content_char_start..content_char_end,
        );

        // 3. Emit closing $ syntax span
        if raw_text.ends_with('$') {
            write!(
                &mut self.writer,
                "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">$</span>",
                syn_id, closing_char_start, closing_char_end
            )?;
            self.current_para.syntax_spans.push(SyntaxSpanInfo {
                syn_id: syn_id.clone(),
                char_range: closing_char_start..closing_char_end,
                syntax_type: SyntaxType::Inline,
                formatted_range: Some(formatted_range.clone()),
            });
            self.record_mapping(
                range.end - 1..range.end,
                closing_char_start..closing_char_end,
            );
        }

        // 4. Emit rendered MathML (always visible, not tied to syn_id)
        // Include data-char-target so clicking moves cursor into the math region
        // contenteditable="false" so DOM walker skips this for offset counting
        match weaver_renderer::math::render_math(text, false) {
            weaver_renderer::math::MathResult::Success(mathml) => {
                write!(
                    &mut self.writer,
                    "<span class=\"math math-inline math-rendered math-clickable\" contenteditable=\"false\" data-char-target=\"{}\">{}</span>",
                    content_char_start, mathml
                )?;
            }
            weaver_renderer::math::MathResult::Error { html, .. } => {
                // Show error indicator (also always visible)
                self.write(&html)?;
            }
        }

        self.last_char_offset = closing_char_end;
        Ok(())
    }

    /// Process display math ($$...$$)
    fn process_display_math(&mut self, text: &str, range: Range<usize>) -> Result<(), fmt::Error> {
        let raw_text = &self.source[range.clone()];
        let syn_id = self.gen_syn_id();
        let opening_char_start = self.last_char_offset;

        // Calculate char positions
        let text_char_len = text.chars().count();
        let opening_char_end = opening_char_start + 2; // "$$"
        let content_char_start = opening_char_end;
        let content_char_end = content_char_start + text_char_len;
        let closing_char_start = content_char_end;
        let closing_char_end = closing_char_start + 2; // "$$"
        let formatted_range = opening_char_start..closing_char_end;

        // 1. Emit opening $$ syntax span
        // Use Block syntax type so visibility is based on "cursor in same paragraph"
        if raw_text.starts_with("$$") {
            write!(
                &mut self.writer,
                "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">$$</span>",
                syn_id, opening_char_start, opening_char_end
            )?;
            self.current_para.syntax_spans.push(SyntaxSpanInfo {
                syn_id: syn_id.clone(),
                char_range: opening_char_start..opening_char_end,
                syntax_type: SyntaxType::Block,
                formatted_range: Some(formatted_range.clone()),
            });
            self.record_mapping(
                range.start..range.start + 2,
                opening_char_start..opening_char_end,
            );
        }

        // 2. Emit raw LaTeX content (hidden with syntax when cursor outside)
        write!(
            &mut self.writer,
            "<span class=\"math-source\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
            syn_id, content_char_start, content_char_end
        )?;
        escape_html(&mut self.writer, text)?;
        self.write("</span>")?;
        self.current_para.syntax_spans.push(SyntaxSpanInfo {
            syn_id: syn_id.clone(),
            char_range: content_char_start..content_char_end,
            syntax_type: SyntaxType::Block,
            formatted_range: Some(formatted_range.clone()),
        });
        self.record_mapping(
            range.start + 2..range.end - 2,
            content_char_start..content_char_end,
        );

        // 3. Emit closing $$ syntax span
        if raw_text.ends_with("$$") {
            write!(
                &mut self.writer,
                "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">$$</span>",
                syn_id, closing_char_start, closing_char_end
            )?;
            self.current_para.syntax_spans.push(SyntaxSpanInfo {
                syn_id: syn_id.clone(),
                char_range: closing_char_start..closing_char_end,
                syntax_type: SyntaxType::Block,
                formatted_range: Some(formatted_range.clone()),
            });
            self.record_mapping(
                range.end - 2..range.end,
                closing_char_start..closing_char_end,
            );
        }

        // 4. Emit rendered MathML (always visible, not tied to syn_id)
        // Include data-char-target so clicking moves cursor into the math region
        // contenteditable="false" so DOM walker skips this for offset counting
        match weaver_renderer::math::render_math(text, true) {
            weaver_renderer::math::MathResult::Success(mathml) => {
                write!(
                    &mut self.writer,
                    "<span class=\"math math-display math-rendered math-clickable\" contenteditable=\"false\" data-char-target=\"{}\">{}</span>",
                    content_char_start, mathml
                )?;
            }
            weaver_renderer::math::MathResult::Error { html, .. } => {
                // Show error indicator (also always visible)
                self.write(&html)?;
            }
        }

        self.last_char_offset = closing_char_end;
        Ok(())
    }
}
