use super::{error::ClientRenderError, types::BlobName};
use crate::{
    Frontmatter, NotebookContext,
    atproto::embed_renderer::{
        fetch_and_render_entry, fetch_and_render_leaflet, fetch_and_render_whitewind_entry,
    },
};
use jacquard::{
    client::{Agent, AgentSession},
    prelude::IdentityResolver,
    types::string::{AtUri, Cid, Did},
};
use markdown_weaver::{CowStr as MdCowStr, LinkType, Tag, WeaverAttributes};
use std::collections::HashMap;
use std::sync::Arc;
use weaver_api::sh_weaver::notebook::entry::Entry;
use weaver_common::{EntryIndex, ResolvedContent};

/// Trait for resolving embed content on the client side
///
/// Implementations can fetch from cache, make HTTP requests, or use other sources.
pub trait EmbedResolver {
    /// Resolve a profile embed by AT URI
    fn resolve_profile(
        &self,
        uri: &AtUri<'_>,
    ) -> impl std::future::Future<Output = Result<String, ClientRenderError>>;

    /// Resolve a post/record embed by AT URI
    fn resolve_post(
        &self,
        uri: &AtUri<'_>,
    ) -> impl std::future::Future<Output = Result<String, ClientRenderError>>;

    /// Resolve a markdown embed from URL
    ///
    /// `depth` parameter tracks recursion depth to prevent infinite loops
    fn resolve_markdown(
        &self,
        url: &str,
        depth: usize,
    ) -> impl std::future::Future<Output = Result<String, ClientRenderError>>;
}

/// Default embed resolver that fetches records from PDSs
///
/// This uses the same fetch/render logic as the preprocessor.
pub struct DefaultEmbedResolver<A: AgentSession + IdentityResolver> {
    agent: Arc<Agent<A>>,
}

impl<A: AgentSession + IdentityResolver> DefaultEmbedResolver<A> {
    pub fn new(agent: Arc<Agent<A>>) -> Self {
        Self { agent }
    }
}

impl<A: AgentSession + IdentityResolver> EmbedResolver for DefaultEmbedResolver<A> {
    async fn resolve_profile(&self, uri: &AtUri<'_>) -> Result<String, ClientRenderError> {
        use crate::atproto::fetch_and_render_profile;
        fetch_and_render_profile(uri.authority(), &*self.agent)
            .await
            .map_err(|e| ClientRenderError::EntryFetch {
                uri: uri.as_ref().to_string(),
                source: Box::new(e),
            })
    }

    async fn resolve_post(&self, uri: &AtUri<'_>) -> Result<String, ClientRenderError> {
        use crate::atproto::{fetch_and_render_generic, fetch_and_render_post};

        // Check if it's a known type
        if let Some(collection) = uri.collection() {
            match collection.as_ref() {
                "app.bsky.feed.post" => {
                    fetch_and_render_post(uri, &*self.agent).await.map_err(|e| {
                        ClientRenderError::EntryFetch {
                            uri: uri.as_ref().to_string(),
                            source: Box::new(e),
                        }
                    })
                }
                "sh.weaver.notebook.entry" => fetch_and_render_entry(uri, &*self.agent)
                    .await
                    .map_err(|e| ClientRenderError::EntryFetch {
                        uri: uri.as_ref().to_string(),
                        source: Box::new(e),
                    }),
                "pub.leaflet.document" => fetch_and_render_leaflet(uri, &*self.agent)
                    .await
                    .map_err(|e| ClientRenderError::EntryFetch {
                        uri: uri.as_ref().to_string(),
                        source: Box::new(e),
                    }),
                "com.whtwnd.blog.entry" => fetch_and_render_whitewind_entry(uri, &*self.agent)
                    .await
                    .map_err(|e| ClientRenderError::EntryFetch {
                        uri: uri.as_ref().to_string(),
                        source: Box::new(e),
                    }),
                _ => fetch_and_render_generic(uri, &*self.agent)
                    .await
                    .map_err(|e| ClientRenderError::EntryFetch {
                        uri: uri.as_ref().to_string(),
                        source: Box::new(e),
                    }),
            }
        } else {
            Err(ClientRenderError::EntryFetch {
                uri: uri.as_ref().to_string(),
                source: "AT URI missing collection".into(),
            })
        }
    }

