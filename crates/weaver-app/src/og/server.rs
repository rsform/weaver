#[cfg(all(feature = "fullstack-server", feature = "server"))]
use crate::fetch;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use crate::og;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use axum::Extension;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use dioxus::prelude::*;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use jacquard::smol_str::SmolStr;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use jacquard::types::string::AtIdentifier;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use std::sync::Arc;

#[cfg(all(feature = "fullstack-server", feature = "server"))]
use jacquard::smol_str::ToSmolStr;

// Route: /og/{ident}/{book_title}/{entry_title} - OpenGraph image for entry
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/og/{ident}/{book_title}/{entry_title}", fetcher: Extension<Arc<fetch::Fetcher>>)]
pub async fn og_image(
    ident: SmolStr,
    book_title: SmolStr,
    entry_title: SmolStr,
) -> Result<axum::response::Response> {
    use axum::{
        http::{
            StatusCode,
            header::{CACHE_CONTROL, CONTENT_TYPE},
        },
        response::IntoResponse,
    };
    use weaver_api::sh_weaver::actor::ProfileDataViewInner;
    use weaver_api::sh_weaver::notebook::Title;

    // Strip .png extension if present
    let entry_title = entry_title.strip_suffix(".png").unwrap_or(&entry_title);

    let Ok(at_ident) = AtIdentifier::new_owned(ident.clone()) else {
        return Ok((StatusCode::BAD_REQUEST, "Invalid identifier").into_response());
    };

    // Fetch entry data
    let entry_result = fetcher
        .get_entry(at_ident.clone(), book_title.clone(), entry_title.into())
        .await;

    let arc_data = match entry_result {
        Ok(Some(data)) => data,
        Ok(None) => return Ok((StatusCode::NOT_FOUND, "Entry not found").into_response()),
        Err(e) => {
            tracing::error!("Failed to fetch entry for OG image: {:?}", e);
            return Ok((StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch entry").into_response());
        }
    };
    let (book_entry, entry) = arc_data.as_ref();

    // Build cache key using entry CID
    let entry_cid = book_entry.entry.cid.as_ref();
    let cache_key = og::cache_key(&ident, &book_title, entry_title, entry_cid);

    // Check cache first
    if let Some(cached) = og::get_cached(&cache_key) {
        return Ok((
            [
                (CONTENT_TYPE, "image/png"),
                (CACHE_CONTROL, "public, max-age=3600"),
            ],
            cached,
        )
            .into_response());
    }

    // Extract metadata
    let title: &str = entry.title.as_ref();

    // Use book_title from URL - it's the notebook slug/title
    // TODO: Could fetch actual notebook record to get display title
    let notebook_title_str: &str = book_title.as_ref();

    let author_handle = book_entry
        .entry
        .authors
        .first()
        .map(|a| match &a.record.inner {
            ProfileDataViewInner::ProfileView(p) => p.handle.as_ref(),
            ProfileDataViewInner::ProfileViewDetailed(p) => p.handle.as_ref(),
            ProfileDataViewInner::TangledProfileView(p) => p.handle.as_ref(),
            _ => "unknown",
        })
        .unwrap_or("unknown");

    // Check for hero image in embeds
    let hero_image_data = if let Some(ref embeds) = entry.embeds {
        if let Some(ref images) = embeds.images {
            if let Some(first_image) = images.images.first() {
                // Get DID from the entry URI
                let did = book_entry.entry.uri.authority();

                let blob = first_image.image.blob();
                let cid = blob.cid();
                let mime = blob.mime_type.as_ref();
                let format = mime.strip_prefix("image/").unwrap_or("jpeg");

                // Build CDN URL
                let cdn_url = format!(
                    "https://cdn.bsky.app/img/feed_fullsize/plain/{}/{}@{}",
                    did.as_str(),
                    cid.as_ref(),
                    format
                );

                // Fetch the image
                match reqwest::get(&cdn_url).await {
                    Ok(response) if response.status().is_success() => {
                        match response.bytes().await {
                            Ok(bytes) => {
                                use base64::Engine;
                                let base64_str =
                                    base64::engine::general_purpose::STANDARD.encode(&bytes);
                                Some(format!("data:{};base64,{}", mime, base64_str))
                            }
                            Err(_) => None,
                        }
                    }
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Extract content snippet - render markdown to HTML then strip tags
    let content_snippet: String = {
        let parser = markdown_weaver::Parser::new(entry.content.as_ref());
        let mut html = String::new();
        markdown_weaver::html::push_html(&mut html, parser);
        // Strip HTML tags
        regex_lite::Regex::new(r"<[^>]+>")
            .unwrap()
            .replace_all(&html, "")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace('\n', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };

    // Generate image - hero or text-only based on available data
    let png_bytes = if let Some(ref hero_data) = hero_image_data {
        match og::generate_hero_image(hero_data, title, &notebook_title_str, &author_handle) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!(
                    "Failed to generate hero OG image: {:?}, falling back to text",
                    e
                );
                og::generate_text_only(title, &content_snippet, &notebook_title_str, &author_handle)
                    .map_err(|e| {
                        tracing::error!("Failed to generate text OG image: {:?}", e);
                    })
                    .ok()
                    .unwrap_or_default()
            }
        }
    } else {
        match og::generate_text_only(title, &content_snippet, &notebook_title_str, &author_handle) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to generate OG image: {:?}", e);
                return Ok((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to generate image",
                )
                    .into_response());
            }
        }
    };

    // Cache the generated image
    og::cache_image(cache_key, png_bytes.clone());

    Ok((
        [
            (CONTENT_TYPE, "image/png"),
            (CACHE_CONTROL, "public, max-age=3600"),
        ],
        png_bytes,
    )
        .into_response())
}

// Route: /og/notebook/{ident}/{book_title}.png - OpenGraph image for notebook index
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/og/notebook/{ident}/{book_title}", fetcher: Extension<Arc<fetch::Fetcher>>)]
pub async fn og_notebook_image(
    ident: SmolStr,
    book_title: SmolStr,
) -> Result<axum::response::Response> {
    use axum::{
        http::{
            StatusCode,
            header::{CACHE_CONTROL, CONTENT_TYPE},
        },
        response::IntoResponse,
    };
    use weaver_api::sh_weaver::actor::ProfileDataViewInner;

    // Strip .png extension if present
    let book_title = book_title.strip_suffix(".png").unwrap_or(&book_title);

    let Ok(at_ident) = AtIdentifier::new_owned(ident.clone()) else {
        return Ok((StatusCode::BAD_REQUEST, "Invalid identifier").into_response());
    };

    // Fetch notebook data
    let notebook_result = fetcher
        .get_notebook(at_ident.clone(), book_title.into())
        .await;

    let arc_data = match notebook_result {
        Ok(Some(data)) => data,
        Ok(None) => return Ok((StatusCode::NOT_FOUND, "Notebook not found").into_response()),
        Err(e) => {
            tracing::error!("Failed to fetch notebook for OG image: {:?}", e);
            return Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch notebook",
            )
                .into_response());
        }
    };
    let (notebook_view, _entries) = arc_data.as_ref();

    // Build cache key using notebook CID
    let notebook_cid = notebook_view.cid.as_ref();
    let cache_key = og::notebook_cache_key(&ident, book_title, notebook_cid);

    // Check cache first
    if let Some(cached) = og::get_cached(&cache_key) {
        return Ok((
            [
                (CONTENT_TYPE, "image/png"),
                (CACHE_CONTROL, "public, max-age=3600"),
            ],
            cached,
        )
            .into_response());
    }

    // Extract metadata
    let title = notebook_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled Notebook");

    let author_handle = notebook_view
        .authors
        .first()
        .map(|a| match &a.record.inner {
            ProfileDataViewInner::ProfileView(p) => p.handle.as_ref(),
            ProfileDataViewInner::ProfileViewDetailed(p) => p.handle.as_ref(),
            ProfileDataViewInner::TangledProfileView(p) => p.handle.as_ref(),
            _ => "unknown",
        })
        .unwrap_or("unknown");

    // Fetch entries to get entry titles and count
    let entries_result = fetcher
        .list_notebook_entries(at_ident.clone(), book_title.into())
        .await;
    let (entry_count, entry_titles) = match entries_result {
        Ok(Some(entries)) => {
            let count = entries.len();
            let titles: Vec<String> = entries
                .iter()
                .take(4)
                .map(|e| {
                    e.entry
                        .title
                        .as_ref()
                        .map(|t| t.as_ref().to_string())
                        .unwrap_or_else(|| "Untitled".to_string())
                })
                .collect();
            (count, titles)
        }
        _ => (0, vec![]),
    };

    // Generate image
    let png_bytes = match og::generate_notebook_og(title, &author_handle, entry_count, entry_titles)
    {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Failed to generate notebook OG image: {:?}", e);
            return Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to generate image",
            )
                .into_response());
        }
    };

    // Cache the generated image
    og::cache_image(cache_key, png_bytes.clone());

    Ok((
        [
            (CONTENT_TYPE, "image/png"),
            (CACHE_CONTROL, "public, max-age=3600"),
        ],
        png_bytes,
    )
        .into_response())
}

