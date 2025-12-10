-- Account events from firehose (#account messages)
-- Tracks account status changes: active, deactivated, deleted, suspended, takendown

CREATE TABLE IF NOT EXISTS raw_account_events (
    -- The DID this account event is about
    did String,

    -- Whether the account is active
    active UInt8,

    -- Account status: 'active', 'deactivated', 'deleted', 'suspended', 'takendown'
    status LowCardinality(String),

    -- Sequence number from firehose
    seq UInt64,

    -- Event timestamp from firehose
    event_time DateTime64(3),

    -- When we indexed this event
    indexed_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (did, indexed_at);
