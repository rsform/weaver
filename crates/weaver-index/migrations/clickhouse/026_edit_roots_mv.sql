-- Populate edit_nodes from edit.root records

CREATE MATERIALIZED VIEW IF NOT EXISTS edit_roots_mv TO edit_nodes AS
SELECT
    did,
    rkey,
    cid,
    'sh.weaver.edit.root' as collection,
    'root' as node_type,

    -- Extract resource type and ref from doc.value union
    multiIf(
        toString(record.doc.value.entry.uri) != '', 'entry',
        toString(record.doc.value.notebook.uri) != '', 'notebook',
        toString(record.doc.value.draftKey) != '', 'draft',
        ''
    ) as resource_type,

    -- Extract resource DID (parse from URI or empty)
    multiIf(
        toString(record.doc.value.entry.uri) != '',
            splitByChar('/', replaceOne(toString(record.doc.value.entry.uri), 'at://', ''))[1],
        toString(record.doc.value.notebook.uri) != '',
            splitByChar('/', replaceOne(toString(record.doc.value.notebook.uri), 'at://', ''))[1],
        ''
    ) as resource_did,

    -- Extract resource rkey (parse from URI or use draftKey)
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

    -- Roots don't have root/prev refs
    '' as root_did,
    '' as root_rkey,
    '' as prev_did,
    '' as prev_rkey,

    -- Roots always have snapshot
    0 as has_inline_diff,
    1 as has_snapshot,

    event_time as created_at,
    event_time,
    now64(3) as indexed_at,
    if(operation = 'delete', event_time, toDateTime64(0, 3)) as deleted_at
FROM raw_records
WHERE collection = 'sh.weaver.edit.root'
