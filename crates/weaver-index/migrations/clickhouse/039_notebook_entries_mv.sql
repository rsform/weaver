-- Populate notebook_entries from notebooks

CREATE MATERIALIZED VIEW IF NOT EXISTS notebook_entries_mv
TO notebook_entries
AS
SELECT
    assumeNotNull(extract(entry_uri, 'at://([^/]+)/')) as entry_did,
    assumeNotNull(extract(entry_uri, '/sh\\.weaver\\.notebook\\.entry/([^/]+)$')) as entry_rkey,

    -- Notebook that contains this entry
    did as notebook_did,
    rkey as notebook_rkey,

    -- Position from array index
    assumeNotNull(entry_position) as position,

    now64(3) as indexed_at
FROM notebooks
ARRAY JOIN
    record.entryList[].uri as entry_uri,
    arrayEnumerate(record.entryList[].uri) as entry_position
WHERE deleted_at = toDateTime64(0, 3)
  AND isNotNull(entry_uri)
  AND entry_uri != ''
  AND extract(entry_uri, 'at://([^/]+)/') != ''
  AND extract(entry_uri, '/sh\\.weaver\\.notebook\\.entry/([^/]+)$') != ''
