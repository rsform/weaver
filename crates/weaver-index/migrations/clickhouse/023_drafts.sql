-- Draft stub records
-- Anchors for unpublished content, enables draft discovery via queries

CREATE TABLE IF NOT EXISTS drafts (
    -- Identity
    did String,
    rkey String,
    cid String,

    -- Materialized URI for convenience
    uri String MATERIALIZED concat('at://', did, '/sh.weaver.edit.draft/', rkey),

    -- Timestamps
    created_at DateTime64(3) DEFAULT toDateTime64(0, 3),
    event_time DateTime64(3),
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Soft delete (epoch = not deleted)
    deleted_at DateTime64(3) DEFAULT toDateTime64(0, 3)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (did, rkey)
