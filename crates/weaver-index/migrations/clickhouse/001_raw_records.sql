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

    -- Materialized AT URI for convenience
    uri String MATERIALIZED concat('at://', did, '/', collection, '/', rkey)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (collection, did, rkey, indexed_at);
