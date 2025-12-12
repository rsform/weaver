-- Resource permissions
-- Refreshable MV that computes who can access each resource
-- Combines: owners (resource creator) + collaborators (invite+accept pairs)

CREATE MATERIALIZED VIEW IF NOT EXISTS permissions
REFRESH EVERY 1 MINUTE
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (resource_did, resource_collection, resource_rkey, grantee_did)
AS
-- Owners: resource creator has owner permission
SELECT
    did as resource_did,
    'sh.weaver.notebook.entry' as resource_collection,
    rkey as resource_rkey,
    concat('at://', did, '/sh.weaver.notebook.entry/', rkey) as resource_uri,

    did as grantee_did,
    'owner' as scope,

    -- Source is the resource itself
    did as source_did,
    'sh.weaver.notebook.entry' as source_collection,
    rkey as source_rkey,

    created_at as granted_at,
    now64(3) as indexed_at
FROM entries

UNION ALL

SELECT
    did as resource_did,
    'sh.weaver.notebook.book' as resource_collection,
    rkey as resource_rkey,
    concat('at://', did, '/sh.weaver.notebook.book/', rkey) as resource_uri,

    did as grantee_did,
    'owner' as scope,

    did as source_did,
    'sh.weaver.notebook.book' as source_collection,
    rkey as source_rkey,

    created_at as granted_at,
    now64(3) as indexed_at
FROM notebooks

UNION ALL

-- Collaborators: invite+accept pairs grant permission
SELECT
    resource_did,
    resource_collection,
    resource_rkey,
    resource_uri,

    collaborator_did as grantee_did,
    if(scope != '', scope, 'collaborator') as scope,

    invite_did as source_did,
    'sh.weaver.collab.invite' as source_collection,
    invite_rkey as source_rkey,

    accepted_at as granted_at,
    indexed_at
FROM collaborators
