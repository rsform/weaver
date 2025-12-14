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

/// Draft with associated edit root info
#[derive(Debug, Clone, Row, Deserialize)]
pub struct DraftWithRootRow {
    pub did: SmolStr,
    pub rkey: SmolStr,
    pub cid: SmolStr,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub root_did: SmolStr,
    pub root_rkey: SmolStr,
    pub root_cid: SmolStr,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis::option")]
    pub last_edit_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Client {
    /// Get edit history for a resource.
    ///
    /// Returns roots and diffs separately, ordered by created_at.
    pub async fn get_edit_history(
        &self,
        resource_did: &str,
        resource_collection: &str,
        resource_rkey: &str,
        cursor: Option<i64>,
        after_rkey: Option<&str>,
        limit: i64,
    ) -> Result<Vec<EditNodeRow>, IndexError> {
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

    /// List drafts for an actor.
    ///
    /// Returns draft records with associated edit root info if available.
    pub async fn list_drafts(
        &self,
        actor_did: &str,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<DraftWithRootRow>, IndexError> {
        // Query drafts table with LEFT JOIN to get associated edit roots
        // Edit roots reference drafts via resource_type/did/rkey fields
        let query = r#"
            SELECT
                d.did,
                d.rkey,
                d.cid,
                d.created_at,
                COALESCE(e.did, '') AS root_did,
                COALESCE(e.rkey, '') AS root_rkey,
                COALESCE(e.cid, '') AS root_cid,
                e.created_at AS last_edit_at
            FROM drafts d FINAL
            LEFT JOIN (
                SELECT
                    did,
                    rkey,
                    cid,
                    created_at,
                    resource_did,
                    resource_rkey
                FROM edit_nodes FINAL
                WHERE node_type = 'root'
                  AND resource_type = 'draft'
                  AND deleted_at = toDateTime64(0, 3)
            ) e ON e.resource_did = d.did AND e.resource_rkey = d.rkey
            WHERE d.did = ?
              AND d.deleted_at = toDateTime64(0, 3)
              AND (? = 0 OR toUnixTimestamp64Milli(d.created_at) < ?)
            ORDER BY d.created_at DESC
            LIMIT ?
        "#;

        let cursor_val = cursor.unwrap_or(0);

        let rows = self
            .inner()
            .query(query)
            .bind(actor_did)
            .bind(cursor_val)
            .bind(cursor_val)
            .bind(limit)
            .fetch_all::<DraftWithRootRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to list drafts".into(),
                source: e,
            })?;

        Ok(rows)
    }
}
