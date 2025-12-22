use std::fmt::Write;

use jacquard::types::string::Did;
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

use crate::facet::{NormalizedFacet, render_faceted_markdown};

pub struct LeafletMarkdownContext {
    pub author_did: Did<'static>,
}

impl LeafletMarkdownContext {
    pub fn new(author_did: Did<'static>) -> Self {
        Self { author_did }
    }

    fn blob_ref(&self, cid: &jacquard::types::cid::Cid<'_>) -> String {
        format!("blob:{}:{}", self.author_did.as_ref(), cid.as_ref())
    }
}

pub fn convert_linear_document(doc: &LinearDocument<'_>, ctx: &LeafletMarkdownContext) -> String {
    let mut md = String::new();

    for block in &doc.blocks {
        md.push_str(&convert_block(block, ctx, 0));
    }

    md
}

pub fn convert_block(block: &Block<'_>, ctx: &LeafletMarkdownContext, indent: usize) -> String {
    let mut md = String::new();

    match &block.block {
        BlockBlock::Text(text) => {
            convert_text_block(&mut md, text);
        }
        BlockBlock::Header(header) => {
            convert_header_block(&mut md, header);
        }
        BlockBlock::Blockquote(quote) => {
            convert_blockquote_block(&mut md, quote);
        }
        BlockBlock::Code(code) => {
            convert_code_block(&mut md, code);
        }
        BlockBlock::UnorderedList(list) => {
            convert_unordered_list(&mut md, list, ctx, indent);
        }
        BlockBlock::Image(image) => {
            convert_image_block(&mut md, image, ctx);
        }
        BlockBlock::Website(website) => {
            convert_website_block(&mut md, website);
        }
        BlockBlock::Iframe(iframe) => {
            convert_iframe_block(&mut md, iframe);
        }
        BlockBlock::BskyPost(post) => {
            convert_bsky_post_block(&mut md, post);
        }
        BlockBlock::Button(button) => {
            convert_button_block(&mut md, button);
        }
        BlockBlock::Poll(poll) => {
            convert_poll_block(&mut md, poll);
        }
        BlockBlock::HorizontalRule(_) => {
            md.push_str("---\n\n");
        }
        BlockBlock::Page(page) => {
            convert_page_block(&mut md, page);
        }
        BlockBlock::Math(math) => {
            convert_math_block(&mut md, math);
        }
        BlockBlock::Unknown(data) => {
            let _ = writeln!(md, "<!-- Unknown block: {:?} -->", data.type_discriminator());
        }
    }

    md
}

fn convert_text_block(md: &mut String, text: &Text<'_>) {
    md.push_str(&render_faceted_text(&text.plaintext, text.facets.as_deref()));
    md.push_str("\n\n");
}

fn convert_header_block(md: &mut String, header: &Header<'_>) {
    let level = header.level.unwrap_or(1).clamp(1, 6) as usize;
    for _ in 0..level {
        md.push('#');
    }
    md.push(' ');
    md.push_str(&render_faceted_text(
        &header.plaintext,
        header.facets.as_deref(),
    ));
    md.push_str("\n\n");
}

fn convert_blockquote_block(md: &mut String, quote: &Blockquote<'_>) {
    let text = render_faceted_text(&quote.plaintext, quote.facets.as_deref());
    for line in text.lines() {
        md.push_str("> ");
        md.push_str(line);
        md.push('\n');
    }
    md.push('\n');
}

fn convert_code_block(md: &mut String, code: &Code<'_>) {
    md.push_str("```");
    if let Some(lang) = &code.language {
        md.push_str(lang.as_ref());
    }
    md.push('\n');
    md.push_str(&code.plaintext);
    if !code.plaintext.ends_with('\n') {
        md.push('\n');
    }
    md.push_str("```\n\n");
}

fn convert_unordered_list(
    md: &mut String,
    list: &UnorderedList<'_>,
    ctx: &LeafletMarkdownContext,
    indent: usize,
) {
    for item in &list.children {
        convert_list_item(md, item, ctx, indent);
    }
    if indent == 0 {
        md.push('\n');
    }
}