    async fn resolve_markdown(
        &self,
        url: &str,
        _depth: usize,
    ) -> Result<String, ClientRenderError> {
        // TODO: implement HTTP fetch + markdown rendering
        Err(ClientRenderError::EntryFetch {
            uri: url.to_string(),
            source: "Markdown URL embeds not yet implemented".into(),
        })
    }
}

impl EmbedResolver for () {
    async fn resolve_profile(&self, _uri: &AtUri<'_>) -> Result<String, ClientRenderError> {
        Ok("".to_string())
    }

    async fn resolve_post(&self, _uri: &AtUri<'_>) -> Result<String, ClientRenderError> {
        Ok("".to_string())
    }

    async fn resolve_markdown(
        &self,
        _url: &str,
        _depth: usize,
    ) -> Result<String, ClientRenderError> {
        Ok("".to_string())
    }
}

impl EmbedResolver for ResolvedContent {
    async fn resolve_profile(&self, uri: &AtUri<'_>) -> Result<String, ClientRenderError> {
        self.get_embed_content(uri)
            .map(|s| s.to_string())
            .ok_or_else(|| ClientRenderError::EntryFetch {
                uri: uri.to_string(),
                source: "Not in pre-resolved content".into(),
            })
    }

    async fn resolve_post(&self, uri: &AtUri<'_>) -> Result<String, ClientRenderError> {
        self.get_embed_content(uri)
            .map(|s| s.to_string())
            .ok_or_else(|| ClientRenderError::EntryFetch {
                uri: uri.to_string(),
                source: "Not in pre-resolved content".into(),
            })
    }

    async fn resolve_markdown(
        &self,
        _url: &str,
        _depth: usize,
    ) -> Result<String, ClientRenderError> {
        Ok("".to_string())
    }
}

const MAX_EMBED_DEPTH: usize = 3;

#[derive(Clone)]
pub struct ClientContext<'a, R = ()> {
    // Entry being rendered
    entry: Entry<'a>,
    creator_did: Did<'a>,

    // Blob resolution
    blob_map: HashMap<BlobName<'static>, Cid<'static>>,

    // Embed resolution (optional, generic over resolver type)
    embed_resolver: Option<Arc<R>>,
    embed_depth: usize,

    // Pre-resolved content for sync rendering
    entry_index: Option<EntryIndex>,
    resolved_content: Option<ResolvedContent>,

    // Shared state
    frontmatter: Frontmatter,
    title: MdCowStr<'a>,
}

impl<'a, R: EmbedResolver> ClientContext<'a, R> {
    pub fn new(entry: Entry<'a>, creator_did: Did<'a>) -> ClientContext<'a, ()> {
        let blob_map = Self::build_blob_map(&entry);
        let title = MdCowStr::Boxed(entry.title.as_ref().into());

        ClientContext {
            entry,
            creator_did,
            blob_map,
            embed_resolver: None,
            embed_depth: 0,
            entry_index: None,
            resolved_content: None,
            frontmatter: Frontmatter::default(),
            title,
        }
    }

    /// Add an entry index for wikilink resolution
    pub fn with_entry_index(mut self, index: EntryIndex) -> Self {
        self.entry_index = Some(index);
        self
    }

    /// Add pre-resolved content for sync rendering
    pub fn with_resolved_content(mut self, content: ResolvedContent) -> Self {
        self.resolved_content = Some(content);
        self
    }
}

impl<'a> ClientContext<'a> {
    /// Add an embed resolver for fetching embed content
    pub fn with_embed_resolver<R: EmbedResolver>(self, resolver: Arc<R>) -> ClientContext<'a, R> {
        ClientContext {
            entry: self.entry,
            creator_did: self.creator_did,
            blob_map: self.blob_map,
            embed_resolver: Some(resolver),
            embed_depth: self.embed_depth,
            entry_index: self.entry_index,
            resolved_content: self.resolved_content,
            frontmatter: self.frontmatter,
            title: self.title,
        }
    }
}

impl<'a, R: EmbedResolver> ClientContext<'a, R> {
    /// Create a child context with incremented embed depth (for recursive embeds)
    fn with_depth(&self, depth: usize) -> Self
    where
        R: Clone,
    {
        Self {
            entry: self.entry.clone(),
            creator_did: self.creator_did.clone(),
            blob_map: self.blob_map.clone(),
            embed_resolver: self.embed_resolver.clone(),
            embed_depth: depth,
            entry_index: self.entry_index.clone(),
            resolved_content: self.resolved_content.clone(),
            frontmatter: self.frontmatter.clone(),
            title: self.title.clone(),
        }
    }

