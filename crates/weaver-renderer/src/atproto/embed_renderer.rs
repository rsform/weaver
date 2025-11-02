//! Fetch and render AT Protocol records as HTML embeds
//!
//! This module provides functions to fetch records from PDSs and render them
//! as HTML strings suitable for embedding in markdown content.

use jacquard::{
    client::{Agent, AgentSession, AgentSessionExt, ClientError},
    prelude::IdentityResolver,
    types::string::{AtUri, Did, Nsid},
    xrpc::{self, Response, XrpcClient},
    http_client::HttpClient,
    Data,
};
use weaver_api::com_atproto::repo::get_record::{GetRecord, GetRecordResponse, GetRecordOutput};
use super::error::AtProtoPreprocessError;

/// Get a record without type validation, returning untyped Data
///
/// This is similar to jacquard's `get_record` but skips collection validation
/// and returns the raw Data value instead of a typed response.
async fn get_record_untyped<'a, A: AgentSession + IdentityResolver>(
    uri: &AtUri<'_>,
    agent: &Agent<A>,
) -> Result<Response<GetRecordResponse>, ClientError> {
    use jacquard::types::ident::AtIdentifier;

    // Extract collection and rkey from URI
    let collection = uri.collection().ok_or_else(|| {
        ClientError::invalid_request("AtUri missing collection")
            .with_help("ensure the URI includes a collection")
    })?;

    let rkey = uri.rkey().ok_or_else(|| {
        ClientError::invalid_request("AtUri missing rkey")
            .with_help("ensure the URI includes a record key after the collection")
    })?;

    // Resolve authority (DID or handle) to get DID and PDS
    let (repo_did, pds_url) = match uri.authority() {
        AtIdentifier::Did(did) => {
            let pds = agent.pds_for_did(did).await.map_err(|e| {
                ClientError::from(e)
                    .with_context("DID document resolution failed during record retrieval")
            })?;
            (did.clone(), pds)
        }
        AtIdentifier::Handle(handle) => agent.pds_for_handle(handle).await.map_err(|e| {
            ClientError::from(e)
                .with_context("handle resolution failed during record retrieval")
        })?,
    };

    // Make stateless XRPC call to that PDS (no auth required for public records)
    let request = GetRecord::new()
        .repo(AtIdentifier::Did(repo_did))
        .collection(collection.clone())
        .rkey(rkey.clone())
        .build();

    let http_request = xrpc::build_http_request(&pds_url, &request, &agent.opts().await)?;

    let http_response = agent
        .send_http(http_request)
        .await
        .map_err(|e| ClientError::transport(e))?;

    xrpc::process_response(http_response)
}

/// Fetch and render a profile record as HTML
///
/// Constructs the profile URI `at://did/app.bsky.actor.profile/self` and fetches it.
pub async fn fetch_and_render_profile<A: AgentSession + IdentityResolver>(
    did: &Did<'_>,
    agent: &Agent<A>,
) -> Result<String, AtProtoPreprocessError> {
    use weaver_api::app_bsky::actor::profile::Profile;

    // Construct profile URI: at://did/app.bsky.actor.profile/self
    let profile_uri = format!("at://{}/app.bsky.actor.profile/self", did.as_ref());

    // Fetch using typed collection
    let record_uri = Profile::uri(&profile_uri)
        .map_err(|e| AtProtoPreprocessError::InvalidUri(e.to_string()))?;

    let output = agent.fetch_record(&record_uri).await
        .map_err(|e| AtProtoPreprocessError::FetchFailed(e.to_string()))?;

    // Render profile to HTML
    render_profile(&output.value, did)
}

/// Fetch and render a Bluesky post as HTML
pub async fn fetch_and_render_post<A: AgentSession + IdentityResolver>(
    uri: &AtUri<'_>,
    agent: &Agent<A>,
) -> Result<String, AtProtoPreprocessError> {
    use weaver_api::app_bsky::feed::post::Post;

    // Fetch using typed collection
    let record_uri = Post::uri(uri.as_ref())
        .map_err(|e| AtProtoPreprocessError::InvalidUri(e.to_string()))?;

    let output = agent.fetch_record(&record_uri).await
        .map_err(|e| AtProtoPreprocessError::FetchFailed(e.to_string()))?;

    // Render post to HTML
    render_post(&output.value, uri)
}

