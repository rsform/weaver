//! Fetch and render AT Protocol records as HTML embeds
//!
//! This module provides functions to fetch records from PDSs and render them
//! as HTML strings suitable for embedding in markdown content.
//!
//! # Reusable render functions
//!
//! The `render_*` functions can be used standalone for rendering different embed types:
//! - `render_external_link` - Link cards with title, description, thumbnail
//! - `render_images` - Image galleries
//! - `render_quoted_record` - Quoted posts/records
//! - `render_author_block` - Author avatar + name + handle

use super::error::AtProtoPreprocessError;
use jacquard::{
    Data, IntoStatic,
    client::AgentSessionExt,
    types::{ident::AtIdentifier, string::AtUri},
};
use weaver_api::app_bsky::{
    actor::ProfileViewBasic,
    embed::{
        external::ViewExternal,
        images::ViewImage,
        record::{ViewRecord, ViewUnionRecord},
    },
    feed::{PostView, PostViewEmbed, get_posts::GetPosts},
};
use weaver_api::sh_weaver::actor::ProfileDataViewInner;
use weaver_common::agent::WeaverExt;

/// Fetch and render a profile record as HTML
///
/// Resolves handle to DID if needed, then fetches profile data from
/// weaver or bsky appview, returning a rich profile view.
pub async fn fetch_and_render_profile<A>(
    ident: &AtIdentifier<'_>,
    agent: &A,
) -> Result<String, AtProtoPreprocessError>
where
    A: AgentSessionExt,
{
    use jacquard::types::string::Did;

    // Resolve to DID if we have a handle
    let did = match ident {
        AtIdentifier::Did(d) => d.clone(),
        AtIdentifier::Handle(h) => {
            let did_str = agent.resolve_handle(h).await.map_err(|e| {
                AtProtoPreprocessError::FetchFailed(format!("resolving handle {:?}", e))
            })?;
            Did::new(&did_str)
                .map_err(|e| AtProtoPreprocessError::InvalidUri(format!("{:?}", e)))?
                .into_static()
        }
    };

    // Use WeaverExt to get hydrated profile (tries weaver profile first, falls back to bsky)
    let (_uri, profile_view) = agent
        .hydrate_profile_view(&did)
        .await
        .map_err(|e| AtProtoPreprocessError::FetchFailed(format!("{:?}", e)))?;

    // Render based on which profile type we got
    render_profile_data_view(&profile_view.inner)
}

/// Fetch and render a Bluesky post as HTML using the appview for rich data
pub async fn fetch_and_render_post<A>(
    uri: &AtUri<'_>,
    agent: &A,
) -> Result<String, AtProtoPreprocessError>
where
    A: AgentSessionExt,
{
    // Use GetPosts for richer data (author info, engagement counts)
    let request = GetPosts::new().uris(vec![uri.clone()]).build();
    let response = agent.send(request).await;
    let response = response.map_err(|e| {
        AtProtoPreprocessError::FetchFailed(format!("getting post from appview {:?}", e))
    })?;

    let output = response
        .into_output()
        .map_err(|e| AtProtoPreprocessError::FetchFailed(format!("{:?}", e)))?;

    let post_view = output
        .posts
        .into_iter()
        .next()
        .ok_or_else(|| AtProtoPreprocessError::FetchFailed("Post not found".to_string()))?;

    render_post_view(&post_view, uri)
}

/// Fetch and render an unknown record type generically
///
/// This fetches the record as untyped Data and probes for likely meaningful fields.
pub async fn fetch_and_render_generic<A>(
    uri: &AtUri<'_>,
    agent: &A,
) -> Result<String, AtProtoPreprocessError>
where
    A: AgentSessionExt,
{
    // Fetch via slingshot (edge-cached, untyped)
    let output = agent
        .fetch_record_slingshot(uri)
        .await
        .map_err(|e| AtProtoPreprocessError::FetchFailed(format!("{:?}", e)))?;

    // Probe for meaningful fields
    render_generic_record(&output.value, uri)
}

/// Fetch and render a notebook entry with full markdown rendering
///
/// Renders the entry content as HTML in a scrollable container with title and author info.
pub async fn fetch_and_render_entry<A>(
    uri: &AtUri<'_>,
    agent: &A,
) -> Result<String, AtProtoPreprocessError>
where
    A: AgentSessionExt,
{
    use crate::atproto::writer::ClientWriter;
    use crate::default_md_options;
    use markdown_weaver::Parser;
    use weaver_common::agent::WeaverExt;

    // Get rkey from URI
    let rkey = uri
        .rkey()
        .ok_or_else(|| AtProtoPreprocessError::FetchFailed("Entry URI missing rkey".to_string()))?;

    // Fetch entry with author info
    let (entry_view, entry) = agent
        .fetch_entry_by_rkey(&uri.authority(), rkey.as_ref())
        .await
        .map_err(|e| AtProtoPreprocessError::FetchFailed(e.to_string()))?;

    // Render the markdown content to HTML
    let parser = Parser::new_ext(entry.content.as_ref(), default_md_options());
    let mut content_html = String::new();
    ClientWriter::<_, _, ()>::new(parser, &mut content_html)
        .run()
        .map_err(|e| {
            AtProtoPreprocessError::FetchFailed(format!("Markdown render failed: {:?}", e))
        })?;

    // Generate unique ID for the toggle checkbox
    let toggle_id = format!("entry-toggle-{}", rkey.as_ref());

    // Build the embed HTML
    let mut html = String::new();
    html.push_str("<div class=\"atproto-embed atproto-entry\" contenteditable=\"false\">");

    // Hidden checkbox for expand/collapse (must come before content for CSS sibling selector)
    html.push_str("<input type=\"checkbox\" class=\"embed-entry-toggle\" id=\"");
    html.push_str(&toggle_id);
    html.push_str("\">");

    // Header with title and author
    html.push_str("<div class=\"embed-entry-header\">");

    // Title
    html.push_str("<span class=\"embed-entry-title\">");
    html.push_str(&html_escape(entry.title.as_ref()));
    html.push_str("</span>");

    // Author info - just show handle (keep it simple for entry embeds)
    if let Some(author) = entry_view.authors.first() {
        let handle = match &author.record.inner {
            ProfileDataViewInner::ProfileView(p) => p.handle.as_ref(),
            ProfileDataViewInner::ProfileViewDetailed(p) => p.handle.as_ref(),
            ProfileDataViewInner::TangledProfileView(p) => p.handle.as_ref(),
            ProfileDataViewInner::Unknown(_) => "",
        };
        if !handle.is_empty() {
            html.push_str("<span class=\"embed-entry-author\">@");
            html.push_str(&html_escape(handle));
            html.push_str("</span>");
        }
    }

    html.push_str("</div>"); // end header

    // Scrollable content container
    html.push_str("<div class=\"embed-entry-content\">");
    html.push_str(&content_html);
    html.push_str("</div>");

    // Expand/collapse label (clickable, targets the checkbox)
    html.push_str("<label class=\"embed-entry-expand\" for=\"");
    html.push_str(&toggle_id);
    html.push_str("\"></label>");

    html.push_str("</div>");

    Ok(html)
}

