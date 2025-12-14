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
    pub created_at: chrono::DateTime<chrono::Utc>,
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
    pub created_at: chrono::DateTime<chrono::Utc>,
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
                created_at,
                indexed_at,
                record
            FROM notebooks
            WHERE did = ?
              AND (path = ? OR title = ?)
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
                created_at,
                indexed_at,
                record
            FROM notebooks
            WHERE did = ?
              AND rkey = ?
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
    /// For now, we just list entries by the same author, ordered by rkey (notebook order).
    pub async fn list_notebook_entries(
        &self,
        did: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<Vec<EntryRow>, IndexError> {
        // Note: rkey ordering is intentional here - it's the notebook's entry order
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
                    created_at,
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
                    created_at,
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
    /// This returns the most recently updated version.
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
                created_at,
                indexed_at,
                record
            FROM entries
            WHERE rkey = ?
              AND did IN ({})
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
                created_at,
                indexed_at,
                record
            FROM entries
            WHERE did = ?
              AND rkey = ?
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
                created_at,
                indexed_at,
                record
            FROM entries
            WHERE did = ?
              AND (path = ? OR title = ?)
              AND deleted_at = toDateTime64(0, 3)
            ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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

    /// List notebooks for an actor.
    ///
    /// Returns notebooks owned by the given DID, ordered by created_at DESC.
    /// Cursor is created_at timestamp in milliseconds.
    pub async fn list_actor_notebooks(
        &self,
        did: &str,
        limit: u32,
        cursor: Option<i64>,
    ) -> Result<Vec<NotebookRow>, IndexError> {
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
                    created_at,
                    indexed_at,
                    record
                FROM notebooks
                WHERE did = ?
                  AND deleted_at = toDateTime64(0, 3)
                  AND created_at < fromUnixTimestamp64Milli(?)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
                    created_at,
                    indexed_at,
                    record
                FROM notebooks
                WHERE did = ?
                  AND deleted_at = toDateTime64(0, 3)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
                LIMIT ?
            "#
        };

        let mut q = self.inner().query(query).bind(did);

        if let Some(c) = cursor {
            q = q.bind(c);
        }

        let rows = q
            .bind(limit)
            .fetch_all::<NotebookRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to list actor notebooks".into(),
                source: e,
            })?;

        Ok(rows)
    }

    /// List entries for an actor.
    ///
    /// Returns entries owned by the given DID, ordered by created_at DESC.
    /// Cursor is created_at timestamp in milliseconds.
    pub async fn list_actor_entries(
        &self,
        did: &str,
        limit: u32,
        cursor: Option<i64>,
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
                    created_at,
                    indexed_at,
                    record
                FROM entries
                WHERE did = ?
                  AND deleted_at = toDateTime64(0, 3)
                  AND created_at < fromUnixTimestamp64Milli(?)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
                    created_at,
                    indexed_at,
                    record
                FROM entries
                WHERE did = ?
                  AND deleted_at = toDateTime64(0, 3)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
                    message: "failed to list actor entries".into(),
                    source: e,
                })?;

        Ok(rows)
    }

    /// Get a global feed of notebooks.
    ///
    /// Returns notebooks ordered by created_at DESC (chronological) or by
    /// popularity metrics if algorithm is "popular".
    /// Cursor is created_at timestamp in milliseconds.
    pub async fn get_notebook_feed(
        &self,
        algorithm: &str,
        tags: Option<&[&str]>,
        limit: u32,
        cursor: Option<i64>,
    ) -> Result<Vec<NotebookRow>, IndexError> {
        // For now, just chronological. Popular would need join with counts.
        let base_query = if tags.is_some() && cursor.is_some() {
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
                    created_at,
                    indexed_at,
                    record
                FROM notebooks
                WHERE deleted_at = toDateTime64(0, 3)
                  AND hasAny(tags, ?)
                  AND created_at < fromUnixTimestamp64Milli(?)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
                LIMIT ?
            "#
        } else if tags.is_some() {
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
                    created_at,
                    indexed_at,
                    record
                FROM notebooks
                WHERE deleted_at = toDateTime64(0, 3)
                  AND hasAny(tags, ?)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
                LIMIT ?
            "#
        } else if cursor.is_some() {
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
                    created_at,
                    indexed_at,
                    record
                FROM notebooks
                WHERE deleted_at = toDateTime64(0, 3)
                  AND created_at < fromUnixTimestamp64Milli(?)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
                    created_at,
                    indexed_at,
                    record
                FROM notebooks
                WHERE deleted_at = toDateTime64(0, 3)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
                LIMIT ?
            "#
        };

        let _ = algorithm; // TODO: implement popular sorting

        let mut q = self.inner().query(base_query);

        if let Some(t) = tags {
            q = q.bind(t);
        }
        if let Some(c) = cursor {
            q = q.bind(c);
        }

        let rows = q
            .bind(limit)
            .fetch_all::<NotebookRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get notebook feed".into(),
                source: e,
            })?;

        Ok(rows)
    }

    /// Get a global feed of entries.
    ///
    /// Returns entries ordered by created_at DESC (chronological).
    /// Cursor is created_at timestamp in milliseconds.
    pub async fn get_entry_feed(
        &self,
        algorithm: &str,
        tags: Option<&[&str]>,
        limit: u32,
        cursor: Option<i64>,
    ) -> Result<Vec<EntryRow>, IndexError> {
        let base_query = if tags.is_some() && cursor.is_some() {
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
                    created_at,
                    indexed_at,
                    record
                FROM entries
                WHERE deleted_at = toDateTime64(0, 3)
                  AND hasAny(tags, ?)
                  AND created_at < fromUnixTimestamp64Milli(?)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
                LIMIT ?
            "#
        } else if tags.is_some() {
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
                    created_at,
                    indexed_at,
                    record
                FROM entries
                WHERE deleted_at = toDateTime64(0, 3)
                  AND hasAny(tags, ?)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
                LIMIT ?
            "#
        } else if cursor.is_some() {
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
                    created_at,
                    indexed_at,
                    record
                FROM entries
                WHERE deleted_at = toDateTime64(0, 3)
                  AND created_at < fromUnixTimestamp64Milli(?)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
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
                    created_at,
                    indexed_at,
                    record
                FROM entries
                WHERE deleted_at = toDateTime64(0, 3)
                ORDER BY toStartOfFiveMinutes(event_time) DESC, created_at DESC
                LIMIT ?
            "#
        };

        let _ = algorithm; // TODO: implement popular sorting

        let mut q = self.inner().query(base_query);

        if let Some(t) = tags {
            q = q.bind(t);
        }
        if let Some(c) = cursor {
            q = q.bind(c);
        }

        let rows =
            q.bind(limit)
                .fetch_all::<EntryRow>()
                .await
                .map_err(|e| ClickHouseError::Query {
                    message: "failed to get entry feed".into(),
                    source: e,
                })?;

        Ok(rows)
    }

    /// Get an entry at a specific index within a notebook.
    ///
    /// Returns the entry at the given 0-based index, plus adjacent entries for prev/next.
    pub async fn get_book_entry_at_index(
        &self,
        notebook_did: &str,
        notebook_rkey: &str,
        index: u32,
    ) -> Result<Option<(EntryRow, Option<EntryRow>, Option<EntryRow>)>, IndexError> {
        // Fetch entries for this notebook with index context
        // We need 3 entries: prev (index-1), current (index), next (index+1)
        let offset = if index > 0 { index - 1 } else { 0 };
        let fetch_count = if index > 0 { 3u32 } else { 2u32 };

        let query = r#"
            SELECT
                e.did AS did,
                e.rkey AS rkey,
                e.cid AS cid,
                e.uri AS uri,
                e.title AS title,
                e.path AS path,
                e.tags AS tags,
                e.author_dids AS author_dids,
                e.created_at AS created_at,
                e.indexed_at AS indexed_at,
                e.record AS record
            FROM notebook_entries ne FINAL
            INNER JOIN entries e ON
                e.did = ne.entry_did
                AND e.rkey = ne.entry_rkey
                AND e.deleted_at = toDateTime64(0, 3)
            WHERE ne.notebook_did = ?
              AND ne.notebook_rkey = ?
            ORDER BY ne.position ASC
            LIMIT ? OFFSET ?
        "#;

        let rows: Vec<EntryRow> = self
            .inner()
            .query(query)
            .bind(notebook_did)
            .bind(notebook_rkey)
            .bind(fetch_count)
            .bind(offset)
            .fetch_all()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get book entry at index".into(),
                source: e,
            })?;

        if rows.is_empty() {
            return Ok(None);
        }

        // Determine which row is which based on the offset
        let mut iter = rows.into_iter();
        if index == 0 {
            // No prev, rows[0] is current, rows[1] is next (if exists)
            let current = iter.next();
            let next = iter.next();
            Ok(current.map(|c| (c, None, next)))
        } else {
            // rows[0] is prev, rows[1] is current, rows[2] is next
            let prev = iter.next();
            let current = iter.next();
            let next = iter.next();
            Ok(current.map(|c| (c, prev, next)))
        }
    }
}
