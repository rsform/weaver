//! Static renderer
//!
//! This mode of the renderer creates a static html and css website from a notebook in a local directory.
//! It does not upload it to the PDS by default (though it can ). This is good for testing and for self-hosting.
//! URLs in the notebook are mostly unaltered. It is compatible with GitHub or Cloudflare Pages
//! and other similar static hosting services.

pub mod context;
pub mod document;
pub mod writer;

use crate::{
    ContextIterator, NotebookProcessor,
    static_site::{
        context::StaticSiteContext,
        document::{CssMode, write_document_footer, write_document_head},
        writer::StaticPageWriter,
    },
    utils::flatten_dir_to_just_one_parent,
    walker::{WalkOptions, vault_contents},
};
use bitflags::bitflags;
use markdown_weaver::{BrokenLink, CowStr, Parser};
use markdown_weaver_escape::FmtWriter;
use miette::IntoDiagnostic;
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use n0_future::io::AsyncWriteExt;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
use tokio::io::AsyncWriteExt;
use unicode_normalization::UnicodeNormalization;
use weaver_common::jacquard::{client::AgentSession, prelude::*};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct StaticSiteOptions:u32 {
        const FLATTEN_STRUCTURE = 1 << 1;
        const UPLOAD_BLOBS = 1 << 2;
        const INLINE_EMBEDS = 1 << 3;
        const ADD_LINK_PREVIEWS = 1 << 4;
        const RESOLVE_AT_IDENTIFIERS = 1 << 5;
        const RESOLVE_AT_URIS = 1 << 6;
        const ADD_BSKY_COMMENTS_EMBED = 1 << 7;
        const CREATE_INDEX = 1 << 8;
        const CREATE_CHAPTERS_BY_DIRECTORY = 1 << 9;
        const CREATE_PAGES_BY_TITLE = 1 << 10;
        const NORMALIZE_DIR_NAMES = 1 << 11;
        const ADD_TOC_TO_PAGES = 1 << 12;
    }
}

impl Default for StaticSiteOptions {
    fn default() -> Self {
        Self::FLATTEN_STRUCTURE
            //| Self::UPLOAD_BLOBS
            | Self::RESOLVE_AT_IDENTIFIERS
            | Self::RESOLVE_AT_URIS
            | Self::CREATE_INDEX
            | Self::CREATE_CHAPTERS_BY_DIRECTORY
            | Self::CREATE_PAGES_BY_TITLE
            | Self::NORMALIZE_DIR_NAMES
    }
}

pub fn default_md_options() -> markdown_weaver::Options {
    markdown_weaver::Options::ENABLE_WIKILINKS
        | markdown_weaver::Options::ENABLE_FOOTNOTES
        | markdown_weaver::Options::ENABLE_TABLES
        | markdown_weaver::Options::ENABLE_GFM
        | markdown_weaver::Options::ENABLE_STRIKETHROUGH
        | markdown_weaver::Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | markdown_weaver::Options::ENABLE_OBSIDIAN_EMBEDS
        | markdown_weaver::Options::ENABLE_MATH
        | markdown_weaver::Options::ENABLE_HEADING_ATTRIBUTES
}

pub struct StaticSiteWriter<A>
where
    A: AgentSession,
{
    context: StaticSiteContext<A>,
}

impl<A> StaticSiteWriter<A>
where
    A: AgentSession,
{
    pub fn new(root: PathBuf, destination: PathBuf, session: Option<A>) -> Self {
        let context = StaticSiteContext::new(root, destination, session);
        Self { context }
    }
}