/// Fetch and render any AT URI, dispatching to the appropriate renderer based on collection.
///
/// Uses typed fetchers for known collections (posts, profiles) and falls back to
/// generic rendering for unknown types.
pub async fn fetch_and_render<A>(
    uri: &AtUri<'_>,
    agent: &A,
) -> Result<String, AtProtoPreprocessError>
where
    A: AgentSessionExt,
{
    let collection = uri.collection().map(|c| c.as_ref());

    match collection {
        Some("app.bsky.feed.post") => {
            let result = fetch_and_render_post(uri, agent).await;
            result
        }
        Some("app.bsky.actor.profile") => {
            // Extract DID from URI authority
            fetch_and_render_profile(uri.authority(), agent).await
        }
        Some("sh.weaver.notebook.entry") => fetch_and_render_entry(uri, agent).await,
        None => fetch_and_render_profile(uri.authority(), agent).await,
        _ => fetch_and_render_generic(uri, agent).await,
    }
}

/// Render a profile from ProfileDataViewInner (weaver, bsky, or tangled)
fn render_profile_data_view(
    inner: &ProfileDataViewInner<'_>,
) -> Result<String, AtProtoPreprocessError> {
    let mut html = String::new();

    match inner {
        ProfileDataViewInner::ProfileView(profile) => {
            // Weaver profile - link to bsky for now
            let profile_url = format!("https://bsky.app/profile/{}", profile.handle.as_ref());
            html.push_str(
                "<span class=\"atproto-embed atproto-profile\" contenteditable=\"false\">",
            );

            // Background link covers whole card
            html.push_str("<a class=\"embed-card-link\" href=\"");
            html.push_str(&html_escape(&profile_url));
            html.push_str("\" target=\"_blank\" rel=\"noopener\" aria-label=\"View profile\"></a>");

            html.push_str("<span class=\"embed-author\">");
            if let Some(avatar) = &profile.avatar {
                html.push_str("<img class=\"embed-avatar\" src=\"");
                html.push_str(&html_escape(avatar.as_ref()));
                html.push_str("\" alt=\"\" width=\"42\" height=\"42\" />");
            }
            html.push_str("<span class=\"embed-author-info\">");
            if let Some(display_name) = &profile.display_name {
                html.push_str("<span class=\"embed-author-name\">");
                html.push_str(&html_escape(display_name.as_ref()));
                html.push_str("</span>");
            }
            html.push_str("<span class=\"embed-author-handle\">@");
            html.push_str(&html_escape(profile.handle.as_ref()));
            html.push_str("</span>");
            html.push_str("</span>");
            html.push_str("</span>");

            if let Some(description) = &profile.description {
                html.push_str("<span class=\"embed-description\">");
                html.push_str(&html_escape(description.as_ref()));
                html.push_str("</span>");
            }

            html.push_str("</span>");
        }
        ProfileDataViewInner::ProfileViewDetailed(profile) => {
            // Bsky profile
            let profile_url = format!("https://bsky.app/profile/{}", profile.handle.as_ref());
            html.push_str(
                "<span class=\"atproto-embed atproto-profile\" contenteditable=\"false\">",
            );

            // Background link covers whole card
            html.push_str("<a class=\"embed-card-link\" href=\"");
            html.push_str(&html_escape(&profile_url));
            html.push_str("\" target=\"_blank\" rel=\"noopener\" aria-label=\"View profile\"></a>");

            html.push_str("<span class=\"embed-author\">");
            if let Some(avatar) = &profile.avatar {
                html.push_str("<img class=\"embed-avatar\" src=\"");
                html.push_str(&html_escape(avatar.as_ref()));
                html.push_str("\" alt=\"\" width=\"42\" height=\"42\" />");
            }
            html.push_str("<span class=\"embed-author-info\">");
            if let Some(display_name) = &profile.display_name {
                html.push_str("<span class=\"embed-author-name\">");
                html.push_str(&html_escape(display_name.as_ref()));
                html.push_str("</span>");
            }
            html.push_str("<span class=\"embed-author-handle\">@");
            html.push_str(&html_escape(profile.handle.as_ref()));
            html.push_str("</span>");
            html.push_str("</span>");
            html.push_str("</span>");

            if let Some(description) = &profile.description {
                html.push_str("<span class=\"embed-description\">");
                html.push_str(&html_escape(description.as_ref()));
                html.push_str("</span>");
            }

            // Stats for bsky profiles
            if profile.followers_count.is_some() || profile.follows_count.is_some() {
                html.push_str("<span class=\"embed-meta\">");
                html.push_str("<span class=\"embed-stats\">");
                if let Some(followers) = profile.followers_count {
                    html.push_str("<span class=\"embed-stat\">");
                    html.push_str(&followers.to_string());
                    html.push_str(" followers</span>");
                }
                if let Some(follows) = profile.follows_count {
                    html.push_str("<span class=\"embed-stat\">");
                    html.push_str(&follows.to_string());
                    html.push_str(" following</span>");
                }
                html.push_str("</span>");
                html.push_str("</span>");
            }

            html.push_str("</span>");
        }
        ProfileDataViewInner::TangledProfileView(profile) => {
            // Tangled profile - link to tangled
            let profile_url = format!("https://tangled.sh/@{}", profile.handle.as_ref());
            html.push_str(
                "<span class=\"atproto-embed atproto-profile\" contenteditable=\"false\">",
            );

            // Background link covers whole card
            html.push_str("<a class=\"embed-card-link\" href=\"");
            html.push_str(&html_escape(&profile_url));
            html.push_str("\" target=\"_blank\" rel=\"noopener\" aria-label=\"View profile\"></a>");

            html.push_str("<span class=\"embed-author\">");
            html.push_str("<span class=\"embed-author-info\">");
            html.push_str("<span class=\"embed-author-handle\">@");
            html.push_str(&html_escape(profile.handle.as_ref()));
            html.push_str("</span>");
            html.push_str("</span>");
            html.push_str("</span>");

            if let Some(description) = &profile.description {
                html.push_str("<span class=\"embed-description\">");
                html.push_str(&html_escape(description.as_ref()));
                html.push_str("</span>");
            }

            html.push_str("</span>");
        }
        ProfileDataViewInner::Unknown(data) => {
            // Unknown - no link, just render
            html.push_str(
                "<span class=\"atproto-embed atproto-profile\" contenteditable=\"false\">",
            );
            html.push_str(&render_generic_data(data));
            html.push_str("</span>");
        }
    }

    Ok(html)
}

