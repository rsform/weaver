//! Static renderer
//!
//! This mode of the renderer creates a static html and css website from a notebook in a local directory.
//! It does not upload it to the PDS by default (though it can ). This is good for testing and for self-hosting.
//! URLs in the notebook are mostly unaltered. It is compatible with GitHub or Cloudflare Pages
//! and other similar static hosting services.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{ContextIterator, NotebookProcessor, base_html::TableState, walker::WalkOptions};
use async_trait::async_trait;
use atrium_api::agent::{Configure, SessionManager};
use bitflags::bitflags;
use dashmap::DashMap;
use markdown_weaver::{
    Alignment, BlockQuoteKind, BrokenLink, CodeBlockKind, CowStr, EmbedType, Event, LinkType,
    Parser, Tag, WeaverAttributes,
};
use markdown_weaver_escape::{
    FmtWriter, StrWrite, escape_href, escape_html, escape_html_body_text,
};
use miette::IntoDiagnostic;
use n0_future::StreamExt;
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use n0_future::io::AsyncWriteExt;
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
use tokio::io::AsyncWriteExt;
use unicode_normalization::UnicodeNormalization;
use weaver_common::{
    agent::{WeaverAgent, WeaverHttpClient},
    aturi_to_http,
};
use yaml_rust2::Yaml;

use crate::{Frontmatter, NotebookContext};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct StaticSiteOptions:u32 {
        const FLATTEN_STRUCTURE = 1 << 1;
        const UPLOAD_BLOBS = 1 << 2;
        const INLINE_EMBEDS = 1 << 3;
        const ADD_LINK_PREVIEWS = 1 << 4;
        const RESOLVE_AT_IDENTIFIERS = 1 << 5;
        const RESOLVE_AT_URIS = 1 << 6;
        const ADD_BSKY_COMMENTS_EMBED = 1 << 7;
        const CREATE_INDEX = 1 << 8;
        const CREATE_CHAPTERS_BY_DIRECTORY = 1 << 9;
        const CREATE_PAGES_BY_TITLE = 1 << 10;
        const NORMALIZE_DIR_NAMES = 1 << 11;
        const ADD_TOC_TO_PAGES = 1 << 12;
    }
}

impl Default for StaticSiteOptions {
    fn default() -> Self {
        Self::FLATTEN_STRUCTURE
            | Self::UPLOAD_BLOBS
            | Self::RESOLVE_AT_IDENTIFIERS
            | Self::RESOLVE_AT_URIS
            | Self::CREATE_INDEX
            | Self::CREATE_CHAPTERS_BY_DIRECTORY
            | Self::CREATE_PAGES_BY_TITLE
            | Self::NORMALIZE_DIR_NAMES
    }
}

pub fn default_md_options() -> markdown_weaver::Options {
    markdown_weaver::Options::ENABLE_WIKILINKS
        | markdown_weaver::Options::ENABLE_FOOTNOTES
        | markdown_weaver::Options::ENABLE_TABLES
        | markdown_weaver::Options::ENABLE_GFM
        | markdown_weaver::Options::ENABLE_STRIKETHROUGH
        | markdown_weaver::Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | markdown_weaver::Options::ENABLE_OBSIDIAN_EMBEDS
        | markdown_weaver::Options::ENABLE_MATH
        | markdown_weaver::Options::ENABLE_HEADING_ATTRIBUTES
}

pub struct StaticSiteContext<'a, M: SessionManager + Send + Sync> {
    options: StaticSiteOptions,
    md_options: markdown_weaver::Options,
    pub bsky_appview: CowStr<'a>,
    root: PathBuf,
    pub destination: PathBuf,
    start_at: PathBuf,
    pub frontmatter: Arc<DashMap<PathBuf, Frontmatter>>,
    dir_contents: Option<Arc<[PathBuf]>>,
    reference_map: Arc<DashMap<CowStr<'a>, PathBuf>>,
    titles: Arc<DashMap<PathBuf, CowStr<'a>>>,
    position: usize,
    agent: Option<Arc<WeaverAgent<M>>>,
}

