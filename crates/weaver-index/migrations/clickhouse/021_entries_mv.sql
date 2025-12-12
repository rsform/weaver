-- Populate entries from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS entries_mv TO entries AS
SELECT
    did,
    rkey,
    cid,
    coalesce(record.title, '') as title,
    coalesce(record.path, '') as path,
    coalesce(record.content, '') as content,
    JSONExtract(toString(record), 'tags', 'Array(String)') as tags,
    arrayMap(x -> JSONExtractString(x, 'did'), JSONExtractArrayRaw(toString(record), 'authors')) as author_dids,
    substring(coalesce(record.content, ''), 1, 500) as content_preview,
    coalesce(toDateTime64(record.createdAt, 3), toDateTime64(0, 3)) as created_at,
    coalesce(toDateTime64(record.updatedAt, 3), toDateTime64(0, 3)) as updated_at,
    event_time,
    now64(3) as indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.notebook.entry'
