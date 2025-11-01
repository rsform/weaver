use dioxus::{CapturedError, Result};
use jacquard::{
    bytes::Bytes,
    client::BasicClient,
    prelude::*,
    smol_str::SmolStr,
    types::{cid::Cid, ident::AtIdentifier},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use weaver_api::com_atproto::sync::get_blob::GetBlob;

#[derive(Clone)]
pub struct BlobCache {
    client: Arc<BasicClient>,
    cache: mini_moka::sync::Cache<Cid<'static>, Bytes>,
    map: mini_moka::sync::Cache<SmolStr, Cid<'static>>,
}

impl BlobCache {
    pub fn new(client: Arc<BasicClient>) -> Self {
        let cache = mini_moka::sync::Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(1200))
            .build();
        let map = mini_moka::sync::Cache::builder()
            .max_capacity(500)
            .time_to_idle(Duration::from_secs(1200))
            .build();

        Self { client, cache, map }
    }

    pub async fn cache(
        &self,
        ident: AtIdentifier<'static>,
        cid: Cid<'static>,
        name: Option<SmolStr>,
    ) -> Result<()> {
        let (repo_did, pds_url) = match ident {
            AtIdentifier::Did(did) => {
                let pds = self.client.pds_for_did(&did).await?;
                (did.clone(), pds)
            }
            AtIdentifier::Handle(handle) => self.client.pds_for_handle(&handle).await?,
        };
        let blob = self
            .client
            .xrpc(pds_url)
            .send(&GetBlob::new().cid(cid.clone()).did(repo_did).build())
            .await?
            .buffer()
            .clone();

        self.cache.insert(cid.clone(), blob);
        if let Some(name) = name {
            self.map.insert(name, cid);
        }

        Ok(())
    }

    pub fn get_cid(&self, cid: &Cid<'static>) -> Option<Bytes> {
        self.cache.get(cid)
    }

    pub fn get_named(&self, name: &SmolStr) -> Option<Bytes> {
        self.map.get(name).and_then(|cid| self.cache.get(&cid))
    }
}
