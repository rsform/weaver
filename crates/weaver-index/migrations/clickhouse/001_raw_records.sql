-- Raw records from firehose/jetstream
-- Core table for all AT Protocol records before denormalization
--
-- Uses ReplacingMergeTree to deduplicate on (collection, did, rkey) keeping latest indexed_at
-- JSON column stores full record, extract fields only when needed for ORDER BY/WHERE/JOINs

CREATE TABLE IF NOT EXISTS raw_records (
    -- Decomposed AT URI components (at://did/collection/rkey)
    did String,
    collection LowCardinality(String),
    rkey String,

    -- Content identifier from the record
    cid String,

    -- Repository revision (TID) - monotonically increasing per DID, used for dedup/ordering
    rev String,

    -- Full record as native JSON (schema-flexible, queryable with record.field.subfield)
    record JSON,

    -- Operation: 'create', 'update', 'delete'
    operation LowCardinality(String),

    -- Firehose sequence number (metadata only, not for ordering - can jump on relay restart)
    seq UInt64,

    -- Event timestamp from firehose
    event_time DateTime64(3),

    -- When we indexed this record
    indexed_at DateTime64(3) DEFAULT now64(3),

    -- Validation state: 'unchecked', 'valid', 'invalid_rev', 'invalid_gap', 'invalid_account'
    -- Populated by async batch validation, not in hot path
    validation_state LowCardinality(String) DEFAULT 'unchecked',

    -- Whether this came from live firehose (true) or backfill (false)
    -- Backfill events may not reflect current state until repo is fully synced
    is_live Bool DEFAULT true,

    -- Materialized AT URI for convenience
    uri String MATERIALIZED concat('at://', did, '/', collection, '/', rkey),

    -- Projection for fast delete lookups by (did, cid)
    -- Delete events include CID, so we can O(1) lookup the original record
    -- to know what to decrement (e.g., which notebook's like count)
    PROJECTION by_did_cid (
        SELECT * ORDER BY (did, cid)
    )
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (collection, did, rkey, event_time, indexed_at)
SETTINGS deduplicate_merge_projection_mode = 'drop';