    /// Build an embed tag with resolved content attached
    fn build_embed_with_content<'s>(
        &self,
        embed_type: markdown_weaver::EmbedType,
        url: String,
        title: MdCowStr<'s>,
        id: MdCowStr<'s>,
        content: String,
        is_at_uri: bool,
    ) -> Tag<'s> {
        let mut attrs = WeaverAttributes {
            classes: vec![],
            attrs: vec![],
        };

        attrs.attrs.push(("content".into(), content.into()));

        // Add metadata for client-side enhancement
        if is_at_uri {
            attrs
                .attrs
                .push(("data-embed-uri".into(), url.clone().into()));

            if let Ok(at_uri) = AtUri::new(&url) {
                if at_uri.collection().is_none() {
                    attrs
                        .attrs
                        .push(("data-embed-type".into(), "profile".into()));
                } else {
                    attrs.attrs.push(("data-embed-type".into(), "post".into()));
                }
            }
        }

        Tag::Embed {
            embed_type,
            dest_url: MdCowStr::Boxed(url.into_boxed_str()),
            title,
            id,
            attrs: Some(attrs),
        }
    }

    fn build_blob_map<'b>(entry: &Entry<'b>) -> HashMap<BlobName<'static>, Cid<'static>> {
        use jacquard::IntoStatic;

        let mut map = HashMap::new();
        if let Some(embeds) = &entry.embeds {
            if let Some(images) = &embeds.images {
                for img in &images.images {
                    if let Some(name) = &img.name {
                        let blob_name = BlobName::from_filename(name.as_ref());
                        map.insert(blob_name, img.image.blob().cid().clone().into_static());
                    }
                }
            }
        }
        map
    }

    pub fn get_blob_cid(&self, name: &str) -> Option<&Cid<'static>> {
        let blob_name = BlobName::from_filename(name);
        self.blob_map.get(&blob_name)
    }
}

/// Convert an AT URI to a web URL based on collection type
///
/// Maps AT Protocol URIs to their web equivalents:
/// - Profile: `at://did:plc:xyz` → `https://weaver.sh/did:plc:xyz`
/// - Bluesky post: `at://{actor}/app.bsky.feed.post/{rkey}` → `https://bsky.app/profile/{actor}/post/{rkey}`
/// - Bluesky list: `at://{actor}/app.bsky.graph.list/{rkey}` → `https://bsky.app/profile/{actor}/lists/{rkey}`
/// - Bluesky feed: `at://{actor}/app.bsky.feed.generator/{rkey}` → `https://bsky.app/profile/{actor}/feed/{rkey}`
/// - Bluesky starterpack: `at://{actor}/app.bsky.graph.starterpack/{rkey}` → `https://bsky.app/starter-pack/{actor}/{rkey}`
/// - Weaver/other: `at://{actor}/{collection}/{rkey}` → `https://weaver.sh/record/{at_uri}`
fn at_uri_to_web_url(at_uri: &AtUri<'_>) -> String {
    let authority = at_uri.authority().as_ref();

    // Profile-only link (no collection/rkey)
    if at_uri.collection().is_none() && at_uri.rkey().is_none() {
        return format!("https://alpha.weaver.sh/{}", authority);
    }

    // Record link
    if let (Some(collection), Some(rkey)) = (at_uri.collection(), at_uri.rkey()) {
        let collection_str = collection.as_ref();
        let rkey_str = rkey.as_ref();

        // Map known Bluesky collections to bsky.app URLs
        match collection_str {
            "app.bsky.feed.post" => {
                format!("https://bsky.app/profile/{}/post/{}", authority, rkey_str)
            }
            "app.bsky.graph.list" => {
                format!("https://bsky.app/profile/{}/lists/{}", authority, rkey_str)
            }
            "app.bsky.feed.generator" => {
                format!("https://bsky.app/profile/{}/feed/{}", authority, rkey_str)
            }
            "app.bsky.graph.starterpack" => {
                format!("https://bsky.app/starter-pack/{}/{}", authority, rkey_str)
            }
            "sh.weaver.notebook.entry" => {
                format!("https://alpha.weaver.sh/{}/e/{}", authority, rkey_str)
            }
            "pub.leaflet.document" => {
                format!("https://alpha.weaver.sh/{}/p/{}", authority, rkey_str)
            }
            "com.whtwnd.blog.entry" => {
                format!("https://alpha.weaver.sh/{}/w/{}", authority, rkey_str)
            }
            // Weaver records and unknown collections go to weaver.sh
            _ => {
                format!("https://alpha.weaver.sh/record/{}", at_uri)
            }
        }
    } else {
        // Fallback for malformed URIs
        format!("https://alpha.weaver.sh/{}", authority)
    }
}

