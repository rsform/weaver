use crate::error::{CarError, IndexError};
use bytes::Bytes;
use jacquard_repo::car::reader::parse_car_bytes;
use smol_str::{SmolStr, ToSmolStr};

use super::consumer::Commit;

/// An extracted record from a firehose commit
#[derive(Debug, Clone)]
pub struct ExtractedRecord {
    /// DID of the repo owner
    pub did: SmolStr,
    /// Collection NSID (e.g., "app.bsky.feed.post")
    pub collection: SmolStr,
    /// Record key within the collection
    pub rkey: SmolStr,
    /// Content identifier
    pub cid: String,
    /// Operation type: "create", "update", or "delete"
    pub operation: SmolStr,
    /// Raw DAG-CBOR bytes of the record (None for deletes)
    pub cbor_bytes: Option<Bytes>,
    /// Sequence number from the firehose event
    pub seq: i64,
    /// Event timestamp (milliseconds since epoch)
    pub event_time_ms: i64,
}

impl ExtractedRecord {
    /// Decode the CBOR bytes to JSON string
    ///
    /// Uses jacquard's RawData type which properly handles CID links
    /// and other AT Protocol specific types.
    pub fn to_json(&self) -> Result<Option<String>, IndexError> {
        use jacquard_common::types::value::{RawData, from_cbor};

        match &self.cbor_bytes {
            Some(bytes) => {
                // RawData handles CID links and other IPLD types correctly
                let value: RawData<'static> =
                    from_cbor::<RawData>(bytes).map_err(|e| CarError::RecordDecode {
                        message: format!("failed to decode DAG-CBOR: {}", e),
                    })?;
                let json = serde_json::to_string(&value).map_err(|e| CarError::RecordDecode {
                    message: format!("failed to encode JSON: {}", e),
                })?;
                Ok(Some(json))
            }
            None => Ok(None),
        }
    }
}

/// Extract records from a firehose commit
///
/// Parses the CAR data and extracts each record referenced by the operations.
pub async fn extract_records(commit: &Commit<'_>) -> Result<Vec<ExtractedRecord>, IndexError> {
    let parsed_car = parse_car_bytes(&commit.blocks)
        .await
        .map_err(|e| CarError::Parse {
            message: e.to_string(),
        })?;

    let event_time_ms = commit.time.as_ref().timestamp_millis();
    let mut records = Vec::with_capacity(commit.ops.len());

    for op in &commit.ops {
        let path: &str = op.path.as_ref();

        // Path format: "collection/rkey"
        let (collection, rkey) = match path.split_once('/') {
            Some((c, r)) => (c.to_smolstr(), r.to_smolstr()),
            None => {
                tracing::warn!(path = %path, "invalid op path format, skipping");
                continue;
            }
        };

        let operation = op.action.to_smolstr();
        let cid_str = op.cid.as_ref().map(|c| c.to_string()).unwrap_or_default();

        // For creates/updates, look up the record in the CAR blocks
        let cbor_bytes = if let Some(cid_link) = &op.cid {
            match cid_link.0.to_ipld() {
                Ok(ipld_cid) => parsed_car.blocks.get(&ipld_cid).cloned(),
                Err(_) => {
                    tracing::warn!(cid = %cid_str, "failed to convert CID to IPLD format");
                    None
                }
            }
        } else {
            None
        };

        records.push(ExtractedRecord {
            did: commit.repo.to_smolstr(),
            collection,
            rkey,
            cid: cid_str,
            operation,
            cbor_bytes,
            seq: commit.seq,
            event_time_ms,
        });
    }

    Ok(records)
}
