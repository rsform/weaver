-- Handle → DID mappings with account status tracking
--
-- Updated from three sources:
-- 1. Identity events (firehose) - handle claims/changes via 009_handle_mappings_identity_mv.sql
-- 2. Account events (firehose) - takedowns/suspensions/deletions via 010_handle_mappings_account_mv.sql
-- 3. Resolution cache flush - XRPC handle resolution results (manual inserts)
--
-- Query pattern: ORDER BY freed ASC, event_time DESC to get active mapping first

CREATE TABLE IF NOT EXISTS handle_mappings (
    handle String,
    did String,

    -- 0 = active, 1 = account deactivated/suspended/deleted
    freed UInt8 DEFAULT 0,

    -- 'active' | 'takendown' | 'suspended' | 'deleted' | 'deactivated'
    account_status LowCardinality(String) DEFAULT 'active',

    -- 'identity' (firehose) | 'account' (firehose) | 'resolution' (xrpc cache)
    source LowCardinality(String),

    -- Canonical event time (firehose event or resolution time)
    event_time DateTime64(3),

    -- When we indexed this mapping
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Fast DID → handle lookups (for account events, profile hydration)
    -- Query with ORDER BY freed ASC, event_time DESC to get active mapping
    PROJECTION by_did (
        SELECT * ORDER BY (did, freed, event_time)
    )
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (handle, did)
SETTINGS deduplicate_merge_projection_mode = 'drop';