// Stub NotebookContext implementation
impl<'a, R> NotebookContext for ClientContext<'a, R>
where
    R: EmbedResolver,
{
    fn set_entry_title(&self, _title: MdCowStr<'_>) {
        // No-op for client context
    }

    fn entry_title(&self) -> MdCowStr<'_> {
        self.title.clone()
    }

    fn frontmatter(&self) -> Frontmatter {
        self.frontmatter.clone()
    }

    fn set_frontmatter(&self, _frontmatter: Frontmatter) {
        // No-op for client context
    }

    async fn handle_link<'s>(&self, link: Tag<'s>) -> Tag<'s> {
        match &link {
            Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            } => {
                // Handle WikiLinks via EntryIndex
                if matches!(link_type, LinkType::WikiLink { .. }) {
                    if let Some(index) = &self.entry_index {
                        let url = dest_url.as_ref();
                        if let Some((path, _title, fragment)) = index.resolve(url) {
                            // Build resolved URL with optional fragment
                            let resolved_url = match fragment {
                                Some(frag) => format!("{}#{}", path, frag),
                                None => path.to_string(),
                            };

                            return Tag::Link {
                                link_type: *link_type,
                                dest_url: MdCowStr::Boxed(resolved_url.into_boxed_str()),
                                title: title.clone(),
                                id: id.clone(),
                            };
                        }
                    }
                    // Unresolved wikilink - render as broken link
                    return Tag::Link {
                        link_type: *link_type,
                        dest_url: MdCowStr::Boxed(format!("#{}", dest_url).into_boxed_str()),
                        title: title.clone(),
                        id: id.clone(),
                    };
                }

                let url = dest_url.as_ref();

                // Try to parse as AT URI
                if let Ok(at_uri) = AtUri::new(url) {
                    let web_url = at_uri_to_web_url(&at_uri);

                    return Tag::Link {
                        link_type: *link_type,
                        dest_url: MdCowStr::Boxed(web_url.into_boxed_str()),
                        title: title.clone(),
                        id: id.clone(),
                    };
                }

                // Entry links starting with / are server-relative, pass through
                // External links pass through
                link
            }
            _ => link,
        }
    }

    async fn handle_image<'s>(&self, image: Tag<'s>) -> Tag<'s> {
        // Images already have canonical paths like /{notebook}/image/{name}
        // The server will handle routing these to the actual blobs
        image
    }

    async fn handle_embed<'s>(&self, embed: Tag<'s>) -> Tag<'s> {
        let Tag::Embed {
            embed_type,
            dest_url,
            title,
            id,
            attrs,
        } = &embed
        else {
            return embed;
        };

        // If content already in attrs (from preprocessor), pass through
        if let Some(attrs) = attrs {
            if attrs.attrs.iter().any(|(k, _)| k.as_ref() == "content") {
                return embed;
            }
        }

        // Own the URL to avoid borrow issues
        let url: String = dest_url.to_string();

        // Check recursion depth
        if self.embed_depth >= MAX_EMBED_DEPTH {
            return embed;
        }

        // First check for pre-resolved AT URI content
        if url.starts_with("at://") {
            if let Ok(at_uri) = AtUri::new(&url) {
                if let Some(resolved) = &self.resolved_content {
                    if let Some(content) = resolved.get_embed_content(&at_uri) {
                        return self.build_embed_with_content(
                            *embed_type,
                            url.clone(),
                            title.clone(),
                            id.clone(),
                            content.to_string(),
                            true,
                        );
                    }
                }
            }
        }

        // Check for wikilink-style embed (![[Entry Name]]) via entry index
        if !url.starts_with("at://") && !url.starts_with("http://") && !url.starts_with("https://")
        {
            if let Some(index) = &self.entry_index {
                if let Some((path, _title, fragment)) = index.resolve(&url) {
                    // Entry embed - link to the entry
                    let resolved_url = match fragment {
                        Some(frag) => format!("{}#{}", path, frag),
                        None => path.to_string(),
                    };
                    return Tag::Embed {
                        embed_type: *embed_type,
                        dest_url: MdCowStr::Boxed(resolved_url.into_boxed_str()),
                        title: title.clone(),
                        id: id.clone(),
                        attrs: attrs.clone(),
                    };
                }
            }
            // Unresolved entry embed - pass through
            return embed;
        }

        // Fallback to async resolver if available
        let Some(resolver) = &self.embed_resolver else {
            return embed;
        };

        // Try to fetch content based on URL type
        let content_result = if url.starts_with("at://") {
            // AT Protocol embed
            if let Ok(at_uri) = AtUri::new(&url) {
                if at_uri.collection().is_none() && at_uri.rkey().is_none() {
                    // Profile embed
                    resolver.resolve_profile(&at_uri).await
                } else {
                    // Post/record embed
                    resolver.resolve_post(&at_uri).await
                }
            } else {
                return embed;
            }
        } else if url.starts_with("http://") || url.starts_with("https://") {
            // Markdown embed
            resolver.resolve_markdown(&url, self.embed_depth + 1).await
        } else {
            return embed;
        };

        // If we got content, attach it
        if let Ok(content) = content_result {
            let is_at = url.starts_with("at://");
            self.build_embed_with_content(
                *embed_type,
                url,
                title.clone(),
                id.clone(),
                content,
                is_at,
            )
        } else {
            embed
        }
    }

    fn handle_reference(&self, reference: MdCowStr<'_>) -> MdCowStr<'_> {
        reference.into_static()
    }

    fn add_reference(&self, _reference: MdCowStr<'_>) {
        // No-op for client context
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jacquard::types::string::{Datetime, Did};
    use weaver_api::sh_weaver::notebook::entry::Entry;

    #[test]
    fn test_client_context_creation() {
        let entry = Entry::new()
            .title("Test")
            .path(weaver_common::normalize_title_path("Test"))
            .content("# Test")
            .created_at(Datetime::now())
            .build();

        let ctx = ClientContext::<()>::new(entry, Did::new("did:plc:test").unwrap());
        assert_eq!(ctx.title.as_ref(), "Test");
    }

    #[test]
    fn test_at_uri_to_web_url_profile() {
        let uri = AtUri::new("at://did:plc:xyz123").unwrap();
        assert_eq!(
            at_uri_to_web_url(&uri),
            "https://alpha.weaver.sh/did:plc:xyz123"
        );
    }

    #[test]
    fn test_at_uri_to_web_url_bsky_post() {
        let uri = AtUri::new("at://did:plc:xyz123/app.bsky.feed.post/3k7qrw5h2").unwrap();
        assert_eq!(
            at_uri_to_web_url(&uri),
            "https://bsky.app/profile/did:plc:xyz123/post/3k7qrw5h2"
        );
    }

    #[test]
    fn test_at_uri_to_web_url_bsky_list() {
        let uri = AtUri::new("at://alice.bsky.social/app.bsky.graph.list/abc123").unwrap();
        assert_eq!(
            at_uri_to_web_url(&uri),
            "https://bsky.app/profile/alice.bsky.social/lists/abc123"
        );
    }

    #[test]
    fn test_at_uri_to_web_url_bsky_feed() {
        let uri = AtUri::new("at://alice.bsky.social/app.bsky.feed.generator/my-feed").unwrap();
        assert_eq!(
            at_uri_to_web_url(&uri),
            "https://bsky.app/profile/alice.bsky.social/feed/my-feed"
        );
    }

    #[test]
    fn test_at_uri_to_web_url_bsky_starterpack() {
        let uri = AtUri::new("at://alice.bsky.social/app.bsky.graph.starterpack/pack123").unwrap();
        assert_eq!(
            at_uri_to_web_url(&uri),
            "https://bsky.app/starter-pack/alice.bsky.social/pack123"
        );
    }

    #[test]
    fn test_at_uri_to_web_url_weaver_entry() {
        let uri = AtUri::new("at://did:plc:xyz123/sh.weaver.notebook.entry/entry123").unwrap();
        assert_eq!(
            at_uri_to_web_url(&uri),
            "https://alpha.weaver.sh/did:plc:xyz123/e/entry123"
        );
    }

    #[test]
    fn test_at_uri_to_web_url_unknown_collection() {
        let uri = AtUri::new("at://did:plc:xyz123/com.example.unknown/rkey").unwrap();
        assert_eq!(
            at_uri_to_web_url(&uri),
            "https://alpha.weaver.sh/record/at://did:plc:xyz123/com.example.unknown/rkey"
        );
    }
}
