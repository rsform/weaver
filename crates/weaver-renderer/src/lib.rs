//! Weaver renderer
//!
//! This crate works with the weaver-markdown crate to render and optionally upload markdown notebooks to your Atproto PDS.
//!

use async_trait::async_trait;
use markdown_weaver::CowStr;
use markdown_weaver::Event;
use markdown_weaver::LinkType;
use markdown_weaver::Tag;
use n0_future::Stream;
use n0_future::StreamExt;
use n0_future::pin;
use n0_future::stream::once_future;
use yaml_rust2::Yaml;
use yaml_rust2::YamlLoader;

use regex::Regex;
use std::iter::Iterator;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;
use std::task::Poll;

pub mod atproto;
pub mod base_html;
pub mod code_pretty;
pub mod static_site;
pub mod types;
pub mod utils;
pub mod walker;

pub static OBSIDIAN_NOTE_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<file>[^#|]+)??(#(?P<section>.+?))??(\|(?P<label>.+?))??$").unwrap()
});

#[derive(Debug, Default)]
pub struct ContextIterator<'a, I: Iterator<Item = Event<'a>>> {
    pub context: Option<EventContext>,
    pub iter: I,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, I: Iterator<Item = Event<'a>>> ContextIterator<'a, I> {
    pub fn new(context: EventContext, iter: I) -> Self {
        Self {
            context: Some(context),
            iter,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn default(iter: I) -> Self {
        Self {
            context: None,
            iter,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'a, I: Iterator<Item = Event<'a>>> Iterator for ContextIterator<'a, I> {
    type Item = (Event<'a>, EventContext);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(next) = self.iter.next() {
            let ctxt = EventContext::get_context(&next, self.context.as_ref());
            self.context = Some(ctxt);
            Some((next, ctxt))
        } else {
            None
        }
    }
}

#[derive(Debug, Default)]
#[pin_project::pin_project]
pub struct NotebookProcessor<'a, I: Iterator<Item = Event<'a>>, CTX> {
    context: CTX,
    iter: ContextIterator<'a, I>,
}

impl<'a, I: Iterator<Item = Event<'a>>, CTX> NotebookProcessor<'a, I, CTX> {
    pub fn new(ctx: CTX, iter: ContextIterator<'a, I>) -> Self {
        Self { context: ctx, iter }
    }
}

impl<'a, I: Iterator<Item = Event<'a>>, CTX: NotebookContext> Stream
    for NotebookProcessor<'a, I, CTX>
{
    type Item = Event<'a>;

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let iter: &mut ContextIterator<'a, I> = this.iter;
        if let Some((event, ctxt)) = iter.next() {
            match ctxt {
                EventContext::EmbedLink => match event {
                    Event::Start(ref tag) => match tag {
                        Tag::Embed { .. } => {
                            let fut = once_future(this.context.handle_embed(tag.clone()));
                            pin!(fut);
                            fut.poll_next(cx)
                                .map(|tag| tag.map(|t| Event::Start(t.into_static())))
                        }
                        _ => Poll::Ready(Some(event)),
                    },
                    _ => Poll::Ready(Some(event)),
                },
                EventContext::CodeBlock => Poll::Ready(Some(event)),
                EventContext::Text => Poll::Ready(Some(event)),
                EventContext::Html => Poll::Ready(Some(event)),
                EventContext::Heading => Poll::Ready(Some(event)),
                EventContext::Reference => match event {
                    Event::Start(ref tag) => match tag {
                        Tag::Link { .. } => {
                            let fut = once_future(this.context.handle_link(tag.clone()));
                            pin!(fut);
                            fut.poll_next(cx).map(|tag| tag.map(|t| Event::Start(t)))
                        }
                        _ => Poll::Ready(Some(event)),
                    },
                    Event::FootnoteReference(ref name) => {
                        this.context.handle_reference(name.clone());
                        Poll::Ready(Some(event))
                    }
                    _ => Poll::Ready(Some(event)),
                },
                EventContext::RefDef => match event {
                    Event::Start(ref tag) => match tag {
                        Tag::FootnoteDefinition(name) => {
                            this.context.add_reference(name.clone());
                            Poll::Ready(Some(event))
                        }
                        _ => Poll::Ready(Some(event)),
                    },
                    _ => Poll::Ready(Some(event)),
                },
                EventContext::Link => match event {
                    Event::Start(ref tag) => match tag {
                        Tag::Link { .. } => {
                            let fut = once_future(this.context.handle_link(tag.clone()));
                            pin!(fut);
                            fut.poll_next(cx).map(|tag| tag.map(|t| Event::Start(t)))
                        }
                        _ => Poll::Ready(Some(event)),
                    },
                    _ => Poll::Ready(Some(event)),
                },
                EventContext::Image => match event {
                    Event::Start(ref tag) => match tag {
                        Tag::Image { .. } => {
                            let fut = once_future(this.context.handle_image(tag.clone()));
                            pin!(fut);
                            fut.poll_next(cx).map(|tag| tag.map(|t| Event::Start(t)))
                        }
                        _ => Poll::Ready(Some(event)),
                    },
                    _ => Poll::Ready(Some(event)),
                },

                EventContext::Table => Poll::Ready(Some(event)),
                EventContext::Metadata => match event {
                    Event::Text(ref text) => {
                        let frontmatter = Frontmatter::new(&text);
                        this.context.set_frontmatter(frontmatter);
                        Poll::Ready(Some(event))
                    }
                    _ => Poll::Ready(Some(event)),
                },
                EventContext::Other => Poll::Ready(Some(event)),
                EventContext::None => Poll::Ready(Some(event)),
            }
        } else {
            Poll::Ready(None)
        }
    }
}

#[async_trait]
pub trait NotebookContext {
    fn set_entry_title(&self, title: CowStr<'_>);
    fn entry_title(&self) -> CowStr<'_>;
    fn normalized_entry_title(&self) -> CowStr<'_> {
        let title = self.entry_title();
        let mut normalized = String::new();
        for c in title.chars() {
            if c.is_ascii_alphanumeric() {
                normalized.push(c);
            } else if c.is_whitespace() && !normalized.is_empty() && !(c == '\n' || c == '\r') {
                normalized.push('-');
            } else if c == '\n' {
                normalized.push('_');
            } else if c == '\r' {
                continue;
            } else if !crate::utils::AVOID_URL_CHARS.contains(&c) {
                normalized.push(c);
            }
        }
        CowStr::Boxed(normalized.into_boxed_str())
    }
    fn frontmatter(&self) -> Frontmatter;
    fn set_frontmatter(&self, frontmatter: Frontmatter);
    async fn handle_link<'s>(&self, link: Tag<'s>) -> Tag<'s>;
    async fn handle_image<'s>(&self, image: Tag<'s>) -> Tag<'s>;
    async fn handle_embed<'s>(&self, embed: Tag<'s>) -> Tag<'s>;
    fn handle_reference(&self, reference: CowStr<'_>) -> CowStr<'_>;
    fn add_reference(&self, reference: CowStr<'_>);
}

#[derive(Debug, Clone)]
pub struct Frontmatter {
    yaml: Arc<RwLock<Vec<Yaml>>>,
}

impl Frontmatter {
    pub fn new(text: &str) -> Self {
        let yaml = YamlLoader::load_from_str(text).unwrap_or_else(|_| vec![Yaml::BadValue]);
        Self {
            yaml: Arc::new(RwLock::new(yaml)),
        }
    }

    pub fn contents(&self) -> Arc<RwLock<Vec<Yaml>>> {
        self.yaml.clone()
    }
}

impl Default for Frontmatter {
    fn default() -> Self {
        Frontmatter {
            yaml: Arc::new(RwLock::new(vec![])),
        }
    }
}

#[derive(thiserror::Error, Debug, miette::Diagnostic)]
pub enum RenderError {
    #[error("WalkDir error at {}", path.display())]
    #[diagnostic(code(crate::static_site::walker))]
    WalkDirError { path: PathBuf, msg: String },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum EventContext {
    EmbedLink,
    CodeBlock,
    #[default]
    Text,
    Html,
    Heading,
    Reference,
    RefDef,
    Image,
    Link,
    Table,
    Metadata,
    Other,
    None,
}

impl EventContext {
    pub fn get_context<'a>(event: &Event<'a>, prev: Option<&Self>) -> Self {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => Self::Text,
                Tag::Heading { .. } => Self::Heading,
                Tag::BlockQuote(_block_quote_kind) => Self::Text,
                Tag::CodeBlock(_code_block_kind) => Self::CodeBlock,
                Tag::HtmlBlock => Self::Text,
                Tag::List(_) => Self::Other,
                Tag::Item => Self::Other,
                Tag::FootnoteDefinition(_cow_str) => Self::RefDef,
                Tag::DefinitionList => Self::Other,
                Tag::DefinitionListTitle => Self::Other,
                Tag::DefinitionListDefinition => Self::Other,
                Tag::Table(_alignments) => Self::Table,
                Tag::TableHead => Self::Table,
                Tag::TableRow => Self::Table,
                Tag::TableCell => Self::Table,
                Tag::Emphasis => Self::Text,
                Tag::Strong => Self::Text,
                Tag::Strikethrough => Self::Text,
                Tag::Superscript => Self::Text,
                Tag::Subscript => Self::Text,
                Tag::Link { .. } => Self::Link,
                Tag::Image { .. } => Self::Image,
                Tag::Embed { .. } => Self::EmbedLink,
                Tag::WeaverBlock(_weaver_block_kind, _weaver_attributes) => Self::Metadata,
                Tag::MetadataBlock(_metadata_block_kind) => Self::Metadata,
            },
            Event::End(_tag_end) => Self::None,
            Event::Text(_cow_str) => match prev {
                Some(ctxt) => match ctxt {
                    EventContext::None => Self::Text,
                    _ => *ctxt,
                },
                None => Self::Text,
            },
            Event::Code(_cow_str) => Self::CodeBlock,
            Event::InlineMath(_cow_str) => Self::Other,
            Event::DisplayMath(_cow_str) => Self::Other,
            Event::Html(_cow_str) => Self::Html,
            Event::InlineHtml(_cow_str) => Self::Html,
            Event::FootnoteReference(_cow_str) => Self::Reference,
            Event::SoftBreak => Self::Other,
            Event::HardBreak => Self::Other,
            Event::Rule => Self::Other,
            Event::TaskListMarker(_cow_str) => Self::Other,
            Event::WeaverBlock(_cow_str) => Self::Other,
        }
    }

    pub fn is_non_writing_block(&self) -> bool {
        match self {
            Self::Metadata => true,
            _ => false,
        }
    }
}