impl<A> StaticSiteWriter<A>
where
    A: AgentSession + IdentityResolver + 'static,
{
    pub async fn run(mut self) -> Result<(), miette::Report> {
        if !self.context.root.exists() {
            return Err(miette::miette!(
                "The path specified ({}) does not exist",
                self.context.root.display()
            ));
        }
        let contents = vault_contents(&self.context.root, WalkOptions::new())?;

        self.context.dir_contents = Some(contents.into());

        if self.context.root.is_file() || self.context.start_at.is_file() {
            let source_filename = self
                .context
                .start_at
                .file_name()
                .expect("wtf how!?")
                .to_string_lossy();

            let dest = if self.context.destination.is_dir() {
                self.context.destination.join(String::from(source_filename))
            } else {
                let parent = self
                    .context
                    .destination
                    .parent()
                    .unwrap_or(&self.context.destination);
                // Avoid recursively creating self.destination through the call to
                // export_note when the parent directory doesn't exist.
                if !parent.exists() {
                    return Err(miette::miette!(
                        "Destination parent path ({}) does not exist, SOMEHOW",
                        parent.display()
                    ));
                }
                self.context.destination.clone()
            };
            // Use standalone writer for single file (inline CSS)
            return write_page_standalone(self.context.clone(), &self.context.start_at, dest).await;
        }

        if !self.context.destination.exists() {
            return Err(miette::miette!(
                "The destination path specified ({}) does not exist",
                self.context.destination.display()
            ));
        }

        // Generate CSS files for multi-file mode
        self.generate_css_files().await?;

        let mut writers =
            Vec::with_capacity(self.context.dir_contents.clone().unwrap_or_default().len());

        for file in self
            .context
            .dir_contents
            .as_ref()
            .unwrap()
            .clone()
            .into_iter()
            .filter(|file| file.starts_with(&self.context.start_at))
        {
            let context = self.context.clone();
            let file = file.clone();
            // we'll see if this is a problem or not
            writers.push(n0_future::task::spawn(async move {
                let relative_path = file
                    .strip_prefix(context.start_at.clone())
                    .expect("file should always be nested under root")
                    .to_path_buf();

                // Check if this is a markdown file
                let is_markdown = file.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext == "md" || ext == "markdown")
                    .unwrap_or(false);

                if !is_markdown {
                    // Copy non-markdown files directly
                    let output_path = if context
                        .options
                        .contains(StaticSiteOptions::FLATTEN_STRUCTURE)
                    {
                        let path_str = relative_path.to_string_lossy();
                        let (parent, fname) = flatten_dir_to_just_one_parent(&path_str);
                        let parent = if parent.is_empty() { "entry" } else { parent };
                        context
                            .destination
                            .join(String::from(parent))
                            .join(String::from(fname))
                    } else {
                        context.destination.join(relative_path.clone())
                    };

                    // Create parent directory if needed
                    if let Some(parent) = output_path.parent() {
                        tokio::fs::create_dir_all(parent).await.into_diagnostic()?;
                    }

                    tokio::fs::copy(&file, &output_path).await.into_diagnostic()?;
                    return Ok(());
                }

                // Process markdown files
                // Check if this is the designated index file
                if let Some(index) = &context.index_file {
                    if &relative_path == index {
                        let output_path = context.destination.join("index.html");
                        return write_page(context.clone(), file, output_path).await;
                    }
                }

                if context
                    .options
                    .contains(StaticSiteOptions::FLATTEN_STRUCTURE)
                {
                    let path_str = relative_path.to_string_lossy();
                    let (parent, fname) = flatten_dir_to_just_one_parent(&path_str);
                    let parent = if parent.is_empty() { "entry" } else { parent };
                    let output_path = context
                        .destination
                        .join(String::from(parent))
                        .join(String::from(fname));

                    write_page(context.clone(), file.clone(), output_path).await?;
                } else {
                    let output_path = context.destination.join(relative_path.clone());

                    write_page(context.clone(), file.clone(), output_path).await?;
                }
                Ok::<(), miette::Report>(())
            }));
        }

        // def want to scope these so we wait until they all complete before we return
        // and then we def want the errors, or at least the first error
        for fut in n0_future::join_all(writers).await.into_iter() {
            fut.into_diagnostic()??;
        }

        // Generate default index if requested and no custom index specified
        if self
            .context
            .options
            .contains(StaticSiteOptions::CREATE_INDEX)
            && self.context.index_file.is_none()
        {
            self.generate_default_index().await?;
        }

        Ok(())
    }

    async fn generate_css_files(&self) -> Result<(), miette::Report> {
        use crate::css::{generate_base_css, generate_syntax_css};
        use crate::theme::Theme;

        let css_dir = self.context.destination.join("css");
        tokio::fs::create_dir_all(&css_dir)
            .await
            .into_diagnostic()?;

        let default_theme = Theme::default();
        let theme = self.context.theme.as_deref().unwrap_or(&default_theme);

        // Write base.css
        let base_css = generate_base_css(theme);
        tokio::fs::write(css_dir.join("base.css"), base_css)
            .await
            .into_diagnostic()?;

        // Write syntax.css
        let syntax_css = generate_syntax_css(theme, &self.context.syntax_set)?;
        tokio::fs::write(css_dir.join("syntax.css"), syntax_css)
            .await
            .into_diagnostic()?;

        Ok(())
    }

    async fn generate_default_index(&self) -> Result<(), miette::Report> {
        let index_path = self.context.destination.join("index.html");
        let mut index_file = crate::utils::create_file(&index_path).await?;

        // Write head
        write_document_head(&self.context, &mut index_file, CssMode::Linked, &index_path).await?;

        // Write title and list
        index_file
            .write_all(b"<h1>Index</h1>\n<ul>\n")
            .await
            .into_diagnostic()?;

        // List all files
        if let Some(contents) = &self.context.dir_contents {
            for file in contents.iter() {
                if let Ok(relative) = file.strip_prefix(&self.context.start_at) {
                    let display_name = relative.to_string_lossy();
                    let link = if self
                        .context
                        .options
                        .contains(StaticSiteOptions::FLATTEN_STRUCTURE)
                    {
                        let (parent, fname) = flatten_dir_to_just_one_parent(&display_name);
                        // Change extension to .html
                        let fname_html =
                            PathBuf::from(fname.as_ref() as &str).with_extension("html");
                        let fname_html_str = fname_html.to_string_lossy();
                        if !parent.is_empty() {
                            format!("./{}/{}", parent, fname_html_str)
                        } else {
                            format!("./entry/{}", fname_html_str)
                        }
                    } else {
                        // Change extension to .html
                        let html_path =
                            PathBuf::from(display_name.as_ref() as &str).with_extension("html");
                        format!("./{}", html_path.to_string_lossy())
                    };

                    index_file
                        .write_all(
                            format!("  <li><a href=\"{}\">{}</a></li>\n", link, display_name)
                                .as_bytes(),
                        )
                        .await
                        .into_diagnostic()?;
                }
            }
        }

        index_file.write_all(b"</ul>\n").await.into_diagnostic()?;

        // Write footer
        write_document_footer(&mut index_file).await?;

        Ok(())
    }
}

