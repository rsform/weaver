use dioxus::prelude::*;
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use wasm_bindgen::prelude::*;

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use wasm_bindgen_futures::JsFuture;
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use web_sys::{RegistrationOptions, ServiceWorkerContainer, Window};

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub async fn register_service_worker() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let navigator = window.navigator();
    let sw_container = navigator.service_worker();

    let promise = sw_container.register("/sw.js");
    JsFuture::from(promise).await?;

    Ok(())
}

/// Register blob mappings from entry images with the service worker
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub async fn register_entry_blobs(
    ident: &jacquard::types::ident::AtIdentifier<'_>,
    book_title: &str,
    images: &weaver_api::sh_weaver::embed::images::Images<'_>,
    fetcher: &crate::fetch::CachedFetcher,
) -> Result<(), JsValue> {
    use jacquard::prelude::IdentityResolver;
    use std::collections::HashMap;

    let mut blob_mappings = HashMap::new();

    // Resolve DID and PDS URL
    let (did, pds_url) = match ident {
        jacquard::types::ident::AtIdentifier::Did(d) => {
            let pds = fetcher.client.pds_for_did(d).await.ok();
            (d.clone(), pds)
        }
        jacquard::types::ident::AtIdentifier::Handle(h) => {
            if let Ok((did, pds)) = fetcher.client.pds_for_handle(h).await {
                (did, Some(pds))
            } else {
                return Ok(());
            }
        }
    };

    if let Some(pds_url) = pds_url {
        for image in &images.images {
            let cid = image.image.blob().cid();

            if let Some(name) = &image.name {
                let blob_url = format!(
                    "{}xrpc/com.atproto.sync.getBlob?did={}&cid={}",
                    pds_url.as_str(),
                    did.as_ref(),
                    cid.as_ref()
                );
                blob_mappings.insert(name.as_ref().to_string(), blob_url);
            }
        }

        // Send mappings to service worker
        if !blob_mappings.is_empty() {
            send_blob_mappings(book_title, blob_mappings)?;
        }
    }

    Ok(())
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
fn send_blob_mappings(
    notebook: &str,
    mappings: std::collections::HashMap<String, String>,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let navigator = window.navigator();
    let sw_container = navigator.service_worker();

    let controller = sw_container
        .controller()
        .ok_or_else(|| JsValue::from_str("no service worker controller"))?;

    // Build message object
    let msg = js_sys::Object::new();
    js_sys::Reflect::set(&msg, &"type".into(), &"register_mappings".into())?;
    js_sys::Reflect::set(&msg, &"notebook".into(), &notebook.into())?;

    // Convert HashMap to JS Object
    let blobs_obj = js_sys::Object::new();
    for (name, url) in mappings {
        js_sys::Reflect::set(&blobs_obj, &name.into(), &url.into())?;
    }
    js_sys::Reflect::set(&msg, &"blobs".into(), &blobs_obj)?;

    controller.post_message(&msg)?;

    Ok(())
}

#[allow(unused)]
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub async fn register_service_worker() -> Result<(), String> {
    Ok(())
}

#[allow(unused)]
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn send_blob_mappings(
    _notebook: &str,
    _mappings: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    Ok(())
}

// #[used]
// static BINDINGS_JS: Asset = asset!("/assets/sw.js", AssetOptions::js().with_hash_suffix(false));
