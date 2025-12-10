-- Firehose cursor persistence
-- Tracks our position in the firehose stream for resumption after restart

CREATE TABLE IF NOT EXISTS firehose_cursor (
    -- Consumer identifier (allows multiple consumers with different cursors)
    consumer_id String,

    -- Last successfully processed sequence number
    seq UInt64,

    -- Timestamp of the last processed event
    event_time DateTime64(3),

    -- When we saved this cursor
    updated_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (consumer_id);
