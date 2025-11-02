use crate::{Frontmatter, NotebookContext};
use super::types::{BlobName, BlobInfo};
use dashmap::DashMap;
use jacquard::{
    client::{Agent, AgentSession, AgentSessionExt},
    prelude::IdentityResolver,
    types::string::{CowStr, Did, Handle},
};
use markdown_weaver::{Tag, CowStr as MdCowStr, WeaverAttributes};
use std::{
    path::PathBuf,
    sync::Arc,
};

pub struct AtProtoPreprocessContext<A: AgentSession + IdentityResolver> {
    // Vault information
    pub(crate) vault_contents: Arc<[PathBuf]>,
    pub(crate) current_path: PathBuf,

    // AT Protocol agent
    agent: Arc<Agent<A>>,

    // Notebook metadata
    pub(crate) notebook_title: CowStr<'static>,
    pub(crate) creator_did: Option<Did<'static>>,
    pub(crate) creator_handle: Option<Handle<'static>>,

    // Blob tracking
    blob_tracking: Arc<DashMap<BlobName<'static>, BlobInfo>>,

    // Shared with static site
    frontmatter: Arc<DashMap<PathBuf, Frontmatter>>,
    titles: Arc<DashMap<PathBuf, MdCowStr<'static>>>,
    reference_map: Arc<DashMap<MdCowStr<'static>, PathBuf>>,

    // Recursion tracking for markdown embeds
    embed_depth: usize,
}

impl<A: AgentSession + IdentityResolver> Clone for AtProtoPreprocessContext<A> {
    fn clone(&self) -> Self {
        Self {
            vault_contents: self.vault_contents.clone(),
            current_path: self.current_path.clone(),
            agent: self.agent.clone(),
            notebook_title: self.notebook_title.clone(),
            creator_did: self.creator_did.clone(),
            creator_handle: self.creator_handle.clone(),
            blob_tracking: self.blob_tracking.clone(),
            frontmatter: self.frontmatter.clone(),
            titles: self.titles.clone(),
            reference_map: self.reference_map.clone(),
            embed_depth: self.embed_depth,
        }
    }
}

impl<A: AgentSession + IdentityResolver> AtProtoPreprocessContext<A> {
    pub fn new(
        vault_contents: Arc<[PathBuf]>,
        notebook_title: impl Into<CowStr<'static>>,
        agent: Arc<Agent<A>>,
    ) -> Self {
        Self {
            vault_contents,
            current_path: PathBuf::new(),
            agent,
            notebook_title: notebook_title.into(),
            creator_did: None,
            creator_handle: None,
            blob_tracking: Arc::new(DashMap::new()),
            frontmatter: Arc::new(DashMap::new()),
            titles: Arc::new(DashMap::new()),
            reference_map: Arc::new(DashMap::new()),
            embed_depth: 0,
        }
    }

    pub fn with_creator(mut self, did: Did<'static>, handle: Handle<'static>) -> Self {
        self.creator_did = Some(did);
        self.creator_handle = Some(handle);
        self
    }

    pub fn blobs(&self) -> Vec<BlobInfo> {
        self.blob_tracking
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn set_current_path(&mut self, path: PathBuf) {
        self.current_path = path;
    }

    fn with_depth(&self, depth: usize) -> Self {
        Self {
            vault_contents: self.vault_contents.clone(),
            current_path: self.current_path.clone(),
            agent: self.agent.clone(),
            notebook_title: self.notebook_title.clone(),
            creator_did: self.creator_did.clone(),
            creator_handle: self.creator_handle.clone(),
            blob_tracking: self.blob_tracking.clone(),
            frontmatter: self.frontmatter.clone(),
            titles: self.titles.clone(),
            reference_map: self.reference_map.clone(),
            embed_depth: depth,
        }
    }
}

// Stub NotebookContext implementation
impl<A: AgentSession + IdentityResolver> NotebookContext for AtProtoPreprocessContext<A> {
    fn set_entry_title(&self, title: MdCowStr<'_>) {
        self.titles.insert(self.current_path.clone(), title.into_static());
    }

    fn entry_title(&self) -> MdCowStr<'_> {
        self.titles
            .get(&self.current_path)
            .map(|t| t.value().clone())
            .unwrap_or(MdCowStr::Borrowed(""))
    }

