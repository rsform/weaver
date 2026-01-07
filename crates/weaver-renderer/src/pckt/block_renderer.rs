use jacquard::client::AgentSessionExt;
use jacquard::types::string::{AtUri, Did};
use jacquard::types::value::Data;
use markdown_weaver_escape::escape_html;
use weaver_api::blog_pckt::block::{
    blockquote::Blockquote, bluesky_embed::BlueskyEmbed, bullet_list::BulletList,
    code_block::CodeBlock, heading::Heading, horizontal_rule::HorizontalRule, iframe::Iframe,
    image::Image, list_item::ListItemContentItem, ordered_list::OrderedList, text::Text,
    website::Website,
};

use crate::facet::{NormalizedFacet, render_faceted_html};

pub struct PcktRenderContext {
    pub author_did: Did<'static>,
}

impl PcktRenderContext {
    pub fn new(author_did: Did<'static>) -> Self {
        Self { author_did }
    }

    fn blob_url(&self, cid: &jacquard::types::cid::Cid<'_>) -> String {
        format!(
            "https://cdn.bsky.app/img/feed_fullsize/plain/{}/{}@jpeg",
            self.author_did.as_ref(),
            cid.as_ref()
        )
    }
}

pub async fn render_content_blocks<A: AgentSessionExt>(
    blocks: &[Data<'_>],
    ctx: &PcktRenderContext,
    agent: &A,
) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"pckt-document\">");
    for block in blocks {
        render_block(&mut html, block, ctx, agent).await;
    }
    html.push_str("</div>");
    html
}

pub async fn render_block<A: AgentSessionExt>(
    html: &mut String,
    block: &Data<'_>,
    ctx: &PcktRenderContext,
    agent: &A,
) {
    let Some(type_tag) = block.type_discriminator() else {
        return;
    };

    match type_tag {
        "blog.pckt.block.text" => {
            if let Ok(text) = jacquard::from_data::<Text>(block) {
                render_text_block(html, &text);
            }
        }
        "blog.pckt.block.heading" => {
            if let Ok(heading) = jacquard::from_data::<Heading>(block) {
                render_heading_block(html, &heading);
            }
        }
        "blog.pckt.block.blockquote" => {
            if let Ok(quote) = jacquard::from_data::<Blockquote>(block) {
                render_blockquote_block(html, &quote);
            }
        }
        "blog.pckt.block.codeBlock" => {
            if let Ok(code) = jacquard::from_data::<CodeBlock>(block) {
                render_code_block(html, &code);
            }
        }
        "blog.pckt.block.bulletList" => {
            if let Ok(list) = jacquard::from_data::<BulletList>(block) {
                render_bullet_list(html, &list, ctx);
            }
        }
        "blog.pckt.block.orderedList" => {
            if let Ok(list) = jacquard::from_data::<OrderedList>(block) {
                render_ordered_list(html, &list, ctx);
            }
        }
        "blog.pckt.block.image" => {
            if let Ok(image) = jacquard::from_data::<Image>(block) {
                render_image_block(html, &image, ctx);
            }
        }
        "blog.pckt.block.website" => {
            if let Ok(website) = jacquard::from_data::<Website>(block) {
                render_website_block(html, &website);
            }
        }
        "blog.pckt.block.iframe" => {
            if let Ok(iframe) = jacquard::from_data::<Iframe>(block) {
                render_iframe_block(html, &iframe);
            }
        }
        "blog.pckt.block.blueskyEmbed" => {
            if let Ok(embed) = jacquard::from_data::<BlueskyEmbed>(block) {
                render_bluesky_embed(html, &embed, agent).await;
            }
        }
        "blog.pckt.block.horizontalRule" => {
            if jacquard::from_data::<HorizontalRule>(block).is_ok() {
                html.push_str("<hr>\n");
            }
        }
        _ => {
            tracing::debug!("Unknown pckt block type: {}", type_tag);
        }
    }
}

