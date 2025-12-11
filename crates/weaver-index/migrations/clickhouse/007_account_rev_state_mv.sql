-- Incremental MV: fires on each insert to raw_records, maintains aggregate state
-- Must be created after both account_rev_state (target) and raw_records (source) exist

CREATE MATERIALIZED VIEW IF NOT EXISTS account_rev_state_mv TO account_rev_state AS
SELECT
    did,
    argMaxState(rev, event_time) as last_rev,
    argMaxState(cid, event_time) as last_cid,
    maxState(seq) as last_seq,
    maxState(event_time) as last_event_time
FROM raw_records
GROUP BY did