/// Fetch and render an unknown record type generically
///
/// This fetches the record as untyped Data and probes for likely meaningful fields.
pub async fn fetch_and_render_generic<A: AgentSession + IdentityResolver>(
    uri: &AtUri<'_>,
    agent: &Agent<A>,
) -> Result<String, AtProtoPreprocessError> {
    // Use untyped fetch
    let response = get_record_untyped(uri, agent).await
        .map_err(|e| AtProtoPreprocessError::FetchFailed(e.to_string()))?;

    // Parse to get GetRecordOutput with Data value
    let output: GetRecordOutput = response.into_output()
        .map_err(|e| AtProtoPreprocessError::ParseFailed(e.to_string()))?;

    // Probe for meaningful fields
    render_generic_record(&output.value, uri)
}

/// Render a profile record as HTML
fn render_profile<'a>(
    profile: &weaver_api::app_bsky::actor::profile::Profile<'a>,
    did: &Did<'_>,
) -> Result<String, AtProtoPreprocessError> {
    let mut html = String::new();

    html.push_str("<div class=\"atproto-embed atproto-profile\">");

    if let Some(display_name) = &profile.display_name {
        html.push_str("<div class=\"profile-name\">");
        html.push_str(&html_escape(display_name.as_ref()));
        html.push_str("</div>");
    }

    html.push_str("<div class=\"profile-did\">");
    html.push_str(&html_escape(did.as_ref()));
    html.push_str("</div>");

    if let Some(description) = &profile.description {
        html.push_str("<div class=\"profile-description\">");
        html.push_str(&html_escape(description.as_ref()));
        html.push_str("</div>");
    }

    html.push_str("</div>");

    Ok(html)
}

/// Render a Bluesky post as HTML
fn render_post<'a>(
    post: &weaver_api::app_bsky::feed::post::Post<'a>,
    _uri: &AtUri<'_>,
) -> Result<String, AtProtoPreprocessError> {
    let mut html = String::new();

    html.push_str("<div class=\"atproto-embed atproto-post\">");

    html.push_str("<div class=\"post-text\">");
    html.push_str(&html_escape(post.text.as_ref()));
    html.push_str("</div>");

    html.push_str("<div class=\"post-meta\">");
    html.push_str("<time>");
    html.push_str(&html_escape(&post.created_at.to_string()));
    html.push_str("</time>");
    html.push_str("</div>");

    html.push_str("</div>");

    Ok(html)
}

/// Render a generic record by probing Data for meaningful fields
fn render_generic_record(
    data: &Data<'_>,
    uri: &AtUri<'_>,
) -> Result<String, AtProtoPreprocessError> {
    let mut html = String::new();

    html.push_str("<div class=\"atproto-embed atproto-record\">");

    // Try common field patterns
    if let Some(text) = data.query("/text").single().and_then(|d| d.as_str()) {
        html.push_str("<div class=\"record-text\">");
        html.push_str(&html_escape(text));
        html.push_str("</div>");
    }

    if let Some(name) = data.query("/name").single().and_then(|d| d.as_str()) {
        html.push_str("<div class=\"record-name\">");
        html.push_str(&html_escape(name));
        html.push_str("</div>");
    }

    if let Some(description) = data.query("/description").single().and_then(|d| d.as_str()) {
        html.push_str("<div class=\"record-description\">");
        html.push_str(&html_escape(description));
        html.push_str("</div>");
    }

    // Show record type
    if let Some(collection) = uri.collection() {
        html.push_str("<div class=\"record-type\">");
        html.push_str(&html_escape(collection.as_ref()));
        html.push_str("</div>");
    }

    html.push_str("</div>");

    Ok(html)
}

/// Simple HTML escaping
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