/// Render a Bluesky post from PostView (rich appview data)
fn render_post_view<'a>(
    post: &PostView<'a>,
    uri: &AtUri<'_>,
) -> Result<String, AtProtoPreprocessError> {
    let mut html = String::new();

    // Build link to post on Bluesky
    let bsky_link = format!(
        "https://bsky.app/profile/{}/post/{}",
        post.author.handle.as_ref(),
        uri.rkey().map(|r| r.as_ref()).unwrap_or("")
    );

    html.push_str("<span class=\"atproto-embed atproto-post\" contenteditable=\"false\">");

    // Background link covers whole card, other links sit on top
    html.push_str("<a class=\"embed-card-link\" href=\"");
    html.push_str(&html_escape(&bsky_link));
    html.push_str("\" target=\"_blank\" rel=\"noopener\" aria-label=\"View post on Bluesky\"></a>");

    // Author header
    html.push_str(&render_author_block(&post.author, true));

    // Post text (parse record as typed Post)
    if let Ok(post_record) =
        jacquard::from_data::<weaver_api::app_bsky::feed::post::Post>(&post.record)
    {
        html.push_str("<span class=\"embed-content\">");
        html.push_str(&html_escape(post_record.text.as_ref()));
        html.push_str("</span>");
    }

    // Embedded content (images, links, quotes, etc.)
    if let Some(embed) = &post.embed {
        html.push_str(&render_post_embed(embed));
    }

    // Engagement stats and timestamp
    html.push_str("<span class=\"embed-meta\">");

    // Stats row
    html.push_str("<span class=\"embed-stats\">");
    if let Some(replies) = post.reply_count {
        html.push_str("<span class=\"embed-stat\">");
        html.push_str(&replies.to_string());
        html.push_str(" replies</span>");
    }
    if let Some(reposts) = post.repost_count {
        html.push_str("<span class=\"embed-stat\">");
        html.push_str(&reposts.to_string());
        html.push_str(" reposts</span>");
    }
    if let Some(likes) = post.like_count {
        html.push_str("<span class=\"embed-stat\">");
        html.push_str(&likes.to_string());
        html.push_str(" likes</span>");
    }
    html.push_str("</span>");

    // Timestamp
    html.push_str("<span class=\"embed-time\">");
    html.push_str(&html_escape(&post.indexed_at.to_string()));
    html.push_str("</span>");

    html.push_str("</span>");
    html.push_str("</span>");

    Ok(html)
}

/// Render a generic record by probing Data for meaningful fields
fn render_generic_record(
    data: &Data<'_>,
    uri: &AtUri<'_>,
) -> Result<String, AtProtoPreprocessError> {
    let mut html = String::new();

    html.push_str("<span class=\"atproto-embed atproto-record\" contenteditable=\"false\">");

    // Show record type as header (full NSID)
    if let Some(collection) = uri.collection() {
        html.push_str("<span class=\"embed-author-handle\">");
        html.push_str(&html_escape(collection.as_ref()));
        html.push_str("</span>");
    }

    // Priority fields to show first (in order)
    let priority_fields = [
        "name",
        "displayName",
        "title",
        "text",
        "description",
        "content",
    ];
    let mut shown_fields = Vec::new();

    if let Some(obj) = data.as_object() {
        for field_name in priority_fields {
            if let Some(value) = obj.get(field_name) {
                if let Some(s) = value.as_str() {
                    let class = match field_name {
                        "name" | "displayName" | "title" => "embed-author-name",
                        "text" | "content" => "embed-content",
                        "description" => "embed-description",
                        _ => "embed-field-value",
                    };
                    html.push_str("<span class=\"");
                    html.push_str(class);
                    html.push_str("\">");
                    // Truncate long content for embed display
                    let display_text = if s.len() > 300 {
                        format!("{}...", &s[..300])
                    } else {
                        s.to_string()
                    };
                    html.push_str(&html_escape(&display_text));
                    html.push_str("</span>");
                    shown_fields.push(field_name);
                }
            }
        }

        // Show remaining fields as a simple list
        html.push_str("<span class=\"embed-fields\">");
        for (key, value) in obj.iter() {
            let key_str: &str = key.as_ref();

            // Skip already shown, internal fields, and complex nested objects
            if shown_fields.contains(&key_str)
                || key_str.starts_with('$')
                || key_str == "facets"
                || key_str == "labels"
                || key_str == "embeds"
            {
                continue;
            }

            if let Some(formatted) = format_field_value(key_str, value) {
                html.push_str("<span class=\"embed-field\">");
                html.push_str("<span class=\"embed-field-name\">");
                html.push_str(&html_escape(&format_field_name(key_str)));
                html.push_str(":</span> ");
                html.push_str(&formatted);
                html.push_str("</span>");
            }
        }
        html.push_str("</span>");
    }

    html.push_str("</span>");

    Ok(html)
}

