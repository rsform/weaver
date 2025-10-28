use jacquard::client::{Agent, FileAuthStore};
use jacquard::oauth::client::OAuthClient;
use jacquard::oauth::loopback::LoopbackConfig;
use jacquard::prelude::XrpcClient;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::CowStr;
use jacquard_api::app_bsky::actor::get_profile::GetProfile;
use miette::{IntoDiagnostic, Result};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
    /// Run a test command with stored auth
    Run {
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
        Commands::Auth { handle, store } => {
            let store_path = store.unwrap_or_else(default_auth_store_path);
            authenticate(handle, store_path).await?;
        }
        Commands::Run { store } => {
            let store_path = store.unwrap_or_else(default_auth_store_path);
            run_test(store_path).await?;
        }
    }

    Ok(())
}

async fn authenticate(handle: String, store_path: PathBuf) -> Result<()> {
    println!("Authenticating with {}...", handle);

    let oauth = OAuthClient::with_default_config(FileAuthStore::new(&store_path));

    let session = oauth
        .login_with_local_server(handle, Default::default(), LoopbackConfig::default())
        .await
        .into_diagnostic()?;

    let (did, session_id) = session.session_info().await;

    // Save DID and session_id for later use
    let config_path = store_path.with_extension("toml");
    let config_content = format!("did = \"{}\"\nsession_id = \"{}\"\n", did, session_id);
    std::fs::write(&config_path, config_content).into_diagnostic()?;

    println!("Successfully authenticated!");
    println!("Session saved to: {}", store_path.display());
    println!("DID: {}", did);

    Ok(())
}

async fn run_test(store_path: PathBuf) -> Result<()> {
    println!("Loading session from {}...", store_path.display());

    // Read DID and session_id from config
    let config_path = store_path.with_extension("toml");
    let config_content = std::fs::read_to_string(&config_path)
        .into_diagnostic()
        .map_err(|_| miette::miette!("No auth config found. Run 'weaver auth' first."))?;

    let did_line = config_content
        .lines()
        .find(|l| l.starts_with("did = "))
        .ok_or_else(|| miette::miette!("Invalid config file"))?;
    let session_id_line = config_content
        .lines()
        .find(|l| l.starts_with("session_id = "))
        .ok_or_else(|| miette::miette!("Invalid config file"))?;

    let did_str = did_line
        .trim_start_matches("did = \"")
        .trim_end_matches('"');
    let session_id = session_id_line
        .trim_start_matches("session_id = \"")
        .trim_end_matches('"');

    let did = jacquard::types::string::Did::new(did_str)
        .map_err(|_| miette::miette!("Invalid DID in config"))?;

    let oauth = OAuthClient::with_default_config(FileAuthStore::new(&store_path));
    let session = oauth.restore(&did, session_id).await.into_diagnostic()?;

    let agent = Agent::from(session);

    println!("Fetching profile for {}...", did);

    let profile = agent
        .send(GetProfile::new().actor(AtIdentifier::Did(did)).build())
        .await
        .into_diagnostic()?
        .into_output()
        .into_diagnostic()?;

    println!("\nProfile:");
    println!("  Handle: {}", profile.value.handle);
    if let Some(display_name) = &profile.value.display_name {
        println!("  Display Name: {}", display_name);
    }
    if let Some(description) = &profile.value.description {
        println!("  Description: {}", description);
    }

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
