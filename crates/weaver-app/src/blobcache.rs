use crate::cache_impl;
use dioxus::{CapturedError, Result};
use jacquard::{
    bytes::Bytes,
    client::UnauthenticatedSession,
    identity::JacquardResolver,
    prelude::*,
    smol_str::SmolStr,
    types::{cid::Cid, collection::Collection, ident::AtIdentifier, nsid::Nsid, string::Rkey},
    xrpc::XrpcExt,
    IntoStatic,
};
use std::{
    sync::Arc,
    time::Duration,
};
use weaver_api::com_atproto::repo::get_record::GetRecord;
use weaver_api::com_atproto::sync::get_blob::GetBlob;
use weaver_api::sh_weaver::notebook::entry::Entry;
use weaver_api::sh_weaver::publish::blob::Blob as PublishedBlob;

#[derive(Clone)]
pub struct BlobCache {
    client: Arc<UnauthenticatedSession<JacquardResolver>>,
    cache: cache_impl::Cache<Cid<'static>, Bytes>,
    map: cache_impl::Cache<SmolStr, Cid<'static>>,
}

impl BlobCache {
    pub fn new(client: Arc<UnauthenticatedSession<JacquardResolver>>) -> Self {
        let cache = cache_impl::new_cache(100, Duration::from_secs(1200));
        let map = cache_impl::new_cache(500, Duration::from_secs(1200));

        Self { client, cache, map }
    }

    /// Resolve DID and PDS URL from an identifier
    async fn resolve_ident(
        &self,
        ident: &AtIdentifier<'_>,
    ) -> Result<(jacquard::types::string::Did<'static>, jacquard::url::Url)> {
        match ident {
            AtIdentifier::Did(did) => {
                let pds = self.client.pds_for_did(did).await?;
                Ok((did.clone().into_static(), pds))
            }
            AtIdentifier::Handle(handle) => {
                let (did, pds) = self.client.pds_for_handle(handle).await?;
                Ok((did, pds))
            }
        }
    }

    /// Fetch a blob by CID from a specific DID's PDS
    async fn fetch_blob(
        &self,
        did: &jacquard::types::string::Did<'_>,
        pds_url: jacquard::url::Url,
        cid: &Cid<'_>,
    ) -> Result<Bytes> {
        match self
            .client
            .xrpc(pds_url.clone())
            .send(
                &GetBlob::new()
                    .cid(cid.clone())
                    .did(did.clone())
                    .build(),
            )
            .await
        {
            Ok(blob_stream) => Ok(blob_stream.buffer().clone()),
            Err(e) => {
                tracing::warn!(
                    did = %did,
                    cid = %cid,
                    pds = %pds_url,
                    error = %e,
                    "PDS blob fetch failed, falling back to Bluesky CDN"
                );
                // Fallback to Bluesky CDN (works for blobs stored on bsky PDSes)
                let bytes = reqwest::get(format!(
                    "https://cdn.bsky.app/img/feed_fullsize/plain/{}/{}@jpeg",
                    did, cid
                ))
                .await?
                .bytes()
                .await?;
                Ok(bytes)
            }
        }
    }

    pub async fn cache(
        &self,
        ident: AtIdentifier<'static>,
        cid: Cid<'static>,
        name: Option<SmolStr>,
    ) -> Result<()> {
        let (repo_did, pds_url) = self.resolve_ident(&ident).await?;
        
        if self.get_cid(&cid).is_some() {
            return Ok(());
        }
        
        let blob = self.fetch_blob(&repo_did, pds_url, &cid).await?;

        self.cache.insert(cid.clone(), blob);
        if let Some(name) = name {
            self.map.insert(name, cid);
        }

        Ok(())
    }

