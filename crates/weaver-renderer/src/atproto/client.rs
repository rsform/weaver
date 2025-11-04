use super::{error::ClientRenderError, types::BlobName};
use crate::{Frontmatter, NotebookContext};
use jacquard::{
    client::{Agent, AgentSession},
    prelude::IdentityResolver,
    types::string::{AtUri, Cid, Did},
};
use markdown_weaver::{CowStr as MdCowStr, Tag, WeaverAttributes};
use std::collections::HashMap;
use std::sync::Arc;
use weaver_api::sh_weaver::notebook::entry::Entry;

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
        use jacquard::types::ident::AtIdentifier;

        // Extract DID from authority
        let did = match uri.authority() {
            AtIdentifier::Did(did) => did,
            AtIdentifier::Handle(_) => {
                return Err(ClientRenderError::EntryFetch {
                    uri: uri.as_ref().to_string(),
                    source: "Profile URI should use DID not handle".into(),
                });
            }
        };

        fetch_and_render_profile(&did, &*self.agent)
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
    async fn resolve_profile(&self, uri: &AtUri<'_>) -> Result<String, ClientRenderError> {
        Ok("".to_string())
    }

    async fn resolve_post(&self, uri: &AtUri<'_>) -> Result<String, ClientRenderError> {
        Ok("".to_string())
    }

    async fn resolve_markdown(&self, url: &str, depth: usize) -> Result<String, ClientRenderError> {
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
            frontmatter: Frontmatter::default(),
            title,
        }
    }

    /// Add an embed resolver for fetching embed content
    pub fn with_embed_resolver(self, resolver: Arc<R>) -> ClientContext<'a, R> {
        ClientContext {
            entry: self.entry,
            creator_did: self.creator_did,
            blob_map: self.blob_map,
            embed_resolver: Some(resolver),
            embed_depth: self.embed_depth,
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
            frontmatter: self.frontmatter.clone(),
            title: self.title.clone(),
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
/// - Weaver/other: `at://{actor}/{collection}/{rkey}` → `https://weaver.sh/{actor}/{collection}/{rkey}`
fn at_uri_to_web_url(at_uri: &AtUri<'_>) -> String {
    let authority = at_uri.authority().as_ref();

    // Profile-only link (no collection/rkey)
    if at_uri.collection().is_none() && at_uri.rkey().is_none() {
        return format!("https://weaver.sh/{}", authority);
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
            // Weaver records and unknown collections go to weaver.sh
            _ => {
                format!(
                    "https://weaver.sh/{}/{}/{}",
                    authority, collection_str, rkey_str
                )
            }
        }
    } else {
        // Fallback for malformed URIs
        format!("https://weaver.sh/{}", authority)
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
        match &embed {
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                // If content already in attrs (from preprocessor), pass through
                if let Some(attrs) = attrs {
                    if attrs.attrs.iter().any(|(k, _)| k.as_ref() == "content") {
                        return embed;
                    }
                }

                // Check if we have a resolver
                let Some(resolver) = &self.embed_resolver else {
                    return embed;
                };

                // Check recursion depth
                if self.embed_depth >= MAX_EMBED_DEPTH {
                    return embed;
                }

                // Try to fetch content based on URL type
                let content_result = if dest_url.starts_with("at://") {
                    // AT Protocol embed
                    if let Ok(at_uri) = AtUri::new(dest_url.as_ref()) {
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
                } else if dest_url.starts_with("http://") || dest_url.starts_with("https://") {
                    // Markdown embed (could be other types, but assume markdown for now)
                    resolver
                        .resolve_markdown(dest_url.as_ref(), self.embed_depth + 1)
                        .await
                } else {
                    // Local path or other - skip for now
                    return embed;
                };

                // If we got content, attach it to attrs
                if let Ok(content) = content_result {
                    let mut new_attrs = attrs.clone().unwrap_or_else(|| WeaverAttributes {
                        classes: vec![],
                        attrs: vec![],
                    });

                    new_attrs.attrs.push(("content".into(), content.into()));

                    // Add metadata for client-side enhancement
                    if dest_url.starts_with("at://") {
                        new_attrs
                            .attrs
                            .push(("data-embed-uri".into(), dest_url.clone()));

                        if let Ok(at_uri) = AtUri::new(dest_url.as_ref()) {
                            if at_uri.collection().is_none() {
                                new_attrs
                                    .attrs
                                    .push(("data-embed-type".into(), "profile".into()));
                            } else {
                                new_attrs
                                    .attrs
                                    .push(("data-embed-type".into(), "post".into()));
                            }
                        }
                    } else {
                        new_attrs
                            .attrs
                            .push(("data-embed-type".into(), "markdown".into()));
                    }

                    Tag::Embed {
                        embed_type: *embed_type,
                        dest_url: dest_url.clone(),
                        title: title.clone(),
                        id: id.clone(),
                        attrs: Some(new_attrs),
                    }
                } else {
                    // Fetch failed, return original
                    embed
                }
            }
            _ => embed,
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
            .content("# Test")
            .created_at(Datetime::now())
            .build();

        let ctx = ClientContext::<()>::new(entry, Did::new("did:plc:test").unwrap());
        assert_eq!(ctx.title.as_ref(), "Test");
    }

    #[test]
    fn test_at_uri_to_web_url_profile() {
        let uri = AtUri::new("at://did:plc:xyz123").unwrap();
        assert_eq!(at_uri_to_web_url(&uri), "https://weaver.sh/did:plc:xyz123");
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
            "https://weaver.sh/did:plc:xyz123/sh.weaver.notebook.entry/entry123"
        );
    }

    #[test]
    fn test_at_uri_to_web_url_unknown_collection() {
        let uri = AtUri::new("at://did:plc:xyz123/com.example.unknown/rkey").unwrap();
        assert_eq!(
            at_uri_to_web_url(&uri),
            "https://weaver.sh/did:plc:xyz123/com.example.unknown/rkey"
        );
    }
}
