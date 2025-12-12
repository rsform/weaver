-- Collaboration invites
-- Half of a two-way collaboration agreement

CREATE TABLE IF NOT EXISTS collab_invites (
    -- Invite record identity
    did String,
    rkey String,
    cid String,
    uri String MATERIALIZED concat('at://', did, '/sh.weaver.collab.invite/', rkey),

    -- Resource being shared (decomposed)
    resource_did String,
    resource_collection LowCardinality(String),
    resource_rkey String,
    resource_uri String MATERIALIZED concat('at://', resource_did, '/', resource_collection, '/', resource_rkey),

    -- Inviter is the record author (did)
    inviter_did String MATERIALIZED did,

    -- Invitee
    invitee_did String,

    -- Optional fields
    scope LowCardinality(String) DEFAULT '',
    message String DEFAULT '',
    expires_at DateTime64(3) DEFAULT toDateTime64(0, 3),

    -- Timestamps
    created_at DateTime64(3),
    event_time DateTime64(3),
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Soft delete (epoch = not deleted)
    deleted_at DateTime64(3) DEFAULT toDateTime64(0, 3)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (did, rkey)
