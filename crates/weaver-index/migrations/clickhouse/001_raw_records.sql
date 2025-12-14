-- Raw records from firehose/jetstream
-- Core table for all AT Protocol records before denormalization

CREATE TABLE IF NOT EXISTS raw_records (
    -- Decomposed AT URI components (at://did/collection/rkey)
    did String,
    collection LowCardinality(String),
    rkey String,
    cid String,
    -- Repository revision (TID) - monotonically increasing per DID, used for ordering
    rev String,
    record JSON,
    -- Operation: 'create', 'update', 'delete', ('cache' - fetched on-demand)
    operation LowCardinality(String),
    -- Firehose sequence number
    seq UInt64,
    -- Event timestamp from firehose
    event_time DateTime64(3),
    -- When the database indexed this record
    indexed_at DateTime64(3) DEFAULT now64(3),
    -- Validation state: 'unchecked', 'valid', 'invalid_rev', 'invalid_gap', 'invalid_account'
    validation_state LowCardinality(String) DEFAULT 'unchecked',
    -- Whether this came from live firehose (true) or backfill (false)
    is_live Bool DEFAULT true,
    -- Materialized AT URI for convenience
    uri String MATERIALIZED concat('at://', did, '/', collection, '/', rkey),
    -- Projection for fast delete lookups by (did, cid)
    PROJECTION by_did_cid (
        SELECT * ORDER BY (did, cid)
    )
)
ENGINE = MergeTree()
ORDER BY (collection, did, rkey, event_time, indexed_at);
