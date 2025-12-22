use jacquard::IntoStatic;
use jacquard::client::{Agent, FileAuthStore};
use jacquard::identity::JacquardResolver;
use jacquard::oauth::client::{OAuthClient, OAuthSession};
use jacquard::oauth::loopback::LoopbackConfig;
use jacquard::prelude::*;
use jacquard::types::string::Handle;
use miette::{IntoDiagnostic, Result};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;
use weaver_common::normalize_title_path;
use weaver_renderer::atproto::AtProtoPreprocessContext;
use weaver_renderer::static_site::StaticSiteWriter;
use weaver_renderer::utils::VaultBrokenLinkCallback;
use weaver_renderer::walker::{WalkOptions, vault_contents};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Weaver - Static site generator for AT Protocol notebooks", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Path to notebook directory
    source: Option<PathBuf>,

    /// Output directory for static site
    dest: Option<PathBuf>,

    /// Path to auth store file
    #[arg(long)]
    store: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with your atproto PDS using OAuth
    Auth {
        /// Handle (e.g., alice.bsky.social), DID, or PDS URL
        handle: String,

        /// Path to auth store file (will be created if missing)
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Publish notebook to AT Protocol
    Publish {
        /// Path to notebook directory
        source: PathBuf,

        /// Notebook title
        //#[arg(long)]
        title: String,

        /// Path to auth store file
        #[arg(long)]
        store: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_miette();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Auth { handle, store }) => {
            let store_path = store.unwrap_or_else(default_auth_store_path);
            authenticate(handle, store_path).await?;
        }
        Some(Commands::Publish {
            source,
            title,
            store,
        }) => {
            let store_path = store.unwrap_or_else(default_auth_store_path);
            publish_notebook(source, title, store_path).await?;
        }
        None => {
            // Render command (default)
            let source = cli.source.ok_or_else(|| {
                miette::miette!("Source directory required. Usage: weaver <source> <dest>")
            })?;
            let dest = cli.dest.ok_or_else(|| {
                miette::miette!("Destination directory required. Usage: weaver <source> <dest>")
            })?;
            let store_path = cli.store.unwrap_or_else(default_auth_store_path);

            render_notebook(source, dest, store_path).await?;
        }
    }

    Ok(())
}

async fn authenticate(handle: String, store_path: PathBuf) -> Result<()> {
    println!("Authenticating as @{handle} ...");

    let oauth = OAuthClient::with_default_config(FileAuthStore::new(&store_path));

    let session = oauth
        .login_with_local_server(handle, Default::default(), LoopbackConfig::default())
        .await
        .into_diagnostic()?;

    let (did, session_id) = session.session_info().await;

    // Save DID and session_id for later use
    let config_path = store_path.with_extension("kdl");
    let config_content = format!("did \"{}\"\nsession-id \"{}\"\n", did, session_id);
    std::fs::write(&config_path, config_content).into_diagnostic()?;

    println!("Successfully authenticated!");
    println!("Session saved to: {}", store_path.display());

    Ok(())
}

async fn try_load_session(
    store_path: &PathBuf,
) -> Option<OAuthSession<JacquardResolver, FileAuthStore>> {
    use kdl::KdlDocument;

    // Check if auth store exists
    if !store_path.exists() {
        return None;
    }

    // Read KDL config
    let config_path = store_path.with_extension("kdl");
    let config_content = std::fs::read_to_string(&config_path).ok()?;

    // Parse KDL
    let doc: KdlDocument = config_content.parse().ok()?;

    // Extract did and session-id
    let did_node = doc.get("did")?;
    let session_id_node = doc.get("session-id")?;

    let did_str = did_node.entries().first()?.value().as_string()?;
    let session_id = session_id_node.entries().first()?.value().as_string()?;

    // Parse DID
    let did = jacquard::types::string::Did::new(did_str).ok()?;

    // Restore OAuth session
    let oauth = OAuthClient::with_default_config(FileAuthStore::new(store_path));
    oauth.restore(&did, session_id).await.ok()
}

