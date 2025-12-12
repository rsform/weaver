-- Permissions cache
-- Local cache of permissions for collab-related hot paths.
-- ClickHouse is authoritative; this is populated on-demand for active resources.
CREATE TABLE permissions (
    -- Resource reference (decomposed)
    resource_did TEXT NOT NULL,
    resource_collection TEXT NOT NULL,
    resource_rkey TEXT NOT NULL,

    did TEXT NOT NULL,  -- user who has permission

    scope TEXT NOT NULL,  -- 'owner' | 'direct' | 'inherited'

    -- Source reference (decomposed) - resource itself for owner, invite for others
    source_did TEXT NOT NULL,
    source_collection TEXT NOT NULL,
    source_rkey TEXT NOT NULL,

    granted_at TEXT NOT NULL,

    PRIMARY KEY (resource_did, resource_collection, resource_rkey, did)
);

CREATE INDEX idx_permissions_did ON permissions(did);