    fn frontmatter(&self) -> Frontmatter {
        self.frontmatter
            .get(&self.current_path)
            .map(|f| f.value().clone())
            .unwrap_or_default()
    }

    fn set_frontmatter(&self, frontmatter: Frontmatter) {
        self.frontmatter.insert(self.current_path.clone(), frontmatter);
    }

    async fn handle_link<'s>(&self, link: Tag<'s>) -> Tag<'s> {
        use crate::utils::lookup_filename_in_vault;
        use weaver_common::LinkUri;

        match &link {
            Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            } => {
                // Resolve link using LinkUri helper
                let resolved = LinkUri::resolve(dest_url.as_ref(), &*self.agent).await;

                match resolved {
                    LinkUri::Path(path) => {
                        // Local wikilink - look up in vault
                        if let Some(file_path) = lookup_filename_in_vault(path.as_ref(), &self.vault_contents) {
                            let entry_title = file_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("untitled");
                            let normalized_title = normalize_title(entry_title);

                            let canonical_url = if let Some(handle) = &self.creator_handle {
                                format!(
                                    "/{}/{}/{}",
                                    handle.as_ref(),
                                    self.notebook_title.as_ref(),
                                    normalized_title
                                )
                            } else {
                                format!(
                                    "/{}/{}",
                                    self.notebook_title.as_ref(),
                                    normalized_title
                                )
                            };

                            return Tag::Link {
                                link_type: *link_type,
                                dest_url: MdCowStr::Boxed(canonical_url.into_boxed_str()),
                                title: title.clone(),
                                id: id.clone(),
                            };
                        }
                    }
                    LinkUri::AtIdent(did, _handle) => {
                        // Profile link - use at://did format
                        let at_uri = format!("at://{}", did.as_ref());
                        return Tag::Link {
                            link_type: *link_type,
                            dest_url: MdCowStr::Boxed(at_uri.into_boxed_str()),
                            title: title.clone(),
                            id: id.clone(),
                        };
                    }
                    LinkUri::AtRecord(uri) => {
                        // AT URI - keep as-is or convert to HTTP
                        // For now, keep the at:// URI
                        return Tag::Link {
                            link_type: *link_type,
                            dest_url: MdCowStr::Boxed(uri.as_str().into()),
                            title: title.clone(),
                            id: id.clone(),
                        };
                    }
                    _ => {}
                }

                // Pass through other link types (web URLs, headings, etc.)
                link.clone()
            }
            _ => link,
        }
    }

    async fn handle_image<'s>(&self, image: Tag<'s>) -> Tag<'s> {
        use crate::utils::is_local_path;
        use tokio::fs;
        use jacquard::bytes::Bytes;
        use jacquard::types::blob::MimeType;
        use mime_sniffer::MimeTypeSniffer;

        match &image {
            Tag::Image {
                link_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                if is_local_path(dest_url) {
                    // Read local file
                    let file_path = if dest_url.starts_with('/') {
                        PathBuf::from(dest_url.as_ref())
                    } else {
                        self.current_path
                            .parent()
                            .unwrap_or(&self.current_path)
                            .join(dest_url.as_ref())
                    };

                    if let Ok(image_data) = fs::read(&file_path).await {
                        // Derive blob name from filename
                        let filename = file_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("image");
                        let blob_name = BlobName::from_filename(filename);

                        // Sniff mime type from data
                        let bytes = Bytes::from(image_data.clone());
                        let mime = MimeType::new_owned(
                            bytes.sniff_mime_type().unwrap_or("application/octet-stream")
                        );

                        // Upload blob (dereference Arc)
                        if let Ok(blob) = (*self.agent).upload_blob(bytes, mime.clone()).await {
                            use jacquard::IntoStatic;

                            // Store blob info
                            let blob_info = BlobInfo {
                                name: blob_name.clone(),
                                blob: blob.into_static(),
                                alt: if title.is_empty() {
                                    None
                                } else {
                                    Some(CowStr::Owned(title.as_ref().into()))
                                },
                            };
                            self.blob_tracking.insert(blob_name.clone(), blob_info);

                            // Rewrite to canonical path
                            let canonical_url = format!(
                                "/{}/image/{}",
                                self.notebook_title.as_ref(),
                                blob_name.as_str()
                            );

                            return Tag::Image {
                                link_type: *link_type,
                                dest_url: MdCowStr::Boxed(canonical_url.into_boxed_str()),
                                title: title.clone(),
                                id: id.clone(),
                                attrs: attrs.clone(),
                            };
                        }
                    }
                }
                // If not local or upload failed, pass through
                image
            }
            _ => image,
        }
    }

    async fn handle_embed<'s>(&self, embed: Tag<'s>) -> Tag<'s> {
        use crate::utils::lookup_filename_in_vault;
        use weaver_common::LinkUri;

        match &embed {
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                // Resolve embed using LinkUri helper
                let resolved = LinkUri::resolve(dest_url.as_ref(), &*self.agent).await;

                match resolved {
                    LinkUri::Path(path) => {
                        // Entry embed - look up in vault
                        if let Some(file_path) = lookup_filename_in_vault(path.as_ref(), &self.vault_contents) {
                            let entry_title = file_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("untitled");
                            let normalized_title = normalize_title(entry_title);

                            let canonical_url = if let Some(handle) = &self.creator_handle {
                                format!(
                                    "/{}/{}/{}",
                                    handle.as_ref(),
                                    self.notebook_title.as_ref(),
                                    normalized_title
                                )
                            } else {
                                format!(
                                    "/{}/{}",
                                    self.notebook_title.as_ref(),
                                    normalized_title
                                )
                            };

                            return Tag::Embed {
                                embed_type: *embed_type,
                                dest_url: MdCowStr::Boxed(canonical_url.into_boxed_str()),
                                title: title.clone(),
                                id: id.clone(),
                                attrs: attrs.clone(),
                            };
                        }
                    }
                    LinkUri::AtIdent(did, _handle) => {
                        // Profile embed - fetch and render
                        use crate::atproto::fetch_and_render_profile;
                        use markdown_weaver::WeaverAttributes;

                        let at_uri = format!("at://{}", did.as_ref());

                        // Fetch and render the profile
                        let content = match fetch_and_render_profile(&did, &*self.agent).await {
                            Ok(html) => Some(html),
                            Err(e) => {
                                eprintln!("Failed to fetch profile {}: {}", did.as_ref(), e);
                                None
                            }
                        };

                        // Build or update attributes
                        let mut new_attrs = attrs.clone().unwrap_or_else(|| WeaverAttributes {
                            classes: vec![],
                            attrs: vec![],
                        });

                        if let Some(content_html) = content {
                            new_attrs.attrs.push(("content".into(), content_html.into()));
                        }

                        return Tag::Embed {
                            embed_type: *embed_type,
                            dest_url: MdCowStr::Boxed(at_uri.into_boxed_str()),
                            title: title.clone(),
                            id: id.clone(),
                            attrs: Some(new_attrs),
                        };
                    }
                    LinkUri::AtRecord(uri) => {
                        // AT URI embed - fetch and render
                        use crate::atproto::{fetch_and_render_post, fetch_and_render_generic};
                        use markdown_weaver::WeaverAttributes;

                        // Determine if this is a known type
                        let content = if let Some(collection) = uri.collection() {
                            match collection.as_ref() {
                                "app.bsky.feed.post" => {
                                    // Bluesky post
                                    match fetch_and_render_post(&uri, &*self.agent).await {
                                        Ok(html) => Some(html),
                                        Err(e) => {
                                            eprintln!("Failed to fetch post {}: {}", uri.as_ref(), e);
                                            None
                                        }
                                    }
                                }
                                _ => {
                                    // Generic record
                                    match fetch_and_render_generic(&uri, &*self.agent).await {
                                        Ok(html) => Some(html),
                                        Err(e) => {
                                            eprintln!("Failed to fetch record {}: {}", uri.as_ref(), e);
                                            None
                                        }
                                    }
                                }
                            }
                        } else {
                            None
                        };

                        // Build or update attributes
                        let mut new_attrs = attrs.clone().unwrap_or_else(|| WeaverAttributes {
                            classes: vec![],
                            attrs: vec![],
                        });

                        if let Some(content_html) = content {
                            new_attrs.attrs.push(("content".into(), content_html.into()));
                        }

                        return Tag::Embed {
                            embed_type: *embed_type,
                            dest_url: MdCowStr::Boxed(uri.as_str().into()),
                            title: title.clone(),
                            id: id.clone(),
                            attrs: Some(new_attrs),
                        };
                    }
                    LinkUri::Path(path) => {
                        // Markdown embed - look up in vault and render
                        use crate::utils::lookup_filename_in_vault;
                        use tokio::fs;

                        // Check depth limit
                        const MAX_DEPTH: usize = 1;
                        if self.embed_depth >= MAX_DEPTH {
                            eprintln!("Max embed depth reached for {}", path.as_ref());
                            return embed.clone();
                        }

                        if let Some(file_path) = lookup_filename_in_vault(path.as_ref(), &self.vault_contents) {
                            // Read the markdown file
                            match fs::read_to_string(&file_path).await {
                                Ok(markdown_content) => {
                                    // Create a child context with incremented depth
                                    let mut child_ctx = self.with_depth(self.embed_depth + 1);
                                    child_ctx.current_path = file_path.clone();

                                    // Render the markdown through the processor
                                    // We'll use markdown_weaver to parse and render to HTML
                                    use markdown_weaver::{Parser, Options};
                                    use markdown_weaver_escape::StrWrite;

                                    let parser = Parser::new_ext(&markdown_content, Options::all());
                                    let mut html_output = String::new();

                                    // Process events through context callbacks
                                    for event in parser {
                                        match event {
                                            markdown_weaver::Event::Start(tag) => {
                                                let processed = match tag {
                                                    Tag::Link { .. } => child_ctx.handle_link(tag).await,
                                                    Tag::Image { .. } => child_ctx.handle_image(tag).await,
                                                    Tag::Embed { .. } => child_ctx.handle_embed(tag).await,
                                                    _ => tag,
                                                };
                                                // Simple HTML writing (reuse escape logic)
                                                match processed {
                                                    Tag::Paragraph => html_output.write_str("<p>").ok(),
                                                    _ => None,
                                                };
                                            }
                                            markdown_weaver::Event::End(tag_end) => {
                                                match tag_end {
                                                    markdown_weaver::TagEnd::Paragraph => html_output.write_str("</p>\n").ok(),
                                                    _ => None,
                                                };
                                            }
                                            markdown_weaver::Event::Text(text) => {
                                                use markdown_weaver_escape::escape_html_body_text;
                                                escape_html_body_text(&mut html_output, &text).ok();
                                            }
                                            _ => {}
                                        }
                                    }

                                    let mut new_attrs = attrs.clone().unwrap_or_else(|| WeaverAttributes {
                                        classes: vec![],
                                        attrs: vec![],
                                    });

                                    new_attrs.attrs.push(("content".into(), html_output.into()));

                                    return Tag::Embed {
                                        embed_type: *embed_type,
                                        dest_url: dest_url.clone(),
                                        title: title.clone(),
                                        id: id.clone(),
                                        attrs: Some(new_attrs),
                                    };
                                }
                                Err(e) => {
                                    eprintln!("Failed to read file {:?}: {}", file_path, e);
                                }
                            }
                        }
                    }
                    _ => {}
                }

                // Pass through other embed types
                embed.clone()
            }
            Tag::Image {
                link_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                // Some embeds come through as explicit Tag::Image
                // Delegate to handle_image for image-specific processing
                self.handle_image(embed).await
            }
            _ => embed,
        }
    }

    fn handle_reference(&self, reference: MdCowStr<'_>) -> MdCowStr<'_> {
        reference.into_static()
    }

    fn add_reference(&self, reference: MdCowStr<'_>) {
        self.reference_map.insert(reference.into_static(), self.current_path.clone());
    }
}

/// Normalize entry title to URL-safe format
fn normalize_title(title: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_space = false;

    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            normalized.push(c);
            last_was_space = false;
        } else if c.is_whitespace() && !last_was_space && !normalized.is_empty() {
            normalized.push('_');
            last_was_space = true;
        }
    }

    // Remove trailing underscore if present
    if normalized.ends_with('_') {
        normalized.pop();
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests require an actual Agent instance, which needs authentication setup.
    // These will be tested via integration tests instead.
}