impl<M: SessionManager + Send + Sync> StaticSiteContext<'_, M> {
    pub fn clone_with_dir_contents(&self, dir_contents: &[PathBuf]) -> Self {
        Self {
            start_at: self.start_at.clone(),
            root: self.root.clone(),
            bsky_appview: self.bsky_appview.clone(),
            options: self.options.clone(),
            md_options: self.md_options.clone(),
            frontmatter: self.frontmatter.clone(),
            dir_contents: Some(Arc::from(dir_contents)),
            destination: self.destination.clone(),
            reference_map: self.reference_map.clone(),
            titles: self.titles.clone(),
            position: self.position,
            agent: self.agent.clone(),
        }
    }

    pub fn clone_with_path(&self, path: impl AsRef<Path>) -> Self {
        let position = if let Some(dir_contents) = &self.dir_contents {
            dir_contents
                .iter()
                .position(|p| p == path.as_ref())
                .unwrap_or(0)
        } else {
            0
        };
        Self {
            start_at: self.start_at.clone(),
            root: self.root.clone(),
            bsky_appview: self.bsky_appview.clone(),
            options: self.options.clone(),
            md_options: self.md_options.clone(),
            frontmatter: self.frontmatter.clone(),
            dir_contents: self.dir_contents.clone(),
            destination: self.destination.clone(),
            reference_map: self.reference_map.clone(),
            titles: self.titles.clone(),
            position,
            agent: self.agent.clone(),
        }
    }
    pub fn new(root: PathBuf, destination: PathBuf, session: Option<M>) -> Self {
        Self {
            start_at: root.clone(),
            root,
            bsky_appview: CowStr::Borrowed("deer.social"),
            options: StaticSiteOptions::default(),
            md_options: default_md_options(),
            frontmatter: Arc::new(DashMap::new()),
            dir_contents: None,
            destination,
            reference_map: Arc::new(DashMap::new()),
            titles: Arc::new(DashMap::new()),
            position: 0,
            agent: session.map(|session| Arc::new(WeaverAgent::new(session))),
        }
    }

    pub fn current_path(&self) -> &PathBuf {
        if let Some(dir_contents) = &self.dir_contents {
            &dir_contents[self.position]
        } else {
            &self.start_at
        }
    }

    #[inline]
    pub fn handle_link_aturi<'s>(&self, link: Tag<'s>) -> Tag<'s> {
        let link = crate::utils::resolve_at_ident_or_uri(&link, &self.bsky_appview);
        self.handle_link_normal(link)
    }

    pub async fn handle_embed_aturi<'s>(&self, embed: Tag<'s>) -> Tag<'s> {
        if let Some(agent) = &self.agent {
            match &embed {
                Tag::Embed {
                    embed_type,
                    dest_url,
                    title,
                    id,
                    attrs,
                } => {
                    if dest_url.starts_with("at://") {
                        let width = if let Some(attrs) = attrs {
                            let mut width = 600;
                            for attr in &attrs.attrs {
                                if attr.0 == CowStr::Borrowed("width".into()) {
                                    width = attr.1.parse::<usize>().unwrap_or(600);
                                    break;
                                }
                            }
                            width
                        } else {
                            600
                        };
                        let html = if let Ok(resp) = agent
                            .client
                            .client
                            .get("https://embed.bsky.app/oembed")
                            .query(&[
                                ("url", dest_url.clone().into_string()),
                                ("maxwidth", width.to_string()),
                            ])
                            .send()
                            .await
                        {
                            resp.text().await.ok()
                        } else {
                            None
                        };
                        if let Some(html) = html {
                            let link = aturi_to_http(&dest_url, &self.bsky_appview)
                                .expect("assuming the at-uri is valid rn");
                            let mut attrs = if let Some(attrs) = attrs {
                                attrs.clone()
                            } else {
                                WeaverAttributes {
                                    classes: vec![],
                                    attrs: vec![],
                                }
                            };
                            attrs.attrs.push(("content".into(), html.into()));
                            Tag::Embed {
                                embed_type: EmbedType::Comments, // change this when i update markdown-weaver
                                dest_url: link.into_static(),
                                title: title.clone(),
                                id: id.clone(),
                                attrs: Some(attrs),
                            }
                        } else {
                            self.handle_embed_normal(embed).await
                        }
                    } else {
                        self.handle_embed_normal(embed).await
                    }
                }
                _ => embed,
            }
        } else {
            self.handle_embed_normal(embed).await
        }
    }

    pub async fn handle_embed_normal<'s>(&self, embed: Tag<'s>) -> Tag<'s> {
        // This option will REALLY slow down iteration over events.
        if self.options.contains(StaticSiteOptions::INLINE_EMBEDS) {
            match &embed {
                Tag::Embed {
                    embed_type: _,
                    dest_url,
                    title,
                    id,
                    attrs,
                } => {
                    let mut attrs = if let Some(attrs) = attrs {
                        attrs.clone()
                    } else {
                        WeaverAttributes {
                            classes: vec![],
                            attrs: vec![],
                        }
                    };
                    let contents = if crate::utils::is_local_path(dest_url) {
                        let file_path = if crate::utils::is_relative_link(dest_url) {
                            let root_path = self.root.clone();
                            root_path.join(Path::new(&dest_url as &str))
                        } else {
                            PathBuf::from(&dest_url as &str)
                        };
                        crate::utils::inline_file(&file_path).await
                    } else if let Some(agent) = &self.agent {
                        if let Ok(resp) = agent
                            .client
                            .client
                            .get(dest_url.clone().into_string())
                            .send()
                            .await
                        {
                            resp.text().await.ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(contents) = contents {
                        attrs.attrs.push(("content".into(), contents.into()));
                        Tag::Embed {
                            embed_type: EmbedType::Markdown, // change this when i update markdown-weaver
                            dest_url: dest_url.clone(),
                            title: title.clone(),
                            id: id.clone(),
                            attrs: Some(attrs),
                        }
                    } else {
                        embed
                    }
                }
                _ => embed,
            }
        } else {
            embed
        }
    }

    /// This is a no-op for the static site renderer currently.
    #[inline]
    pub fn handle_link_normal<'s>(&self, link: Tag<'s>) -> Tag<'s> {
        link
    }

    /// This is a no-op for the static site renderer currently.
    #[inline]
    pub fn handle_image_normal<'s>(&self, image: Tag<'s>) -> Tag<'s> {
        image
    }
}

