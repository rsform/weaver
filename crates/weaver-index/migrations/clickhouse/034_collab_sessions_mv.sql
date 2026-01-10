-- Populate collab_sessions from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS collab_sessions_mv TO collab_sessions AS
SELECT
    did,
    rkey,
    cid,

    -- Parse resource strongRef
    splitByChar('/', replaceOne(toString(record.resource.uri), 'at://', ''))[1] as resource_did,
    splitByChar('/', replaceOne(toString(record.resource.uri), 'at://', ''))[2] as resource_collection,
    splitByChar('/', replaceOne(toString(record.resource.uri), 'at://', ''))[3] as resource_rkey,

    toString(record.nodeId) as node_id,
    coalesce(toString(record.relayUrl), '') as relay_url,
    coalesce(parseDateTime64BestEffortOrNull(toString(record.createdAt), 3), event_time) as created_at,
    parseDateTime64BestEffortOrZero(toString(record.expiresAt), 3) as expires_at,
    event_time,
    indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.collab.session'
