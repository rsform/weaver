-- Dead-letter queue for malformed events
-- Events that couldn't be parsed or processed land here for debugging

CREATE TABLE IF NOT EXISTS raw_events_dlq (
    -- Event type we attempted to parse (if known)
    event_type LowCardinality(String),

    -- Raw event data (JSON string of whatever we received)
    raw_data String,

    -- Error message describing why parsing failed
    error_message String,

    -- Sequence number from firehose (if available)
    seq UInt64,

    -- When we received this event
    received_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (received_at);
