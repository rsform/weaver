-- Resource contributors
-- Precomputed MV that tracks who has contributed to each resource
-- Contributors = owners + editors (edit nodes) + collaborators who published

CREATE MATERIALIZED VIEW IF NOT EXISTS contributors
REFRESH EVERY 1 MINUTE
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (resource_did, resource_collection, resource_rkey, contributor_did)
AS
-- Owners: resource creator is always a contributor
SELECT
    did as resource_did,
    'sh.weaver.notebook.entry' as resource_collection,
    rkey as resource_rkey,

    did as contributor_did,
    'owner' as contribution_type,

    concat('at://', did, '/sh.weaver.notebook.entry/', rkey) as source_uri,
    created_at as contributed_at,
    now64(3) as indexed_at
FROM entries
WHERE deleted_at = toDateTime64(0, 3)

UNION ALL

SELECT
    did as resource_did,
    'sh.weaver.notebook.book' as resource_collection,
    rkey as resource_rkey,

    did as contributor_did,
    'owner' as contribution_type,

    concat('at://', did, '/sh.weaver.notebook.book/', rkey) as source_uri,
    created_at as contributed_at,
    now64(3) as indexed_at
FROM notebooks
WHERE deleted_at = toDateTime64(0, 3)

UNION ALL

-- Editors: anyone with edit nodes for the resource
SELECT
    resource_did,
    resource_collection,
    resource_rkey,

    did as contributor_did,
    'editor' as contribution_type,

    uri as source_uri,
    created_at as contributed_at,
    now64(3) as indexed_at
FROM edit_nodes
WHERE deleted_at = toDateTime64(0, 3)
  AND resource_did != ''

UNION ALL

-- Collaborators who have published (same rkey in their repo)
-- Entry collaborators
SELECT
    p.resource_did,
    p.resource_collection,
    p.resource_rkey,

    e.did as contributor_did,
    'collaborator' as contribution_type,

    e.uri as source_uri,
    e.created_at as contributed_at,
    now64(3) as indexed_at
FROM entries e
INNER JOIN permissions p ON e.did = p.grantee_did
    AND e.rkey = p.resource_rkey
    AND p.resource_collection = 'sh.weaver.notebook.entry'
    AND p.scope = 'collaborator'
WHERE e.deleted_at = toDateTime64(0, 3)

UNION ALL

-- Notebook collaborators
SELECT
    p.resource_did,
    p.resource_collection,
    p.resource_rkey,

    n.did as contributor_did,
    'collaborator' as contribution_type,

    n.uri as source_uri,
    n.created_at as contributed_at,
    now64(3) as indexed_at
FROM notebooks n
INNER JOIN permissions p ON n.did = p.grantee_did
    AND n.rkey = p.resource_rkey
    AND p.resource_collection = 'sh.weaver.notebook.book'
    AND p.scope = 'collaborator'
WHERE n.deleted_at = toDateTime64(0, 3)