    /// Resolve an image from a published entry by name.
    /// 
    /// Looks up the entry record at `{ident}/sh.weaver.notebook.entry/{rkey}`,
    /// finds the image by name in the embeds, and returns the blob bytes.
    pub async fn resolve_from_entry(
        &self,
        ident: &AtIdentifier<'_>,
        rkey: &str,
        name: &str,
    ) -> Result<Bytes> {
        let (repo_did, pds_url) = self.resolve_ident(ident).await?;
        
        // Fetch the entry record
        let resp = self
            .client
            .xrpc(pds_url.clone())
            .send(
                &GetRecord::new()
                    .repo(AtIdentifier::Did(repo_did.clone()))
                    .collection(Nsid::raw(<Entry as Collection>::NSID))
                    .rkey(Rkey::new(rkey).map_err(|e| CapturedError::from_display(e))?)
                    .build(),
            )
            .await
            .map_err(|e| CapturedError::from_display(format!("Failed to fetch entry: {}", e)))?;

        let record = resp
            .into_output()
            .map_err(|e| CapturedError::from_display(format!("Failed to parse entry: {}", e)))?;

        // Parse the entry
        let entry: Entry = jacquard::from_data(&record.value)
            .map_err(|e| CapturedError::from_display(format!("Failed to deserialize entry: {}", e)))?;

        // Find the image by name
        let cid = entry
            .embeds
            .as_ref()
            .and_then(|e| e.images.as_ref())
            .and_then(|imgs| {
                imgs.images.iter().find(|img| {
                    img.name.as_ref().map(|n| n.as_ref()) == Some(name)
                })
            })
            .map(|img| img.image.blob().cid().clone().into_static())
            .ok_or_else(|| CapturedError::from_display(format!("Image '{}' not found in entry", name)))?;

        // Check cache first
        if let Some(bytes) = self.get_cid(&cid) {
            return Ok(bytes);
        }

        // Fetch and cache the blob
        let blob = self.fetch_blob(&repo_did, pds_url, &cid).await?;
        self.cache.insert(cid.clone(), blob.clone());
        self.map.insert(name.into(), cid);
        
        Ok(blob)
    }

    /// Resolve an image from a draft (unpublished) entry via PublishedBlob record.
    /// 
    /// Looks up the PublishedBlob record at `{ident}/sh.weaver.publish.blob/{blob_rkey}`,
    /// gets the CID from it, and returns the blob bytes.
    pub async fn resolve_from_draft(
        &self,
        ident: &AtIdentifier<'_>,
        blob_rkey: &str,
    ) -> Result<Bytes> {
        let (repo_did, pds_url) = self.resolve_ident(ident).await?;
        
        // Fetch the PublishedBlob record
        let resp = self
            .client
            .xrpc(pds_url.clone())
            .send(
                &GetRecord::new()
                    .repo(AtIdentifier::Did(repo_did.clone()))
                    .collection(Nsid::raw(<PublishedBlob as Collection>::NSID))
                    .rkey(Rkey::new(blob_rkey).map_err(|e| CapturedError::from_display(e))?)
                    .build(),
            )
            .await
            .map_err(|e| CapturedError::from_display(format!("Failed to fetch PublishedBlob: {}", e)))?;

        let record = resp
            .into_output()
            .map_err(|e| CapturedError::from_display(format!("Failed to parse PublishedBlob: {}", e)))?;

        // Parse the PublishedBlob
        let published: PublishedBlob = jacquard::from_data(&record.value)
            .map_err(|e| CapturedError::from_display(format!("Failed to deserialize PublishedBlob: {}", e)))?;

        // Get CID from the upload blob ref
        let cid = published.upload.blob().cid().clone().into_static();

        // Check cache first
        if let Some(bytes) = self.get_cid(&cid) {
            return Ok(bytes);
        }

        // Fetch and cache the blob
        let blob = self.fetch_blob(&repo_did, pds_url, &cid).await?;
        self.cache.insert(cid, blob.clone());
        
        Ok(blob)
    }

    /// Resolve an image from a notebook entry by name.
    /// 
    /// This is a convenience method that looks up the notebook, finds the entry,
    /// and resolves the image. Used for `/image/{notebook}/{name}` paths.
    pub async fn resolve_from_notebook(
        &self,
        notebook_title: &str,
        image_name: &str,
    ) -> Result<Bytes> {
        // For now, just try to get from the name cache
        // Full notebook resolution would require fetching the notebook record
        // and iterating through entries to find the image
        self.get_named(&image_name.into())
            .ok_or_else(|| CapturedError::from_display(format!(
                "Image '{}' not found in notebook '{}'", 
                image_name, notebook_title
            )))
    }

    pub fn get_cid(&self, cid: &Cid<'static>) -> Option<Bytes> {
        self.cache.get(cid)
    }

    pub fn get_named(&self, name: &SmolStr) -> Option<Bytes> {
        self.map.get(name).and_then(|cid| self.cache.get(&cid))
    }
}
