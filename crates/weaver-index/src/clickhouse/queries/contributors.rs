//! Contributor queries
//!
//! Finds contributors (authors) for resources based on evidence.
//! Uses the precomputed `contributors` MV which tracks:
//! - Resource owners
//! - Editors (edit nodes)
//! - Collaborators who have published (same rkey in their repo)

use clickhouse::Row;
use serde::Deserialize;
use smol_str::SmolStr;

use crate::clickhouse::Client;
use crate::error::{ClickHouseError, IndexError};

/// Single-column row for contributor DID
#[derive(Debug, Clone, Row, Deserialize)]
struct ContributorRow {
    contributor_did: SmolStr,
}

/// Row for notebook lookup
#[derive(Debug, Clone, Row, Deserialize)]
struct NotebookRefRow {
    notebook_did: SmolStr,
    notebook_rkey: SmolStr,
}

impl Client {
    /// Find notebooks containing an entry.
    ///
    /// Returns (notebook_did, notebook_rkey) pairs for notebooks that contain this entry.
    pub async fn get_notebooks_for_entry(
        &self,
        entry_did: &str,
        entry_rkey: &str,
    ) -> Result<Vec<(SmolStr, SmolStr)>, IndexError> {
        let query = r#"
            SELECT notebook_did, notebook_rkey
            FROM notebook_entries FINAL
            WHERE entry_did = ?
              AND entry_rkey = ?
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(entry_did)
            .bind(entry_rkey)
            .fetch_all::<NotebookRefRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get notebooks for entry".into(),
                source: e,
            })?;

        Ok(rows
            .into_iter()
            .map(|r| (r.notebook_did, r.notebook_rkey))
            .collect())
    }

    /// Get contributors for an entry (direct only, no cascade).
    ///
    /// Returns DIDs of users who have directly contributed to this entry.
    pub async fn get_entry_contributors_direct(
        &self,
        resource_did: &str,
        resource_rkey: &str,
    ) -> Result<Vec<SmolStr>, IndexError> {
        let query = r#"
            SELECT DISTINCT contributor_did
            FROM contributors FINAL
            WHERE resource_did = ?
              AND resource_rkey = ?
              AND resource_collection = 'sh.weaver.notebook.entry'
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(resource_did)
            .bind(resource_rkey)
            .fetch_all::<ContributorRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get entry contributors".into(),
                source: e,
            })?;

        Ok(rows.into_iter().map(|r| r.contributor_did).collect())
    }

    /// Get contributors for an entry, including cascaded notebook-level collaborators.
    ///
    /// Returns DIDs of users who have contributed to this entry OR are
    /// notebook-level collaborators for a notebook containing this entry.
    pub async fn get_entry_contributors(
        &self,
        entry_did: &str,
        entry_rkey: &str,
    ) -> Result<Vec<SmolStr>, IndexError> {
        // Single query that unions:
        // 1. Direct entry contributors
        // 2. Notebook collaborators for notebooks containing this entry
        let query = r#"
            SELECT DISTINCT contributor_did FROM (
                -- Direct entry contributors
                SELECT contributor_did
                FROM contributors FINAL
                WHERE resource_did = ?
                  AND resource_rkey = ?
                  AND resource_collection = 'sh.weaver.notebook.entry'

                UNION ALL

                -- Notebook-level collaborators (cascaded)
                SELECT c.contributor_did
                FROM notebook_entries ne FINAL
                INNER JOIN contributors c FINAL ON
                    c.resource_did = ne.notebook_did
                    AND c.resource_rkey = ne.notebook_rkey
                    AND c.resource_collection = 'sh.weaver.notebook.book'
                WHERE ne.entry_did = ?
                  AND ne.entry_rkey = ?
            )
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(entry_did)
            .bind(entry_rkey)
            .bind(entry_did)
            .bind(entry_rkey)
            .fetch_all::<ContributorRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get entry contributors".into(),
                source: e,
            })?;

        Ok(rows.into_iter().map(|r| r.contributor_did).collect())
    }

    /// Get contributors for a notebook.
    ///
    /// Returns DIDs of users who have contributed to this notebook.
    /// Uses the precomputed contributors MV.
    pub async fn get_notebook_contributors(
        &self,
        resource_did: &str,
        resource_rkey: &str,
    ) -> Result<Vec<SmolStr>, IndexError> {
        let query = r#"
            SELECT DISTINCT contributor_did
            FROM contributors FINAL
            WHERE resource_did = ?
              AND resource_rkey = ?
              AND resource_collection = 'sh.weaver.notebook.book'
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(resource_did)
            .bind(resource_rkey)
            .fetch_all::<ContributorRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get notebook contributors".into(),
                source: e,
            })?;

        Ok(rows.into_iter().map(|r| r.contributor_did).collect())
    }
}
