-- Populate drafts from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS drafts_mv TO drafts AS
SELECT
    did,
    rkey,
    cid,
    coalesce(toDateTime64(record.createdAt, 3), toDateTime64(0, 3)) as created_at,
    event_time,
    now64(3) as indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.edit.draft'
