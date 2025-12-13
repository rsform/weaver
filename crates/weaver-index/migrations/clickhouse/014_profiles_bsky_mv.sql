-- Populate profiles_bsky from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS profiles_bsky_mv TO profiles_bsky AS
SELECT
    did,
    toString(record) as profile,
    coalesce(record.displayName, '') as display_name,
    coalesce(record.description, '') as description,
    coalesce(record.avatar.ref.`$link`, '') as avatar_cid,
    coalesce(record.banner.ref.`$link`, '') as banner_cid,
    parseDateTime64BestEffortOrZero(toString(record.createdAt), 3) as created_at,
    event_time,
    now64(3) as indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'app.bsky.actor.profile'