impl<M: SessionManager + Configure + Send + Sync> StaticSiteContext<'_, M> {
    pub async fn upload_image<'s>(&self, image: Tag<'s>) -> Tag<'s> {
        if let Some(agent) = &self.agent {
            match &image {
                Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                    attrs,
                } => {
                    if crate::utils::is_local_path(&dest_url) {
                        let root_path = self.root.clone();
                        let file_path = root_path.join(Path::new(&dest_url as &str));
                        if let Ok(bytes) = std::fs::read(&file_path) {
                            if let Ok(blob_data) = agent.upload_blob(bytes).await {
                                let url = weaver_common::blob_url(
                                    agent.did().await.as_ref().unwrap(),
                                    &agent.pds(),
                                    &blob_data.blob,
                                );
                                return Tag::Image {
                                    link_type: *link_type,
                                    dest_url: url.into(),
                                    title: title.clone(),
                                    id: id.clone(),
                                    attrs: attrs.clone(),
                                };
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        image
    }
}

#[async_trait]
impl<M: SessionManager + Configure + Send + Sync> NotebookContext for StaticSiteContext<'_, M> {
    fn set_entry_title(&self, title: CowStr<'_>) {
        let path = self.current_path();
        self.titles
            .insert(path.clone(), title.clone().into_static());
        self.frontmatter.get_mut(path).map(|frontmatter| {
            if let Ok(mut yaml) = frontmatter.yaml.write() {
                if yaml.get(0).is_some_and(|y| y.is_hash()) {
                    let map = yaml.get_mut(0).unwrap().as_mut_hash().unwrap();
                    map.insert(
                        Yaml::String("title".into()),
                        Yaml::String(title.into_static().into()),
                    );
                }
            }
        });
    }
    fn entry_title(&self) -> CowStr<'_> {
        let path = self.current_path();
        self.titles.get(path).unwrap().clone()
    }

    fn frontmatter(&self) -> Frontmatter {
        let path = self.current_path();
        self.frontmatter.get(path).unwrap().value().clone()
    }

    fn set_frontmatter(&self, frontmatter: Frontmatter) {
        let path = self.current_path();
        self.frontmatter.insert(path.clone(), frontmatter);
    }

    async fn handle_link<'s>(&self, link: Tag<'s>) -> Tag<'s> {
        bitflags::bitflags_match!(self.options, {
            // Split this somehow or just combine the options
            StaticSiteOptions::RESOLVE_AT_URIS | StaticSiteOptions::RESOLVE_AT_IDENTIFIERS => {
                self.handle_link_aturi(link)
            }
            _ => match &link {
                Tag::Link { link_type, dest_url, title, id } => {
                    if self.options.contains(StaticSiteOptions::FLATTEN_STRUCTURE) {
                        let (parent, filename) = crate::utils::flatten_dir_to_just_one_parent(&dest_url);
                        let dest_url = if crate::utils::is_relative_link(&dest_url)
                            && self.options.contains(StaticSiteOptions::CREATE_CHAPTERS_BY_DIRECTORY) {
                            CowStr::Boxed(format!("./{}/{}", parent, filename).into_boxed_str())
                        } else {
                            CowStr::Boxed(format!("./entry/{}", filename).into_boxed_str())
                        };
                        Tag::Link {
                            link_type: *link_type,
                            dest_url,
                            title: title.clone(),
                            id: id.clone(),
                        }
                    } else {
                        link

                    }
                },
                _ => link,
            }
        })
    }

    async fn handle_image<'s>(&self, image: Tag<'s>) -> Tag<'s> {
        if self.options.contains(StaticSiteOptions::UPLOAD_BLOBS) {
            self.upload_image(image).await
        } else {
            self.handle_image_normal(image)
        }
    }

    async fn handle_embed<'s>(&self, embed: Tag<'s>) -> Tag<'s> {
        if self.options.contains(StaticSiteOptions::RESOLVE_AT_URIS)
            || self.options.contains(StaticSiteOptions::ADD_LINK_PREVIEWS)
        {
            self.handle_embed_aturi(embed).await
        } else {
            self.handle_embed_normal(embed).await
        }
    }

    fn handle_reference(&self, reference: CowStr<'_>) -> CowStr<'_> {
        let reference = reference.into_static();
        if let Some(reference) = self.reference_map.get(&reference) {
            let path = reference.value().clone();
            CowStr::Boxed(path.to_string_lossy().into_owned().into_boxed_str())
        } else {
            reference
        }
    }

    fn add_reference(&self, reference: CowStr<'_>) {
        let path = self.current_path();
        self.reference_map
            .insert(reference.into_static(), path.clone());
    }
}

