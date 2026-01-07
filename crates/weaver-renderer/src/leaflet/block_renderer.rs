use std::fmt::Write;

use jacquard::client::AgentSessionExt;
use jacquard::types::cid::Cid;
use jacquard::types::string::{AtUri, Did};
use markdown_weaver_escape::escape_html;
use weaver_api::pub_leaflet::blocks::{
    blockquote::Blockquote,
    bsky_post::BskyPost,
    button::Button,
    code::Code,
    header::Header,
    iframe::Iframe,
    image::Image,
    math::Math,
    page::Page,
    poll::Poll,
    text::Text,
    unordered_list::{ListItem, ListItemContent, UnorderedList},
    website::Website,
};
use weaver_api::pub_leaflet::pages::linear_document::{Block, BlockBlock, LinearDocument};

use crate::facet::{NormalizedFacet, render_faceted_html};

pub struct LeafletRenderContext {
    pub author_did: Did<'static>,
}

impl LeafletRenderContext {
    pub fn new(author_did: Did<'static>) -> Self {
        Self { author_did }
    }

    fn blob_url(&self, cid: &Cid<'_>) -> String {
        format!(
            "https://leaflet.pub/api/atproto_images?did={}&cid={}",
            self.author_did.as_ref(),
            cid.as_ref()
        )
    }
}

pub async fn render_linear_document<A: AgentSessionExt>(
    doc: &LinearDocument<'_>,
    ctx: &LeafletRenderContext,
    agent: &A,
) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"leaflet-document\">");

    for block in &doc.blocks {
        html.push_str(&render_block(block, ctx, agent).await);
    }

    html.push_str("</div>");
    html
}

pub async fn render_block<A: AgentSessionExt>(
    block: &Block<'_>,
    ctx: &LeafletRenderContext,
    agent: &A,
) -> String {
    let mut html = String::new();

    let alignment_class = block
        .alignment
        .as_ref()
        .map(|a| match a.as_ref() {
            "pub.leaflet.pages.linearDocument#textAlignCenter" => " align-center",
            "pub.leaflet.pages.linearDocument#textAlignRight" => " align-right",
            "pub.leaflet.pages.linearDocument#textAlignJustify" => " align-justify",
            _ => "",
        })
        .unwrap_or("");

    match &block.block {
        BlockBlock::Text(text) => {
            render_text_block(&mut html, text, alignment_class);
        }
        BlockBlock::Header(header) => {
            render_header_block(&mut html, header, alignment_class);
        }
        BlockBlock::Blockquote(quote) => {
            render_blockquote_block(&mut html, quote);
        }
        BlockBlock::Code(code) => {
            render_code_block(&mut html, code);
        }
        BlockBlock::UnorderedList(list) => {
            render_unordered_list(&mut html, list, ctx, agent).await;
        }
        BlockBlock::Image(image) => {
            render_image_block(&mut html, image, ctx);
        }
        BlockBlock::Website(website) => {
            render_website_block(&mut html, website, ctx);
        }
        BlockBlock::Iframe(iframe) => {
            render_iframe_block(&mut html, iframe);
        }
        BlockBlock::BskyPost(post) => {
            render_bsky_post_block(&mut html, post, agent).await;
        }
        BlockBlock::Button(button) => {
            render_button_block(&mut html, button);
        }
        BlockBlock::Poll(poll) => {
            render_poll_block(&mut html, poll);
        }
        BlockBlock::HorizontalRule(_) => {
            html.push_str("<hr />\n");
        }
        BlockBlock::Page(page) => {
            render_page_block(&mut html, page);
        }
        BlockBlock::Math(math) => {
            render_math_block(&mut html, math);
        }
        BlockBlock::Unknown(data) => {
            let _ = write!(
                html,
                "<div class=\"embed-unknown\">[Unknown block: {:?}]</div>\n",
                data.type_discriminator()
            );
        }
    }

    html
}

