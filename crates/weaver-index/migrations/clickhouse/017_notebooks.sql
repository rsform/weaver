-- Notebooks table
-- Populated by MV from raw_records

CREATE TABLE IF NOT EXISTS notebooks (
    -- Identity
    did String,
    rkey String,
    cid String,

    -- Materialized URI for convenience
    uri String MATERIALIZED concat('at://', did, '/sh.weaver.notebook.book/', rkey),

    -- Extracted fields
    title String DEFAULT '',
    path String DEFAULT '',
    description String DEFAULT '',
    tags Array(String) DEFAULT [],
    publish_global UInt8 DEFAULT 0,

    -- Authors array (DIDs)
    author_dids Array(String) DEFAULT [],

    -- Entry count (length of entryList)
    entry_count UInt32 DEFAULT 0,

    -- Timestamps
    created_at DateTime64(3) DEFAULT toDateTime64(0, 3),
    updated_at DateTime64(3) DEFAULT toDateTime64(0, 3),
    event_time DateTime64(3),
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Soft delete (epoch = not deleted)
    deleted_at DateTime64(3) DEFAULT toDateTime64(0, 3)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (did, rkey)
