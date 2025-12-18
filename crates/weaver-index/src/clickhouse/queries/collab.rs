//! Collaboration and permission queries

use clickhouse::Row;
use serde::Deserialize;
use smol_str::SmolStr;

use crate::clickhouse::Client;
use crate::error::{ClickHouseError, IndexError};

/// Permission row from the permissions materialized view
#[derive(Debug, Clone, Row, Deserialize)]
pub struct PermissionRow {
    pub resource_did: SmolStr,
    pub resource_collection: SmolStr,
    pub resource_rkey: SmolStr,
    pub resource_uri: SmolStr,
    pub grantee_did: SmolStr,
    pub scope: SmolStr,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub granted_at: chrono::DateTime<chrono::Utc>,
}

impl Client {
    /// Get all permissions for a resource by URI.
    ///
    /// Returns owner and all collaborators who have accepted invites.
    pub async fn get_resource_permissions(
        &self,
        resource_uri: &str,
    ) -> Result<Vec<PermissionRow>, IndexError> {
        let query = r#"
            SELECT
                resource_did,
                resource_collection,
                resource_rkey,
                resource_uri,
                grantee_did,
                scope,
                granted_at
            FROM permissions FINAL
            WHERE resource_uri = ?
            ORDER BY scope DESC, granted_at ASC
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(resource_uri)
            .fetch_all::<PermissionRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get resource permissions".into(),
                source: e,
            })?;

        Ok(rows)
    }

    /// Check if a DID can edit a resource.
    ///
    /// Returns true if the DID is owner or collaborator.
    pub async fn can_edit_resource(
        &self,
        resource_uri: &str,
        did: &str,
    ) -> Result<bool, IndexError> {
        let query = r#"
            SELECT count(*) as cnt
            FROM permissions FINAL
            WHERE resource_uri = ?
              AND grantee_did = ?
        "#;

        #[derive(Row, Deserialize)]
        struct CountRow {
            cnt: u64,
        }

        let row = self
            .inner()
            .query(query)
            .bind(resource_uri)
            .bind(did)
            .fetch_one::<CountRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to check edit permission".into(),
                source: e,
            })?;

        Ok(row.cnt > 0)
    }

    /// Get active collaboration sessions for a resource.
    ///
    /// Sessions are stored as records with collection `sh.weaver.collab.session`.
    /// They have a TTL (expiresAt) and should be filtered to only return unexpired sessions.
    pub async fn get_resource_sessions(
        &self,
        resource_uri: &str,
    ) -> Result<Vec<SessionRow>, IndexError> {
        let query = r#"
            SELECT
                did,
                rkey,
                cid,
                record.nodeId AS node_id,
                record.relayUrl AS relay_url,
                record.createdAt AS created_at,
                record.expiresAt AS expires_at
            FROM raw_records
            WHERE collection = 'sh.weaver.collab.session'
              AND is_live = 1
              AND record.resource.uri = ?
              AND (
                  record.expiresAt IS NULL
                  OR record.expiresAt > now64(3)
              )
            ORDER BY record.createdAt.:DateTime64 DESC
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(resource_uri)
            .fetch_all::<SessionRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get resource sessions".into(),
                source: e,
            })?;

        Ok(rows)
    }
}

/// Session record row
#[derive(Debug, Clone, Row, Deserialize)]
pub struct SessionRow {
    pub did: SmolStr,
    pub rkey: SmolStr,
    pub cid: SmolStr,
    pub node_id: SmolStr,
    pub relay_url: SmolStr,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis::option")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}