fn convert_list_item(
    md: &mut String,
    item: &ListItem<'_>,
    ctx: &LeafletMarkdownContext,
    indent: usize,
) {
    let prefix: String = "  ".repeat(indent);
    md.push_str(&prefix);
    md.push_str("- ");

    match &item.content {
        ListItemContent::Text(text) => {
            md.push_str(&render_faceted_text(&text.plaintext, text.facets.as_deref()));
        }
        ListItemContent::Header(header) => {
            md.push_str("**");
            md.push_str(&render_faceted_text(
                &header.plaintext,
                header.facets.as_deref(),
            ));
            md.push_str("**");
        }
        ListItemContent::Image(image) => {
            md.push_str("![");
            if let Some(alt) = &image.alt {
                md.push_str(alt.as_ref());
            }
            md.push_str("](");
            md.push_str(&ctx.blob_ref(image.image.blob().cid()));
            md.push(')');
        }
        ListItemContent::Unknown(data) => {
            let _ = write!(md, "<!-- Unknown: {:?} -->", data.type_discriminator());
        }
    }
    md.push('\n');

    if let Some(children) = &item.children {
        for child in children {
            convert_list_item(md, child, ctx, indent + 1);
        }
    }
}

fn convert_image_block(md: &mut String, image: &Image<'_>, ctx: &LeafletMarkdownContext) {
    md.push_str("![");
    if let Some(alt) = &image.alt {
        md.push_str(alt.as_ref());
    }
    md.push_str("](");
    md.push_str(&ctx.blob_ref(image.image.blob().cid()));
    md.push_str(")\n\n");
}

fn convert_website_block(md: &mut String, website: &Website<'_>) {
    md.push('[');
    if let Some(title) = &website.title {
        md.push_str(title.as_ref());
    } else {
        md.push_str(website.src.as_ref());
    }
    md.push_str("](");
    md.push_str(website.src.as_ref());
    md.push_str(")\n\n");
}

fn convert_iframe_block(md: &mut String, iframe: &Iframe<'_>) {
    let height = iframe.height.unwrap_or(400);
    let _ = writeln!(
        md,
        "<iframe src=\"{}\" height=\"{}\" frameborder=\"0\" allowfullscreen></iframe>\n",
        iframe.url.as_ref(),
        height
    );
}

fn convert_bsky_post_block(md: &mut String, post: &BskyPost<'_>) {
    let _ = writeln!(md, "![[{}]]\n", post.post_ref.uri.as_ref());
}

fn convert_button_block(md: &mut String, button: &Button<'_>) {
    md.push('[');
    md.push_str(button.text.as_ref());
    md.push_str("](");
    md.push_str(button.url.as_ref());
    md.push_str(")\n\n");
}

fn convert_poll_block(md: &mut String, poll: &Poll<'_>) {
    md.push_str("> [!poll]\n");
    let _ = writeln!(md, "> {}\n", poll.poll_ref.uri.as_ref());
}

fn convert_page_block(md: &mut String, page: &Page<'_>) {
    let _ = writeln!(md, "[[{}]]\n", page.id.as_ref());
}

fn convert_math_block(md: &mut String, math: &Math<'_>) {
    md.push_str("$$\n");
    md.push_str(&math.tex);
    if !math.tex.ends_with('\n') {
        md.push('\n');
    }
    md.push_str("$$\n\n");
}

fn render_faceted_text(
    text: &str,
    facets: Option<&[weaver_api::pub_leaflet::richtext::facet::Facet<'_>]>,
) -> String {
    if let Some(facets) = facets {
        let normalized: Vec<NormalizedFacet<'_>> =
            facets.iter().map(NormalizedFacet::from).collect();
        render_faceted_markdown(text, &normalized).unwrap_or_else(|_| text.to_string())
    } else {
        text.to_string()
    }
}
