//! Background task for extracting draft titles from Loro snapshots.
//!
//! Periodically scans for drafts where the edit head has changed since
//! the last title extraction, fetches the edit chain from PDS, reconstructs
//! the Loro document, and extracts the title.

use std::sync::Arc;
use std::time::Duration;

use jacquard::client::UnauthenticatedSession;
use jacquard::identity::JacquardResolver;
use jacquard::prelude::{IdentityResolver, XrpcExt};
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::{Cid, Did};
use loro::LoroDoc;
use mini_moka::sync::Cache;
use tracing::{debug, error, info, warn};

use crate::clickhouse::{Client, StaleDraftRow};
use crate::error::IndexError;

use weaver_api::com_atproto::repo::get_record::GetRecord;
use weaver_api::com_atproto::sync::get_blob::GetBlob;
use weaver_api::sh_weaver::edit::diff::Diff;
use weaver_api::sh_weaver::edit::root::Root;

/// Cache for PDS blob fetches.
///
/// Blobs are content-addressed so safe to cache indefinitely.
/// Key is (did, cid) as a string.
#[derive(Clone)]
pub struct BlobCache {
    cache: Cache<String, Arc<Vec<u8>>>,
}

impl BlobCache {
    pub fn new(max_capacity: u64) -> Self {
        Self {
            cache: Cache::new(max_capacity),
        }
    }

    fn key(did: &str, cid: &str) -> String {
        format!("{}:{}", did, cid)
    }

    pub fn get(&self, did: &str, cid: &str) -> Option<Arc<Vec<u8>>> {
        self.cache.get(&Self::key(did, cid))
    }

    pub fn insert(&self, did: &str, cid: &str, data: Vec<u8>) {
        self.cache.insert(Self::key(did, cid), Arc::new(data));
    }
}

/// Configuration for the draft title extraction task
#[derive(Debug, Clone)]
pub struct DraftTitleTaskConfig {
    /// How often to check for stale titles
    pub interval: Duration,
    /// Maximum drafts to process per run
    pub batch_size: i64,
}

impl Default for DraftTitleTaskConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(120), // 2 minutes
            batch_size: 50,
        }
    }
}

/// Run the draft title extraction task in a loop
pub async fn run_draft_title_task(
    client: Arc<Client>,
    resolver: UnauthenticatedSession<JacquardResolver>,
    config: DraftTitleTaskConfig,
) {
    info!(
        interval_secs = config.interval.as_secs(),
        batch_size = config.batch_size,
        "starting draft title extraction task"
    );

    // Cache for blob fetches - blobs are content-addressed, safe to cache indefinitely
    // 1000 entries is plenty for typical edit chains
    let blob_cache = BlobCache::new(1000);

    loop {
        match process_stale_drafts(&client, &resolver, &blob_cache, config.batch_size).await {
            Ok(count) => {
                if count > 0 {
                    info!(processed = count, "draft title extraction complete");
                } else {
                    debug!("no stale draft titles to process");
                }
            }
            Err(e) => {
                error!(error = ?e, "draft title extraction failed");
            }
        }

        tokio::time::sleep(config.interval).await;
    }
}

/// Process a batch of stale drafts
async fn process_stale_drafts(
    client: &Client,
    resolver: &UnauthenticatedSession<JacquardResolver>,
    blob_cache: &BlobCache,
    batch_size: i64,
) -> Result<usize, IndexError> {
    let stale = client.get_stale_draft_titles(batch_size).await?;

    if stale.is_empty() {
        return Ok(0);
    }

    debug!(count = stale.len(), "found stale draft titles");

    let mut processed = 0;
    for draft in stale {
        match extract_and_save_title(client, resolver, blob_cache, &draft).await {
            Ok(title) => {
                debug!(
                    did = %draft.did,
                    rkey = %draft.rkey,
                    title = %title,
                    "extracted draft title"
                );
                processed += 1;
            }
            Err(e) => {
                warn!(
                    did = %draft.did,
                    rkey = %draft.rkey,
                    error = ?e,
                    "failed to extract draft title"
                );
            }
        }
    }

    Ok(processed)
}

