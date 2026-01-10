-- Populate drafts from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS drafts_mv TO drafts AS
SELECT
    did,
    rkey,
    cid,
    parseDateTime64BestEffortOrZero(toString(record.createdAt), 3) as created_at,
    event_time,
    indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.edit.draft'