pub struct StaticSiteWriter<'a, M>
where
    M: SessionManager + Send + Sync,
{
    context: StaticSiteContext<'a, M>,
}

impl<'a, M> StaticSiteWriter<'a, M>
where
    M: SessionManager + Send + Sync,
{
    pub fn new(root: PathBuf, destination: PathBuf, session: Option<M>) -> Self {
        let context = StaticSiteContext::new(root, destination, session);
        Self { context }
    }
}

impl<'a, M> StaticSiteWriter<'a, M>
where
    M: SessionManager + Configure + Send + Sync,
{
    pub async fn run(self) -> Result<(), miette::Report> {
        todo!()
    }

    pub async fn export_page<'s, 'input>(
        &'s self,
        contents: &'input str,
        context: StaticSiteContext<'s, M>,
    ) -> Result<String, miette::Report> {
        let callback = if let Some(dir_contents) = context.dir_contents.clone() {
            Some(VaultBrokenLinkCallback {
                vault_contents: dir_contents,
            })
        } else {
            None
        };
        let parser = Parser::new_with_broken_link_callback(&contents, context.md_options, callback);
        let iterator = ContextIterator::default(parser);
        let mut output = String::new();
        let writer = StaticPageWriter::new(
            NotebookProcessor::new(context, iterator),
            FmtWriter(&mut output),
        );
        writer.run().await.into_diagnostic()?;
        Ok(output)
    }

    pub async fn write_page(&'a self, path: PathBuf) -> Result<(), miette::Report> {
        let contents = tokio::fs::read_to_string(&path).await.into_diagnostic()?;
        let mut output_file = crate::utils::create_file(&path).await?;
        let context = self.context.clone_with_path(&path);
        let output = self.export_page(&contents, context).await?;
        output_file
            .write_all(output.as_bytes())
            .await
            .into_diagnostic()?;
        Ok(())
    }
}

pub struct StaticPageWriter<
    'a,
    'input,
    I: Iterator<Item = Event<'input>>,
    M: SessionManager + Send + Sync,
    W: StrWrite,
> {
    context: NotebookProcessor<'input, I, StaticSiteContext<'a, M>>,
    writer: W,
    /// Whether or not the last write wrote a newline.
    end_newline: bool,

    /// Whether if inside a metadata block (text should not be written)
    in_non_writing_block: bool,

    table_state: TableState,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,
    numbers: DashMap<CowStr<'a>, usize>,
}