/// Extract title from a single draft and save it
async fn extract_and_save_title(
    client: &Client,
    resolver: &UnauthenticatedSession<JacquardResolver>,
    blob_cache: &BlobCache,
    draft: &StaleDraftRow,
) -> Result<String, IndexError> {
    // Get the edit chain from ClickHouse
    let chain = client
        .get_edit_chain(
            &draft.root_did,
            &draft.root_rkey,
            &draft.head_did,
            &draft.head_rkey,
        )
        .await?;

    if chain.is_empty() {
        return Err(IndexError::NotFound {
            resource: format!("edit chain for {}:{}", draft.did, draft.rkey),
        });
    }

    // Resolve PDS for the root DID
    let root_did = Did::new(&draft.root_did).map_err(|e| IndexError::NotFound {
        resource: format!("invalid root DID: {}", e),
    })?;

    let pds_url = resolver
        .pds_for_did(&root_did)
        .await
        .map_err(|e| IndexError::NotFound {
            resource: format!("PDS for {}: {}", root_did, e),
        })?;

    // Initialize Loro doc
    let doc = LoroDoc::new();

    // Process chain: first node should be root, rest are diffs
    for (i, node) in chain.iter().enumerate() {
        let node_did = Did::new(&node.did).map_err(|e| IndexError::NotFound {
            resource: format!("invalid node DID: {}", e),
        })?;

        if node.node_type == "root" {
            // Fetch root record
            let root_record =
                fetch_root_record(resolver, pds_url.clone(), &node_did, &node.rkey).await?;

            // Fetch snapshot blob
            let snapshot_cid = root_record.snapshot.blob().cid();
            let snapshot_bytes =
                fetch_blob(resolver, blob_cache, pds_url.clone(), &node_did, snapshot_cid).await?;

            // Import snapshot
            doc.import(&snapshot_bytes)
                .map_err(|e| IndexError::NotFound {
                    resource: format!("failed to import root snapshot: {}", e),
                })?;

            debug!(
                did = %node.did,
                rkey = %node.rkey,
                bytes = snapshot_bytes.len(),
                "imported root snapshot"
            );
        } else {
            // Fetch diff record
            let diff_record =
                fetch_diff_record(resolver, pds_url.clone(), &node_did, &node.rkey).await?;

            // Diffs can have inline diff bytes or a snapshot blob reference
            let diff_bytes = if let Some(ref inline) = diff_record.inline_diff {
                // Use inline diff (base64 decoded by serde)
                inline.to_vec()
            } else if let Some(ref snapshot_blob) = diff_record.snapshot {
                // Fetch snapshot blob
                let snapshot_cid = snapshot_blob.blob().cid();
                fetch_blob(resolver, blob_cache, pds_url.clone(), &node_did, snapshot_cid).await?
            } else {
                warn!(
                    did = %node.did,
                    rkey = %node.rkey,
                    "diff has neither inline nor snapshot data, skipping"
                );
                continue;
            };

            // Import diff
            doc.import(&diff_bytes).map_err(|e| IndexError::NotFound {
                resource: format!("failed to import diff {}: {}", i, e),
            })?;

            debug!(
                did = %node.did,
                rkey = %node.rkey,
                bytes = diff_bytes.len(),
                "imported diff"
            );
        }
    }

    // Extract title from Loro doc
    let title = doc.get_text("title").to_string();

    // Save to ClickHouse
    client
        .upsert_draft_title(
            &draft.did,
            &draft.rkey,
            &title,
            &draft.head_did,
            &draft.head_rkey,
            &draft.head_cid,
        )
        .await?;

    Ok(title)
}

/// Fetch an edit.root record from PDS
async fn fetch_root_record(
    resolver: &UnauthenticatedSession<JacquardResolver>,
    pds_url: jacquard::url::Url,
    did: &Did<'_>,
    rkey: &str,
) -> Result<Root<'static>, IndexError> {
    use jacquard::IntoStatic;
    use jacquard::types::string::Nsid;

    let request = GetRecord::new()
        .repo(AtIdentifier::Did(did.clone()))
        .collection(Nsid::new_static("sh.weaver.edit.root").unwrap())
        .rkey(
            jacquard::types::recordkey::RecordKey::any(rkey).map_err(|e| IndexError::NotFound {
                resource: format!("invalid rkey: {}", e),
            })?,
        )
        .build();

    let response =
        resolver
            .xrpc(pds_url)
            .send(&request)
            .await
            .map_err(|e| IndexError::NotFound {
                resource: format!("root record {}/{}: {}", did, rkey, e),
            })?;

    let output = response.into_output().map_err(|e| IndexError::NotFound {
        resource: format!("parse root record: {}", e),
    })?;

    let root: Root = jacquard::from_data(&output.value).map_err(|e| IndexError::NotFound {
        resource: format!("deserialize root: {}", e),
    })?;

    Ok(root.into_static())
}

/// Fetch an edit.diff record from PDS
async fn fetch_diff_record(
    resolver: &UnauthenticatedSession<JacquardResolver>,
    pds_url: jacquard::url::Url,
    did: &Did<'_>,
    rkey: &str,
) -> Result<Diff<'static>, IndexError> {
    use jacquard::IntoStatic;
    use jacquard::types::string::Nsid;

    let request = GetRecord::new()
        .repo(AtIdentifier::Did(did.clone()))
        .collection(Nsid::new_static("sh.weaver.edit.diff").unwrap())
        .rkey(
            jacquard::types::recordkey::RecordKey::any(rkey).map_err(|e| IndexError::NotFound {
                resource: format!("invalid rkey: {}", e),
            })?,
        )
        .build();

    let response =
        resolver
            .xrpc(pds_url)
            .send(&request)
            .await
            .map_err(|e| IndexError::NotFound {
                resource: format!("diff record {}/{}: {}", did, rkey, e),
            })?;

    let output = response.into_output().map_err(|e| IndexError::NotFound {
        resource: format!("parse diff record: {}", e),
    })?;

    let diff: Diff = jacquard::from_data(&output.value).map_err(|e| IndexError::NotFound {
        resource: format!("deserialize diff: {}", e),
    })?;

    Ok(diff.into_static())
}

/// Fetch a blob from PDS, using cache when available
async fn fetch_blob(
    resolver: &UnauthenticatedSession<JacquardResolver>,
    cache: &BlobCache,
    pds_url: jacquard::url::Url,
    did: &Did<'_>,
    cid: &Cid<'_>,
) -> Result<Vec<u8>, IndexError> {
    // Check cache first - blobs are content-addressed
    if let Some(cached) = cache.get(did.as_str(), cid.as_str()) {
        debug!(cid = %cid, "blob cache hit");
        return Ok(cached.as_ref().clone());
    }

    let request = GetBlob::new().did(did.clone()).cid(cid.clone()).build();

    let response =
        resolver
            .xrpc(pds_url)
            .send(&request)
            .await
            .map_err(|e| IndexError::NotFound {
                resource: format!("blob {}: {}", cid, e),
            })?;

    let bytes = response.buffer().to_vec();

    // Cache for future use
    cache.insert(did.as_str(), cid.as_str(), bytes.clone());

    Ok(bytes)
}
