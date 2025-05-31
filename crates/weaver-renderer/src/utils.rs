use std::{fmt::Arguments, path::Path, sync::OnceLock};

use markdown_weaver::{CodeBlockKind, CowStr, Event, Tag};
use markdown_weaver_escape::StrWrite;
use miette::IntoDiagnostic;
use n0_future::TryFutureExt;
use n0_future::io::AsyncWrite;
use n0_future::io::AsyncWriteExt;
use regex::Regex;

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub async fn inline_file(path: impl AsRef<Path>) -> Option<String> {
    tokio::fs::read_to_string(path).await.ok()
}
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub async fn inline_file(path: impl AsRef<Path>) -> Option<String> {
    todo!()
}

pub const AVOID_URL_CHARS: &[char] = &[
    '!', '#', '$', '&', '\'', '(', ')', '*', '+', ',', ';', '=', ':', '@', '%', '[', ']', '?', '/',
    '~', '|', '{', '}', '^', '`',
];

pub fn resolve_at_ident_or_uri<'s>(
    link: &markdown_weaver::Tag<'s>,
    appview: &str,
) -> markdown_weaver::Tag<'s> {
    use markdown_weaver::Tag;
    match link {
        Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        } => {
            if dest_url.starts_with("at://") {
                // Make the appview string swappable
                let at_uri = weaver_common::aturi_to_http(dest_url.as_ref(), appview);
                if let Some(at_uri) = at_uri {
                    Tag::Link {
                        link_type: *link_type,
                        dest_url: at_uri.into_static(),
                        title: title.clone(),
                        id: id.clone(),
                    }
                } else {
                    link.clone()
                }
            } else if dest_url.starts_with("@") {
                let maybe_identifier = dest_url.strip_prefix("@").unwrap();
                if let Some(identifier) = weaver_common::match_identifier(maybe_identifier) {
                    let link = CowStr::Boxed(
                        format!("https://{}/profile/{}", appview, identifier).into_boxed_str(),
                    );
                    Tag::Link {
                        link_type: *link_type,
                        dest_url: link,
                        title: title.clone(),
                        id: id.clone(),
                    }
                } else {
                    link.clone()
                }
            } else {
                link.clone()
            }
        }
        _ => link.clone(),
    }
}

/// Rough and ready check if a path is a local path.
/// Basically checks if the path is absolute and if so, is it accessible.
/// Relative paths are assumed to be local
pub fn is_local_path(path: &str) -> bool {
    let path = Path::new(path);
    path.is_relative() || path.try_exists().unwrap_or(false)
}

/// Is this link relative to somewhere?
/// Rust has built-in checks for file paths, so this just wraps that.
pub fn is_relative_link(link: &str) -> bool {
    let path = Path::new(link);
    path.is_relative()
}

/// Flatten a directory path to just the parent and filename, if present.
/// Maybe worth to swap to using the Path tools, but this works.
pub fn flatten_dir_to_just_one_parent(path: &str) -> (&str, &str) {
    static RE_PARENT_DIR: OnceLock<Regex> = OnceLock::new();
    let caps = RE_PARENT_DIR
        .get_or_init(|| {
            Regex::new(r".*[/\\](?P<parent>[^/\\]+)[/\\](?P<filename>[^/\\]+)$").unwrap()
        })
        .captures(path);
    if let Some(caps) = caps {
        if let Some(parent) = caps.name("parent") {
            if let Some(filename) = caps.name("filename") {
                return (parent.as_str(), filename.as_str());
            }
            return (parent.as_str(), "");
        }
        if let Some(filename) = caps.name("filename") {
            return ("", filename.as_str());
        }
    }
    ("", path)
}

fn event_to_owned<'a>(event: Event<'a>) -> Event<'a> {
    match event {
        Event::Start(tag) => Event::Start(tag_to_owned(tag)),
        Event::End(tag) => Event::End(tag),
        Event::Text(cowstr) => Event::Text(CowStr::from(cowstr.into_string())),
        Event::Code(cowstr) => Event::Code(CowStr::from(cowstr.into_string())),
        Event::Html(cowstr) => Event::Html(CowStr::from(cowstr.into_string())),
        Event::InlineHtml(cowstr) => Event::InlineHtml(CowStr::from(cowstr.into_string())),
        Event::FootnoteReference(cowstr) => {
            Event::FootnoteReference(CowStr::from(cowstr.into_string()))
        }
        Event::SoftBreak => Event::SoftBreak,
        Event::HardBreak => Event::HardBreak,
        Event::Rule => Event::Rule,
        Event::TaskListMarker(checked) => Event::TaskListMarker(checked),
        Event::InlineMath(cowstr) => Event::InlineMath(CowStr::from(cowstr.into_string())),
        Event::DisplayMath(cowstr) => Event::DisplayMath(CowStr::from(cowstr.into_string())),
        Event::WeaverBlock(cow_str) => todo!(),
    }
}