pub async fn export_page<'input, A>(
    contents: &'input str,
    context: StaticSiteContext<A>,
) -> Result<String, miette::Report>
where
    A: AgentSession + IdentityResolver,
{
    let callback = if let Some(dir_contents) = context.dir_contents.clone() {
        Some(VaultBrokenLinkCallback {
            vault_contents: dir_contents,
        })
    } else {
        None
    };
    let parser = Parser::new_with_broken_link_callback(&contents, context.md_options, callback);
    let iterator = ContextIterator::default(parser);
    let mut output = String::new();
    let writer = StaticPageWriter::new(
        NotebookProcessor::new(context, iterator),
        FmtWriter(&mut output),
    );
    writer.run().await.into_diagnostic()?;
    Ok(output)
}

pub async fn write_page<A>(
    context: StaticSiteContext<A>,
    input_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<(), miette::Report>
where
    A: AgentSession + IdentityResolver,
{
    let contents = tokio::fs::read_to_string(&input_path)
        .await
        .into_diagnostic()?;

    // Change extension to .html
    let output_path = output_path.as_ref().with_extension("html");
    let mut output_file = crate::utils::create_file(&output_path).await?;
    let context = context.clone_with_path(input_path);

    // Write document head
    write_document_head(&context, &mut output_file, CssMode::Linked, &output_path).await?;

    // Write body content
    let output = export_page(&contents, context).await?;
    output_file
        .write_all(output.as_bytes())
        .await
        .into_diagnostic()?;

    // Write document footer
    write_document_footer(&mut output_file).await?;

    Ok(())
}

pub async fn write_page_standalone<A>(
    context: StaticSiteContext<A>,
    input_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<(), miette::Report>
where
    A: AgentSession + IdentityResolver,
{
    let contents = tokio::fs::read_to_string(&input_path)
        .await
        .into_diagnostic()?;

    // Change extension to .html
    let output_path = output_path.as_ref().with_extension("html");
    let mut output_file = crate::utils::create_file(&output_path).await?;
    let context = context.clone_with_path(input_path);

    // Write document head with inline CSS
    write_document_head(&context, &mut output_file, CssMode::Inline, &output_path).await?;

    // Write body content
    let output = export_page(&contents, context).await?;
    output_file
        .write_all(output.as_bytes())
        .await
        .into_diagnostic()?;

    // Write document footer
    write_document_footer(&mut output_file).await?;

    Ok(())
}

/// Path lookup in an Obsidian vault
///
/// Credit to https://github.com/zoni
///
/// Taken from https://github.com/zoni/obsidian-export/blob/main/src/lib.rs.rs on 2025-05-21
///
pub fn lookup_filename_in_vault<'a>(
    filename: &str,
    vault_contents: &'a [PathBuf],
) -> Option<&'a PathBuf> {
    let filename = PathBuf::from(filename);
    let filename_normalized: String = filename.to_string_lossy().nfc().collect();

    vault_contents.iter().find(|path| {
        let path_normalized_str: String = path.to_string_lossy().nfc().collect();
        let path_normalized = PathBuf::from(&path_normalized_str);
        let path_normalized_lowered = PathBuf::from(&path_normalized_str.to_lowercase());

        // It would be convenient if we could just do `filename.set_extension("md")` at the start
        // of this funtion so we don't need multiple separate + ".md" match cases here, however
        // that would break with a reference of `[[Note.1]]` linking to `[[Note.1.md]]`.

        path_normalized.ends_with(&filename_normalized)
            || path_normalized.ends_with(filename_normalized.clone() + ".md")
            || path_normalized_lowered.ends_with(filename_normalized.to_lowercase())
            || path_normalized_lowered.ends_with(filename_normalized.to_lowercase() + ".md")
    })
}

