use atrium_api::agent::Agent;
use atrium_oauth::AuthorizeOptions;
use atrium_oauth::CallbackParams;
use atrium_oauth::KnownScope;
use atrium_oauth::Scope;
use rouille::Server;
use std::error;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let (tx, mut rx) = mpsc::channel(5);
    let server = Server::new("0.0.0.0:4000", move |request| {
        create_callback_router(request, tx.clone())
    })
    .expect("Could not start server");
    let (server_handle, server_stop) = server.stoppable();
    let client = weaver_common::oauth::default_native_oauth_client()?;
    println!(
        "To authenticate with your PDS, visit:\r\n\t {}",
        client
            .authorize(
                std::env::var("HANDLE").unwrap_or(String::from("https://atproto.systems")),
                AuthorizeOptions {
                    scopes: vec![
                        Scope::Known(KnownScope::Atproto),
                        Scope::Known(KnownScope::TransitionGeneric)
                    ],
                    ..Default::default()
                }
            )
            .await?
    );

    let params = rx.recv().await.unwrap();
    let (session, _) = client.callback(params).await?;
    server_stop.send(()).expect("Failed to stop callbackserver");
    let agent = Agent::new(session);
    let output = agent
        .api
        .app
        .bsky
        .feed
        .get_timeline(
            atrium_api::app::bsky::feed::get_timeline::ParametersData {
                algorithm: None,
                cursor: None,
                limit: 3.try_into().ok(),
            }
            .into(),
        )
        .await?;
    for feed in &output.feed {
        println!("{feed:?}");
    }
    server_handle.join().unwrap();
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
