use jacquard::client::{Agent, FileAuthStore};
use jacquard::identity::JacquardResolver;
use jacquard::oauth::client::{OAuthClient, OAuthSession};
use jacquard::oauth::loopback::LoopbackConfig;
use jacquard::prelude::XrpcClient;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::CowStr;
use jacquard_api::app_bsky::actor::get_profile::GetProfile;
use miette::{IntoDiagnostic, Result};
use std::path::PathBuf;
use weaver_renderer::static_site::StaticSiteWriter;

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

fn init_miette() {
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                .with_cause_chain()
                .with_syntax_highlighting(miette::highlighters::SyntectHighlighter::default())
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
