use core::fmt;
use std::ops::Range;

use markdown_weaver::{Alignment, BlockQuoteKind, CodeBlockKind, EmbedType, Event, LinkType, Tag};
use markdown_weaver_escape::{StrWrite, escape_href, escape_html, escape_html_body_text};

use crate::components::editor::{
    OffsetMapping, SyntaxSpanInfo, SyntaxType,
    writer::{
        EditorWriter, TableState,
        embed::{EmbedContentProvider, ImageResolver},
        syntax::classify_syntax,
    },
};

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, E: EmbedContentProvider, R: ImageResolver>
    EditorWriter<'a, I, E, R>
{
    pub(crate) fn start_tag(
        &mut self,
        tag: Tag<'_>,
        range: Range<usize>,
    ) -> Result<(), fmt::Error> {
        // Check if this is a block-level tag that should have syntax inside
        let is_block_tag = matches!(tag, Tag::Heading { .. } | Tag::BlockQuote(_));

        // For inline tags, emit syntax before tag
        if !is_block_tag && range.start < range.end {
            let raw_text = &self.source[range.clone()];
            let opening_syntax = match &tag {
                Tag::Strong => {
                    if raw_text.starts_with("**") {
                        Some("**")
                    } else if raw_text.starts_with("__") {
                        Some("__")
                    } else {
                        None
                    }
                }
                Tag::Emphasis => {
                    if raw_text.starts_with("*") {
                        Some("*")
                    } else if raw_text.starts_with("_") {
                        Some("_")
                    } else {
                        None
                    }
                }
                Tag::Strikethrough => {
                    if raw_text.starts_with("~~") {
                        Some("~~")
                    } else {
                        None
                    }
                }
                Tag::Link { link_type, .. } => {
                    if matches!(link_type, LinkType::WikiLink { .. }) {
                        if raw_text.starts_with("[[") {
                            Some("[[")
                        } else {
                            None
                        }
                    } else if raw_text.starts_with('[') {
                        Some("[")
                    } else {
                        None
                    }
                }
                // Note: Tag::Image and Tag::Embed handle their own syntax spans
                // in their respective handlers, so don't emit here
                _ => None,
            };

            if let Some(syntax) = opening_syntax {
                let syntax_type = classify_syntax(syntax);
                let class = match syntax_type {
                    SyntaxType::Inline => "md-syntax-inline",
                    SyntaxType::Block => "md-syntax-block",
                };

                let char_start = self.last_char_offset;
                let syntax_char_len = syntax.chars().count();
                let char_end = char_start + syntax_char_len;
                let syntax_byte_len = syntax.len();

                // Generate unique ID for this syntax span
                let syn_id = self.gen_syn_id();

                write!(
                    &mut self.writer,
                    "<span class=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                    class, syn_id, char_start, char_end
                )?;
                escape_html(&mut self.writer, syntax)?;
                self.write("</span>")?;

                // Record syntax span info for visibility toggling
                self.syntax_spans.push(SyntaxSpanInfo {
                    syn_id: syn_id.clone(),
                    char_range: char_start..char_end,
                    syntax_type,
                    formatted_range: None, // Will be updated when closing tag is emitted
                });

                // Record offset mapping for cursor positioning
                // This is critical - without it, current_node_char_offset is wrong
                // and all subsequent cursor positions are shifted
                let byte_start = range.start;
                let byte_end = range.start + syntax_byte_len;
                self.record_mapping(byte_start..byte_end, char_start..char_end);

                // For paired inline syntax, track opening span for formatted_range
                if matches!(
                    tag,
                    Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. }
                ) {
                    self.pending_inline_formats.push((syn_id, char_start));
                }

                // Update tracking - we've consumed this opening syntax
                self.last_char_offset = char_end;
                self.last_byte_offset = range.start + syntax_byte_len;
            }
        }

        // Emit the opening tag
        match tag {
            // HTML blocks get their own paragraph to try and corral them better
            Tag::HtmlBlock => {
                // Record paragraph start for boundary tracking
                // BUT skip if inside a list - list owns the paragraph boundary
                if self.list_depth == 0 {
                    self.current_paragraph_start =
                        Some((self.last_byte_offset, self.last_char_offset));
                }
                let node_id = self.gen_node_id();

                if self.end_newline {
                    write!(
                        &mut self.writer,
                        r#"<p id="{}" class="html-embed html-embed-block">"#,
                        node_id
                    )?;
                } else {
                    write!(
                        &mut self.writer,
                        r#"\n<p id="{}" class="html-embed html-embed-block">"#,
                        node_id
                    )?;
                }
                self.begin_node(node_id.clone());

                // Map the start position of the paragraph (before any content)
                // This allows cursor to be placed at the very beginning
                let para_start_char = self.last_char_offset;
                let mapping = OffsetMapping {
                    byte_range: range.start..range.start,
                    char_range: para_start_char..para_start_char,
                    node_id,
                    char_offset_in_node: 0,
                    child_index: Some(0), // position before first child
                    utf16_len: 0,
                };
                self.offset_maps.push(mapping);

                Ok(())
            }
            Tag::Paragraph(_) => {
                // Handle wrapper before block
                self.emit_wrapper_start()?;

                // Record paragraph start for boundary tracking
                // BUT skip if inside a list - list owns the paragraph boundary
                if self.list_depth == 0 {
                    self.current_paragraph_start =
                        Some((self.last_byte_offset, self.last_char_offset));
                }

                let node_id = self.gen_node_id();
                if self.end_newline {
                    write!(&mut self.writer, "<p id=\"{}\">", node_id)?;
                } else {
                    write!(&mut self.writer, "\n<p id=\"{}\">", node_id)?;
                }
                self.begin_node(node_id.clone());

                // Map the start position of the paragraph (before any content)
                // This allows cursor to be placed at the very beginning
                let para_start_char = self.last_char_offset;
                let mapping = OffsetMapping {
                    byte_range: range.start..range.start,
                    char_range: para_start_char..para_start_char,
                    node_id,
                    char_offset_in_node: 0,
                    child_index: Some(0), // position before first child
                    utf16_len: 0,
                };
                self.offset_maps.push(mapping);

                // Emit > syntax if we're inside a blockquote
                if let Some(bq_range) = self.pending_blockquote_range.take() {
                    if bq_range.start < bq_range.end {
                        let raw_text = &self.source[bq_range.clone()];
                        if let Some(gt_pos) = raw_text.find('>') {
                            // Extract > [!NOTE] or just >
                            let after_gt = &raw_text[gt_pos + 1..];
                            let syntax_end = if after_gt.trim_start().starts_with("[!") {
                                // Find the closing ]
                                if let Some(close_bracket) = after_gt.find(']') {
                                    gt_pos + 1 + close_bracket + 1
                                } else {
                                    gt_pos + 1
                                }
                            } else {
                                // Just > and maybe a space
                                (gt_pos + 1).min(raw_text.len())
                            };

                            let syntax = &raw_text[gt_pos..syntax_end];
                            let syntax_byte_start = bq_range.start + gt_pos;
                            self.emit_inner_syntax(syntax, syntax_byte_start, SyntaxType::Block)?;
                        }
                    }
                }
                Ok(())
            }
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => {
                // Emit wrapper if pending (but don't close on heading end - wraps following block too)
                self.emit_wrapper_start()?;

                // Record paragraph start for boundary tracking
                // Treat headings as paragraph-level blocks
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));

                if !self.end_newline {
                    self.write("\n")?;
                }

                // Generate node ID for offset tracking
                let node_id = self.gen_node_id();

                self.write("<")?;
                write!(&mut self.writer, "{}", level)?;

                // Add our tracking ID as data attribute (preserve user's id if present)
                self.write(" data-node-id=\"")?;
                self.write(&node_id)?;
                self.write("\"")?;

                if let Some(id) = id {
                    self.write(" id=\"")?;
                    escape_html(&mut self.writer, &id)?;
                    self.write("\"")?;
                }
                if !classes.is_empty() {
                    self.write(" class=\"")?;
                    for (i, class) in classes.iter().enumerate() {
                        if i > 0 {
                            self.write(" ")?;
                        }
                        escape_html(&mut self.writer, class)?;
                    }
                    self.write("\"")?;
                }
                for (attr, value) in attrs {
                    self.write(" ")?;
                    escape_html(&mut self.writer, &attr)?;
                    if let Some(val) = value {
                        self.write("=\"")?;
                        escape_html(&mut self.writer, &val)?;
                        self.write("\"")?;
                    } else {
                        self.write("=\"\"")?;
                    }
                }
                self.write(">")?;

                // Begin node tracking for offset mapping
                self.begin_node(node_id.clone());

                // Map the start position of the heading (before any content)
                // This allows cursor to be placed at the very beginning
                let heading_start_char = self.last_char_offset;
                let mapping = OffsetMapping {
                    byte_range: range.start..range.start,
                    char_range: heading_start_char..heading_start_char,
                    node_id: node_id.clone(),
                    char_offset_in_node: 0,
                    child_index: Some(0), // position before first child
                    utf16_len: 0,
                };
                self.offset_maps.push(mapping);

                // Emit # syntax inside the heading tag
                if range.start < range.end {
                    let raw_text = &self.source[range.clone()];
                    let count = level as usize;
                    let pattern = "#".repeat(count);

                    // Find where the # actually starts (might have leading whitespace)
                    if let Some(hash_pos) = raw_text.find(&pattern) {
                        // Extract "# " or "## " etc
                        let syntax_end = (hash_pos + count + 1).min(raw_text.len());
                        let syntax = &raw_text[hash_pos..syntax_end];
                        let syntax_byte_start = range.start + hash_pos;

                        self.emit_inner_syntax(syntax, syntax_byte_start, SyntaxType::Block)?;
                    }
                }
                Ok(())
            }
            Tag::Table(alignments) => {
                if self.render_tables_as_markdown {
                    // Store start offset and skip HTML rendering
                    self.table_start_offset = Some(range.start);
                    self.in_non_writing_block = true; // Suppress content output
                    Ok(())
                } else {
                    self.emit_wrapper_start()?;
                    self.table_alignments = alignments;
                    self.write("<table>")
                }
            }
            Tag::TableHead => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    self.table_state = TableState::Head;
                    self.table_cell_index = 0;
                    self.write("<thead><tr>")
                }
            }
            Tag::TableRow => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    self.table_cell_index = 0;
                    self.write("<tr>")
                }
            }
            Tag::TableCell => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    match self.table_state {
                        TableState::Head => self.write("<th")?,
                        TableState::Body => self.write("<td")?,
                    }
                    match self.table_alignments.get(self.table_cell_index) {
                        Some(&Alignment::Left) => self.write(" style=\"text-align: left\">"),
                        Some(&Alignment::Center) => self.write(" style=\"text-align: center\">"),
                        Some(&Alignment::Right) => self.write(" style=\"text-align: right\">"),
                        _ => self.write(">"),
                    }
                }
            }
            Tag::BlockQuote(kind) => {
                self.emit_wrapper_start()?;

                let class_str = match kind {
                    None => "",
                    Some(BlockQuoteKind::Note) => " class=\"markdown-alert-note\"",
                    Some(BlockQuoteKind::Tip) => " class=\"markdown-alert-tip\"",
                    Some(BlockQuoteKind::Important) => " class=\"markdown-alert-important\"",
                    Some(BlockQuoteKind::Warning) => " class=\"markdown-alert-warning\"",
                    Some(BlockQuoteKind::Caution) => " class=\"markdown-alert-caution\"",
                };
                if self.end_newline {
                    write!(&mut self.writer, "<blockquote{}>\n", class_str)?;
                } else {
                    write!(&mut self.writer, "\n<blockquote{}>\n", class_str)?;
                }

                // Store range for emitting > inside the next paragraph
                self.pending_blockquote_range = Some(range);
                Ok(())
            }
            Tag::CodeBlock(info) => {
                self.emit_wrapper_start()?;

                // Track code block as paragraph-level block
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));

                if !self.end_newline {
                    self.write_newline()?;
                }

                // Generate node ID for code block
                let node_id = self.gen_node_id();

                match info {
                    CodeBlockKind::Fenced(info) => {
                        // Emit opening ```language and track both char and byte offsets
                        if range.start < range.end {
                            let raw_text = &self.source[range.clone()];
                            if let Some(fence_pos) = raw_text.find("```") {
                                let fence_end = (fence_pos + 3 + info.len()).min(raw_text.len());
                                let syntax = &raw_text[fence_pos..fence_end];
                                let syntax_char_len = syntax.chars().count() + 1; // +1 for newline
                                let syntax_byte_len = syntax.len() + 1; // +1 for newline

                                let syn_id = self.gen_syn_id();
                                let char_start = self.last_char_offset;
                                let char_end = char_start + syntax_char_len;

                                write!(
                                    &mut self.writer,
                                    "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                                    syn_id, char_start, char_end
                                )?;
                                escape_html(&mut self.writer, syntax)?;
                                self.write("</span>\n")?;

                                // Track opening span index for formatted_range update later
                                self.code_block_opening_span_idx = Some(self.syntax_spans.len());
                                self.code_block_char_start = Some(char_start);

                                self.syntax_spans.push(SyntaxSpanInfo {
                                    syn_id,
                                    char_range: char_start..char_end,
                                    syntax_type: SyntaxType::Block,
                                    formatted_range: None, // Will be set in TagEnd::CodeBlock
                                });

                                self.last_char_offset += syntax_char_len;
                                self.last_byte_offset = range.start + fence_pos + syntax_byte_len;
                            }
                        }

                        let lang = info.split(' ').next().unwrap();
                        let lang_opt = if lang.is_empty() {
                            None
                        } else {
                            Some(lang.to_string())
                        };
                        // Start buffering
                        self.code_buffer = Some((lang_opt, String::new()));

                        // Begin node tracking for offset mapping
                        self.begin_node(node_id);
                        Ok(())
                    }
                    CodeBlockKind::Indented => {
                        // Ignore indented code blocks (as per executive decision)
                        self.code_buffer = Some((None, String::new()));

                        // Begin node tracking for offset mapping
                        self.begin_node(node_id);
                        Ok(())
                    }
                }
            }
            Tag::List(Some(1)) => {
                self.emit_wrapper_start()?;
                // Track list as paragraph-level block
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));
                self.list_depth += 1;
                if self.end_newline {
                    self.write("<ol>\n")
                } else {
                    self.write("\n<ol>\n")
                }
            }
            Tag::List(Some(start)) => {
                self.emit_wrapper_start()?;
                // Track list as paragraph-level block
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));
                self.list_depth += 1;
                if self.end_newline {
                    self.write("<ol start=\"")?;
                } else {
                    self.write("\n<ol start=\"")?;
                }
                write!(&mut self.writer, "{}", start)?;
                self.write("\">\n")
            }
            Tag::List(None) => {
                self.emit_wrapper_start()?;
                // Track list as paragraph-level block
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));
                self.list_depth += 1;
                if self.end_newline {
                    self.write("<ul>\n")
                } else {
                    self.write("\n<ul>\n")
                }
            }
            Tag::Item => {
                // Generate node ID for list item
                let node_id = self.gen_node_id();

                if self.end_newline {
                    write!(&mut self.writer, "<li data-node-id=\"{}\">", node_id)?;
                } else {
                    write!(&mut self.writer, "\n<li data-node-id=\"{}\">", node_id)?;
                }

                // Begin node tracking
                self.begin_node(node_id);

                // Emit list marker syntax inside the <li> tag and track both offsets
                if range.start < range.end {
                    let raw_text = &self.source[range.clone()];

                    // Try to find the list marker (-, *, or digit.)
                    let trimmed = raw_text.trim_start();
                    let leading_ws_bytes = raw_text.len() - trimmed.len();
                    let leading_ws_chars = raw_text.chars().count() - trimmed.chars().count();

                    if let Some(marker) = trimmed.chars().next() {
                        if marker == '-' || marker == '*' {
                            // Unordered list: extract "- " or "* "
                            let marker_end = trimmed
                                .find(|c: char| c != '-' && c != '*')
                                .map(|pos| pos + 1)
                                .unwrap_or(1);
                            let syntax = &trimmed[..marker_end.min(trimmed.len())];
                            let char_start = self.last_char_offset;
                            let syntax_char_len = leading_ws_chars + syntax.chars().count();
                            let syntax_byte_len = leading_ws_bytes + syntax.len();
                            let char_end = char_start + syntax_char_len;

                            let syn_id = self.gen_syn_id();
                            write!(
                                &mut self.writer,
                                "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                                syn_id, char_start, char_end
                            )?;
                            escape_html(&mut self.writer, syntax)?;
                            self.write("</span>")?;

                            self.syntax_spans.push(SyntaxSpanInfo {
                                syn_id,
                                char_range: char_start..char_end,
                                syntax_type: SyntaxType::Block,
                                formatted_range: None,
                            });

                            // Record offset mapping for cursor positioning
                            self.record_mapping(
                                range.start..range.start + syntax_byte_len,
                                char_start..char_end,
                            );
                            self.last_char_offset = char_end;
                            self.last_byte_offset = range.start + syntax_byte_len;
                        } else if marker.is_ascii_digit() {
                            // Ordered list: extract "1. " or similar (including trailing space)
                            if let Some(dot_pos) = trimmed.find('.') {
                                let syntax_end = (dot_pos + 2).min(trimmed.len());
                                let syntax = &trimmed[..syntax_end];
                                let char_start = self.last_char_offset;
                                let syntax_char_len = leading_ws_chars + syntax.chars().count();
                                let syntax_byte_len = leading_ws_bytes + syntax.len();
                                let char_end = char_start + syntax_char_len;

                                let syn_id = self.gen_syn_id();
                                write!(
                                    &mut self.writer,
                                    "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                                    syn_id, char_start, char_end
                                )?;
                                escape_html(&mut self.writer, syntax)?;
                                self.write("</span>")?;

                                self.syntax_spans.push(SyntaxSpanInfo {
                                    syn_id,
                                    char_range: char_start..char_end,
                                    syntax_type: SyntaxType::Block,
                                    formatted_range: None,
                                });

                                // Record offset mapping for cursor positioning
                                self.record_mapping(
                                    range.start..range.start + syntax_byte_len,
                                    char_start..char_end,
                                );
                                self.last_char_offset = char_end;
                                self.last_byte_offset = range.start + syntax_byte_len;
                            }
                        }
                    }
                }
                Ok(())
            }
            Tag::DefinitionList => {
                self.emit_wrapper_start()?;
                if self.end_newline {
                    self.write("<dl>\n")
                } else {
                    self.write("\n<dl>\n")
                }
            }
            Tag::DefinitionListTitle => {
                let node_id = self.gen_node_id();

                if self.end_newline {
                    write!(&mut self.writer, "<dt data-node-id=\"{}\">", node_id)?;
                } else {
                    write!(&mut self.writer, "\n<dt data-node-id=\"{}\">", node_id)?;
                }

                self.begin_node(node_id);
                Ok(())
            }
            Tag::DefinitionListDefinition => {
                let node_id = self.gen_node_id();

                if self.end_newline {
                    write!(&mut self.writer, "<dd data-node-id=\"{}\">", node_id)?;
                } else {
                    write!(&mut self.writer, "\n<dd data-node-id=\"{}\">", node_id)?;
                }

                self.begin_node(node_id);
                Ok(())
            }
            Tag::Subscript => self.write("<sub>"),
            Tag::Superscript => self.write("<sup>"),
            Tag::Emphasis => self.write("<em>"),
            Tag::Strong => self.write("<strong>"),
            Tag::Strikethrough => self.write("<s>"),
            Tag::Link {
                link_type: LinkType::Email,
                dest_url,
                title,
                ..
            } => {
                self.write("<a href=\"mailto:")?;
                escape_href(&mut self.writer, &dest_url)?;
                if !title.is_empty() {
                    self.write("\" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                }
                self.write("\">")
            }
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                // Collect refs for later resolution
                let url = dest_url.as_ref();
                if matches!(link_type, LinkType::WikiLink { .. }) {
                    let (target, fragment) = weaver_common::EntryIndex::parse_wikilink(url);
                    self.ref_collector.add_wikilink(target, fragment, None);
                } else if url.starts_with("at://") {
                    self.ref_collector.add_at_link(url);
                }

                // Determine link validity class for wikilinks
                let validity_class = if matches!(link_type, LinkType::WikiLink { .. }) {
                    if let Some(index) = &self.entry_index {
                        if index.resolve(dest_url.as_ref()).is_some() {
                            " link-valid"
                        } else {
                            " link-broken"
                        }
                    } else {
                        ""
                    }
                } else {
                    ""
                };

                self.write("<a class=\"link")?;
                self.write(validity_class)?;
                self.write("\" href=\"")?;
                escape_href(&mut self.writer, &dest_url)?;
                if !title.is_empty() {
                    self.write("\" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                }
                self.write("\">")
            }
            Tag::Image {
                link_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                // Check if this is actually an AT embed disguised as a wikilink image
                // (markdown-weaver parses ![[at://...]] as Image with WikiLink link_type)
                let url = dest_url.as_ref();
                if matches!(link_type, LinkType::WikiLink { .. })
                    && (url.starts_with("at://") || url.starts_with("did:"))
                {
                    return self.write_embed(
                        range,
                        EmbedType::Other, // AT embeds - disambiguated via NSID later
                        dest_url,
                        title,
                        id,
                        attrs,
                    );
                }

                // Image rendering: all syntax elements share one syn_id for visibility toggling
                // Structure: ![  alt text  ](url)  <img>  cursor-landing
                let raw_text = &self.source[range.clone()];
                let syn_id = self.gen_syn_id();
                let opening_char_start = self.last_char_offset;

                // Find the alt text and closing syntax positions
                let paren_pos = raw_text.rfind("](").unwrap_or(raw_text.len());
                let alt_text = if raw_text.starts_with("![") && paren_pos > 2 {
                    &raw_text[2..paren_pos]
                } else {
                    ""
                };
                let closing_syntax = if paren_pos < raw_text.len() {
                    &raw_text[paren_pos..]
                } else {
                    ""
                };

                // Calculate char positions
                let alt_char_len = alt_text.chars().count();
                let closing_char_len = closing_syntax.chars().count();
                let opening_char_end = opening_char_start + 2; // "!["
                let alt_char_start = opening_char_end;
                let alt_char_end = alt_char_start + alt_char_len;
                let closing_char_start = alt_char_end;
                let closing_char_end = closing_char_start + closing_char_len;
                let formatted_range = opening_char_start..closing_char_end;

                // 1. Emit opening ![ syntax span
                if raw_text.starts_with("![") {
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">![</span>",
                        syn_id, opening_char_start, opening_char_end
                    )?;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: opening_char_start..opening_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });

                    // Record offset mapping for ![
                    self.record_mapping(
                        range.start..range.start + 2,
                        opening_char_start..opening_char_end,
                    );
                }

                // 2. Emit alt text span (same syn_id, editable when visible)
                if !alt_text.is_empty() {
                    write!(
                        &mut self.writer,
                        "<span class=\"image-alt\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                        syn_id, alt_char_start, alt_char_end
                    )?;
                    escape_html(&mut self.writer, alt_text)?;
                    self.write("</span>")?;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: alt_char_start..alt_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });

                    // Record offset mapping for alt text
                    self.record_mapping(
                        range.start + 2..range.start + 2 + alt_text.len(),
                        alt_char_start..alt_char_end,
                    );
                }

                // 3. Emit closing ](url) syntax span
                if !closing_syntax.is_empty() {
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                        syn_id, closing_char_start, closing_char_end
                    )?;
                    escape_html(&mut self.writer, closing_syntax)?;
                    self.write("</span>")?;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: closing_char_start..closing_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });

                    // Record offset mapping for ](url)
                    self.record_mapping(
                        range.start + paren_pos..range.end,
                        closing_char_start..closing_char_end,
                    );
                }

                // 4. Emit <img> element (no syn_id - always visible)
                self.write("<img src=\"")?;
                let resolved_url = self
                    .image_resolver
                    .as_ref()
                    .and_then(|r| r.resolve_image_url(&dest_url));
                if let Some(ref cdn_url) = resolved_url {
                    escape_href(&mut self.writer, cdn_url)?;
                } else {
                    escape_href(&mut self.writer, &dest_url)?;
                }
                self.write("\" alt=\"")?;
                escape_html(&mut self.writer, alt_text)?;
                self.write("\"")?;
                if !title.is_empty() {
                    self.write(" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                    self.write("\"")?;
                }
                if let Some(attrs) = attrs {
                    if !attrs.classes.is_empty() {
                        self.write(" class=\"")?;
                        for (i, class) in attrs.classes.iter().enumerate() {
                            if i > 0 {
                                self.write(" ")?;
                            }
                            escape_html(&mut self.writer, class)?;
                        }
                        self.write("\"")?;
                    }
                    for (attr, value) in &attrs.attrs {
                        self.write(" ")?;
                        escape_html(&mut self.writer, attr)?;
                        self.write("=\"")?;
                        escape_html(&mut self.writer, value)?;
                        self.write("\"")?;
                    }
                }
                self.write(" />")?;

                // Consume the text events for alt (they're still in the iterator)
                // Use consume_until_end() since we already wrote alt text from source
                self.consume_until_end();

                // Update offsets
                self.last_char_offset = closing_char_end;
                self.last_byte_offset = range.end;

                Ok(())
            }
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => self.write_embed(range, embed_type, dest_url, title, id, attrs),
            Tag::WeaverBlock(_, attrs) => {
                self.in_non_writing_block = true;
                self.weaver_block_buffer.clear();
                self.weaver_block_char_start = Some(self.last_char_offset);
                // Store attrs from Start tag, will merge with parsed text on End
                if !attrs.classes.is_empty() || !attrs.attrs.is_empty() {
                    self.pending_block_attrs = Some(attrs.into_static());
                }
                Ok(())
            }
            Tag::FootnoteDefinition(name) => {
                // Emit the [^name]: prefix as a hideable syntax span
                // The source should have "[^name]: " at the start
                let prefix = format!("[^{}]: ", name);
                let char_start = self.last_char_offset;
                let prefix_char_len = prefix.chars().count();
                let char_end = char_start + prefix_char_len;
                let syn_id = self.gen_syn_id();

                if !self.end_newline {
                    self.write("\n")?;
                }

                write!(
                    &mut self.writer,
                    "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                    syn_id, char_start, char_end
                )?;
                escape_html(&mut self.writer, &prefix)?;
                self.write("</span>")?;

                // Track this span for linking with the footnote reference
                let def_span_index = self.syntax_spans.len();
                self.syntax_spans.push(SyntaxSpanInfo {
                    syn_id,
                    char_range: char_start..char_end,
                    syntax_type: SyntaxType::Block,
                    formatted_range: None, // Set at FootnoteDefinition end
                });

                // Store the definition info for linking at end
                self.current_footnote_def = Some((name.to_string(), def_span_index, char_start));

                // Record offset mapping for the syntax span
                self.record_mapping(
                    range.start..range.start + prefix.len(),
                    char_start..char_end,
                );

                // Update tracking for the prefix
                self.last_char_offset = char_end;
                self.last_byte_offset = range.start + prefix.len();

                // Emit the definition container
                write!(
                    &mut self.writer,
                    "<div class=\"footnote-definition\" id=\"fn-{}\">",
                    name
                )?;

                // Get/create footnote number for the label
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name.to_string()).or_insert(len);
                write!(
                    &mut self.writer,
                    "<sup class=\"footnote-definition-label\">{}</sup>",
                    number
                )?;

                Ok(())
            }
            Tag::MetadataBlock(_) => {
                self.in_non_writing_block = true;
                Ok(())
            }
        }
    }

    pub(crate) fn end_tag(
        &mut self,
        tag: markdown_weaver::TagEnd,
        range: Range<usize>,
    ) -> Result<(), fmt::Error> {
        use markdown_weaver::TagEnd;

        // Emit tag HTML first
        let result = match tag {
            TagEnd::HtmlBlock => {
                // Capture paragraph boundary info BEFORE writing closing HTML
                // Skip if inside a list - list owns the paragraph boundary
                let para_boundary = if self.list_depth == 0 {
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        })
                } else {
                    None
                };

                // Write closing HTML to current segment
                self.end_node();
                self.write("</p>\n")?;

                // Now finalize paragraph (starts new segment)
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::Paragraph(_) => {
                // Capture paragraph boundary info BEFORE writing closing HTML
                // Skip if inside a list - list owns the paragraph boundary
                let para_boundary = if self.list_depth == 0 {
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        })
                } else {
                    None
                };

                // Write closing HTML to current segment
                self.end_node();
                self.write("</p>\n")?;
                self.close_wrapper()?;

                // Now finalize paragraph (starts new segment)
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::Heading(level) => {
                // Capture paragraph boundary info BEFORE writing closing HTML
                let para_boundary =
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        });

                // Write closing HTML to current segment
                self.end_node();
                self.write("</")?;
                write!(&mut self.writer, "{}", level)?;
                self.write(">\n")?;
                // Note: Don't close wrapper here - headings typically go with following block

                // Now finalize paragraph (starts new segment)
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::Table => {
                if self.render_tables_as_markdown {
                    // Emit the raw markdown table
                    if let Some(start) = self.table_start_offset.take() {
                        let table_text = &self.source[start..range.end];
                        self.in_non_writing_block = false;

                        // Wrap in a pre or div for styling
                        self.write("<pre class=\"table-markdown\">")?;
                        escape_html(&mut self.writer, table_text)?;
                        self.write("</pre>\n")?;
                    }
                    Ok(())
                } else {
                    self.write("</tbody></table>\n")
                }
            }
            TagEnd::TableHead => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    self.write("</tr></thead><tbody>\n")?;
                    self.table_state = TableState::Body;
                    Ok(())
                }
            }
            TagEnd::TableRow => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    self.write("</tr>\n")
                }
            }
            TagEnd::TableCell => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    match self.table_state {
                        TableState::Head => self.write("</th>")?,
                        TableState::Body => self.write("</td>")?,
                    }
                    self.table_cell_index += 1;
                    Ok(())
                }
            }
            TagEnd::BlockQuote(_) => {
                // If pending_blockquote_range is still set, the blockquote was empty
                // (no paragraph inside). Emit the > as its own minimal paragraph.
                let mut para_boundary = None;
                if let Some(bq_range) = self.pending_blockquote_range.take() {
                    if bq_range.start < bq_range.end {
                        let raw_text = &self.source[bq_range.clone()];
                        if let Some(gt_pos) = raw_text.find('>') {
                            let para_byte_start = bq_range.start + gt_pos;
                            let para_char_start = self.last_char_offset;

                            // Create a minimal paragraph for the empty blockquote
                            let node_id = self.gen_node_id();
                            write!(&mut self.writer, "<div id=\"{}\"", node_id)?;

                            // Record start-of-node mapping for cursor positioning
                            self.offset_maps.push(OffsetMapping {
                                byte_range: para_byte_start..para_byte_start,
                                char_range: para_char_start..para_char_start,
                                node_id: node_id.clone(),
                                char_offset_in_node: gt_pos,
                                child_index: Some(0),
                                utf16_len: 0,
                            });

                            // Emit the > as block syntax
                            let syntax = &raw_text[gt_pos..gt_pos + 1];
                            self.emit_inner_syntax(syntax, para_byte_start, SyntaxType::Block)?;

                            self.write("</div>\n")?;
                            self.end_node();

                            // Capture paragraph boundary for later finalization
                            let byte_range = para_byte_start..bq_range.end;
                            let char_range = para_char_start..self.last_char_offset;
                            para_boundary = Some((byte_range, char_range));
                        }
                    }
                }
                self.write("</blockquote>\n")?;
                self.close_wrapper()?;

                // Now finalize paragraph if we had one
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::CodeBlock => {
                use std::sync::LazyLock;
                use syntect::parsing::SyntaxSet;
                static SYNTAX_SET: LazyLock<SyntaxSet> =
                    LazyLock::new(|| SyntaxSet::load_defaults_newlines());

                if let Some((lang, buffer)) = self.code_buffer.take() {
                    // Create offset mapping for code block content if we tracked ranges
                    if let (Some(code_byte_range), Some(code_char_range)) = (
                        self.code_buffer_byte_range.take(),
                        self.code_buffer_char_range.take(),
                    ) {
                        // Record mapping before writing HTML
                        // (current_node_id should be set by start_tag for CodeBlock)
                        self.record_mapping(code_byte_range, code_char_range);
                    }

                    // Get node_id for data-node-id attribute (needed for cursor positioning)
                    let node_id = self.current_node_id.clone();

                    if let Some(ref lang_str) = lang {
                        // Use a temporary String buffer for syntect
                        let mut temp_output = String::new();
                        match weaver_renderer::code_pretty::highlight(
                            &SYNTAX_SET,
                            Some(lang_str),
                            &buffer,
                            &mut temp_output,
                        ) {
                            Ok(_) => {
                                // Inject data-node-id into the <pre> tag for cursor positioning
                                if let Some(ref nid) = node_id {
                                    let injected = temp_output.replacen(
                                        "<pre>",
                                        &format!("<pre data-node-id=\"{}\">", nid),
                                        1,
                                    );
                                    self.write(&injected)?;
                                } else {
                                    self.write(&temp_output)?;
                                }
                            }
                            Err(_) => {
                                // Fallback to plain code block
                                if let Some(ref nid) = node_id {
                                    write!(
                                        &mut self.writer,
                                        "<pre data-node-id=\"{}\"><code class=\"language-",
                                        nid
                                    )?;
                                } else {
                                    self.write("<pre><code class=\"language-")?;
                                }
                                escape_html(&mut self.writer, lang_str)?;
                                self.write("\">")?;
                                escape_html_body_text(&mut self.writer, &buffer)?;
                                self.write("</code></pre>\n")?;
                            }
                        }
                    } else {
                        if let Some(ref nid) = node_id {
                            write!(&mut self.writer, "<pre data-node-id=\"{}\"><code>", nid)?;
                        } else {
                            self.write("<pre><code>")?;
                        }
                        escape_html_body_text(&mut self.writer, &buffer)?;
                        self.write("</code></pre>\n")?;
                    }

                    // End node tracking
                    self.end_node();
                } else {
                    self.write("</code></pre>\n")?;
                }

                // Emit closing ``` (emit_gap_before is skipped while buffering)
                // Track the opening span index and char start before we potentially clear them
                let opening_span_idx = self.code_block_opening_span_idx.take();
                let code_block_start = self.code_block_char_start.take();

                if range.start < range.end {
                    let raw_text = &self.source[range.clone()];
                    if let Some(fence_line) = raw_text.lines().last() {
                        if fence_line.trim().starts_with("```") {
                            let fence = fence_line.trim();
                            let fence_char_len = fence.chars().count();

                            let syn_id = self.gen_syn_id();
                            let char_start = self.last_char_offset;
                            let char_end = char_start + fence_char_len;

                            write!(
                                &mut self.writer,
                                "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                                syn_id, char_start, char_end
                            )?;
                            escape_html(&mut self.writer, fence)?;
                            self.write("</span>")?;

                            self.last_char_offset += fence_char_len;
                            self.last_byte_offset += fence.len();

                            // Compute formatted_range for entire code block (opening fence to closing fence)
                            let formatted_range =
                                code_block_start.map(|start| start..self.last_char_offset);

                            // Update opening fence span with formatted_range
                            if let (Some(idx), Some(fr)) =
                                (opening_span_idx, formatted_range.as_ref())
                            {
                                if let Some(span) = self.syntax_spans.get_mut(idx) {
                                    span.formatted_range = Some(fr.clone());
                                }
                            }

                            // Push closing fence span with formatted_range
                            self.syntax_spans.push(SyntaxSpanInfo {
                                syn_id,
                                char_range: char_start..char_end,
                                syntax_type: SyntaxType::Block,
                                formatted_range,
                            });
                        }
                    }
                }

                // Finalize code block paragraph
                if let Some((byte_start, char_start)) = self.current_paragraph_start.take() {
                    let byte_range = byte_start..self.last_byte_offset;
                    let char_range = char_start..self.last_char_offset;
                    self.finalize_paragraph(byte_range, char_range);
                }

                Ok(())
            }
            TagEnd::List(true) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                // Capture paragraph boundary BEFORE writing closing HTML
                let para_boundary =
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        });

                self.write("</ol>\n")?;
                self.close_wrapper()?;

                // Finalize paragraph after closing HTML
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::List(false) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                // Capture paragraph boundary BEFORE writing closing HTML
                let para_boundary =
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        });

                self.write("</ul>\n")?;
                self.close_wrapper()?;

                // Finalize paragraph after closing HTML
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::Item => {
                self.end_node();
                self.write("</li>\n")
            }
            TagEnd::DefinitionList => {
                self.write("</dl>\n")?;
                self.close_wrapper()
            }
            TagEnd::DefinitionListTitle => {
                self.end_node();
                self.write("</dt>\n")
            }
            TagEnd::DefinitionListDefinition => {
                self.end_node();
                self.write("</dd>\n")
            }
            TagEnd::Emphasis => {
                // Write closing tag FIRST, then emit closing syntax OUTSIDE the tag
                self.write("</em>")?;
                self.emit_gap_before(range.end)?;
                self.finalize_paired_inline_format();
                Ok(())
            }
            TagEnd::Superscript => self.write("</sup>"),
            TagEnd::Subscript => self.write("</sub>"),
            TagEnd::Strong => {
                // Write closing tag FIRST, then emit closing syntax OUTSIDE the tag
                self.write("</strong>")?;
                self.emit_gap_before(range.end)?;
                self.finalize_paired_inline_format();
                Ok(())
            }
            TagEnd::Strikethrough => {
                // Write closing tag FIRST, then emit closing syntax OUTSIDE the tag
                self.write("</s>")?;
                self.emit_gap_before(range.end)?;
                self.finalize_paired_inline_format();
                Ok(())
            }
            TagEnd::Link => {
                self.write("</a>")?;
                // Check if this is a wiki link (ends with ]]) vs regular link (ends with ))
                let raw_text = &self.source[range.clone()];
                if raw_text.ends_with("]]") {
                    // WikiLink: emit ]] as closing syntax
                    let syn_id = self.gen_syn_id();
                    let char_start = self.last_char_offset;
                    let char_end = char_start + 2;

                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">]]</span>",
                        syn_id, char_start, char_end
                    )?;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None, // Will be set by finalize
                    });

                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;
                } else {
                    self.emit_gap_before(range.end)?;
                }
                self.finalize_paired_inline_format();
                Ok(())
            }
            TagEnd::Image => Ok(()), // No-op: raw_text() already consumed the End(Image) event
            TagEnd::Embed => Ok(()),
            TagEnd::WeaverBlock(_) => {
                self.in_non_writing_block = false;

                // Emit the { content } as a hideable syntax span
                if let Some(char_start) = self.weaver_block_char_start.take() {
                    // Build the full syntax text: { buffered_content }
                    let syntax_text = format!("{{{}}}", self.weaver_block_buffer);
                    let syntax_char_len = syntax_text.chars().count();
                    let char_end = char_start + syntax_char_len;

                    let syn_id = self.gen_syn_id();

                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                        syn_id, char_start, char_end
                    )?;
                    escape_html(&mut self.writer, &syntax_text)?;
                    self.write("</span>")?;

                    // Track the syntax span
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type: SyntaxType::Block,
                        formatted_range: None,
                    });

                    // Record offset mapping for the syntax span
                    self.record_mapping(range.clone(), char_start..char_end);

                    // Update tracking
                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;
                }

                // Parse the buffered text for attrs and store for next block
                if !self.weaver_block_buffer.is_empty() {
                    let parsed = Self::parse_weaver_attrs(&self.weaver_block_buffer);
                    self.weaver_block_buffer.clear();
                    // Merge with any existing pending attrs or set new
                    if let Some(ref mut existing) = self.pending_block_attrs {
                        existing.classes.extend(parsed.classes);
                        existing.attrs.extend(parsed.attrs);
                    } else {
                        self.pending_block_attrs = Some(parsed);
                    }
                }

                Ok(())
            }
            TagEnd::FootnoteDefinition => {
                self.write("</div>\n")?;

                // Link the footnote definition span with its reference span
                if let Some((name, def_span_index, _def_char_start)) =
                    self.current_footnote_def.take()
                {
                    let def_char_end = self.last_char_offset;

                    // Look up the reference span
                    if let Some(&(ref_span_index, ref_char_start)) =
                        self.footnote_ref_spans.get(&name)
                    {
                        // Create formatted_range spanning from ref start to def end
                        let formatted_range = ref_char_start..def_char_end;

                        // Update both spans with the same formatted_range
                        // so they show/hide together based on cursor proximity
                        if let Some(ref_span) = self.syntax_spans.get_mut(ref_span_index) {
                            ref_span.formatted_range = Some(formatted_range.clone());
                        }
                        if let Some(def_span) = self.syntax_spans.get_mut(def_span_index) {
                            def_span.formatted_range = Some(formatted_range);
                        }
                    }
                }

                Ok(())
            }
            TagEnd::MetadataBlock(_) => {
                self.in_non_writing_block = false;
                Ok(())
            }
        };

        result?;

        // Note: Closing syntax for inline formatting tags (Strong, Emphasis, Strikethrough)
        // is handled INSIDE their respective match arms above, AFTER writing the closing HTML.
        // This ensures the closing syntax span appears OUTSIDE the formatted element.
        // Other End events have their closing syntax emitted by emit_gap_before() in the main loop.

        Ok(())
    }
}