pub struct VaultBrokenLinkCallback {
    vault_contents: Arc<[PathBuf]>,
}

impl<'input> markdown_weaver::BrokenLinkCallback<'input> for VaultBrokenLinkCallback {
    fn handle_broken_link(
        &mut self,
        link: BrokenLink<'input>,
    ) -> Option<(CowStr<'input>, CowStr<'input>)> {
        let text = link.reference;
        let captures = crate::OBSIDIAN_NOTE_LINK_RE
            .captures(&text)
            .expect("note link regex didn't match - bad input?");
        let file = captures.name("file").map(|v| v.as_str().trim());
        let label = captures.name("label").map(|v| v.as_str());
        let section = captures.name("section").map(|v| v.as_str().trim());

        if let Some(file) = file {
            if let Some(path) = lookup_filename_in_vault(file, self.vault_contents.as_ref()) {
                let mut link_text = String::from(path.to_string_lossy());
                if let Some(section) = section {
                    link_text.push('#');
                    link_text.push_str(section);
                    if let Some(label) = label {
                        let label = label.to_string();
                        Some((CowStr::from(link_text), CowStr::from(label)))
                    } else {
                        Some((link_text.into(), format!("{} > {}", file, section).into()))
                    }
                } else {
                    Some((link_text.into(), format!("{}", file).into()))
                }
            } else {
                None
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::NotebookContext;

    use super::*;
    use std::path::PathBuf;
    use weaver_common::jacquard::client::{
        AtpSession, MemorySessionStore,
        credential_session::{CredentialSession, SessionKey},
    };

    /// Type alias for the session used in tests
    type TestSession = CredentialSession<
        MemorySessionStore<SessionKey, AtpSession>,
        weaver_common::jacquard::identity::JacquardResolver,
    >;

    /// Helper: Create test context without network capabilities
    fn test_context() -> StaticSiteContext<TestSession> {
        let root = PathBuf::from("/tmp/test");
        let destination = PathBuf::from("/tmp/output");
        let mut ctx = StaticSiteContext::new(root, destination, None);
        ctx.client = None; // Explicitly disable network
        ctx
    }

    /// Helper: Render markdown to HTML using test context
    async fn render_markdown(input: &str) -> String {
        let context = test_context();
        export_page(input, context).await.unwrap()
    }

    #[tokio::test]
    async fn test_smoke() {
        let output = render_markdown("Hello world").await;
        assert!(output.contains("Hello world"));
    }

    #[tokio::test]
    async fn test_paragraph_rendering() {
        let input = "This is a paragraph.\n\nThis is another paragraph.";
        let output = render_markdown(input).await;
        insta::assert_snapshot!(output);
    }

    #[tokio::test]
    async fn test_heading_rendering() {
        let input = "# Heading 1\n\n## Heading 2\n\n### Heading 3";
        let output = render_markdown(input).await;
        insta::assert_snapshot!(output);
    }

    #[tokio::test]
    async fn test_list_rendering() {
        let input = "- Item 1\n- Item 2\n  - Nested\n\n1. Ordered 1\n2. Ordered 2";
        let output = render_markdown(input).await;
        insta::assert_snapshot!(output);
    }

    #[tokio::test]
    async fn test_code_block_rendering() {
        let input = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```";
        let output = render_markdown(input).await;
        insta::assert_snapshot!(output);
    }

    #[tokio::test]
    async fn test_table_rendering() {
        let input = "| Left | Center | Right |\n|:-----|:------:|------:|\n| A | B | C |";
        let output = render_markdown(input).await;
        insta::assert_snapshot!(output);
    }

    #[tokio::test]
    async fn test_blockquote_rendering() {
        let input = "> This is a quote\n>\n> With multiple lines";
        let output = render_markdown(input).await;
        insta::assert_snapshot!(output);
    }

    #[tokio::test]
    async fn test_math_rendering() {
        let input = "Inline $x^2$ and display:\n\n$$\ny = mx + b\n$$";
        let output = render_markdown(input).await;
        insta::assert_snapshot!(output);
    }

    #[tokio::test]
    async fn test_wikilink_resolution() {
        let vault_contents = vec![
            PathBuf::from("notes/First Note.md"),
            PathBuf::from("notes/Second Note.md"),
        ];

        let mut context = test_context();
        context.dir_contents = Some(vault_contents.into());

        let input = "[[First Note]] and [[Second Note]]";
        let output = export_page(input, context).await.unwrap();
        println!("{output}");
        assert!(output.contains("./First%20Note"));
        assert!(output.contains("./Second%20Note"));
    }

    #[tokio::test]
    async fn test_broken_wikilink() {
        let vault_contents = vec![PathBuf::from("notes/Exists.md")];

        let mut context = test_context();
        context.dir_contents = Some(vault_contents.into());

        let input = "[[Does Not Exist]]";
        let output = export_page(input, context).await.unwrap();

        // Broken wikilinks become links (they just don't point anywhere valid)
        // This is acceptable - static site will show 404 on click
        assert!(output.contains("<a href="));
        assert!(output.contains("Does Not Exist</a>") || output.contains("Does%20Not%20Exist"));
    }

    #[tokio::test]
    async fn test_wikilink_with_section() {
        let vault_contents = vec![PathBuf::from("Note.md")];

        let mut context = test_context();
        context.dir_contents = Some(vault_contents.into());

        let input = "[[Note#Section]]";
        let output = export_page(input, context).await.unwrap();
        println!("{output}");
        assert!(output.contains("Note#Section"));
    }

    #[tokio::test]
    async fn test_link_flattening_enabled() {
        let mut context = test_context();
        context.options = StaticSiteOptions::FLATTEN_STRUCTURE;

        let input = "[Link](path/to/nested/file.md)";
        let output = export_page(input, context).await.unwrap();
        println!("{output}");
        // Should flatten to single parent directory
        assert!(output.contains("./entry/file.md"));
    }

    #[tokio::test]
    async fn test_link_flattening_disabled() {
        let mut context = test_context();
        context.options = StaticSiteOptions::empty();

        let input = "[Link](path/to/nested/file.md)";
        let output = export_page(input, context).await.unwrap();
        println!("{output}");
        // Should preserve original path
        assert!(output.contains("path/to/nested/file.md"));
    }

    #[tokio::test]
    async fn test_frontmatter_parsing() {
        let input = "---\ntitle: Test Page\nauthor: Test Author\n---\n\nContent here";
        let context = test_context();
        let output = export_page(input, context.clone()).await.unwrap();

        // Frontmatter should be parsed but not rendered
        assert!(!output.contains("title: Test Page"));
        assert!(output.contains("Content here"));

        // Verify frontmatter was captured
        let frontmatter = context.frontmatter();
        let yaml = frontmatter.contents();
        let yaml_guard = yaml.read().unwrap();
        assert!(yaml_guard.len() > 0);
    }

    #[tokio::test]
    async fn test_empty_frontmatter() {
        let input = "---\n---\n\nContent";
        let output = render_markdown(input).await;

        assert!(output.contains("Content"));
        assert!(!output.contains("---"));
    }

    #[tokio::test]
    async fn test_empty_input() {
        let output = render_markdown("").await;
        assert_eq!(output, "");
    }

    #[tokio::test]
    async fn test_html_and_special_characters() {
        // Test that markdown correctly handles HTML and special chars per CommonMark spec
        let input = "Text with <special> & some text. Valid tags: <em>emphasis</em> and <strong>bold</strong>";
        let output = render_markdown(input).await;

        // & must be escaped for valid HTML
        assert!(output.contains("&amp;"));

        // Inline HTML tags pass through (CommonMark behavior)
        assert!(output.contains("<special>"));
        assert!(output.contains("<em>emphasis</em>"));
        assert!(output.contains("<strong>bold</strong>"));
    }

    #[tokio::test]
    async fn test_unicode_content() {
        let input = "Unicode: 你好 🎉 café";
        let output = render_markdown(input).await;

        assert!(output.contains("你好"));
        assert!(output.contains("🎉"));
        assert!(output.contains("café"));
    }
}
