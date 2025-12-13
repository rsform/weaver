//! Identity resolution queries (handle <-> DID mappings)

use clickhouse::Row;
use serde::Deserialize;
use smol_str::SmolStr;

use crate::clickhouse::Client;
use crate::error::{ClickHouseError, IndexError};

/// Handle mapping row from handle_mappings table
#[derive(Debug, Clone, Row, Deserialize)]
pub struct HandleMappingRow {
    pub handle: SmolStr,
    pub did: SmolStr,
    pub freed: u8,
    pub account_status: SmolStr,
}

impl Client {
    /// Resolve a handle to a DID using the handle_mappings table.
    ///
    /// Returns the active (non-freed) mapping for the handle, if one exists.
    /// Query orders by freed ASC, event_time DESC to get active mapping first.
    pub async fn resolve_handle(&self, handle: &str) -> Result<Option<HandleMappingRow>, IndexError> {
        let query = r#"
            SELECT handle, did, freed, account_status
            FROM handle_mappings
            WHERE handle = ?
            ORDER BY freed ASC, event_time DESC
            LIMIT 1
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(handle)
            .fetch_optional::<HandleMappingRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to resolve handle".into(),
                source: e,
            })?;

        // Only return if not freed (active mapping)
        Ok(row.filter(|r| r.freed == 0))
    }

    /// Resolve a DID to its current handle using the handle_mappings table.
    ///
    /// Uses the by_did projection for efficient lookup.
    pub async fn resolve_did_to_handle(&self, did: &str) -> Result<Option<HandleMappingRow>, IndexError> {
        let query = r#"
            SELECT handle, did, freed, account_status
            FROM handle_mappings
            WHERE did = ?
            ORDER BY freed ASC, event_time DESC
            LIMIT 1
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(did)
            .fetch_optional::<HandleMappingRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to resolve DID to handle".into(),
                source: e,
            })?;

        // Only return if not freed (active mapping)
        Ok(row.filter(|r| r.freed == 0))
    }

    /// Cache a handle resolution result in handle_mappings.
    ///
    /// Used when we resolve via external resolver and want to cache the result.
    pub async fn cache_handle_resolution(
        &self,
        handle: &str,
        did: &str,
    ) -> Result<(), IndexError> {
        use chrono::Utc;

        let query = r#"
            INSERT INTO handle_mappings (handle, did, freed, account_status, source, event_time)
            VALUES (?, ?, 0, 'active', 'resolution', ?)
        "#;

        self.inner()
            .query(query)
            .bind(handle)
            .bind(did)
            .bind(Utc::now().timestamp_millis())
            .execute()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to cache handle resolution".into(),
                source: e,
            })?;

        Ok(())
    }
}
