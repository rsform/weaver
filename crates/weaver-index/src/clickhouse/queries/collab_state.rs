//! Collaboration state queries

use clickhouse::Row;
use serde::Deserialize;
use smol_str::SmolStr;

use crate::clickhouse::Client;
use crate::error::{ClickHouseError, IndexError};

/// Collaborator row from the collaborators MV
#[derive(Debug, Clone, Row, Deserialize)]
pub struct CollaboratorRow {
    pub resource_uri: SmolStr,
    pub collaborator_did: SmolStr,
    pub inviter_did: SmolStr,
    pub invite_uri: SmolStr,
    pub accept_uri: SmolStr,
    pub scope: SmolStr,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub invited_at: chrono::DateTime<chrono::Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub accepted_at: chrono::DateTime<chrono::Utc>,
}

/// Edit head row from the edit_heads MV
#[derive(Debug, Clone, Row, Deserialize)]
pub struct EditHeadRow {
    pub resource_uri: SmolStr,
    pub head_did: SmolStr,
    pub head_rkey: SmolStr,
    pub head_cid: SmolStr,
    pub head_uri: SmolStr,
    pub head_type: SmolStr,
    pub root_did: SmolStr,
    pub root_rkey: SmolStr,
    pub root_cid: SmolStr,
    pub root_uri: SmolStr,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub head_created_at: chrono::DateTime<chrono::Utc>,
}

impl Client {
    /// Get collaborators for a resource (matched invite+accept pairs).
    pub async fn get_collaborators(
        &self,
        resource_uri: &str,
    ) -> Result<Vec<CollaboratorRow>, IndexError> {
        let query = r#"
            SELECT
                resource_uri,
                collaborator_did,
                inviter_did,
                invite_uri,
                accept_uri,
                scope,
                invited_at,
                accepted_at
            FROM collaborators FINAL
            WHERE resource_uri = ?
            ORDER BY accepted_at ASC
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(resource_uri)
            .fetch_all::<CollaboratorRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get collaborators".into(),
                source: e,
            })?;

        Ok(rows)
    }

    /// Get edit heads for a resource.
    ///
    /// Multiple heads means divergent branches.
    pub async fn get_edit_heads(&self, resource_uri: &str) -> Result<Vec<EditHeadRow>, IndexError> {
        let query = r#"
            SELECT
                resource_uri,
                head_did,
                head_rkey,
                head_cid,
                head_uri,
                head_type,
                root_did,
                root_rkey,
                root_cid,
                root_uri,
                head_created_at
            FROM edit_heads FINAL
            WHERE resource_uri = ?
            ORDER BY head_created_at DESC
        "#;

        let rows = self
            .inner()
            .query(query)
            .bind(resource_uri)
            .fetch_all::<EditHeadRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get edit heads".into(),
                source: e,
            })?;

        Ok(rows)
    }

    /// Check if resource has divergent branches (more than one head).
    pub async fn has_divergence(&self, resource_uri: &str) -> Result<bool, IndexError> {
        let heads = self.get_edit_heads(resource_uri).await?;
        Ok(heads.len() > 1)
    }

    /// Get CID for a record from raw_records.
    pub async fn get_record_cid(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
    ) -> Result<Option<SmolStr>, IndexError> {
        #[derive(Row, Deserialize)]
        struct CidRow {
            cid: SmolStr,
        }

        let query = r#"
            SELECT cid
            FROM raw_records FINAL
            WHERE did = ?
              AND collection = ?
              AND rkey = ?
              AND operation != 'delete'
            LIMIT 1
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(did)
            .bind(collection)
            .bind(rkey)
            .fetch_optional::<CidRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get record cid".into(),
                source: e,
            })?;

        Ok(row.map(|r| r.cid))
    }
}
