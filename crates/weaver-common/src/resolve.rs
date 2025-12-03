//! Wikilink and embed resolution types for rendering without network calls
//!
//! This module provides pre-resolution infrastructure so that markdown rendering
//! can happen synchronously without network calls in the hot path.

use std::collections::HashMap;

use jacquard::CowStr;
use jacquard::smol_str::SmolStr;
use jacquard::types::string::AtUri;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;

/// Pre-resolved data for rendering without network calls.
///
/// Populated during an async collection phase, then passed to the sync render phase.
#[derive(Debug, Clone, Default)]
pub struct ResolvedContent {
    /// Wikilink target (lowercase) → resolved entry info
    pub entry_links: HashMap<SmolStr, ResolvedEntry>,
    /// AT URI → rendered HTML content
    pub embed_content: HashMap<AtUri<'static>, CowStr<'static>>,
    /// AT URI → StrongRef for populating records array
    pub embed_refs: Vec<StrongRef<'static>>,
}

/// A resolved entry reference from a wikilink
#[derive(Debug, Clone)]
pub struct ResolvedEntry {
    /// The canonical URL path (e.g., "/handle/notebook/entry_path")
    pub canonical_path: CowStr<'static>,
    /// The original entry title for display
    pub display_title: CowStr<'static>,
}

impl ResolvedContent {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a wikilink target, returns the resolved entry if found
    pub fn resolve_wikilink(&self, target: &str) -> Option<&ResolvedEntry> {
        // Strip fragment if present
        let (target, _fragment) = target.split_once('#').unwrap_or((target, ""));
        let key = SmolStr::new(target.to_lowercase());
        self.entry_links.get(&key)
    }

    /// Get pre-rendered embed content for an AT URI
    pub fn get_embed_content(&self, uri: &AtUri<'_>) -> Option<&str> {
        // Need to look up by equivalent URI, not exact reference
        self.embed_content
            .iter()
            .find(|(k, _)| k.as_str() == uri.as_str())
            .map(|(_, v)| v.as_ref())
    }

    /// Add a resolved entry link
    pub fn add_entry(
        &mut self,
        target: &str,
        canonical_path: impl Into<CowStr<'static>>,
        display_title: impl Into<CowStr<'static>>,
    ) {
        self.entry_links.insert(
            SmolStr::new(target.to_lowercase()),
            ResolvedEntry {
                canonical_path: canonical_path.into(),
                display_title: display_title.into(),
            },
        );
    }

    /// Add resolved embed content
    pub fn add_embed(
        &mut self,
        uri: AtUri<'static>,
        html: impl Into<CowStr<'static>>,
        strong_ref: Option<StrongRef<'static>>,
    ) {
        self.embed_content.insert(uri, html.into());
        if let Some(sr) = strong_ref {
            self.embed_refs.push(sr);
        }
    }
}

/// Index of entries within a notebook for wikilink resolution.
///
/// Supports case-insensitive matching against entry title OR path slug.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntryIndex {
    /// lowercase title → (canonical_path, original_title)
    by_title: HashMap<SmolStr, (CowStr<'static>, CowStr<'static>)>,
    /// lowercase path slug → (canonical_path, original_title)
    by_path: HashMap<SmolStr, (CowStr<'static>, CowStr<'static>)>,
}

impl EntryIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entry to the index
    pub fn add_entry(
        &mut self,
        title: &str,
        path: &str,
        canonical_url: impl Into<CowStr<'static>>,
    ) {
        let canonical: CowStr<'static> = canonical_url.into();
        let title_cow: CowStr<'static> = CowStr::from(title.to_string());

        self.by_title.insert(
            SmolStr::new(title.to_lowercase()),
            (canonical.clone(), title_cow.clone()),
        );
        self.by_path
            .insert(SmolStr::new(path.to_lowercase()), (canonical, title_cow));
    }

    /// Resolve a wikilink target to (canonical_path, display_title, fragment)
    ///
    /// Matches case-insensitively against title first, then path slug.
    /// Fragment (if present) is returned with the input's lifetime.
    pub fn resolve<'a, 'b>(
        &'a self,
        wikilink: &'b str,
    ) -> Option<(&'a str, &'a str, Option<&'b str>)> {
        let (target, fragment) = match wikilink.split_once('#') {
            Some((t, f)) => (t, Some(f)),
            None => (wikilink, None),
        };
        let key = SmolStr::new(target.to_lowercase());

        // Try title match first
        if let Some((path, title)) = self.by_title.get(&key) {
            return Some((path.as_ref(), title.as_ref(), fragment));
        }

        // Try path match
        if let Some((path, title)) = self.by_path.get(&key) {
            return Some((path.as_ref(), title.as_ref(), fragment));
        }

        None
    }

    /// Parse a wikilink into (target, fragment)
    pub fn parse_wikilink(wikilink: &str) -> (&str, Option<&str>) {
        match wikilink.split_once('#') {
            Some((t, f)) => (t, Some(f)),
            None => (wikilink, None),
        }
    }

    /// Check if the index contains any entries
    pub fn is_empty(&self) -> bool {
        self.by_title.is_empty()
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.by_title.len()
    }
}

