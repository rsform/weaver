-- Populate entries from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS entries_mv TO entries AS
SELECT
    did,
    rkey,
    cid,
    coalesce(record.title, '') as title,
    coalesce(record.path, '') as path,
    JSONExtract(toString(record), 'tags', 'Array(String)') as tags,
    arrayMap(x -> JSONExtractString(x, 'did'), JSONExtractArrayRaw(toString(record), 'authors')) as author_dids,
    parseDateTime64BestEffortOrZero(toString(record.createdAt), 3) as created_at,
    parseDateTime64BestEffortOrZero(toString(record.updatedAt), 3) as updated_at,
    event_time,
    indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at,
    record
FROM raw_records
WHERE collection = 'sh.weaver.notebook.entry'
