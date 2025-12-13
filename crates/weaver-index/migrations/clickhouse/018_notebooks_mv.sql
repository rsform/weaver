-- Populate notebooks from raw_records

CREATE MATERIALIZED VIEW IF NOT EXISTS notebooks_mv TO notebooks AS
SELECT
    did,
    rkey,
    cid,
    coalesce(record.title, '') as title,
    coalesce(record.path, '') as path,
    '' as description,  -- notebooks don't have description field
    JSONExtract(toString(record), 'tags', 'Array(String)') as tags,
    if(record.publishGlobal = true, 1, 0) as publish_global,
    arrayMap(x -> JSONExtractString(x, 'did'), JSONExtractArrayRaw(toString(record), 'authors')) as author_dids,
    length(JSONExtractArrayRaw(toString(record), 'entryList')) as entry_count,
    parseDateTime64BestEffortOrZero(toString(record.createdAt), 3) as created_at,
    parseDateTime64BestEffortOrZero(toString(record.updatedAt), 3) as updated_at,
    event_time,
    now64(3) as indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.notebook.book'
