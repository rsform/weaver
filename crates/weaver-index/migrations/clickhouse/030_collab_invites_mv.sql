-- Populate collab_invites from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS collab_invites_mv TO collab_invites AS
SELECT
    did,
    rkey,
    cid,

    -- Parse resource strongRef
    splitByChar('/', replaceOne(toString(record.resource.uri), 'at://', ''))[1] as resource_did,
    splitByChar('/', replaceOne(toString(record.resource.uri), 'at://', ''))[2] as resource_collection,
    splitByChar('/', replaceOne(toString(record.resource.uri), 'at://', ''))[3] as resource_rkey,

    toString(record.invitee) as invitee_did,
    coalesce(toString(record.scope), '') as scope,
    coalesce(toString(record.message), '') as message,
    coalesce(toDateTime64(record.expiresAt, 3), toDateTime64(0, 3)) as expires_at,
    coalesce(toDateTime64(record.createdAt, 3), event_time) as created_at,
    event_time,
    now64(3) as indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.collab.invite'
