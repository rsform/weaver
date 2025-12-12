-- Bluesky profile source table
-- Populated by MV from raw_records, merged into profiles by refreshable MV

CREATE TABLE IF NOT EXISTS profiles_bsky (
    did String,

    -- Raw profile JSON
    profile String,

    -- Extracted fields for coalescing
    display_name String DEFAULT '',
    description String DEFAULT '',
    avatar_cid String DEFAULT '',
    banner_cid String DEFAULT '',

    -- Timestamps
    created_at DateTime64(3) DEFAULT toDateTime64(0, 3),
    event_time DateTime64(3),
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Soft delete (epoch = not deleted)
    deleted_at DateTime64(3) DEFAULT toDateTime64(0, 3)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY did