// =============================================================================
// Reusable render functions for embed components
// =============================================================================

/// Render an author block (avatar + name + handle)
///
/// Used for posts, profiles, and any record with an author.
/// When `link_to_profile` is true, avatar, display name, and handle all link to the profile.
pub fn render_author_block(author: &ProfileViewBasic<'_>, link_to_profile: bool) -> String {
    render_author_block_inner(
        author.avatar.as_ref().map(|u| u.as_ref()),
        author.display_name.as_ref().map(|s| s.as_ref()),
        author.handle.as_ref(),
        link_to_profile,
    )
}

/// Render author block from ProfileView (has same fields as ProfileViewBasic)
pub fn render_author_block_full(
    author: &weaver_api::app_bsky::actor::ProfileView<'_>,
    link_to_profile: bool,
) -> String {
    render_author_block_inner(
        author.avatar.as_ref().map(|u| u.as_ref()),
        author.display_name.as_ref().map(|s| s.as_ref()),
        author.handle.as_ref(),
        link_to_profile,
    )
}

fn render_author_block_inner(
    avatar: Option<&str>,
    display_name: Option<&str>,
    handle: &str,
    link_to_profile: bool,
) -> String {
    let mut html = String::new();
    let profile_url = format!("https://bsky.app/profile/{}", handle);

    html.push_str("<span class=\"embed-author\">");

    if let Some(avatar_url) = avatar {
        if link_to_profile {
            html.push_str("<a class=\"embed-avatar-link\" href=\"");
            html.push_str(&html_escape(&profile_url));
            html.push_str("\" target=\"_blank\" rel=\"noopener\">");
            html.push_str("<img class=\"embed-avatar\" src=\"");
            html.push_str(&html_escape(avatar_url));
            html.push_str("\" alt=\"\" width=\"42\" height=\"42\" />");
            html.push_str("</a>");
        } else {
            html.push_str("<img class=\"embed-avatar\" src=\"");
            html.push_str(&html_escape(avatar_url));
            html.push_str("\" alt=\"\" width=\"42\" height=\"42\" />");
        }
    }

    html.push_str("<span class=\"embed-author-info\">");

    if let Some(name) = display_name {
        if link_to_profile {
            html.push_str("<a class=\"embed-author-name\" href=\"");
            html.push_str(&html_escape(&profile_url));
            html.push_str("\" target=\"_blank\" rel=\"noopener\">");
            html.push_str(&html_escape(name));
            html.push_str("</a>");
        } else {
            html.push_str("<span class=\"embed-author-name\">");
            html.push_str(&html_escape(name));
            html.push_str("</span>");
        }
    }

    if link_to_profile {
        html.push_str("<a class=\"embed-author-handle\" href=\"");
        html.push_str(&html_escape(&profile_url));
        html.push_str("\" target=\"_blank\" rel=\"noopener\">@");
        html.push_str(&html_escape(handle));
        html.push_str("</a>");
    } else {
        html.push_str("<span class=\"embed-author-handle\">@");
        html.push_str(&html_escape(handle));
        html.push_str("</span>");
    }

    html.push_str("</span>");
    html.push_str("</span>");

    html
}

/// Render an external link card (title, description, thumbnail)
///
/// Used for link previews in posts and standalone link embeds.
pub fn render_external_link(external: &ViewExternal<'_>) -> String {
    let mut html = String::new();

    html.push_str("<a class=\"embed-external\" href=\"");
    html.push_str(&html_escape(external.uri.as_ref()));
    html.push_str("\" target=\"_blank\" rel=\"noopener\">");

    if let Some(thumb) = &external.thumb {
        html.push_str("<img class=\"embed-external-thumb\" src=\"");
        html.push_str(&html_escape(thumb.as_ref()));
        html.push_str("\" alt=\"\" />");
    }

    html.push_str("<span class=\"embed-external-info\">");
    html.push_str("<span class=\"embed-external-title\">");
    html.push_str(&html_escape(external.title.as_ref()));
    html.push_str("</span>");

    if !external.description.is_empty() {
        html.push_str("<span class=\"embed-external-description\">");
        html.push_str(&html_escape(external.description.as_ref()));
        html.push_str("</span>");
    }

    html.push_str("<span class=\"embed-external-url\">");
    // Show just the domain
    if let Some(domain) = extract_domain(external.uri.as_ref()) {
        html.push_str(&html_escape(domain));
    } else {
        html.push_str(&html_escape(external.uri.as_ref()));
    }
    html.push_str("</span>");

    html.push_str("</span>");
    html.push_str("</a>");

    html
}