impl<'a, 'input, I: Iterator<Item = Event<'input>>, M: SessionManager + Send + Sync, W: StrWrite>
    StaticPageWriter<'a, 'input, I, M, W>
{
    pub fn new(context: NotebookProcessor<'input, I, StaticSiteContext<'a, M>>, writer: W) -> Self {
        Self {
            context,
            writer,
            end_newline: true,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: DashMap::new(),
        }
    }

    /// Writes a new line.
    #[inline]
    fn write_newline(&mut self) -> Result<(), W::Error> {
        self.end_newline = true;
        self.writer.write_str("\n")
    }

    /// Writes a buffer, and tracks whether or not a newline was written.
    #[inline]
    fn write(&mut self, s: &str) -> Result<(), W::Error> {
        self.writer.write_str(s)?;

        if !s.is_empty() {
            self.end_newline = s.ends_with('\n');
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: markdown_weaver::TagEnd) -> Result<(), W::Error> {
        use markdown_weaver::TagEnd;
        match tag {
            TagEnd::HtmlBlock => {}
            TagEnd::Paragraph => {
                self.write("</p>\n")?;
            }
            TagEnd::Heading(level) => {
                self.write("</")?;
                write!(&mut self.writer, "{}", level)?;
                self.write(">\n")?;
            }
            TagEnd::Table => {
                self.write("</tbody></table>\n")?;
            }
            TagEnd::TableHead => {
                self.write("</tr></thead><tbody>\n")?;
                self.table_state = TableState::Body;
            }
            TagEnd::TableRow => {
                self.write("</tr>\n")?;
            }
            TagEnd::TableCell => {
                match self.table_state {
                    TableState::Head => {
                        self.write("</th>")?;
                    }
                    TableState::Body => {
                        self.write("</td>")?;
                    }
                }
                self.table_cell_index += 1;
            }
            TagEnd::BlockQuote(_) => {
                self.write("</blockquote>\n")?;
            }
            TagEnd::CodeBlock => {
                self.write("</code></pre>\n")?;
            }
            TagEnd::List(true) => {
                self.write("</ol>\n")?;
            }
            TagEnd::List(false) => {
                self.write("</ul>\n")?;
            }
            TagEnd::Item => {
                self.write("</li>\n")?;
            }
            TagEnd::DefinitionList => {
                self.write("</dl>\n")?;
            }
            TagEnd::DefinitionListTitle => {
                self.write("</dt>\n")?;
            }
            TagEnd::DefinitionListDefinition => {
                self.write("</dd>\n")?;
            }
            TagEnd::Emphasis => {
                self.write("</em>")?;
            }
            TagEnd::Superscript => {
                self.write("</sup>")?;
            }
            TagEnd::Subscript => {
                self.write("</sub>")?;
            }
            TagEnd::Strong => {
                self.write("</strong>")?;
            }
            TagEnd::Strikethrough => {
                self.write("</del>")?;
            }
            TagEnd::Link => {
                self.write("</a>")?;
            }
            TagEnd::Image => (), // shouldn't happen, handled in start
            TagEnd::Embed => (), // shouldn't happen, handled in start
            TagEnd::WeaverBlock(_) => {
                self.in_non_writing_block = false;
            }
            TagEnd::FootnoteDefinition => {
                self.write("</div>\n")?;
            }
            TagEnd::MetadataBlock(_) => {
                self.in_non_writing_block = false;
            }
        }
        Ok(())
    }
}

impl<
    'a,
    'input,
    I: Iterator<Item = Event<'input>>,
    M: SessionManager + Configure + Send + Sync,
    W: StrWrite,
> StaticPageWriter<'a, 'input, I, M, W>
{
    async fn run(mut self) -> Result<(), W::Error> {
        while let Some(event) = self.context.next().await {
            self.process_event(event).await?
        }
        Ok(())
    }

    async fn process_event(&mut self, event: Event<'input>) -> Result<(), W::Error> {
        use markdown_weaver::Event::*;
        match event {
            Start(tag) => {
                self.start_tag(tag).await?;
            }
            End(tag) => {
                self.end_tag(tag)?;
            }
            Text(text) => {
                if !self.in_non_writing_block {
                    escape_html_body_text(&mut self.writer, &text)?;
                    self.end_newline = text.ends_with('\n');
                }
            }
            Code(text) => {
                self.write("<code>")?;
                escape_html_body_text(&mut self.writer, &text)?;
                self.write("</code>")?;
            }
            InlineMath(text) => {
                self.write(r#"<span class="math math-inline">"#)?;
                escape_html(&mut self.writer, &text)?;
                self.write("</span>")?;
            }
            DisplayMath(text) => {
                self.write(r#"<span class="math math-display">"#)?;
                escape_html(&mut self.writer, &text)?;
                self.write("</span>")?;
            }
            Html(html) | InlineHtml(html) => {
                self.write(&html)?;
            }
            SoftBreak => {
                self.write_newline()?;
            }
            HardBreak => {
                self.write("<br />\n")?;
            }
            Rule => {
                if self.end_newline {
                    self.write("<hr />\n")?;
                } else {
                    self.write("\n<hr />\n")?;
                }
            }
            FootnoteReference(name) => {
                let len = self.numbers.len() + 1;
                self.write("<sup class=\"footnote-reference\"><a href=\"#")?;
                escape_html(&mut self.writer, &name)?;
                self.write("\">")?;
                let number = *self.numbers.entry(name.into_static()).or_insert(len);
                write!(&mut self.writer, "{}", number)?;
                self.write("</a></sup>")?;
            }
            TaskListMarker(true) => {
                self.write("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>\n")?;
            }
            TaskListMarker(false) => {
                self.write("<input disabled=\"\" type=\"checkbox\"/>\n")?;
            }
            WeaverBlock(_text) => {}
        }
        Ok(())
    }

    // run raw text, consuming end tag
    async fn raw_text(&mut self) -> Result<(), W::Error> {
        use markdown_weaver::Event::*;
        let mut nest = 0;
        while let Some(event) = self.context.next().await {
            match event {
                Start(_) => nest += 1,
                End(_) => {
                    if nest == 0 {
                        break;
                    }
                    nest -= 1;
                }
                Html(_) => {}
                InlineHtml(text) | Code(text) | Text(text) => {
                    // Don't use escape_html_body_text here.
                    // The output of this function is used in the `alt` attribute.
                    escape_html(&mut self.writer, &text)?;
                    self.end_newline = text.ends_with('\n');
                }
                InlineMath(text) => {
                    self.write("$")?;
                    escape_html(&mut self.writer, &text)?;
                    self.write("$")?;
                }
                DisplayMath(text) => {
                    self.write("$$")?;
                    escape_html(&mut self.writer, &text)?;
                    self.write("$$")?;
                }
                SoftBreak | HardBreak | Rule => {
                    self.write(" ")?;
                }
                FootnoteReference(name) => {
                    let len = self.numbers.len() + 1;
                    let number = *self.numbers.entry(name.into_static()).or_insert(len);
                    write!(&mut self.writer, "[{}]", number)?;
                }
                TaskListMarker(true) => self.write("[x]")?,
                TaskListMarker(false) => self.write("[ ]")?,
                WeaverBlock(_) => {
                    println!("Weaver block internal");
                }
            }
        }
        Ok(())
    }

    /// Writes the start of an HTML tag.
    async fn start_tag(&mut self, tag: Tag<'input>) -> Result<(), W::Error> {
        match tag {
            Tag::HtmlBlock => Ok(()),
            Tag::Paragraph => {
                if self.end_newline {
                    self.write("<p>")
                } else {
                    self.write("\n<p>")
                }
            }
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => {
                if self.end_newline {
                    self.write("<")?;
                } else {
                    self.write("\n<")?;
                }
                write!(&mut self.writer, "{}", level)?;
                if let Some(id) = id {
                    self.write(" id=\"")?;
                    escape_html(&mut self.writer, &id)?;
                    self.write("\"")?;
                }
                let mut classes = classes.iter();
                if let Some(class) = classes.next() {
                    self.write(" class=\"")?;
                    escape_html(&mut self.writer, class)?;
                    for class in classes {
                        self.write(" ")?;
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
                self.write(">")
            }
            Tag::Table(alignments) => {
                self.table_alignments = alignments;
                self.write("<table>")
            }
            Tag::TableHead => {
                self.table_state = TableState::Head;
                self.table_cell_index = 0;
                self.write("<thead><tr>")
            }
            Tag::TableRow => {
                self.table_cell_index = 0;
                self.write("<tr>")
            }
            Tag::TableCell => {
                match self.table_state {
                    TableState::Head => {
                        self.write("<th")?;
                    }
                    TableState::Body => {
                        self.write("<td")?;
                    }
                }
                match self.table_alignments.get(self.table_cell_index) {
                    Some(&Alignment::Left) => self.write(" style=\"text-align: left\">"),
                    Some(&Alignment::Center) => self.write(" style=\"text-align: center\">"),
                    Some(&Alignment::Right) => self.write(" style=\"text-align: right\">"),
                    _ => self.write(">"),
                }
            }
            Tag::BlockQuote(kind) => {
                let class_str = match kind {
                    None => "",
                    Some(kind) => match kind {
                        BlockQuoteKind::Note => " class=\"markdown-alert-note\"",
                        BlockQuoteKind::Tip => " class=\"markdown-alert-tip\"",
                        BlockQuoteKind::Important => " class=\"markdown-alert-important\"",
                        BlockQuoteKind::Warning => " class=\"markdown-alert-warning\"",
                        BlockQuoteKind::Caution => " class=\"markdown-alert-caution\"",
                    },
                };
                if self.end_newline {
                    self.write(&format!("<blockquote{}>\n", class_str))
                } else {
                    self.write(&format!("\n<blockquote{}>\n", class_str))
                }
            }
            Tag::CodeBlock(info) => {
                if !self.end_newline {
                    self.write_newline()?;
                }
                match info {
                    CodeBlockKind::Fenced(info) => {
                        let lang = info.split(' ').next().unwrap();
                        if lang.is_empty() {
                            self.write("<pre><code>")
                        } else {
                            self.write("<pre><code class=\"language-")?;
                            escape_html(&mut self.writer, lang)?;
                            self.write("\">")
                        }
                    }
                    CodeBlockKind::Indented => self.write("<pre><code>"),
                }
            }
            Tag::List(Some(1)) => {
                if self.end_newline {
                    self.write("<ol>\n")
                } else {
                    self.write("\n<ol>\n")
                }
            }
            Tag::List(Some(start)) => {
                if self.end_newline {
                    self.write("<ol start=\"")?;
                } else {
                    self.write("\n<ol start=\"")?;
                }
                write!(&mut self.writer, "{}", start)?;
                self.write("\">\n")
            }
            Tag::List(None) => {
                if self.end_newline {
                    self.write("<ul>\n")
                } else {
                    self.write("\n<ul>\n")
                }
            }
            Tag::Item => {
                if self.end_newline {
                    self.write("<li>")
                } else {
                    self.write("\n<li>")
                }
            }
            Tag::DefinitionList => {
                if self.end_newline {
                    self.write("<dl>\n")
                } else {
                    self.write("\n<dl>\n")
                }
            }
            Tag::DefinitionListTitle => {
                if self.end_newline {
                    self.write("<dt>")
                } else {
                    self.write("\n<dt>")
                }
            }
            Tag::DefinitionListDefinition => {
                if self.end_newline {
                    self.write("<dd>")
                } else {
                    self.write("\n<dd>")
                }
            }
            Tag::Subscript => self.write("<sub>"),
            Tag::Superscript => self.write("<sup>"),
            Tag::Emphasis => self.write("<em>"),
            Tag::Strong => self.write("<strong>"),
            Tag::Strikethrough => self.write("<del>"),
            Tag::Link {
                link_type: LinkType::Email,
                dest_url,
                title,
                id: _,
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
                link_type: _,
                dest_url,
                title,
                id: _,
            } => {
                self.write("<a href=\"")?;
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
                self.write_image(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                    attrs,
                })
                .await
            }
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                if let Some(attrs) = attrs {
                    if let Some((_, content)) = attrs
                        .attrs
                        .iter()
                        .find(|(attr, _)| attr.as_ref() == "content")
                    {
                        match embed_type {
                            EmbedType::Image => {
                                self.write_image(Tag::Image {
                                    link_type: LinkType::Inline,
                                    dest_url,
                                    title,
                                    id,
                                    attrs: Some(attrs.clone()),
                                })
                                .await?
                            }
                            EmbedType::Comments => {
                                self.write("leaflet would go here\n")?;
                            }
                            EmbedType::Post => {
                                // Bluesky post embed, basically just render the raw html we got
                                self.write(content)?;
                                self.write_newline()?;
                            }
                            EmbedType::Markdown => {
                                // let context = self
                                //     .context
                                //     .context
                                //     .clone_with_path(&Path::new(&dest_url.to_string()));
                                // let callback =
                                //     if let Some(dir_contents) = context.dir_contents.clone() {
                                //         Some(VaultBrokenLinkCallback {
                                //             vault_contents: dir_contents,
                                //         })
                                //     } else {
                                //         None
                                //     };
                                // let parser = Parser::new_with_broken_link_callback(
                                //     &content,
                                //     context.md_options,
                                //     callback,
                                // );
                                // let iterator = ContextIterator::default(parser);
                                // let mut stream = NotebookProcessor::new(context, iterator);
                                // while let Some(event) = stream.next().await {
                                //     self.process_event(event).await?;
                                // }
                                //
                                self.write("markdown embed would go here\n")?;
                            }
                            EmbedType::Leaflet => {
                                self.write("leaflet would go here\n")?;
                            }
                            EmbedType::Other => {
                                self.write("other embed would go here\n")?;
                            }
                        }
                    }
                } else {
                    self.write("<iframe src=\"")?;
                    escape_href(&mut self.writer, &dest_url)?;
                    self.write("\" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                    if !id.is_empty() {
                        self.write("\" id=\"")?;
                        escape_html(&mut self.writer, &id)?;
                        self.write("\"")?;
                    }
                    if let Some(attrs) = attrs {
                        self.write(" ")?;
                        if !attrs.classes.is_empty() {
                            self.write("class=\"")?;
                            for class in &attrs.classes {
                                escape_html(&mut self.writer, class)?;
                                self.write(" ")?;
                            }
                            self.write("\" ")?;
                        }
                        if !attrs.attrs.is_empty() {
                            for (attr, value) in &attrs.attrs {
                                escape_html(&mut self.writer, attr)?;
                                self.write("=\"")?;
                                escape_html(&mut self.writer, value)?;
                                self.write("\" ")?;
                            }
                        }
                    }
                    self.write("/>")?;
                }
                Ok(())
            }
            Tag::WeaverBlock(_, _attrs) => {
                println!("Weaver block");
                self.in_non_writing_block = true;
                Ok(())
            }
            Tag::FootnoteDefinition(name) => {
                if self.end_newline {
                    self.write("<div class=\"footnote-definition\" id=\"")?;
                } else {
                    self.write("\n<div class=\"footnote-definition\" id=\"")?;
                }
                escape_html(&mut self.writer, &name)?;
                self.write("\"><sup class=\"footnote-definition-label\">")?;
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name.into_static()).or_insert(len);
                write!(&mut self.writer, "{}", number)?;
                self.write("</sup>")
            }
            Tag::MetadataBlock(_) => {
                self.in_non_writing_block = true;
                Ok(())
            }
        }
    }

    async fn write_image(&mut self, tag: Tag<'input>) -> Result<(), W::Error> {
        if let Tag::Image {
            link_type: _,
            dest_url,
            title,
            id: _,
            attrs,
        } = tag
        {
            self.write("<img src=\"")?;
            escape_href(&mut self.writer, &dest_url)?;
            if let Some(attrs) = attrs {
                if !attrs.classes.is_empty() {
                    self.write("\" class=\"")?;
                    for class in &attrs.classes {
                        escape_html(&mut self.writer, class)?;
                        self.write(" ")?;
                    }
                    self.write("\" ")?;
                } else {
                    self.write("\" ")?;
                }
                if !attrs.attrs.is_empty() {
                    for (attr, value) in &attrs.attrs {
                        escape_html(&mut self.writer, attr)?;
                        self.write("=\"")?;
                        escape_html(&mut self.writer, value)?;
                        self.write("\" ")?;
                    }
                }
            } else {
                self.write("\" ")?;
            }
            self.write("alt=\"")?;
            self.raw_text().await?;
            if !title.is_empty() {
                self.write("\" title=\"")?;
                escape_html(&mut self.writer, &title)?;
            }
            self.write("\" />")
        } else {
            self.write_newline()
        }
    }
}

/// Path lookup in an Obsidian vault
///
/// Credit to https://github.com/zoni
///
/// Taken from https://github.com/zoni/obsidian-export/blob/main/src/lib.rs.rs on 2025-05-21
///
pub fn lookup_filename_in_vault<'a>(
    filename: &str,
    vault_contents: &'a [PathBuf],
) -> Option<&'a PathBuf> {
    let filename = PathBuf::from(filename);
    let filename_normalized: String = filename.to_string_lossy().nfc().collect();

    vault_contents.iter().find(|path| {
        let path_normalized_str: String = path.to_string_lossy().nfc().collect();
        let path_normalized = PathBuf::from(&path_normalized_str);
        let path_normalized_lowered = PathBuf::from(&path_normalized_str.to_lowercase());

        // It would be convenient if we could just do `filename.set_extension("md")` at the start
        // of this funtion so we don't need multiple separate + ".md" match cases here, however
        // that would break with a reference of `[[Note.1]]` linking to `[[Note.1.md]]`.

        path_normalized.ends_with(&filename_normalized)
            || path_normalized.ends_with(filename_normalized.clone() + ".md")
            || path_normalized_lowered.ends_with(filename_normalized.to_lowercase())
            || path_normalized_lowered.ends_with(filename_normalized.to_lowercase() + ".md")
    })
}

