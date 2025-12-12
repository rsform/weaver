-- Valid collaborators (invite + accept pairs)
CREATE TABLE collaborators (
    -- Resource reference (decomposed)
    resource_did TEXT NOT NULL,
    resource_collection TEXT NOT NULL,
    resource_rkey TEXT NOT NULL,

    collaborator_did TEXT NOT NULL,

    -- Invite record reference (decomposed)
    invite_did TEXT NOT NULL,
    invite_rkey TEXT NOT NULL,

    -- Accept record reference (decomposed)
    accept_did TEXT NOT NULL,
    accept_rkey TEXT NOT NULL,

    scope TEXT NOT NULL,  -- 'direct' | 'inherited'
    granted_at TEXT NOT NULL,
    indexed_at TEXT NOT NULL,

    PRIMARY KEY (resource_did, resource_collection, resource_rkey, collaborator_did)
);

CREATE INDEX idx_collaborators_did ON collaborators(collaborator_did);

-- Active sessions (TTL-based, cleaned up on expiry)
CREATE TABLE sessions (
    -- Session record identity (decomposed)
    did TEXT NOT NULL,
    rkey TEXT NOT NULL,

    -- Resource reference (decomposed)
    resource_did TEXT NOT NULL,
    resource_collection TEXT NOT NULL,
    resource_rkey TEXT NOT NULL,

    participant_did TEXT NOT NULL,
    node_id TEXT NOT NULL,
    relay_url TEXT,  -- NULL if no relay
    created_at TEXT NOT NULL,
    expires_at TEXT,  -- NULL = no expiry
    indexed_at TEXT NOT NULL,

    PRIMARY KEY (did, rkey)
);

CREATE INDEX idx_sessions_resource ON sessions(resource_did, resource_collection, resource_rkey);
CREATE INDEX idx_sessions_expires ON sessions(expires_at);

-- Pending invites (no accept yet)
CREATE TABLE pending_invites (
    -- Invite record identity (decomposed)
    did TEXT NOT NULL,  -- inviter DID
    rkey TEXT NOT NULL,

    -- Resource reference (decomposed)
    resource_did TEXT NOT NULL,
    resource_collection TEXT NOT NULL,
    resource_rkey TEXT NOT NULL,

    inviter_did TEXT NOT NULL,  -- same as did
    invitee_did TEXT NOT NULL,
    message TEXT,  -- NULL if no message
    expires_at TEXT,  -- NULL = no expiry
    created_at TEXT NOT NULL,
    indexed_at TEXT NOT NULL,

    PRIMARY KEY (did, rkey)
);

CREATE INDEX idx_pending_invites_resource ON pending_invites(resource_did, resource_collection, resource_rkey);
CREATE INDEX idx_pending_invites_invitee ON pending_invites(invitee_did);