fn render_text_block(html: &mut String, text: &Text<'_>) {
    if text.plaintext.is_empty() {
        html.push_str("<p>&nbsp;</p>\n");
        return;
    }
    html.push_str("<p>");
    html.push_str(&render_faceted_text(
        &text.plaintext,
        text.facets.as_deref(),
    ));
    html.push_str("</p>\n");
}

fn render_heading_block(html: &mut String, heading: &Heading<'_>) {
    let level = heading.level.unwrap_or(1).clamp(1, 6);
    html.push_str(&format!("<h{}>", level));
    html.push_str(&render_faceted_text(
        &heading.plaintext,
        heading.facets.as_deref(),
    ));
    html.push_str(&format!("</h{}>\n", level));
}

fn render_blockquote_block(html: &mut String, quote: &Blockquote<'_>) {
    html.push_str("<blockquote>\n");
    for text in &quote.content {
        html.push_str("<p>");
        html.push_str(&render_faceted_text(
            &text.plaintext,
            text.facets.as_deref(),
        ));
        html.push_str("</p>\n");
    }
    html.push_str("</blockquote>\n");
}

fn render_code_block(html: &mut String, code: &CodeBlock<'_>) {
    html.push_str("<pre><code");
    if let Some(lang) = &code.language {
        html.push_str(" class=\"language-");
        let _ = escape_html(&mut *html, lang.as_ref());
        html.push('"');
    }
    html.push('>');
    let _ = escape_html(&mut *html, &code.plaintext);
    html.push_str("</code></pre>\n");
}

fn render_bullet_list(html: &mut String, list: &BulletList<'_>, ctx: &PcktRenderContext) {
    html.push_str("<ul>\n");
    for item in &list.content {
        html.push_str("<li>");
        for content in &item.content {
            render_list_item_content(html, content, ctx);
        }
        html.push_str("</li>\n");
    }
    html.push_str("</ul>\n");
}

fn render_ordered_list(html: &mut String, list: &OrderedList<'_>, ctx: &PcktRenderContext) {
    html.push_str("<ol>\n");
    for item in &list.content {
        html.push_str("<li>");
        for content in &item.content {
            render_list_item_content(html, content, ctx);
        }
        html.push_str("</li>\n");
    }
    html.push_str("</ol>\n");
}

fn render_list_item_content(
    html: &mut String,
    content: &ListItemContentItem<'_>,
    ctx: &PcktRenderContext,
) {
    match content {
        ListItemContentItem::Text(text) => {
            html.push_str(&render_faceted_text(
                &text.plaintext,
                text.facets.as_deref(),
            ));
        }
        ListItemContentItem::BulletList(list) => {
            render_bullet_list(html, list, ctx);
        }
        ListItemContentItem::OrderedList(list) => {
            render_ordered_list(html, list, ctx);
        }
        ListItemContentItem::Unknown(_) => {
            // TODO use data to do smarter things here

            html.push_str("<span class=\"unknown\">Unknown content</span>");
        }
    }
}

fn render_image_block(html: &mut String, image: &Image<'_>, ctx: &PcktRenderContext) {
    html.push_str("<figure><img src=\"");

    // Prefer blob if available, fall back to src
    if let Some(blob) = &image.attrs.blob {
        let url = ctx.blob_url(blob.blob().cid());
        let _ = escape_html(&mut *html, &url);
    } else {
        let _ = escape_html(&mut *html, image.attrs.src.as_ref());
    }

    html.push('"');
    if let Some(alt) = &image.attrs.alt {
        html.push_str(" alt=\"");
        let _ = escape_html(&mut *html, alt.as_ref());
        html.push('"');
    }
    html.push_str(" loading=\"lazy\">");
    html.push_str("</figure>\n");
}

