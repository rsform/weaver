//! Edit history queries

use clickhouse::Row;
use serde::Deserialize;
use smol_str::SmolStr;

use crate::clickhouse::Client;
use crate::error::{ClickHouseError, IndexError};

/// Edit node row from the edit_nodes table
#[derive(Debug, Clone, Row, Deserialize)]
pub struct EditNodeRow {
    pub did: SmolStr,
    pub rkey: SmolStr,
    pub cid: SmolStr,
    pub collection: SmolStr,
    pub node_type: SmolStr,
    pub root_did: SmolStr,
    pub root_rkey: SmolStr,
    pub root_cid: SmolStr,
    pub prev_did: SmolStr,
    pub prev_rkey: SmolStr,
    pub prev_cid: SmolStr,
    pub has_inline_diff: u8,
    pub has_snapshot: u8,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Client {
    /// Get edit history for a resource.
    ///
    /// Returns roots and diffs separately, ordered by created_at.
    /// The resource_uri should be an at:// URI for an entry or notebook.
    pub async fn get_edit_history(
        &self,
        resource_uri: &str,
        cursor: Option<i64>,
        after_rkey: Option<&str>,
        limit: i64,
    ) -> Result<Vec<EditNodeRow>, IndexError> {
        // Parse resource URI to extract did/collection/rkey
        let parts: Vec<&str> = resource_uri
            .strip_prefix("at://")
            .unwrap_or(resource_uri)
            .split('/')
            .collect();

        if parts.len() < 3 {
            return Ok(Vec::new());
        }

        let resource_did = parts[0];
        let resource_collection = parts[1];
        let resource_rkey = parts[2];

        let query = r#"
            SELECT
                did,
                rkey,
                cid,
                collection,
                node_type,
                root_did,
                root_rkey,
                root_cid,
                prev_did,
                prev_rkey,
                prev_cid,
                has_inline_diff,
                has_snapshot,
                created_at
            FROM edit_nodes FINAL
            WHERE resource_did = ?
              AND resource_collection = ?
              AND resource_rkey = ?
              AND deleted_at = toDateTime64(0, 3)
              AND (? = 0 OR toUnixTimestamp64Milli(created_at) < ?)
              AND (? = '' OR rkey > ?)
            ORDER BY created_at DESC
            LIMIT ?
        "#;

        let cursor_val = cursor.unwrap_or(0);
        let after_rkey_val = after_rkey.unwrap_or("");

        let rows = self
            .inner()
            .query(query)
            .bind(resource_did)
            .bind(resource_collection)
            .bind(resource_rkey)
            .bind(cursor_val)
            .bind(cursor_val)
            .bind(after_rkey_val)
            .bind(after_rkey_val)
            .bind(limit)
            .fetch_all::<EditNodeRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get edit history".into(),
                source: e,
            })?;

        Ok(rows)
    }
}
