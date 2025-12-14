-- Identity events from firehose (#identity messages)
-- Tracks handle changes, key rotation, etc.

CREATE TABLE IF NOT EXISTS raw_identity_events (
    -- The DID this identity event is about
    did String,

    -- Handle (may be empty)
    handle String,

    -- Sequence number from firehose
    seq UInt64,

    -- Event timestamp from firehose
    event_time DateTime64(3),

    -- When we indexed this event
    indexed_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (did, indexed_at);
