-- Resource contributors
-- Precomputed MV that tracks who has contributed to each resource
-- Contributors = owners + editors (edit nodes) + collaborators who published

CREATE MATERIALIZED VIEW IF NOT EXISTS contributors
REFRESH EVERY 1 MINUTE
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (resource_did, resource_collection, resource_rkey, contributor_did)
AS
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
    AND (p.scope = 'collaborator' OR p.scope = 'owner')
WHERE e.deleted_at = toDateTime64(0, 3)