fn render_text_block(html: &mut String, text: &Text<'_>, alignment_class: &str) {
    let _ = write!(html, "<p class=\"leaflet-text{}\">", alignment_class);
    html.push_str(&render_faceted_text(
        &text.plaintext,
        text.facets.as_deref(),
    ));
    html.push_str("</p>\n");
}

fn render_header_block(html: &mut String, header: &Header<'_>, alignment_class: &str) {
    let level = header.level.unwrap_or(1).clamp(1, 6);
    let _ = write!(html, "<h{}{}>", level, alignment_class);
    html.push_str(&render_faceted_text(
        &header.plaintext,
        header.facets.as_deref(),
    ));
    let _ = write!(html, "</h{}>\n", level);
}

fn render_blockquote_block(html: &mut String, quote: &Blockquote<'_>) {
    html.push_str("<blockquote>");
    html.push_str(&render_faceted_text(
        &quote.plaintext,
        quote.facets.as_deref(),
    ));
    html.push_str("</blockquote>\n");
}

fn render_code_block(html: &mut String, code: &Code<'_>) {
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

async fn render_unordered_list<A: AgentSessionExt>(
    html: &mut String,
    list: &UnorderedList<'_>,
    ctx: &LeafletRenderContext,
    agent: &A,
) {
    html.push_str("<ul>\n");
    for item in &list.children {
        render_list_item(html, item, ctx, agent).await;
    }
    html.push_str("</ul>\n");
}

async fn render_list_item<A: AgentSessionExt>(
    html: &mut String,
    item: &ListItem<'_>,
    ctx: &LeafletRenderContext,
    agent: &A,
) {
    html.push_str("<li>");

    match &item.content {
        ListItemContent::Text(text) => {
            html.push_str(&render_faceted_text(
                &text.plaintext,
                text.facets.as_deref(),
            ));
        }
        ListItemContent::Header(header) => {
            let level = header.level.unwrap_or(1).clamp(1, 6);
            let _ = write!(html, "<h{}>", level);
            html.push_str(&render_faceted_text(
                &header.plaintext,
                header.facets.as_deref(),
            ));
            let _ = write!(html, "</h{}>", level);
        }
        ListItemContent::Image(image) => {
            render_image_inline(html, image, ctx);
        }
        ListItemContent::Unknown(data) => {
            let _ = write!(html, "[Unknown: {:?}]", data.type_discriminator());
        }
    }

    if let Some(children) = &item.children {
        html.push_str("\n<ul>\n");
        for child in children {
            Box::pin(render_list_item(html, child, ctx, agent)).await;
        }
        html.push_str("</ul>\n");
    }

    html.push_str("</li>\n");
}

fn render_image_block(html: &mut String, image: &Image<'_>, ctx: &LeafletRenderContext) {
    html.push_str("<figure>");
    render_image_inline(html, image, ctx);
    if let Some(alt) = &image.alt {
        html.push_str("<figcaption>");
        let _ = escape_html(&mut *html, alt.as_ref());
        html.push_str("</figcaption>");
    }
    html.push_str("</figure>\n");
}

fn render_image_inline(html: &mut String, image: &Image<'_>, ctx: &LeafletRenderContext) {
    let src = ctx.blob_url(image.image.blob().cid());
    html.push_str("<img src=\"");
    let _ = escape_html(&mut *html, &src);
    html.push('"');
    if let Some(alt) = &image.alt {
        html.push_str(" alt=\"");
        let _ = escape_html(&mut *html, alt.as_ref());
        html.push('"');
    }
    let _ = write!(
        html,
        " style=\"aspect-ratio: {} / {};\"",
        image.aspect_ratio.width, image.aspect_ratio.height
    );
    html.push_str(" />");
}

fn render_website_block(html: &mut String, website: &Website<'_>, ctx: &LeafletRenderContext) {
    html.push_str("<a class=\"embed-external\" href=\"");
    let _ = escape_html(&mut *html, website.src.as_ref());
    html.push_str("\" target=\"_blank\" rel=\"noopener\">");

    if let Some(preview) = &website.preview_image {
        let thumb_url = ctx.blob_url(preview.blob().cid());
        html.push_str("<img class=\"embed-external-thumb\" src=\"");
        let _ = escape_html(&mut *html, &thumb_url);
        html.push_str("\" />");
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

fn render_iframe_block(html: &mut String, iframe: &Iframe<'_>) {
    let height = iframe.height.unwrap_or(400);
    html.push_str("<iframe class=\"html-embed-block\" src=\"");
    let _ = escape_html(&mut *html, iframe.url.as_ref());
    let _ = write!(
        html,
        "\" height=\"{}\" frameborder=\"0\" allowfullscreen></iframe>\n",
        height
    );
}

async fn render_bsky_post_block<A: AgentSessionExt>(
    html: &mut String,
    post: &BskyPost<'_>,
    agent: &A,
) {
    let uri_str = post.post_ref.uri.as_ref();

    // Try to fetch and render the actual post (using fetch_and_render_post directly
    // to avoid potential infinite recursion through fetch_and_render dispatch)
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

    // Fallback: render as placeholder
    html.push_str("<div class=\"embed-video-placeholder\" data-aturi=\"");
    let _ = escape_html(&mut *html, uri_str);
    html.push_str("\">[Bluesky Post: ");
    let _ = escape_html(&mut *html, uri_str);
    html.push_str("]</div>\n");
}

fn render_button_block(html: &mut String, button: &Button<'_>) {
    html.push_str("<a class=\"leaflet-button\" href=\"");
    let _ = escape_html(&mut *html, button.url.as_ref());
    html.push_str("\">");
    let _ = escape_html(&mut *html, button.text.as_ref());
    html.push_str("</a>\n");
}

fn render_poll_block(html: &mut String, poll: &Poll<'_>) {
    html.push_str("<div class=\"embed-video-placeholder\">[Poll: ");
    let _ = escape_html(&mut *html, poll.poll_ref.uri.as_ref());
    html.push_str("]</div>\n");
}

fn render_page_block(html: &mut String, page: &Page<'_>) {
    html.push_str("<div class=\"embed-video-placeholder\">[Page Reference: ");
    let _ = escape_html(&mut *html, page.id.as_ref());
    html.push_str("]</div>\n");
}

fn render_math_block(html: &mut String, math: &Math<'_>) {
    match crate::math::render_math(&math.tex, true) {
        crate::math::MathResult::Success(mathml) => {
            html.push_str("<div class=\"math-display\">");
            html.push_str(&mathml);
            html.push_str("</div>\n");
        }
        crate::math::MathResult::Error { html: err_html, .. } => {
            html.push_str(&err_html);
            html.push('\n');
        }
    }
}

fn render_faceted_text(
    text: &str,
    facets: Option<&[weaver_api::pub_leaflet::richtext::facet::Facet<'_>]>,
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

fn extract_domain(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|s| s.split('/').next())
        .unwrap_or(url)
}

/// Sync version of render_linear_document that uses pre-resolved embeds.
pub fn render_linear_document_sync(
    doc: &LinearDocument<'_>,
    ctx: &LeafletRenderContext,
    resolved_content: Option<&weaver_common::ResolvedContent>,
) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"leaflet-document\">");

    for block in &doc.blocks {
        html.push_str(&render_block_sync(block, ctx, resolved_content));
    }

    html.push_str("</div>");
    html
}

/// Sync version of render_block that uses pre-resolved embeds for BskyPost blocks.
pub fn render_block_sync(
    block: &Block<'_>,
    ctx: &LeafletRenderContext,
    resolved_content: Option<&weaver_common::ResolvedContent>,
) -> String {
    let mut html = String::new();

    let alignment_class = block
        .alignment
        .as_ref()
        .map(|a| match a.as_ref() {
            "pub.leaflet.pages.linearDocument#textAlignCenter" => " align-center",
            "pub.leaflet.pages.linearDocument#textAlignRight" => " align-right",
            "pub.leaflet.pages.linearDocument#textAlignJustify" => " align-justify",
            _ => "",
        })
        .unwrap_or("");

    match &block.block {
        BlockBlock::Text(text) => {
            render_text_block(&mut html, text, alignment_class);
        }
        BlockBlock::Header(header) => {
            render_header_block(&mut html, header, alignment_class);
        }
        BlockBlock::Blockquote(quote) => {
            render_blockquote_block(&mut html, quote);
        }
        BlockBlock::Code(code) => {
            render_code_block(&mut html, code);
        }
        BlockBlock::UnorderedList(list) => {
            render_unordered_list_sync(&mut html, list, ctx, resolved_content);
        }
        BlockBlock::Image(image) => {
            render_image_block(&mut html, image, ctx);
        }
        BlockBlock::Website(website) => {
            render_website_block(&mut html, website, ctx);
        }
        BlockBlock::Iframe(iframe) => {
            render_iframe_block(&mut html, iframe);
        }
        BlockBlock::BskyPost(post) => {
            render_bsky_post_block_sync(&mut html, post, resolved_content);
        }
        BlockBlock::Button(button) => {
            render_button_block(&mut html, button);
        }
        BlockBlock::Poll(poll) => {
            render_poll_block(&mut html, poll);
        }
        BlockBlock::HorizontalRule(_) => {
            html.push_str("<hr />\n");
        }
        BlockBlock::Page(page) => {
            render_page_block(&mut html, page);
        }
        BlockBlock::Math(math) => {
            render_math_block(&mut html, math);
        }
        BlockBlock::Unknown(data) => {
            let _ = write!(
                html,
                "<div class=\"embed-unknown\">[Unknown block: {:?}]</div>\n",
                data.type_discriminator()
            );
        }
    }

    html
}

fn render_unordered_list_sync(
    html: &mut String,
    list: &UnorderedList<'_>,
    ctx: &LeafletRenderContext,
    resolved_content: Option<&weaver_common::ResolvedContent>,
) {
    html.push_str("<ul>\n");
    for item in &list.children {
        render_list_item_sync(html, item, ctx, resolved_content);
    }
    html.push_str("</ul>\n");
}

fn render_list_item_sync(
    html: &mut String,
    item: &ListItem<'_>,
    ctx: &LeafletRenderContext,
    resolved_content: Option<&weaver_common::ResolvedContent>,
) {
    html.push_str("<li>");

    match &item.content {
        ListItemContent::Text(text) => {
            html.push_str(&render_faceted_text(
                &text.plaintext,
                text.facets.as_deref(),
            ));
        }
        ListItemContent::Header(header) => {
            let level = header.level.unwrap_or(1).clamp(1, 6);
            let _ = write!(html, "<h{}>", level);
            html.push_str(&render_faceted_text(
                &header.plaintext,
                header.facets.as_deref(),
            ));
            let _ = write!(html, "</h{}>", level);
        }
        ListItemContent::Image(image) => {
            render_image_inline(html, image, ctx);
        }
        ListItemContent::Unknown(data) => {
            let _ = write!(html, "[Unknown: {:?}]", data.type_discriminator());
        }
    }

    if let Some(children) = &item.children {
        html.push_str("\n<ul>\n");
        for child in children {
            render_list_item_sync(html, child, ctx, resolved_content);
        }
        html.push_str("</ul>\n");
    }

    html.push_str("</li>\n");
}

fn render_bsky_post_block_sync(
    html: &mut String,
    post: &BskyPost<'_>,
    resolved_content: Option<&weaver_common::ResolvedContent>,
) {
    let uri_str = post.post_ref.uri.as_ref();

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
    // Format: at://did/app.bsky.feed.post/rkey -> https://bsky.app/profile/did/post/rkey
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

    // Last resort: placeholder.
    html.push_str("<div class=\"embed-video-placeholder\" data-aturi=\"");
    let _ = escape_html(&mut *html, uri_str);
    html.push_str("\">[Bluesky Post: ");
    let _ = escape_html(&mut *html, uri_str);
    html.push_str("]</div>\n");
}