/// Render an image gallery
///
/// Used for image embeds in posts.
pub fn render_images(images: &[ViewImage<'_>]) -> String {
    let mut html = String::new();

    let class = match images.len() {
        1 => "embed-images embed-images-1",
        2 => "embed-images embed-images-2",
        3 => "embed-images embed-images-3",
        _ => "embed-images embed-images-4",
    };

    html.push_str("<span class=\"");
    html.push_str(class);
    html.push_str("\">");

    for img in images {
        html.push_str("<a class=\"embed-image-link\" href=\"");
        html.push_str(&html_escape(img.fullsize.as_ref()));
        html.push_str("\" target=\"_blank\"");

        // Add aspect-ratio style if available
        if let Some(aspect) = &img.aspect_ratio {
            html.push_str(" style=\"aspect-ratio: ");
            html.push_str(&aspect.width.to_string());
            html.push_str(" / ");
            html.push_str(&aspect.height.to_string());
            html.push_str(";\"");
        }

        html.push_str(">");
        html.push_str("<img class=\"embed-image\" src=\"");
        html.push_str(&html_escape(img.thumb.as_ref()));
        html.push_str("\" alt=\"");
        html.push_str(&html_escape(img.alt.as_ref()));
        html.push_str("\" />");
        html.push_str("</a>");
    }

    html.push_str("</span>");

    html
}

/// Render a quoted/embedded record
///
/// Used for quote posts and record embeds. Dispatches based on record type.
pub fn render_quoted_record(record: &ViewRecord<'_>) -> String {
    let mut html = String::new();

    html.push_str("<span class=\"embed-quote\">");

    // Dispatch based on record type
    match record.value.type_discriminator() {
        Some("app.bsky.feed.post") => {
            // Post - show author and text
            html.push_str(&render_author_block(&record.author, true));
            if let Ok(post) =
                jacquard::from_data::<weaver_api::app_bsky::feed::post::Post>(&record.value)
            {
                html.push_str("<span class=\"embed-content\">");
                html.push_str(&html_escape(post.text.as_ref()));
                html.push_str("</span>");
            }
        }
        Some("app.bsky.feed.generator") => {
            // Custom feed - show feed info with type label
            if let Ok(generator) = jacquard::from_data::<
                weaver_api::app_bsky::feed::generator::Generator,
            >(&record.value)
            {
                html.push_str("<span class=\"embed-type\">Custom Feed</span>");
                html.push_str("<span class=\"embed-author-name\">");
                html.push_str(&html_escape(generator.display_name.as_ref()));
                html.push_str("</span>");
                if let Some(desc) = &generator.description {
                    html.push_str("<span class=\"embed-description\">");
                    html.push_str(&html_escape(desc.as_ref()));
                    html.push_str("</span>");
                }
                html.push_str(&render_author_block(&record.author, true));
            }
        }
        Some("app.bsky.graph.list") => {
            // List - show list info
            if let Ok(list) =
                jacquard::from_data::<weaver_api::app_bsky::graph::list::List>(&record.value)
            {
                html.push_str("<span class=\"embed-type\">List</span>");
                html.push_str("<span class=\"embed-author-name\">");
                html.push_str(&html_escape(list.name.as_ref()));
                html.push_str("</span>");
                if let Some(desc) = &list.description {
                    html.push_str("<span class=\"embed-description\">");
                    html.push_str(&html_escape(desc.as_ref()));
                    html.push_str("</span>");
                }
                html.push_str(&render_author_block(&record.author, true));
            }
        }
        Some("app.bsky.graph.starterpack") => {
            // Starter pack
            if let Ok(sp) = jacquard::from_data::<
                weaver_api::app_bsky::graph::starterpack::Starterpack,
            >(&record.value)
            {
                html.push_str("<span class=\"embed-type\">Starter Pack</span>");
                html.push_str("<span class=\"embed-author-name\">");
                html.push_str(&html_escape(sp.name.as_ref()));
                html.push_str("</span>");
                if let Some(desc) = &sp.description {
                    html.push_str("<span class=\"embed-description\">");
                    html.push_str(&html_escape(desc.as_ref()));
                    html.push_str("</span>");
                }
                html.push_str(&render_author_block(&record.author, true));
            }
        }
        _ => {
            // Unknown type - show author and probe for common fields
            html.push_str(&render_author_block(&record.author, true));
            html.push_str(&render_generic_data(&record.value));
        }
    }

    // Render nested embeds if present (applies to all types)
    if let Some(embeds) = &record.embeds {
        for embed in embeds {
            html.push_str(&render_view_record_embed(embed));
        }
    }

    html.push_str("</span>");

    html
}

/// Render an embed item from a ViewRecord (nested embeds in quotes)
fn render_view_record_embed(
    embed: &weaver_api::app_bsky::embed::record::ViewRecordEmbedsItem<'_>,
) -> String {
    use weaver_api::app_bsky::embed::record::ViewRecordEmbedsItem;

    match embed {
        ViewRecordEmbedsItem::ImagesView(images) => render_images(&images.images),
        ViewRecordEmbedsItem::ExternalView(external) => render_external_link(&external.external),
        ViewRecordEmbedsItem::View(record_view) => render_record_embed(&record_view.record),
        ViewRecordEmbedsItem::RecordWithMediaView(rwm) => {
            let mut html = String::new();
            // Render media first
            match &rwm.media {
                weaver_api::app_bsky::embed::record_with_media::ViewMedia::ImagesView(img) => {
                    html.push_str(&render_images(&img.images));
                }
                weaver_api::app_bsky::embed::record_with_media::ViewMedia::ExternalView(ext) => {
                    html.push_str(&render_external_link(&ext.external));
                }
                weaver_api::app_bsky::embed::record_with_media::ViewMedia::VideoView(_) => {
                    html.push_str("<span class=\"embed-video-placeholder\">[Video]</span>");
                }
                weaver_api::app_bsky::embed::record_with_media::ViewMedia::Unknown(_) => {}
            }
            // Then the record
            html.push_str(&render_record_embed(&rwm.record.record));
            html
        }
        ViewRecordEmbedsItem::VideoView(_) => {
            "<span class=\"embed-video-placeholder\">[Video]</span>".to_string()
        }
        ViewRecordEmbedsItem::Unknown(data) => render_generic_data(data),
    }
}

/// Render a PostViewEmbed (images, external, record, video, etc.)
pub fn render_post_embed(embed: &PostViewEmbed<'_>) -> String {
    match embed {
        PostViewEmbed::ImagesView(images) => render_images(&images.images),
        PostViewEmbed::ExternalView(external) => render_external_link(&external.external),
        PostViewEmbed::RecordView(record) => render_record_embed(&record.record),
        PostViewEmbed::RecordWithMediaView(rwm) => {
            let mut html = String::new();
            // Render media first
            match &rwm.media {
                weaver_api::app_bsky::embed::record_with_media::ViewMedia::ImagesView(img) => {
                    html.push_str(&render_images(&img.images));
                }
                weaver_api::app_bsky::embed::record_with_media::ViewMedia::ExternalView(ext) => {
                    html.push_str(&render_external_link(&ext.external));
                }
                weaver_api::app_bsky::embed::record_with_media::ViewMedia::VideoView(_) => {
                    html.push_str("<span class=\"embed-video-placeholder\">[Video]</span>");
                }
                weaver_api::app_bsky::embed::record_with_media::ViewMedia::Unknown(_) => {}
            }
            // Then the record
            html.push_str(&render_record_embed(&rwm.record.record));
            html
        }
        PostViewEmbed::VideoView(_) => {
            "<span class=\"embed-video-placeholder\">[Video]</span>".to_string()
        }
        PostViewEmbed::Unknown(data) => render_generic_data(data),
    }
}

/// Render a ViewUnionRecord (the actual content of a record embed)
fn render_record_embed(record: &ViewUnionRecord<'_>) -> String {
    match record {
        ViewUnionRecord::ViewRecord(r) => render_quoted_record(r),
        ViewUnionRecord::ViewNotFound(_) => {
            "<span class=\"embed-not-found\">Record not found</span>".to_string()
        }
        ViewUnionRecord::ViewBlocked(_) => {
            "<span class=\"embed-blocked\">Content blocked</span>".to_string()
        }
        ViewUnionRecord::ViewDetached(_) => {
            "<span class=\"embed-detached\">Content unavailable</span>".to_string()
        }
        ViewUnionRecord::GeneratorView(generator) => {
            let mut html = String::new();
            html.push_str("<span class=\"embed-record-card\">");

            // Icon + title + type (like author block layout)
            html.push_str("<span class=\"embed-author\">");
            if let Some(avatar) = &generator.avatar {
                html.push_str("<img class=\"embed-avatar\" src=\"");
                html.push_str(&html_escape(avatar.as_ref()));
                html.push_str("\" alt=\"\" width=\"42\" height=\"42\" />");
            }
            html.push_str("<span class=\"embed-author-info\">");
            html.push_str("<span class=\"embed-author-name\">");
            html.push_str(&html_escape(generator.display_name.as_ref()));
            html.push_str("</span>");
            html.push_str("<span class=\"embed-author-handle\">Feed</span>");
            html.push_str("</span>");
            html.push_str("</span>");

            // Description
            if let Some(desc) = &generator.description {
                html.push_str("<span class=\"embed-description\">");
                html.push_str(&html_escape(desc.as_ref()));
                html.push_str("</span>");
            }

            // Creator
            html.push_str(&render_author_block_full(&generator.creator, true));

            // Stats
            if let Some(likes) = generator.like_count {
                html.push_str("<span class=\"embed-stats\">");
                html.push_str("<span class=\"embed-stat\">");
                html.push_str(&likes.to_string());
                html.push_str(" likes</span>");
                html.push_str("</span>");
            }

            html.push_str("</span>");
            html
        }
        ViewUnionRecord::ListView(list) => {
            let mut html = String::new();
            html.push_str("<span class=\"embed-record-card\">");

            // Icon + title + type (like author block layout)
            html.push_str("<span class=\"embed-author\">");
            if let Some(avatar) = &list.avatar {
                html.push_str("<img class=\"embed-avatar\" src=\"");
                html.push_str(&html_escape(avatar.as_ref()));
                html.push_str("\" alt=\"\" width=\"42\" height=\"42\" />");
            }
            html.push_str("<span class=\"embed-author-info\">");
            html.push_str("<span class=\"embed-author-name\">");
            html.push_str(&html_escape(list.name.as_ref()));
            html.push_str("</span>");
            html.push_str("<span class=\"embed-author-handle\">List</span>");
            html.push_str("</span>");
            html.push_str("</span>");

            // Description
            if let Some(desc) = &list.description {
                html.push_str("<span class=\"embed-description\">");
                html.push_str(&html_escape(desc.as_ref()));
                html.push_str("</span>");
            }

            // Creator
            html.push_str(&render_author_block_full(&list.creator, true));

            // Stats
            if let Some(count) = list.list_item_count {
                html.push_str("<span class=\"embed-stats\">");
                html.push_str("<span class=\"embed-stat\">");
                html.push_str(&count.to_string());
                html.push_str(" members</span>");
                html.push_str("</span>");
            }

            html.push_str("</span>");
            html
        }
        ViewUnionRecord::LabelerView(labeler) => {
            let mut html = String::new();
            html.push_str("<span class=\"embed-record-card\">");

            // Labeler uses creator as the identity, add type label
            html.push_str("<span class=\"embed-author\">");
            if let Some(avatar) = &labeler.creator.avatar {
                html.push_str("<img class=\"embed-avatar\" src=\"");
                html.push_str(&html_escape(avatar.as_ref()));
                html.push_str("\" alt=\"\" width=\"42\" height=\"42\" />");
            }
            html.push_str("<span class=\"embed-author-info\">");
            if let Some(name) = &labeler.creator.display_name {
                html.push_str("<span class=\"embed-author-name\">");
                html.push_str(&html_escape(name.as_ref()));
                html.push_str("</span>");
            }
            html.push_str("<span class=\"embed-author-handle\">Labeler</span>");
            html.push_str("</span>");
            html.push_str("</span>");

            // Stats
            if let Some(likes) = labeler.like_count {
                html.push_str("<span class=\"embed-stats\">");
                html.push_str("<span class=\"embed-stat\">");
                html.push_str(&likes.to_string());
                html.push_str(" likes</span>");
                html.push_str("</span>");
            }

            html.push_str("</span>");
            html
        }
        ViewUnionRecord::StarterPackViewBasic(sp) => {
            let mut html = String::new();
            html.push_str("<span class=\"embed-record-card\">");

            // Use author block layout: avatar + info (name, subtitle)
            html.push_str("<span class=\"embed-author\">");
            if let Some(avatar) = &sp.creator.avatar {
                html.push_str("<img class=\"embed-avatar\" src=\"");
                html.push_str(&html_escape(avatar.as_ref()));
                html.push_str("\" alt=\"\" width=\"42\" height=\"42\" />");
            }
            html.push_str("<span class=\"embed-author-info\">");

            // Name as title
            if let Some(name) = sp.record.query("name").single().and_then(|d| d.as_str()) {
                html.push_str("<span class=\"embed-author-name\">");
                html.push_str(&html_escape(name));
                html.push_str("</span>");
            }

            // "Starter pack by @handle"
            html.push_str("<span class=\"embed-author-handle\">by @");
            html.push_str(&html_escape(sp.creator.handle.as_ref()));
            html.push_str("</span>");

            html.push_str("</span>"); // end info
            html.push_str("</span>"); // end author

            // Description
            if let Some(desc) = sp
                .record
                .query("description")
                .single()
                .and_then(|d| d.as_str())
            {
                html.push_str("<span class=\"embed-description\">");
                html.push_str(&html_escape(desc));
                html.push_str("</span>");
            }

            // Stats
            let has_stats = sp.list_item_count.is_some() || sp.joined_all_time_count.is_some();
            if has_stats {
                html.push_str("<span class=\"embed-stats\">");
                if let Some(count) = sp.list_item_count {
                    html.push_str("<span class=\"embed-stat\">");
                    html.push_str(&count.to_string());
                    html.push_str(" users</span>");
                }
                if let Some(joined) = sp.joined_all_time_count {
                    html.push_str("<span class=\"embed-stat\">");
                    html.push_str(&joined.to_string());
                    html.push_str(" joined</span>");
                }
                html.push_str("</span>");
            }

            html.push_str("</span>");
            html
        }
        ViewUnionRecord::Unknown(data) => render_generic_data(data),
    }
}

/// Render generic/unknown data by iterating fields intelligently
///
/// Used as fallback for Unknown variants of open unions.
fn render_generic_data(data: &Data<'_>) -> String {
    render_generic_data_with_depth(data, 0)
}

/// Render generic data with depth tracking for nested objects
fn render_generic_data_with_depth(data: &Data<'_>, depth: u8) -> String {
    let mut html = String::new();

    // Only wrap in card at top level
    let is_nested = depth > 0;
    if is_nested {
        html.push_str("<span class=\"embed-fields\">");
    } else {
        html.push_str("<span class=\"embed-record-card\">");
    }

    // Show record type as header if present
    if let Some(record_type) = data.type_discriminator() {
        html.push_str("<span class=\"embed-author-handle\">");
        html.push_str(&html_escape(record_type));
        html.push_str("</span>");
    }

    // Priority fields to show first (in order)
    let priority_fields = ["name", "displayName", "title", "text", "description"];
    let mut shown_fields = Vec::new();

    if let Some(obj) = data.as_object() {
        for field_name in priority_fields {
            if let Some(value) = obj.get(field_name) {
                if let Some(s) = value.as_str() {
                    let class = match field_name {
                        "name" | "displayName" | "title" => "embed-author-name",
                        "text" => "embed-content",
                        "description" => "embed-description",
                        _ => "embed-field-value",
                    };
                    html.push_str("<span class=\"");
                    html.push_str(class);
                    html.push_str("\">");
                    html.push_str(&html_escape(s));
                    html.push_str("</span>");
                    shown_fields.push(field_name);
                }
            }
        }

        // Show remaining fields as a simple list
        if !is_nested {
            html.push_str("<span class=\"embed-fields\">");
        }
        for (key, value) in obj.iter() {
            let key_str: &str = key.as_ref();

            // Skip already shown, internal fields
            if shown_fields.contains(&key_str)
                || key_str.starts_with('$')
                || key_str == "facets"
                || key_str == "labels"
            {
                continue;
            }

            if let Some(formatted) = format_field_value_with_depth(key_str, value, depth) {
                html.push_str("<span class=\"embed-field\">");
                html.push_str("<span class=\"embed-field-name\">");
                html.push_str(&html_escape(&format_field_name(key_str)));
                html.push_str(":</span> ");
                html.push_str(&formatted);
                html.push_str("</span>");
            }
        }
        if !is_nested {
            html.push_str("</span>");
        }
    }

    html.push_str("</span>");
    html
}

/// Format a field name for display (camelCase -> "Camel Case")
fn format_field_name(name: &str) -> String {
    let mut result = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push(' ');
        }
        if i == 0 {
            result.extend(c.to_uppercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Format a field value for display, returning None for complex/unrenderable values
fn format_field_value(key: &str, value: &Data<'_>) -> Option<String> {
    format_field_value_with_depth(key, value, 0)
}

/// Maximum nesting depth for rendering nested objects
const MAX_NESTED_DEPTH: u8 = 2;

/// Format a field value for display with depth tracking
fn format_field_value_with_depth(key: &str, value: &Data<'_>, depth: u8) -> Option<String> {
    // String values - detect AT Protocol types
    if let Some(s) = value.as_str() {
        return Some(format_string_value(key, s));
    }

    // Numbers
    if let Some(n) = value.as_integer() {
        return Some(format!("<span class=\"embed-field-number\">{}</span>", n));
    }

    // Booleans
    if let Some(b) = value.as_boolean() {
        let class = if b {
            "embed-field-bool-true"
        } else {
            "embed-field-bool-false"
        };
        return Some(format!(
            "<span class=\"{}\">{}</span>",
            class,
            if b { "yes" } else { "no" }
        ));
    }

    // Arrays - show count or render items if simple
    if let Some(arr) = value.as_array() {
        return Some(format_array_value(arr, depth));
    }

    // Nested objects - render if within depth limit
    if value.as_object().is_some() {
        if depth < MAX_NESTED_DEPTH {
            return Some(render_generic_data_with_depth(value, depth + 1));
        } else {
            // At max depth, just show field count
            let count = value.as_object().map(|o| o.len()).unwrap_or(0);
            return Some(format!(
                "<span class=\"embed-field-count\">{} field{}</span>",
                count,
                if count == 1 { "" } else { "s" }
            ));
        }
    }

    None
}

/// Format an array value, rendering items if simple enough
fn format_array_value(arr: &jacquard::Array<'_>, depth: u8) -> String {
    let len = arr.len();

    // Empty array
    if len == 0 {
        return "<span class=\"embed-field-count\">empty</span>".to_string();
    }

    // For small arrays of simple values, show them inline
    if len <= 3 && depth < MAX_NESTED_DEPTH {
        let mut items = Vec::new();
        let mut all_simple = true;

        for item in arr.iter() {
            if let Some(formatted) = format_simple_value(item) {
                items.push(formatted);
            } else {
                all_simple = false;
                break;
            }
        }

        if all_simple {
            return format!(
                "<span class=\"embed-field-value\">[{}]</span>",
                items.join(", ")
            );
        }
    }

    // Otherwise just show count
    format!(
        "<span class=\"embed-field-count\">{} item{}</span>",
        len,
        if len == 1 { "" } else { "s" }
    )
}

/// Format a simple value (string, number, bool) without field name context
fn format_simple_value(value: &Data<'_>) -> Option<String> {
    if let Some(s) = value.as_str() {
        // Keep it short for array display
        let display = if s.len() > 50 {
            format!("{}…", &s[..50])
        } else {
            s.to_string()
        };
        return Some(format!("\"{}\"", html_escape(&display)));
    }

    if let Some(n) = value.as_integer() {
        return Some(n.to_string());
    }

    if let Some(b) = value.as_boolean() {
        return Some(if b { "true" } else { "false" }.to_string());
    }

    None
}

/// Format a string value with smart detection of AT Protocol types
fn format_string_value(key: &str, s: &str) -> String {
    // AT URI - link to record
    if s.starts_with("at://") {
        return format!(
            "<a class=\"embed-field-aturi\" href=\"{}\">{}</a>",
            html_escape(s),
            format_aturi_display(s)
        );
    }

    // DID
    if s.starts_with("did:") {
        return format_did_display(s);
    }

    // Regular URL
    if s.starts_with("http://") || s.starts_with("https://") {
        let domain = extract_domain(s).unwrap_or(s);
        return format!(
            "<a class=\"embed-field-link\" href=\"{}\">{}</a>",
            html_escape(s),
            html_escape(domain)
        );
    }

    // Datetime fields - show just the date
    if key.ends_with("At") || key == "createdAt" || key == "indexedAt" {
        let date_part = s.split('T').next().unwrap_or(s);
        return format!(
            "<span class=\"embed-field-date\">{}</span>",
            html_escape(date_part)
        );
    }

    // NSID (e.g., app.bsky.feed.post)
    if s.contains('.')
        && s.chars().all(|c| c.is_alphanumeric() || c == '.')
        && s.matches('.').count() >= 2
    {
        return format!("<span class=\"embed-field-nsid\">{}</span>", html_escape(s));
    }

    // Handle (contains dots, no colons or slashes)
    if s.contains('.')
        && !s.contains(':')
        && !s.contains('/')
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return format!(
            "<span class=\"embed-field-handle\">@{}</span>",
            html_escape(s)
        );
    }

    // Plain string
    html_escape(s)
}

/// Format an AT URI for display with highlighted parts
fn format_aturi_display(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("at://") {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        let mut result = String::from("<span class=\"aturi-scheme\">at://</span>");

        if !parts.is_empty() {
            result.push_str(&format!(
                "<span class=\"aturi-authority\">{}</span>",
                html_escape(parts[0])
            ));
        }
        if parts.len() > 1 {
            result.push_str("<span class=\"aturi-slash\">/</span>");
            result.push_str(&format!(
                "<span class=\"aturi-collection\">{}</span>",
                html_escape(parts[1])
            ));
        }
        if parts.len() > 2 {
            result.push_str("<span class=\"aturi-slash\">/</span>");
            result.push_str(&format!(
                "<span class=\"aturi-rkey\">{}</span>",
                html_escape(parts[2])
            ));
        }
        result
    } else {
        html_escape(uri)
    }
}

/// Format a DID for display with highlighted parts
fn format_did_display(did: &str) -> String {
    if let Some(rest) = did.strip_prefix("did:") {
        if let Some((method, identifier)) = rest.split_once(':') {
            return format!(
                "<span class=\"embed-field-did\">\
                    <span class=\"did-scheme\">did:</span>\
                    <span class=\"did-method\">{}</span>\
                    <span class=\"did-separator\">:</span>\
                    <span class=\"did-identifier\">{}</span>\
                </span>",
                html_escape(method),
                html_escape(identifier)
            );
        }
    }
    format!(
        "<span class=\"embed-field-did\">{}</span>",
        html_escape(did)
    )
}

// =============================================================================
// Helper functions
// =============================================================================

/// Extract domain from a URL
fn extract_domain(url: &str) -> Option<&str> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    without_scheme.split('/').next()
}

/// Simple HTML escaping
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
