use atrium_api::types::string::Did;
use atrium_oauth::AuthorizeOptions;
use atrium_oauth::CallbackParams;
use atrium_oauth::KnownScope;
use atrium_oauth::Scope;
use miette::miette;
use miette::{IntoDiagnostic, Result};
use rouille::Server;
use std::path::Path;
use std::path::PathBuf;
use tokio::sync::mpsc;
use weaver_common::agent::WeaverAgent;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with your atproto PDS, using your handle or the URL of the PDS. You can also set the HANDLE environment variable.
    Auth(AuthArgs),
    /// Test the native oauth client
    Run,
}

#[derive(Args)]
struct AuthArgs {
    handle: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_miette();
    let args = Cli::parse();
    let base_dir = weaver_common::filestore::config_dir();
    tokio::fs::create_dir_all(&base_dir)
        .await
        .into_diagnostic()?;
    let client = weaver_common::oauth::test_native_oauth_client().into_diagnostic()?;
    let (session, server_handle) = if let Some(Commands::Auth(AuthArgs { handle })) = args.command {
        let handle = handle.unwrap_or(String::from("https://atproto.systems"));
        let (tx, mut rx) = mpsc::channel(5);
        let server = Server::new("0.0.0.0:4000", move |request| {
            create_callback_router(request, tx.clone())
        })
        .expect("Could not start server");
        let (server_handle, server_stop) = server.stoppable();

        println!(
            "To authenticate with your PDS, visit:\r\n\t {}",
            client
                .authorize(
                    handle,
                    AuthorizeOptions {
                        scopes: vec![
                            Scope::Known(KnownScope::Atproto),
                            Scope::Known(KnownScope::TransitionGeneric)
                        ],
                        ..Default::default()
                    }
                )
                .await
                .into_diagnostic()?
        );

        let params = rx.recv().await.unwrap();
        let (session, _) = client.callback(params).await.into_diagnostic()?;
        server_stop
            .send(())
            .expect("Failed to stop callback server");
        Ok::<_, weaver_common::Error>((session, Some(server_handle)))
    } else {
        let did = find_session_did(base_dir).await?;
        let session = client.restore(&did).await.into_diagnostic()?;
        Ok((session, None))
    }?;

    let agent = WeaverAgent::new(session);
    let output = agent
        .get_profile_pds(agent.did().await.unwrap().into())
        .await?;
    println!("{output:?}");
    if let Some(server_handle) = server_handle {
        server_handle.join().unwrap();
    }
    Ok(())
}

pub fn create_callback_router(
    request: &rouille::Request,
    tx: mpsc::Sender<CallbackParams>,
) -> rouille::Response {
    rouille::router!(request,
            (GET) (/oauth/callback) => {
                let state = request.get_param("state").unwrap();
                let code = request.get_param("code").unwrap();
                let iss = request.get_param("iss").unwrap();
                let callback_params = CallbackParams {
                    state: Some(state),
                    code,
                    iss: Some(iss),
                };
                tx.try_send(callback_params).unwrap();
                rouille::Response::text("Logged in!")
            },
            _ => rouille::Response::empty_404()
    )
}

pub async fn find_session_did(path: impl AsRef<Path>) -> Result<Did> {
    let path = path.as_ref();
    let mut dir = tokio::fs::read_dir(path).await.into_diagnostic()?;
    while let Some(entry) = dir.next_entry().await.into_diagnostic()? {
        let file_name = entry.file_name();
        if let Some(file_name) = file_name.to_str() {
            if file_name.ends_with("_session.json") {
                let did_string = file_name
                    .strip_suffix("_session.json")
                    .expect("we JUST checked the suffix lol")
                    .to_string();
                let did = Did::new(did_string).expect("we should only be writing valid dids");
                return Ok(did);
            }
        }
    }
    Err(miette!("couldn't find any existing sessions!"))
}

fn init_miette() {
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                //.rgb_colors(miette::RgbColors::)
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
