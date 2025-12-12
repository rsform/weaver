-- Collaboration accepts
-- Completes the two-way collaboration agreement

CREATE TABLE IF NOT EXISTS collab_accepts (
    -- Accept record identity
    did String,
    rkey String,
    cid String,
    uri String MATERIALIZED concat('at://', did, '/sh.weaver.collab.accept/', rkey),

    -- Invite being accepted (decomposed)
    invite_did String,
    invite_rkey String,
    invite_cid String DEFAULT '',
    invite_uri String MATERIALIZED concat('at://', invite_did, '/sh.weaver.collab.invite/', invite_rkey),

    -- Resource (denormalized in the record)
    resource_did String,
    resource_collection LowCardinality(String),
    resource_rkey String,
    resource_uri String MATERIALIZED concat('at://', resource_did, '/', resource_collection, '/', resource_rkey),

    -- Accepter is the record author (did)
    accepter_did String MATERIALIZED did,

    -- Timestamps
    created_at DateTime64(3),
    event_time DateTime64(3),
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Soft delete (epoch = not deleted)
    deleted_at DateTime64(3) DEFAULT toDateTime64(0, 3)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (did, rkey)
