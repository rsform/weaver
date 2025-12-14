

CREATE MATERIALIZED VIEW IF NOT EXISTS account_rev_state_mv TO account_rev_state AS
SELECT
    did,
    argMaxState(rev, event_time) as last_rev,
    argMaxState(cid, event_time) as last_cid,
    maxState(seq) as last_seq,
    maxState(event_time) as last_event_time
FROM raw_records
GROUP BY did
