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
    /// Title extracted from Loro doc (may be empty if not yet extracted)
    pub title: SmolStr,
}

/// Draft needing title extraction (stale or missing from draft_titles)
#[derive(Debug, Clone, Row, Deserialize)]
pub struct StaleDraftRow {
    /// Draft DID
    pub did: SmolStr,
    /// Draft rkey
    pub rkey: SmolStr,
    /// Current head DID
    pub head_did: SmolStr,
    /// Current head rkey
    pub head_rkey: SmolStr,
    /// Current head CID
    pub head_cid: SmolStr,
    /// Root DID for this edit chain
    pub root_did: SmolStr,
    /// Root rkey
    pub root_rkey: SmolStr,
    /// Root CID
    pub root_cid: SmolStr,
}

/// Edit chain node for reconstructing Loro doc
#[derive(Debug, Clone, Row, Deserialize)]
pub struct EditChainNode {
    pub did: SmolStr,
    pub rkey: SmolStr,
    pub cid: SmolStr,
    pub node_type: SmolStr,
    pub prev_did: SmolStr,
    pub prev_rkey: SmolStr,
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
    /// Returns draft records with associated edit root info and title if available.
    pub async fn list_drafts(
        &self,
        actor_did: &str,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<DraftWithRootRow>, IndexError> {
        // Query drafts table with LEFT JOINs to get edit roots and titles
        // Edit roots reference drafts via resource_type/did/rkey fields
        // Titles are extracted from Loro snapshots by background task
        let query = r#"
            SELECT
                d.did,
                d.rkey,
                d.cid,
                d.created_at,
                COALESCE(e.did, '') AS root_did,
                COALESCE(e.rkey, '') AS root_rkey,
                COALESCE(e.cid, '') AS root_cid,
                e.created_at AS last_edit_at,
                COALESCE(t.title, '') AS title
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
            LEFT JOIN draft_titles t FINAL ON t.did = d.did AND t.rkey = d.rkey
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

    /// Find drafts with stale or missing titles.
    ///
    /// Compares edit_heads to draft_titles to find drafts where the current
    /// head doesn't match the head used for title extraction.
    pub async fn get_stale_draft_titles(&self, limit: i64) -> Result<Vec<StaleDraftRow>, IndexError> {
        // Join drafts -> edit_heads (for current head) -> draft_titles (to check staleness)
        // edit_heads uses resource_type='draft' and resource_did/resource_rkey to link
        let query = r#"
            SELECT
                d.did,
                d.rkey,
                h.head_did,
                h.head_rkey,
                h.head_cid,
                h.root_did,
                h.root_rkey,
                h.root_cid
            FROM drafts d FINAL
            INNER JOIN edit_heads h FINAL
                ON h.resource_did = d.did
                AND h.resource_rkey = d.rkey
                AND h.resource_collection = 'sh.weaver.edit.draft'
            LEFT JOIN draft_titles t FINAL
                ON t.did = d.did
                AND t.rkey = d.rkey
            WHERE d.deleted_at = toDateTime64(0, 3)
              AND (t.head_cid IS NULL OR t.head_cid = '' OR t.head_cid != h.head_cid)
            LIMIT ?
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(limit)
            .fetch_all::<StaleDraftRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get stale draft titles".into(),
                source: e,
            })?;

        Ok(rows)
    }

    /// Get the edit chain from head back to root for reconstructing a Loro doc.
    ///
    /// Returns nodes in order from root to head (for sequential application).
    /// Validates that the chain terminates at the expected root.
    pub async fn get_edit_chain(
        &self,
        root_did: &str,
        root_rkey: &str,
        head_did: &str,
        head_rkey: &str,
    ) -> Result<Vec<EditChainNode>, IndexError> {
        // Walk backwards from head to root via prev links
        // Use recursive CTE to traverse the chain, stopping when we hit the expected root
        let query = r#"
            WITH RECURSIVE chain AS (
                -- Start from head
                SELECT did, rkey, cid, node_type, prev_did, prev_rkey, 0 as depth,
                       (did = ? AND rkey = ?) as is_root
                FROM edit_nodes FINAL
                WHERE did = ? AND rkey = ?
                  AND deleted_at = toDateTime64(0, 3)

                UNION ALL

                -- Follow prev links until we hit the root
                SELECT e.did, e.rkey, e.cid, e.node_type, e.prev_did, e.prev_rkey, c.depth + 1,
                       (e.did = ? AND e.rkey = ?) as is_root
                FROM edit_nodes e FINAL
                INNER JOIN chain c ON e.did = c.prev_did AND e.rkey = c.prev_rkey
                WHERE e.deleted_at = toDateTime64(0, 3)
                  AND c.is_root = 0  -- stop when we've reached the root
                  AND c.depth < 1000  -- safety limit
            )
            SELECT did, rkey, cid, node_type, prev_did, prev_rkey
            FROM chain
            ORDER BY depth DESC  -- root first, then diffs in order
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(root_did)
            .bind(root_rkey)
            .bind(head_did)
            .bind(head_rkey)
            .bind(root_did)
            .bind(root_rkey)
            .fetch_all::<EditChainNode>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get edit chain".into(),
                source: e,
            })?;

        // Validate chain terminates at expected root
        if let Some(first) = rows.first() {
            if first.did != root_did || first.rkey != root_rkey {
                return Err(ClickHouseError::Query {
                    message: format!(
                        "edit chain did not terminate at expected root {}:{}, got {}:{}",
                        root_did, root_rkey, first.did, first.rkey
                    ),
                    source: clickhouse::error::Error::Custom("chain validation failed".into()),
                }
                .into());
            }
        }

        Ok(rows)
    }

    /// Upsert a draft title after extraction.
    pub async fn upsert_draft_title(
        &self,
        did: &str,
        rkey: &str,
        title: &str,
        head_did: &str,
        head_rkey: &str,
        head_cid: &str,
    ) -> Result<(), IndexError> {
        let query = r#"
            INSERT INTO draft_titles (did, rkey, title, head_did, head_rkey, head_cid)
            VALUES (?, ?, ?, ?, ?, ?)
        "#;

        self.inner()
            .query(query)
            .bind(did)
            .bind(rkey)
            .bind(title)
            .bind(head_did)
            .bind(head_rkey)
            .bind(head_cid)
            .execute()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to upsert draft title".into(),
                source: e,
            })?;

        Ok(())
    }
}