// Route: /og/profile/{ident}.png - OpenGraph image for profile/repository
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/og/profile/{ident}", fetcher: Extension<Arc<fetch::Fetcher>>)]
pub async fn og_profile_image(ident: SmolStr) -> Result<axum::response::Response> {
    use axum::{
        http::{
            StatusCode,
            header::{CACHE_CONTROL, CONTENT_TYPE},
        },
        response::IntoResponse,
    };
    use weaver_api::sh_weaver::actor::ProfileDataViewInner;

    // Strip .png extension if present
    let ident = ident.strip_suffix(".png").unwrap_or(&ident);

    let Ok(at_ident) = AtIdentifier::new_owned(ident.to_string()) else {
        return Ok((StatusCode::BAD_REQUEST, "Invalid identifier").into_response());
    };

    // Fetch profile data
    let profile_result = fetcher.fetch_profile(&at_ident).await;

    let profile_view = match profile_result {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Failed to fetch profile for OG image: {:?}", e);
            return Ok(
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch profile").into_response(),
            );
        }
    };

    // Extract profile fields based on type
    // Use DID as cache key since profiles don't have a CID field
    let (display_name, handle, bio, avatar_url, banner_url, cache_id) = match &profile_view.inner {
        ProfileDataViewInner::ProfileView(p) => (
            p.display_name
                .as_ref()
                .map(|n| n.as_ref())
                .unwrap_or_default(),
            p.handle.as_ref(),
            p.description
                .as_ref()
                .map(|d| d.as_ref())
                .unwrap_or_default(),
            p.avatar.as_ref().map(|u| u.as_ref()),
            None::<&str>,
            p.did.as_ref(),
        ),
        ProfileDataViewInner::ProfileViewDetailed(p) => (
            p.display_name
                .as_ref()
                .map(|n| n.as_ref())
                .unwrap_or_default(),
            p.handle.as_ref(),
            p.description
                .as_ref()
                .map(|d| d.as_ref())
                .unwrap_or_default(),
            p.avatar.as_ref().map(|u| u.as_ref()),
            p.banner.as_ref().map(|u| u.as_ref()),
            p.did.as_ref(),
        ),
        ProfileDataViewInner::TangledProfileView(p) => {
            ("", p.handle.as_ref(), "", None, None, p.did.as_ref())
        }
        _ => return Ok((StatusCode::NOT_FOUND, "Profile type not supported").into_response()),
    };

    // Build cache key
    let cache_key = og::profile_cache_key(ident, &cache_id);

    // Check cache first
    if let Some(cached) = og::get_cached(&cache_key) {
        return Ok((
            [
                (CONTENT_TYPE, "image/png"),
                (CACHE_CONTROL, "public, max-age=3600"),
            ],
            cached,
        )
            .into_response());
    }

    // Fetch notebook count
    let notebooks_result = fetcher.fetch_notebooks_for_did(&at_ident).await;
    let notebook_count = notebooks_result.map(|n| n.len()).unwrap_or(0);

    // Fetch avatar as base64 if available
    let avatar_data = if let Some(url) = avatar_url {
        match reqwest::get(url).await {
            Ok(response) if response.status().is_success() => {
                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("image/jpeg")
                    .to_smolstr();
                match response.bytes().await {
                    Ok(bytes) => {
                        use base64::Engine;
                        let base64_str = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        Some(format!("data:{};base64,{}", content_type, base64_str))
                    }
                    Err(_) => None,
                }
            }
            _ => None,
        }
    } else {
        None
    };

    // Check for banner and generate appropriate template
    let png_bytes = if let Some(banner_url) = banner_url {
        // Fetch banner image
        let banner_data = match reqwest::get(banner_url).await {
            Ok(response) if response.status().is_success() => {
                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("image/jpeg")
                    .to_smolstr();
                match response.bytes().await {
                    Ok(bytes) => {
                        use base64::Engine;
                        let base64_str = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        Some(format!("data:{};base64,{}", content_type, base64_str))
                    }
                    Err(_) => None,
                }
            }
            _ => None,
        };

        if let Some(banner_data) = banner_data {
            match og::generate_profile_banner_og(
                &display_name,
                &handle,
                &bio,
                banner_data,
                avatar_data.clone(),
                notebook_count,
            ) {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::error!(
                        "Failed to generate profile banner OG image: {:?}, falling back",
                        e
                    );
                    og::generate_profile_og(
                        &display_name,
                        &handle,
                        &bio,
                        avatar_data,
                        notebook_count,
                    )
                    .unwrap_or_default()
                }
            }
        } else {
            og::generate_profile_og(&display_name, &handle, &bio, avatar_data, notebook_count)
                .unwrap_or_default()
        }
    } else {
        match og::generate_profile_og(&display_name, &handle, &bio, avatar_data, notebook_count) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to generate profile OG image: {:?}", e);
                return Ok((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to generate image",
                )
                    .into_response());
            }
        }
    };

    // Cache the generated image
    og::cache_image(cache_key, png_bytes.clone());

    Ok((
        [
            (CONTENT_TYPE, "image/png"),
            (CACHE_CONTROL, "public, max-age=3600"),
        ],
        png_bytes,
    )
        .into_response())
}

// Route: /og/site.png - OpenGraph image for homepage
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/og/site.png")]
pub async fn og_site_image() -> Result<axum::response::Response> {
    use axum::{
        http::{
            StatusCode,
            header::{CACHE_CONTROL, CONTENT_TYPE},
        },
        response::IntoResponse,
    };

    // Site OG is static, cache aggressively
    static SITE_OG_CACHE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();

    let png_bytes = SITE_OG_CACHE.get_or_init(|| og::generate_site_og().unwrap_or_default());

    if png_bytes.is_empty() {
        return Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to generate image",
        )
            .into_response());
    }

    Ok((
        [
            (CONTENT_TYPE, "image/png"),
            (CACHE_CONTROL, "public, max-age=86400"),
        ],
        png_bytes.clone(),
    )
        .into_response())
}