fn render_website_block(html: &mut String, website: &Website<'_>) {
    html.push_str("<a class=\"embed-external\" href=\"");
    let _ = escape_html(&mut *html, website.src.as_ref());
    html.push_str("\" target=\"_blank\" rel=\"noopener\">");

    if let Some(preview) = &website.preview_image {
        html.push_str("<img class=\"embed-external-thumb\" src=\"");
        let _ = escape_html(&mut *html, preview.as_ref());
        html.push_str("\" alt=\"\" loading=\"lazy\" />");
    }

    html.push_str("<span class=\"embed-external-info\">");

    if let Some(title) = &website.title {
        html.push_str("<span class=\"embed-external-title\">");
        let _ = escape_html(&mut *html, title.as_ref());
        html.push_str("</span>");
    }

    if let Some(desc) = &website.description {
        html.push_str("<span class=\"embed-external-description\">");
        let _ = escape_html(&mut *html, desc.as_ref());
        html.push_str("</span>");
    }

    html.push_str("<span class=\"embed-external-url\">");
    html.push_str(extract_domain(website.src.as_ref()));
    html.push_str("</span>");

    html.push_str("</span></a>\n");
}

fn extract_domain(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|s| s.split('/').next())
        .unwrap_or(url)
}

fn render_iframe_block(html: &mut String, iframe: &Iframe<'_>) {
    html.push_str("<div class=\"iframe-embed\"><iframe src=\"");
    let _ = escape_html(&mut *html, iframe.url.as_ref());
    html.push_str("\" frameborder=\"0\" allowfullscreen loading=\"lazy\"></iframe></div>\n");
}

async fn render_bluesky_embed<A: AgentSessionExt>(
    html: &mut String,
    embed: &BlueskyEmbed<'_>,
    agent: &A,
) {
    let uri_str = embed.post_ref.uri.as_ref();

    // Try to fetch and render the actual post
    if let Ok(uri) = AtUri::new(uri_str) {
        match crate::atproto::fetch_and_render_post(&uri, agent).await {
            Ok(rendered) => {
                html.push_str(&rendered);
                return;
            }
            Err(e) => {
                tracing::warn!("Failed to fetch embedded post {}: {:?}", uri_str, e);
            }
        }
    }

    // Fallback: render a placeholder link
    html.push_str("<div class=\"bsky-embed-placeholder\">");
    html.push_str("<a href=\"https://bsky.app/profile/");

    if let Some(rest) = uri_str.strip_prefix("at://") {
        if let Some((did, path)) = rest.split_once('/') {
            let _ = escape_html(&mut *html, did);
            html.push_str("/post/");
            if let Some(rkey) = path.strip_prefix("app.bsky.feed.post/") {
                let _ = escape_html(&mut *html, rkey);
            }
        }
    }

    html.push_str("\" target=\"_blank\" rel=\"noopener\">View post on Bluesky</a></div>\n");
}

fn render_faceted_text(
    text: &str,
    facets: Option<&[weaver_api::blog_pckt::richtext::facet::Facet<'_>]>,
) -> String {
    if let Some(facets) = facets {
        let normalized: Vec<NormalizedFacet<'_>> =
            facets.iter().map(NormalizedFacet::from).collect();
        render_faceted_html(text, &normalized).unwrap_or_else(|_| {
            let mut escaped = String::new();
            let _ = escape_html(&mut escaped, text);
            escaped
        })
    } else {
        let mut escaped = String::new();
        let _ = escape_html(&mut escaped, text);
        escaped
    }
}

/// Sync version of render_content_blocks that uses pre-resolved embeds.
pub fn render_content_blocks_sync(
    blocks: &[jacquard::types::value::Data<'_>],
    ctx: &PcktRenderContext,
    resolved_content: Option<&weaver_common::ResolvedContent>,
) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"pckt-document\">");
    for block in blocks {
        render_block_sync(&mut html, block, ctx, resolved_content);
    }
    html.push_str("</div>");
    html
}

