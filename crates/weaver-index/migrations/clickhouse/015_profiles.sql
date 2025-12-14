-- Unified profiles view
CREATE MATERIALIZED VIEW IF NOT EXISTS profiles
REFRESH EVERY 1 MINUTE
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY did
AS SELECT
    if(w.did != '', w.did, b.did) as did,

    coalesce(h.handle, '') as handle,

    -- Raw profiles per source
    coalesce(w.profile, '') as weaver_profile,
    coalesce(b.profile, '') as bsky_profile,

    -- Coalesced fields (weaver > bsky priority)
    coalesce(nullIf(w.display_name, ''), b.display_name, '') as display_name,
    coalesce(nullIf(w.description, ''), b.description, '') as description,
    coalesce(nullIf(w.avatar_cid, ''), b.avatar_cid, '') as avatar_cid,
    coalesce(nullIf(w.banner_cid, ''), b.banner_cid, '') as banner_cid,

    -- Presence flags
    if(w.did != '', 1, 0) as has_weaver,
    if(b.did != '', 1, 0) as has_bsky,

    -- Timestamps
    coalesce(w.created_at, b.created_at, toDateTime64(0, 3)) as created_at,
    greatest(coalesce(w.event_time, toDateTime64(0, 3)), coalesce(b.event_time, toDateTime64(0, 3))) as event_time,
    now64(3) as indexed_at
FROM (SELECT * FROM profiles_weaver WHERE deleted_at = toDateTime64(0, 3)) w
FULL OUTER JOIN (SELECT * FROM profiles_bsky WHERE deleted_at = toDateTime64(0, 3)) b ON w.did = b.did
LEFT JOIN (
    SELECT did, argMax(handle, event_time) as handle
    FROM handle_mappings
    WHERE freed = 0 AND did != ''
    GROUP BY did
) h ON h.did = if(w.did != '', w.did, b.did)
WHERE if(w.did != '', w.did, b.did) != ''
