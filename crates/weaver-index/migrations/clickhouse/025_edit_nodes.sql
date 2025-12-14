-- Edit nodes (roots and diffs combined)
-- Used for querying edit history from ClickHouse
-- Real-time collab queries use SQLite hot tier

CREATE TABLE IF NOT EXISTS edit_nodes (
    -- Identity
    did String,
    rkey String,
    cid String,
    collection LowCardinality(String),  -- 'sh.weaver.edit.root' or 'sh.weaver.edit.diff'

    -- Materialized URI
    uri String MATERIALIZED concat('at://', did, '/', collection, '/', rkey),

    -- Node type derived from collection
    node_type LowCardinality(String),  -- 'root' or 'diff'

    -- Resource being edited (extracted from doc.value)
    resource_type LowCardinality(String) DEFAULT '',  -- 'entry', 'notebook', 'draft'
    resource_did String DEFAULT '',
    resource_rkey String DEFAULT '',
    resource_collection LowCardinality(String) DEFAULT '',

    -- For diffs: reference to root
    root_did String DEFAULT '',
    root_rkey String DEFAULT '',
    root_cid String DEFAULT '',

    -- For diffs: reference to previous node
    prev_did String DEFAULT '',
    prev_rkey String DEFAULT '',
    prev_cid String DEFAULT '',

    has_inline_diff UInt8 DEFAULT 0,
    has_snapshot UInt8 DEFAULT 0,

    -- Timestamps
    created_at DateTime64(3) DEFAULT toDateTime64(0, 3),
    event_time DateTime64(3),
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Soft delete (epoch = not deleted)
    deleted_at DateTime64(3) DEFAULT toDateTime64(0, 3)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (did, rkey)
