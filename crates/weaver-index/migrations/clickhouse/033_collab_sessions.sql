-- Active collaboration sessions
-- Published when joining collaborative editing, deleted on disconnect

CREATE TABLE IF NOT EXISTS collab_sessions (
    -- Session record identity
    did String,
    rkey String,
    cid String,
    uri String MATERIALIZED concat('at://', did, '/sh.weaver.collab.session/', rkey),

    -- Resource being edited (decomposed)
    resource_did String,
    resource_collection LowCardinality(String),
    resource_rkey String,
    resource_uri String MATERIALIZED concat('at://', resource_did, '/', resource_collection, '/', resource_rkey),

    -- Participant is the record author (did)
    participant_did String MATERIALIZED did,

    -- Connection info
    node_id String,
    relay_url String DEFAULT '',

    -- Timestamps
    created_at DateTime64(3),
    expires_at DateTime64(3) DEFAULT toDateTime64(0, 3),
    event_time DateTime64(3),
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Soft delete (epoch = not deleted)
    deleted_at DateTime64(3) DEFAULT toDateTime64(0, 3)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (did, rkey)
