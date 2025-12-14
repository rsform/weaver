-- Auto-populate freed status from account events

CREATE MATERIALIZED VIEW IF NOT EXISTS handle_mappings_from_account_mv TO handle_mappings AS
SELECT
    h.handle,
    a.did,
    1 as freed,
    a.status as account_status,
    'account' as source,
    a.event_time,
    now64(3) as indexed_at
FROM raw_account_events a
INNER JOIN handle_mappings h ON h.did = a.did AND h.freed = 0
WHERE a.active = 0 AND a.status != ''