/// Reference extracted from markdown that needs resolution
#[derive(Debug, Clone, PartialEq)]
pub enum ExtractedRef {
    /// Wikilink like [[Entry Name]] or [[Entry Name#header]]
    Wikilink {
        target: String,
        fragment: Option<String>,
        display_text: Option<String>,
    },
    /// AT Protocol embed like ![[at://did/collection/rkey]] or ![alt](at://...)
    AtEmbed {
        uri: String,
        alt_text: Option<String>,
    },
    /// AT Protocol link like [text](at://...)
    AtLink { uri: String },
}

/// Collector for refs encountered during rendering.
///
/// Pass this to renderers to collect refs as a side effect of the render pass.
/// This avoids a separate parsing pass just for collection.
#[derive(Debug, Clone, Default)]
pub struct RefCollector {
    pub refs: Vec<ExtractedRef>,
}

impl RefCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a wikilink reference
    pub fn add_wikilink(
        &mut self,
        target: &str,
        fragment: Option<&str>,
        display_text: Option<&str>,
    ) {
        self.refs.push(ExtractedRef::Wikilink {
            target: target.to_string(),
            fragment: fragment.map(|s| s.to_string()),
            display_text: display_text.map(|s| s.to_string()),
        });
    }

    /// Record an AT Protocol embed reference
    pub fn add_at_embed(&mut self, uri: &str, alt_text: Option<&str>) {
        self.refs.push(ExtractedRef::AtEmbed {
            uri: uri.to_string(),
            alt_text: alt_text.map(|s| s.to_string()),
        });
    }

    /// Record an AT Protocol link reference
    pub fn add_at_link(&mut self, uri: &str) {
        self.refs.push(ExtractedRef::AtLink {
            uri: uri.to_string(),
        });
    }

    /// Get wikilinks that need resolution
    pub fn wikilinks(&self) -> impl Iterator<Item = &str> {
        self.refs.iter().filter_map(|r| match r {
            ExtractedRef::Wikilink { target, .. } => Some(target.as_str()),
            _ => None,
        })
    }

    /// Get AT URIs that need fetching
    pub fn at_uris(&self) -> impl Iterator<Item = &str> {
        self.refs.iter().filter_map(|r| match r {
            ExtractedRef::AtEmbed { uri, .. } | ExtractedRef::AtLink { uri } => Some(uri.as_str()),
            _ => None,
        })
    }

    /// Take ownership of collected refs
    pub fn take(self) -> Vec<ExtractedRef> {
        self.refs
    }
}

