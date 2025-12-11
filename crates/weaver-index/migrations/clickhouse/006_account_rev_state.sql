-- Per-account revision state tracking
-- Maintains latest rev/cid per DID for dedup and gap detection
--
-- AggregatingMergeTree with incremental MV from raw_records
-- Query with argMaxMerge/maxMerge to finalize aggregates

CREATE TABLE IF NOT EXISTS account_rev_state (
    -- Account DID
    did String,

    -- Latest revision (TID) seen for this account
    last_rev AggregateFunction(argMax, String, DateTime64(3)),

    -- CID of the latest revision
    last_cid AggregateFunction(argMax, String, DateTime64(3)),

    -- Latest sequence number seen
    last_seq AggregateFunction(max, UInt64),

    -- Latest event time seen
    last_event_time AggregateFunction(max, DateTime64(3))
)
ENGINE = AggregatingMergeTree()
ORDER BY did
