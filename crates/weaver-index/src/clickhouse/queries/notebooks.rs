//! Notebook and entry queries

use clickhouse::Row;
use serde::Deserialize;
use smol_str::SmolStr;

use crate::clickhouse::Client;
use crate::error::{ClickHouseError, IndexError};

/// Notebook row from the notebooks table
#[derive(Debug, Clone, Row, Deserialize)]
pub struct NotebookRow {
    pub did: SmolStr,
    pub rkey: SmolStr,
    pub cid: SmolStr,
    pub uri: SmolStr,
    pub title: SmolStr,
    pub path: SmolStr,
    pub tags: Vec<SmolStr>,
    pub author_dids: Vec<SmolStr>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub indexed_at: chrono::DateTime<chrono::Utc>,
    pub record: SmolStr,
}

/// Entry row from the entries table
#[derive(Debug, Clone, Row, Deserialize)]
pub struct EntryRow {
    pub did: SmolStr,
    pub rkey: SmolStr,
    pub cid: SmolStr,
    pub uri: SmolStr,
    pub title: SmolStr,
    pub path: SmolStr,
    pub tags: Vec<SmolStr>,
    pub author_dids: Vec<SmolStr>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub indexed_at: chrono::DateTime<chrono::Utc>,
    pub record: SmolStr,
}

impl Client {
    /// Resolve a notebook by actor DID and path/title.
    ///
    /// Searches both path and title fields for flexibility.
    pub async fn resolve_notebook(
        &self,
        did: &str,
        name: &str,
    ) -> Result<Option<NotebookRow>, IndexError> {
        let query = r#"
            SELECT
                did,
                rkey,
                cid,
                uri,
                title,
                path,
                tags,
                author_dids,
                indexed_at,
                record
            FROM notebooks
            WHERE did = ?
              AND (path = ? OR title = ?)
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY event_time DESC
            LIMIT 1
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(did)
            .bind(name)
            .bind(name)
            .fetch_optional::<NotebookRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to resolve notebook".into(),
                source: e,
            })?;

        Ok(row)
    }

    /// Get a notebook by DID + rkey.
    ///
    /// Use this after parsing/resolving AT URIs in the handler.
    pub async fn get_notebook(
        &self,
        did: &str,
        rkey: &str,
    ) -> Result<Option<NotebookRow>, IndexError> {
        let query = r#"
            SELECT
                did,
                rkey,
                cid,
                uri,
                title,
                path,
                tags,
                author_dids,
                indexed_at,
                record
            FROM notebooks
            WHERE did = ?
              AND rkey = ?
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY event_time DESC
            LIMIT 1
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(did)
            .bind(rkey)
            .fetch_optional::<NotebookRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get notebook".into(),
                source: e,
            })?;

        Ok(row)
    }

    /// List entries for a notebook's author (did).
    ///
    /// Note: This is a simplified version. The full implementation would
    /// need to join with notebook's entryList to get proper ordering.
    /// For now, we just list entries by the same author.
    pub async fn list_notebook_entries(
        &self,
        did: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<Vec<EntryRow>, IndexError> {
        let query = if cursor.is_some() {
            r#"
                SELECT
                    did,
                    rkey,
                    cid,
                    uri,
                    title,
                    path,
                    tags,
                    author_dids,
                    indexed_at,
                    record
                FROM entries
                WHERE did = ?
                  AND deleted_at = toDateTime64(0, 3)
                  AND rkey > ?
                ORDER BY rkey ASC
                LIMIT ?
            "#
        } else {
            r#"
                SELECT
                    did,
                    rkey,
                    cid,
                    uri,
                    title,
                    path,
                    tags,
                    author_dids,
                    indexed_at,
                    record
                FROM entries
                WHERE did = ?
                  AND deleted_at = toDateTime64(0, 3)
                ORDER BY rkey ASC
                LIMIT ?
            "#
        };

        let mut q = self.inner().query(query).bind(did);

        if let Some(c) = cursor {
            q = q.bind(c);
        }

        let rows =
            q.bind(limit)
                .fetch_all::<EntryRow>()
                .await
                .map_err(|e| ClickHouseError::Query {
                    message: "failed to list notebook entries".into(),
                    source: e,
                })?;

        Ok(rows)
    }

    /// Get an entry by rkey, picking the most recent version across collaborators.
    ///
    /// For collaborative entries, the same rkey may exist in multiple repos.
    /// This returns the most recently updated version, with indexed_at as tiebreaker.
    ///
    /// `candidate_dids` should include the notebook owner + all collaborator DIDs.
    pub async fn get_entry(
        &self,
        rkey: &str,
        candidate_dids: &[&str],
    ) -> Result<Option<EntryRow>, IndexError> {
        if candidate_dids.is_empty() {
            return Ok(None);
        }

        // Build placeholders for IN clause
        let placeholders: Vec<_> = candidate_dids.iter().map(|_| "?").collect();
        let query = format!(
            r#"
            SELECT
                did,
                rkey,
                cid,
                uri,
                title,
                path,
                tags,
                author_dids,
                indexed_at,
                record
            FROM entries
            WHERE rkey = ?
              AND did IN ({})
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY updated_at DESC, indexed_at DESC
            LIMIT 1
            "#,
            placeholders.join(", ")
        );

        let mut q = self.inner().query(&query).bind(rkey);
        for did in candidate_dids {
            q = q.bind(*did);
        }

        let row = q
            .fetch_optional::<EntryRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get entry".into(),
                source: e,
            })?;

        Ok(row)
    }

    /// Get an entry by exact DID + rkey (no collaborator lookup).
    ///
    /// Use when you know the specific repo you want.
    pub async fn get_entry_exact(
        &self,
        did: &str,
        rkey: &str,
    ) -> Result<Option<EntryRow>, IndexError> {
        let query = r#"
            SELECT
                did,
                rkey,
                cid,
                uri,
                title,
                path,
                tags,
                author_dids,
                indexed_at,
                record
            FROM entries
            WHERE did = ?
              AND rkey = ?
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY updated_at DESC, indexed_at DESC
            LIMIT 1
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(did)
            .bind(rkey)
            .fetch_optional::<EntryRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get entry".into(),
                source: e,
            })?;

        Ok(row)
    }

    /// Resolve an entry by actor DID and path/title.
    pub async fn resolve_entry(
        &self,
        did: &str,
        name: &str,
    ) -> Result<Option<EntryRow>, IndexError> {
        let query = r#"
            SELECT
                did,
                rkey,
                cid,
                uri,
                title,
                path,
                tags,
                author_dids,
                indexed_at,
                record
            FROM entries
            WHERE did = ?
              AND (path = ? OR title = ?)
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY event_time DESC
            LIMIT 1
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(did)
            .bind(name)
            .bind(name)
            .fetch_optional::<EntryRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to resolve entry".into(),
                source: e,
            })?;

        Ok(row)
    }
}
