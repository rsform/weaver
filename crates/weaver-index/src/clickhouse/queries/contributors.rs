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

/// Row for batch contributor query (includes entry identity)
#[derive(Debug, Clone, Row, Deserialize)]
struct BatchContributorRow {
    entry_did: SmolStr,
    entry_rkey: SmolStr,
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

    /// Get contributors for an entry.
    ///
    /// Returns DIDs of users who have contributed to this entry.
    pub async fn get_entry_contributors(
        &self,
        entry_did: &str,
        entry_rkey: &str,
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

    /// Batch get contributors for multiple entries.
    ///
    /// Returns a map of (did, rkey) -> Vec<contributor_did>.
    pub async fn get_entry_contributors_batch(
        &self,
        entries: &[(&str, &str)], // Vec of (did, rkey)
    ) -> Result<std::collections::HashMap<(SmolStr, SmolStr), Vec<SmolStr>>, IndexError> {
        use std::collections::HashMap;

        if entries.is_empty() {
            return Ok(HashMap::new());
        }

        // Build (did, rkey) tuples for the IN clause
        let tuples: String = entries
            .iter()
            .map(|(did, rkey)| format!("('{}', '{}')", did, rkey))
            .collect::<Vec<_>>()
            .join(", ");

        let query = format!(
            r#"
            SELECT resource_did AS entry_did, resource_rkey AS entry_rkey, contributor_did
            FROM contributors FINAL
            WHERE (resource_did, resource_rkey) IN ({tuples})
              AND resource_collection = 'sh.weaver.notebook.entry'
            "#
        );

        let rows = self
            .inner()
            .query(&query)
            .fetch_all::<BatchContributorRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to batch get entry contributors".into(),
                source: e,
            })?;

        // Group by (entry_did, entry_rkey)
        let mut result: HashMap<(SmolStr, SmolStr), Vec<SmolStr>> = HashMap::new();
        for row in rows {
            result
                .entry((row.entry_did, row.entry_rkey))
                .or_default()
                .push(row.contributor_did);
        }

        // Dedupe each entry's contributors
        for contributors in result.values_mut() {
            contributors.sort();
            contributors.dedup();
        }

        Ok(result)
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