pub struct VaultBrokenLinkCallback {
    vault_contents: Arc<[PathBuf]>,
}

impl<'input> markdown_weaver::BrokenLinkCallback<'input> for VaultBrokenLinkCallback {
    fn handle_broken_link(
        &mut self,
        link: BrokenLink<'input>,
    ) -> Option<(CowStr<'input>, CowStr<'input>)> {
        let text = link.reference;
        let captures = crate::OBSIDIAN_NOTE_LINK_RE
            .captures(&text)
            .expect("note link regex didn't match - bad input?");
        let file = captures.name("file").map(|v| v.as_str().trim());
        let label = captures.name("label").map(|v| v.as_str());
        let section = captures.name("section").map(|v| v.as_str().trim());

        if let Some(file) = file {
            if let Some(path) = lookup_filename_in_vault(file, self.vault_contents.as_ref()) {
                let mut link_text = String::from(path.to_string_lossy());
                if let Some(section) = section {
                    link_text.push('#');
                    link_text.push_str(section);
                    if let Some(label) = label {
                        let label = label.to_string();
                        Some((CowStr::from(link_text), CowStr::from(label)))
                    } else {
                        Some((link_text.into(), format!("{} > {}", file, section).into()))
                    }
                } else {
                    Some((link_text.into(), format!("{}", file).into()))
                }
            } else {
                None
            }
        } else {
            None
        }
    }
}
