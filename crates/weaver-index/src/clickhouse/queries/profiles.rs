//! Profile queries

use clickhouse::Row;
use serde::Deserialize;
use smol_str::SmolStr;

use crate::clickhouse::Client;
use crate::error::{ClickHouseError, IndexError};

/// Profile row from the profiles materialized view
#[derive(Debug, Clone, Row, Deserialize)]
pub struct ProfileRow {
    pub did: SmolStr,
    pub handle: SmolStr,
    pub weaver_profile: SmolStr,
    pub bsky_profile: SmolStr,
    pub display_name: SmolStr,
    pub description: SmolStr,
    pub avatar_cid: SmolStr,
    pub banner_cid: SmolStr,
    pub has_weaver: u8,
    pub has_bsky: u8,
}

/// Profile counts from the profile_counts table (SummingMergeTree)
#[derive(Debug, Clone, Row, Deserialize)]
pub struct ProfileCountsRow {
    pub did: SmolStr,
    pub follower_count: i64,
    pub following_count: i64,
    pub notebook_count: i64,
    pub entry_count: i64,
}

/// Combined profile with counts for getProfile response
#[derive(Debug, Clone)]
pub struct ProfileWithCounts {
    pub profile: ProfileRow,
    pub counts: Option<ProfileCountsRow>,
}

impl Client {
    /// Get a profile by DID from the profiles materialized view.
    pub async fn get_profile(&self, did: &str) -> Result<Option<ProfileRow>, IndexError> {
        let query = r#"
            SELECT
                did,
                handle,
                weaver_profile,
                bsky_profile,
                display_name,
                description,
                avatar_cid,
                banner_cid,
                has_weaver,
                has_bsky
            FROM profiles
            WHERE did = ?
            LIMIT 1
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(did)
            .fetch_optional::<ProfileRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get profile".into(),
                source: e,
            })?;

        Ok(row)
    }

    /// Get profile counts for a DID from profile_counts (SummingMergeTree).
    ///
    /// Note: SummingMergeTree requires sum() to get final values.
    pub async fn get_profile_counts(&self, did: &str) -> Result<Option<ProfileCountsRow>, IndexError> {
        let query = r#"
            SELECT
                did,
                sum(follower_count) as follower_count,
                sum(following_count) as following_count,
                sum(notebook_count) as notebook_count,
                sum(entry_count) as entry_count
            FROM profile_counts
            WHERE did = ?
            GROUP BY did
        "#;

        let row = self
            .inner()
            .query(query)
            .bind(did)
            .fetch_optional::<ProfileCountsRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get profile counts".into(),
                source: e,
            })?;

        Ok(row)
    }

    /// Get a profile with counts in a single call.
    ///
    /// Runs both queries concurrently for efficiency.
    pub async fn get_profile_with_counts(&self, did: &str) -> Result<Option<ProfileWithCounts>, IndexError> {
        let (profile, counts) = tokio::join!(
            self.get_profile(did),
            self.get_profile_counts(did)
        );

        let profile = profile?;
        let counts = counts?;

        Ok(profile.map(|p| ProfileWithCounts {
            profile: p,
            counts,
        }))
    }

    /// Batch get profiles for multiple DIDs.
    ///
    /// Useful for hydrating author lists on notebooks/entries.
    pub async fn get_profiles_batch(&self, dids: &[&str]) -> Result<Vec<ProfileRow>, IndexError> {
        if dids.is_empty() {
            return Ok(Vec::new());
        }

        // Build placeholders for IN clause
        let placeholders: Vec<_> = dids.iter().map(|_| "?").collect();
        let query = format!(
            r#"
            SELECT
                did,
                handle,
                weaver_profile,
                bsky_profile,
                display_name,
                description,
                avatar_cid,
                banner_cid,
                has_weaver,
                has_bsky
            FROM profiles
            WHERE did IN ({})
            "#,
            placeholders.join(", ")
        );

        let mut q = self.inner().query(&query);
        for did in dids {
            q = q.bind(*did);
        }

        let rows = q
            .fetch_all::<ProfileRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to batch get profiles".into(),
                source: e,
            })?;

        Ok(rows)
    }
}
