-- Populate collab_accepts from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS collab_accepts_mv TO collab_accepts AS
SELECT
    did,
    rkey,
    cid,

    -- Parse invite strongRef
    splitByChar('/', replaceOne(toString(record.invite.uri), 'at://', ''))[1] as invite_did,
    splitByChar('/', replaceOne(toString(record.invite.uri), 'at://', ''))[3] as invite_rkey,
    toString(record.invite.cid) as invite_cid,

    -- Parse resource AT-URI
    splitByChar('/', replaceOne(toString(record.resource), 'at://', ''))[1] as resource_did,
    splitByChar('/', replaceOne(toString(record.resource), 'at://', ''))[2] as resource_collection,
    splitByChar('/', replaceOne(toString(record.resource), 'at://', ''))[3] as resource_rkey,

    coalesce(parseDateTime64BestEffortOrNull(toString(record.createdAt), 3), event_time) as created_at,
    event_time,
    now64(3) as indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.collab.accept'
