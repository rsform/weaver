use atrium_api::agent::Agent;
use atrium_api::xrpc::http::Uri;
use atrium_oauth::AuthorizeOptions;
use atrium_oauth::KnownScope;
use atrium_oauth::Scope;
use std::{
    error,
    io::{BufRead, Write, stdin, stdout},
    sync::Arc,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let client = weaver_common::oauth::default_oauth_client("http://127.0.0.1/callback")?;
    println!(
        "Authorization url: {}",
        client
            .authorize(
                std::env::var("PDS_URL").unwrap_or(String::from("https://atproto.systems")),
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

    print!("Redirected url: ");
    stdout().lock().flush()?;
    let mut url = String::new();
    stdin().lock().read_line(&mut url)?;

    let uri = url.trim().parse::<Uri>()?;
    let params = serde_html_form::from_str(uri.query().unwrap())?;
    let (session, _) = client.callback(params).await?;
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
    Ok(())
}