async fn render_notebook(source: PathBuf, dest: PathBuf, store_path: PathBuf) -> Result<()> {
    // Validate source exists
    if !source.exists() {
        return Err(miette::miette!(
            "Source directory not found: {}",
            source.display()
        ));
    }

    // Try to load session
    let session = try_load_session(&store_path).await;

    // Log auth status
    if session.is_some() {
        println!("✓ Found authentication");
    } else {
        println!("⚠ No authentication found");
        println!("  Run 'weaver auth <handle>' to enable network features");
    }

    // Create dest parent directories if needed
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).into_diagnostic()?;
        }
    }

    // Create renderer
    let writer = StaticSiteWriter::new(source, dest.clone(), session);

    // Render
    println!("→ Rendering notebook...");
    let start = std::time::Instant::now();
    writer.run().await?;
    let elapsed = start.elapsed();

    // Report success
    println!("✓ Rendered in {:.2}s", elapsed.as_secs_f64());
    println!("✓ Output: {}", dest.display());

    Ok(())
}

fn default_auth_store_path() -> PathBuf {
    dirs::config_dir()
        .expect("Could not determine config directory")
        .join("weaver")
        .join("auth.json")
}

async fn publish_notebook(source: PathBuf, title: String, store_path: PathBuf) -> Result<()> {
    // Initialize tracing for debugging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .init();

    println!("Publishing notebook from: {}", source.display());
    println!("Title: {}", title);

    // Validate source exists
    if !source.exists() {
        return Err(miette::miette!(
            "Source directory not found: {}",
            source.display()
        ));
    }

    // Try to load session, trigger auth if needed
    let session = match try_load_session(&store_path).await {
        Some(session) => {
            println!("✓ Authenticated");
            session
        }
        None => {
            println!("⚠ No authentication found");
            println!("Please enter your handle to authenticate:");

            let mut handle = String::new();
            let stdin = std::io::stdin();
            stdin.lock().read_line(&mut handle).into_diagnostic()?;
            let handle = handle.trim().to_string();

            authenticate(handle, store_path.clone()).await?;

            // Load the session we just created
            try_load_session(&store_path)
                .await
                .ok_or_else(|| miette::miette!("Failed to load session after authentication"))?
        }
    };

    // Get user DID and handle from session

    // Create agent and resolve DID document to get handle
    let agent = Agent::new(session);
    let (did, _session_id) = agent
        .info()
        .await
        .ok_or_else(|| miette::miette!("No session info available"))?;
    let did_doc_response = agent.resolve_did_doc(&did).await?;
    let did_doc = did_doc_response.parse()?;

    // Extract handle from alsoKnownAs
    let aka_vec = did_doc
        .also_known_as
        .ok_or_else(|| miette::miette!("No alsoKnownAs in DID document"))?;
    let handle_str = aka_vec
        .get(0)
        .and_then(|aka| aka.as_ref().strip_prefix("at://"))
        .ok_or_else(|| miette::miette!("No handle found in DID document"))?;
    let handle = Handle::new(handle_str)?;

    println!("Publishing as @{}", handle.as_ref());

    // Walk vault directory
    println!("→ Scanning vault...");
    tracing::debug!("Scanning directory: {}", source.display());
    let contents = vault_contents(&source, WalkOptions::new())?;

    // Convert to Arc first
    let agent = Arc::new(agent);
    let vault_arc: Arc<[PathBuf]> = contents.into();

    // Filter markdown files after converting to Arc
    let md_files: Vec<PathBuf> = vault_arc
        .iter()
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext == "md" || ext == "markdown")
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    println!("Found {} markdown files", md_files.len());

    // Create preprocessing context
    let context = AtProtoPreprocessContext::new(vault_arc.clone(), title.clone(), agent.clone())
        .with_creator(did.clone().into_static(), handle.clone().into_static());

    // Process each file
    for file_path in &md_files {
        let _span = tracing::info_span!("process_file", path = %file_path.display()).entered();
        println!("Processing: {}", file_path.display());

        // Read file content
        let contents = tokio::fs::read_to_string(&file_path)
            .await
            .into_diagnostic()?;

        // Clone context for this file
        let mut file_context = context.clone();
        file_context.set_current_path(file_path.clone());
        let callback = Some(VaultBrokenLinkCallback {
            vault_contents: vault_arc.clone(),
        });

        // Parse markdown
        use markdown_weaver::Parser;
        use weaver_renderer::default_md_options;
        let parser =
            Parser::new_with_broken_link_callback(&contents, default_md_options(), callback)
                .into_offset_iter();
        let iterator = weaver_renderer::ContextIterator::default(parser);

        // Process through NotebookProcessor
        use n0_future::StreamExt;
        use weaver_renderer::{NotebookContext, NotebookProcessor};
        let mut processor = NotebookProcessor::new(file_context.clone(), iterator);

        // Write canonical markdown with MarkdownWriter
        use markdown_weaver_escape::FmtWriter;
        use weaver_renderer::atproto::MarkdownWriter;
        let mut output = String::new();
        let mut md_writer = MarkdownWriter::new(FmtWriter(&mut output));

        // Process all events
        while let Some((event, _)) = processor.next().await {
            md_writer
                .write_event(event)
                .map_err(|e| miette::miette!("Failed to write markdown: {:?}", e))?;
        }

        // Extract blobs and entry metadata
        let blobs = file_context.blobs();
        let entry_title = file_context.entry_title();

        if !blobs.is_empty() {
            tracing::debug!("Uploaded {} image(s)", blobs.len());
        }

        // Build Entry record with blobs
        use jacquard::types::blob::BlobRef;
        use jacquard::types::string::Datetime;
        use weaver_api::sh_weaver::embed::images::{Image, Images};
        use weaver_api::sh_weaver::notebook::entry::{Entry, EntryEmbeds};

        let embeds = if !blobs.is_empty() {
            // Build images from blobs
            let images: Vec<Image> = blobs
                .iter()
                .map(|blob_info| {
                    Image::new()
                        .image(BlobRef::Blob(blob_info.blob.clone()))
                        .alt(blob_info.alt.as_ref().map(|a| a.as_ref()).unwrap_or(""))
                        .maybe_name(Some(blob_info.name.as_str().into()))
                        .build()
                })
                .collect();

            Some(EntryEmbeds {
                images: Some(Images::new().images(images).build()),
                externals: None,
                records: None,
                records_with_media: None,
                videos: None,
                extra_data: None,
            })
        } else {
            None
        };

        let entry = Entry::new()
            .content(output.as_str())
            .title(entry_title.as_ref())
            .path(normalize_title_path(entry_title.as_ref()))
            .created_at(Datetime::now())
            .maybe_embeds(embeds)
            .build();

        // Use WeaverExt to upsert entry (handles notebook + entry creation/updates)
        use jacquard::http_client::HttpClient;
        use weaver_common::WeaverExt;
        let (entry_ref, _, was_created) = agent
            .upsert_entry(&title, entry_title.as_ref(), entry, None)
            .await?;

        if was_created {
            println!("  ✓ Created new entry: {}", entry_ref.uri.as_ref());
        } else {
            println!("  ✓ Updated existing entry: {}", entry_ref.uri.as_ref());
        }
    }

    println!("✓ Published {} entries", md_files.len());

    Ok(())
}

fn init_miette() {
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                .with_cause_chain()
                .color(true)
                .context_lines(5)
                .tab_width(2)
                .break_words(true)
                .build(),
        )
    }))
    .expect("couldn't set the miette hook");
    miette::set_panic_hook();
}
