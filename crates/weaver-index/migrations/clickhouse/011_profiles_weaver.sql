-- Weaver profile source table

CREATE TABLE IF NOT EXISTS profiles_weaver (
    did String,
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