/// Sync version of render_block that uses pre-resolved embeds for BlueskyEmbed blocks.
pub fn render_block_sync(
    html: &mut String,
    block: &jacquard::types::value::Data<'_>,
    ctx: &PcktRenderContext,
    resolved_content: Option<&weaver_common::ResolvedContent>,
) {
    let Some(type_tag) = block.type_discriminator() else {
        return;
    };

    match type_tag {
        "blog.pckt.block.text" => {
            if let Ok(text) = jacquard::from_data::<Text>(block) {
                render_text_block(html, &text);
            }
        }
        "blog.pckt.block.heading" => {
            if let Ok(heading) = jacquard::from_data::<Heading>(block) {
                render_heading_block(html, &heading);
            }
        }
        "blog.pckt.block.blockquote" => {
            if let Ok(quote) = jacquard::from_data::<Blockquote>(block) {
                render_blockquote_block(html, &quote);
            }
        }
        "blog.pckt.block.codeBlock" => {
            if let Ok(code) = jacquard::from_data::<CodeBlock>(block) {
                render_code_block(html, &code);
            }
        }
        "blog.pckt.block.bulletList" => {
            if let Ok(list) = jacquard::from_data::<BulletList>(block) {
                render_bullet_list(html, &list, ctx);
            }
        }
        "blog.pckt.block.orderedList" => {
            if let Ok(list) = jacquard::from_data::<OrderedList>(block) {
                render_ordered_list(html, &list, ctx);
            }
        }
        "blog.pckt.block.image" => {
            if let Ok(image) = jacquard::from_data::<Image>(block) {
                render_image_block(html, &image, ctx);
            }
        }
        "blog.pckt.block.website" => {
            if let Ok(website) = jacquard::from_data::<Website>(block) {
                render_website_block(html, &website);
            }
        }
        "blog.pckt.block.iframe" => {
            if let Ok(iframe) = jacquard::from_data::<Iframe>(block) {
                render_iframe_block(html, &iframe);
            }
        }
        "blog.pckt.block.blueskyEmbed" => {
            if let Ok(embed) = jacquard::from_data::<BlueskyEmbed>(block) {
                render_bluesky_embed_sync(html, &embed, resolved_content);
            }
        }
        "blog.pckt.block.horizontalRule" => {
            if jacquard::from_data::<HorizontalRule>(block).is_ok() {
                html.push_str("<hr>\n");
            }
        }
        _ => {
            tracing::debug!("Unknown pckt block type: {}", type_tag);
        }
    }
}

fn render_bluesky_embed_sync(
    html: &mut String,
    embed: &BlueskyEmbed<'_>,
    resolved_content: Option<&weaver_common::ResolvedContent>,
) {
    let uri_str = embed.post_ref.uri.as_ref();

    // Look up pre-rendered content.
    if let Some(resolved) = resolved_content {
        if let Ok(at_uri) = AtUri::new(uri_str) {
            if let Some(rendered) = resolved.get_embed_content(&at_uri) {
                html.push_str(rendered);
                return;
            }
        }
    }

    // Fallback: use bsky embed iframe.
    // Format: at://did/app.bsky.feed.post/rkey -> https://embed.bsky.app/embed/did/post/rkey
    if let Some(rest) = uri_str.strip_prefix("at://") {
        if let Some((did, path)) = rest.split_once('/') {
            if let Some(rkey) = path.strip_prefix("app.bsky.feed.post/") {
                html.push_str("<iframe class=\"bsky-embed-iframe\" src=\"https://embed.bsky.app/embed/");
                let _ = escape_html(&mut *html, did);
                html.push_str("/post/");
                let _ = escape_html(&mut *html, rkey);
                html.push_str("\" frameborder=\"0\" scrolling=\"no\" loading=\"lazy\" style=\"border: none; width: 100%; height: 240px;\"></iframe>\n");
                return;
            }
        }
    }

    // Last resort: placeholder link.
    html.push_str("<div class=\"bsky-embed-placeholder\">");
    html.push_str("<a href=\"https://bsky.app/profile/");

    if let Some(rest) = uri_str.strip_prefix("at://") {
        if let Some((did, path)) = rest.split_once('/') {
            let _ = escape_html(&mut *html, did);
            html.push_str("/post/");
            if let Some(rkey) = path.strip_prefix("app.bsky.feed.post/") {
                let _ = escape_html(&mut *html, rkey);
            }
        }
    }

    html.push_str("\" target=\"_blank\" rel=\"noopener\">View post on Bluesky</a></div>\n");
}