fn tag_to_owned<'a>(tag: Tag<'a>) -> Tag<'a> {
    match tag {
        Tag::Paragraph => Tag::Paragraph,
        Tag::Heading {
            level: heading_level,
            id,
            classes,
            attrs,
        } => Tag::Heading {
            level: heading_level,
            id: id.map(|cowstr| CowStr::from(cowstr.into_string())),
            classes: classes
                .into_iter()
                .map(|cowstr| CowStr::from(cowstr.into_string()))
                .collect(),
            attrs: attrs
                .into_iter()
                .map(|(attr, value)| {
                    (
                        CowStr::from(attr.into_string()),
                        value.map(|cowstr| CowStr::from(cowstr.into_string())),
                    )
                })
                .collect(),
        },
        Tag::BlockQuote(blockquote_kind) => Tag::BlockQuote(blockquote_kind),
        Tag::CodeBlock(codeblock_kind) => Tag::CodeBlock(codeblock_kind_to_owned(codeblock_kind)),
        Tag::List(optional) => Tag::List(optional),
        Tag::Item => Tag::Item,
        Tag::FootnoteDefinition(cowstr) => {
            Tag::FootnoteDefinition(CowStr::from(cowstr.into_string()))
        }
        Tag::Table(alignment_vector) => Tag::Table(alignment_vector),
        Tag::TableHead => Tag::TableHead,
        Tag::TableRow => Tag::TableRow,
        Tag::TableCell => Tag::TableCell,
        Tag::Emphasis => Tag::Emphasis,
        Tag::Strong => Tag::Strong,
        Tag::Strikethrough => Tag::Strikethrough,
        Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        } => Tag::Link {
            link_type,
            dest_url: CowStr::from(dest_url.into_string()),
            title: CowStr::from(title.into_string()),
            id: CowStr::from(id.into_string()),
        },
        Tag::Embed {
            embed_type,
            dest_url,
            title,
            id,
            attrs,
        } => Tag::Embed {
            embed_type,
            dest_url: CowStr::from(dest_url.into_string()),
            title: CowStr::from(title.into_string()),
            id: CowStr::from(id.into_string()),
            attrs,
        },
        Tag::Image {
            link_type,
            dest_url,
            title,
            id,
            attrs,
        } => Tag::Image {
            link_type,
            dest_url: CowStr::from(dest_url.into_string()),
            title: CowStr::from(title.into_string()),
            id: CowStr::from(id.into_string()),
            attrs,
        },
        Tag::HtmlBlock => Tag::HtmlBlock,
        Tag::MetadataBlock(metadata_block_kind) => Tag::MetadataBlock(metadata_block_kind),
        Tag::DefinitionList => Tag::DefinitionList,
        Tag::DefinitionListTitle => Tag::DefinitionListTitle,
        Tag::DefinitionListDefinition => Tag::DefinitionListDefinition,
        Tag::Superscript => todo!(),
        Tag::Subscript => todo!(),
        Tag::WeaverBlock(weaver_block_kind, weaver_attributes) => todo!(),
    }
}

fn codeblock_kind_to_owned<'a>(codeblock_kind: CodeBlockKind<'_>) -> CodeBlockKind<'a> {
    match codeblock_kind {
        CodeBlockKind::Indented => CodeBlockKind::Indented,
        CodeBlockKind::Fenced(cowstr) => CodeBlockKind::Fenced(CowStr::from(cowstr.into_string())),
    }
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
use tokio::fs::{self, File};

pub async fn create_file(dest: &Path) -> miette::Result<File> {
    let file = File::create(dest)
        .or_else(async |err| {
            {
                if err.kind() == std::io::ErrorKind::NotFound {
                    let parent = dest.parent().expect("file should have a parent directory");
                    fs::create_dir_all(parent).await?
                }
                File::create(dest)
            }
            .await
        })
        .await
        .into_diagnostic()?;
    Ok(file)
}