/// Extract all references from markdown that need resolution.
///
/// **Note:** This does a separate parsing pass. For production use, prefer
/// passing a `RefCollector` to the renderer to collect during the render pass.
/// This function is primarily useful for testing or quick analysis.
#[cfg(any(test, feature = "standalone-collection"))]
pub fn collect_refs_from_markdown(markdown: &str) -> Vec<ExtractedRef> {
    use markdown_weaver::{Event, LinkType, Options, Parser, Tag};

    let mut collector = RefCollector::new();
    let options = Options::all();
    let parser = Parser::new_ext(markdown, options);

    for event in parser {
        match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                let url = dest_url.as_ref();

                if matches!(link_type, LinkType::WikiLink { .. }) {
                    let (target, fragment) = match url.split_once('#') {
                        Some((t, f)) => (t, Some(f)),
                        None => (url, None),
                    };
                    collector.add_wikilink(target, fragment, None);
                } else if url.starts_with("at://") {
                    collector.add_at_link(url);
                }
            }
            Event::Start(Tag::Embed {
                dest_url, title, ..
            }) => {
                let url = dest_url.as_ref();

                if url.starts_with("at://") || url.starts_with("did:") {
                    let alt = if title.is_empty() {
                        None
                    } else {
                        Some(title.as_ref())
                    };
                    collector.add_at_embed(url, alt);
                } else if !url.starts_with("http://") && !url.starts_with("https://") {
                    let (target, fragment) = match url.split_once('#') {
                        Some((t, f)) => (t, Some(f)),
                        None => (url, None),
                    };
                    collector.add_wikilink(target, fragment, None);
                }
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                let url = dest_url.as_ref();

                if url.starts_with("at://") {
                    let alt = if title.is_empty() {
                        None
                    } else {
                        Some(title.as_ref())
                    };
                    collector.add_at_embed(url, alt);
                }
            }
            _ => {}
        }
    }

    collector.take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jacquard::IntoStatic;

    #[test]
    fn test_entry_index_resolve_by_title() {
        let mut index = EntryIndex::new();
        index.add_entry(
            "My First Note",
            "my_first_note",
            "/alice/notebook/my_first_note",
        );

        let result = index.resolve("My First Note");
        assert!(result.is_some());
        let (path, title, fragment) = result.unwrap();
        assert_eq!(path, "/alice/notebook/my_first_note");
        assert_eq!(title, "My First Note");
        assert_eq!(fragment, None);
    }

    #[test]
    fn test_entry_index_resolve_case_insensitive() {
        let mut index = EntryIndex::new();
        index.add_entry(
            "My First Note",
            "my_first_note",
            "/alice/notebook/my_first_note",
        );

        let result = index.resolve("my first note");
        assert!(result.is_some());
    }

    #[test]
    fn test_entry_index_resolve_by_path() {
        let mut index = EntryIndex::new();
        index.add_entry(
            "My First Note",
            "my_first_note",
            "/alice/notebook/my_first_note",
        );

        let result = index.resolve("my_first_note");
        assert!(result.is_some());
    }

    #[test]
    fn test_entry_index_resolve_with_fragment() {
        let mut index = EntryIndex::new();
        index.add_entry("My Note", "my_note", "/alice/notebook/my_note");

        let result = index.resolve("My Note#section");
        assert!(result.is_some());
        let (path, title, fragment) = result.unwrap();
        assert_eq!(path, "/alice/notebook/my_note");
        assert_eq!(title, "My Note");
        assert_eq!(fragment, Some("section"));
    }

    #[test]
    fn test_collect_refs_wikilink() {
        let markdown = "Check out [[My Note]] for more info.";
        let refs = collect_refs_from_markdown(markdown);

        assert_eq!(refs.len(), 1);
        assert!(matches!(
            &refs[0],
            ExtractedRef::Wikilink { target, .. } if target == "My Note"
        ));
    }

    #[test]
    fn test_collect_refs_at_link() {
        let markdown = "See [this post](at://did:plc:xyz/app.bsky.feed.post/abc)";
        let refs = collect_refs_from_markdown(markdown);

        assert_eq!(refs.len(), 1);
        assert!(matches!(
            &refs[0],
            ExtractedRef::AtLink { uri } if uri == "at://did:plc:xyz/app.bsky.feed.post/abc"
        ));
    }

    #[test]
    fn test_collect_refs_at_embed() {
        let markdown = "![[at://did:plc:xyz/app.bsky.feed.post/abc]]";
        let refs = collect_refs_from_markdown(markdown);

        assert_eq!(refs.len(), 1);
        assert!(matches!(
            &refs[0],
            ExtractedRef::AtEmbed { uri, .. } if uri == "at://did:plc:xyz/app.bsky.feed.post/abc"
        ));
    }

    #[test]
    fn test_resolved_content_wikilink_lookup() {
        let mut content = ResolvedContent::new();
        content.add_entry("My Note", "/alice/notebook/my_note", "My Note");

        let result = content.resolve_wikilink("my note");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().canonical_path.as_ref(),
            "/alice/notebook/my_note"
        );
    }

    #[test]
    fn test_resolved_content_embed_lookup() {
        let mut content = ResolvedContent::new();
        let uri = AtUri::new("at://did:plc:xyz/app.bsky.feed.post/abc").unwrap();
        content.add_embed(uri.into_static(), "<div>post content</div>", None);

        let lookup_uri = AtUri::new("at://did:plc:xyz/app.bsky.feed.post/abc").unwrap();
        let result = content.get_embed_content(&lookup_uri);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "<div>post content</div>");
    }
}
