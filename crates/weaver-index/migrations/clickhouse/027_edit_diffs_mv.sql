-- Populate edit_nodes from edit.diff records

CREATE MATERIALIZED VIEW IF NOT EXISTS edit_diffs_mv TO edit_nodes AS
SELECT
    did,
    rkey,
    cid,
    'sh.weaver.edit.diff' as collection,
    'diff' as node_type,

    -- Extract resource type and ref from doc.value union
    multiIf(
        toString(record.doc.value.entry.uri) != '', 'entry',
        toString(record.doc.value.notebook.uri) != '', 'notebook',
        toString(record.doc.value.draftKey) != '', 'draft',
        ''
    ) as resource_type,

    -- Extract resource DID
    multiIf(
        toString(record.doc.value.entry.uri) != '',
            splitByChar('/', replaceOne(toString(record.doc.value.entry.uri), 'at://', ''))[1],
        toString(record.doc.value.notebook.uri) != '',
            splitByChar('/', replaceOne(toString(record.doc.value.notebook.uri), 'at://', ''))[1],
        ''
    ) as resource_did,

    -- Extract resource rkey
    multiIf(
        toString(record.doc.value.entry.uri) != '',
            splitByChar('/', replaceOne(toString(record.doc.value.entry.uri), 'at://', ''))[3],
        toString(record.doc.value.notebook.uri) != '',
            splitByChar('/', replaceOne(toString(record.doc.value.notebook.uri), 'at://', ''))[3],
        toString(record.doc.value.draftKey) != '',
            toString(record.doc.value.draftKey),
        ''
    ) as resource_rkey,

    -- Extract resource collection
    multiIf(
        toString(record.doc.value.entry.uri) != '', 'sh.weaver.notebook.entry',
        toString(record.doc.value.notebook.uri) != '', 'sh.weaver.notebook.book',
        toString(record.doc.value.draftKey) != '', 'sh.weaver.edit.draft',
        ''
    ) as resource_collection,

    -- Root reference
    splitByChar('/', replaceOne(toString(record.root.uri), 'at://', ''))[1] as root_did,
    splitByChar('/', replaceOne(toString(record.root.uri), 'at://', ''))[3] as root_rkey,

    -- Prev reference (optional)
    if(toString(record.prev.uri) != '',
        splitByChar('/', replaceOne(toString(record.prev.uri), 'at://', ''))[1],
        '') as prev_did,
    if(toString(record.prev.uri) != '',
        splitByChar('/', replaceOne(toString(record.prev.uri), 'at://', ''))[3],
        '') as prev_rkey,

    -- Check for inline diff vs snapshot
    if(length(toString(record.inlineDiff)) > 0, 1, 0) as has_inline_diff,
    if(toString(record.snapshot.ref.`$link`) != '', 1, 0) as has_snapshot,

    coalesce(parseDateTime64BestEffortOrNull(toString(record.createdAt), 3), event_time) as created_at,
    event_time,
    now64(3) as indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.edit.diff'
