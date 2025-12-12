-- Valid collaborators (matched invite + accept pairs)
-- Refreshable MV that joins invites and accepts

CREATE MATERIALIZED VIEW IF NOT EXISTS collaborators
REFRESH EVERY 1 MINUTE
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (resource_did, resource_collection, resource_rkey, collaborator_did)
AS
SELECT
    -- Resource
    i.resource_did,
    i.resource_collection,
    i.resource_rkey,
    concat('at://', i.resource_did, '/', i.resource_collection, '/', i.resource_rkey) as resource_uri,

    -- Collaborator (the invitee who accepted)
    i.invitee_did as collaborator_did,

    -- Inviter
    i.did as inviter_did,

    -- Invite record
    i.did as invite_did,
    i.rkey as invite_rkey,
    i.cid as invite_cid,
    concat('at://', i.did, '/sh.weaver.collab.invite/', i.rkey) as invite_uri,

    -- Accept record
    a.did as accept_did,
    a.rkey as accept_rkey,
    a.cid as accept_cid,
    concat('at://', a.did, '/sh.weaver.collab.accept/', a.rkey) as accept_uri,

    -- Scope
    i.scope,

    -- Timestamps
    i.created_at as invited_at,
    a.created_at as accepted_at,
    now64(3) as indexed_at
FROM collab_invites i
INNER JOIN collab_accepts a ON
    a.invite_did = i.did
    AND a.invite_rkey = i.rkey
WHERE
    -- Invite not expired
    (i.expires_at = toDateTime64(0, 3) OR i.expires_at > now64(3))
