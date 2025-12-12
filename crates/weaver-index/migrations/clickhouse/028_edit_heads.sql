-- Edit heads per resource
-- Refreshable MV that tracks all branch heads for each resource
-- A head is a node with no children (nothing has prev pointing to it)
-- Multiple heads = divergent branches needing merge

CREATE MATERIALIZED VIEW IF NOT EXISTS edit_heads
REFRESH EVERY 1 MINUTE
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (resource_did, resource_collection, resource_rkey, head_did, head_rkey)
AS
WITH
    -- All nodes
    all_nodes AS (
        SELECT
            did, rkey, cid, collection, node_type,
            resource_did, resource_collection, resource_rkey,
            root_did, root_rkey,
            prev_did, prev_rkey,
            created_at
        FROM edit_nodes
        WHERE resource_did != ''
    ),
    -- Nodes that are pointed to by prev (have children)
    has_children AS (
        SELECT DISTINCT prev_did as did, prev_rkey as rkey
        FROM all_nodes
        WHERE prev_did != ''
    ),
    -- Root CIDs lookup
    root_cids AS (
        SELECT did, rkey, cid
        FROM edit_nodes
        WHERE node_type = 'root'
    )
-- Heads are nodes with no children
SELECT
    n.resource_did,
    n.resource_collection,
    n.resource_rkey,
    concat('at://', n.resource_did, '/', n.resource_collection, '/', n.resource_rkey) as resource_uri,

    -- This head
    n.did as head_did,
    n.rkey as head_rkey,
    n.cid as head_cid,
    n.collection as head_collection,
    n.node_type as head_type,
    concat('at://', n.did, '/', n.collection, '/', n.rkey) as head_uri,

    -- Root for this branch (with CID)
    if(n.node_type = 'root', n.did, n.root_did) as root_did,
    if(n.node_type = 'root', n.rkey, n.root_rkey) as root_rkey,
    if(n.node_type = 'root', n.cid, coalesce(r.cid, '')) as root_cid,
    if(n.node_type = 'root',
        concat('at://', n.did, '/', n.collection, '/', n.rkey),
        if(n.root_did != '', concat('at://', n.root_did, '/sh.weaver.edit.root/', n.root_rkey), '')
    ) as root_uri,

    n.created_at as head_created_at,
    now64(3) as indexed_at
FROM all_nodes n
LEFT ANTI JOIN has_children c ON n.did = c.did AND n.rkey = c.rkey
LEFT JOIN root_cids r ON r.did = n.root_did AND r.rkey = n.root_rkey
