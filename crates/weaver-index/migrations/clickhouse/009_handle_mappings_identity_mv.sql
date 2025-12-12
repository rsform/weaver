-- Auto-populate handle_mappings from identity events when handle is present

CREATE MATERIALIZED VIEW IF NOT EXISTS handle_mappings_from_identity_mv TO handle_mappings AS
SELECT
    handle,
    did,
    0 as freed,
    'active' as account_status,
    'identity' as source,
    event_time,
    now64(3) as indexed_at
FROM raw_identity_events
WHERE handle != ''
