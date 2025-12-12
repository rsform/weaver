-- Edit graph storage (roots and diffs)
-- Supports DAG structure for future merge support

CREATE TABLE edit_nodes (
    -- Edit record identity (decomposed)
    did TEXT NOT NULL,
    collection TEXT NOT NULL,  -- 'sh.weaver.edit.root' or 'sh.weaver.edit.diff'
    rkey TEXT NOT NULL,

    -- Resource being edited (decomposed)
    resource_did TEXT NOT NULL,
    resource_collection TEXT NOT NULL,
    resource_rkey TEXT NOT NULL,

    node_type TEXT NOT NULL,  -- 'root' | 'diff'
    created_at TEXT NOT NULL,
    indexed_at TEXT NOT NULL,

    PRIMARY KEY (did, collection, rkey)
);

CREATE INDEX idx_edit_nodes_resource ON edit_nodes(resource_did, resource_collection, resource_rkey);
CREATE INDEX idx_edit_nodes_author ON edit_nodes(did);

-- Edit graph edges (supports DAG)
CREATE TABLE edit_edges (
    -- Child reference (decomposed)
    child_did TEXT NOT NULL,
    child_collection TEXT NOT NULL,
    child_rkey TEXT NOT NULL,

    -- Parent reference (decomposed)
    parent_did TEXT NOT NULL,
    parent_collection TEXT NOT NULL,
    parent_rkey TEXT NOT NULL,

    edge_type TEXT NOT NULL,  -- 'prev' | 'merge' (future)

    PRIMARY KEY (child_did, child_collection, child_rkey, parent_did, parent_collection, parent_rkey),
    FOREIGN KEY (child_did, child_collection, child_rkey) REFERENCES edit_nodes(did, collection, rkey),
    FOREIGN KEY (parent_did, parent_collection, parent_rkey) REFERENCES edit_nodes(did, collection, rkey)
);

CREATE INDEX idx_edit_edges_parent ON edit_edges(parent_did, parent_collection, parent_rkey);

-- Fast path: track current head per resource
CREATE TABLE edit_heads (
    -- Resource identity (decomposed)
    resource_did TEXT NOT NULL,
    resource_collection TEXT NOT NULL,
    resource_rkey TEXT NOT NULL,

    -- Latest root reference (decomposed)
    root_did TEXT,
    root_collection TEXT,
    root_rkey TEXT,

    -- Current head reference (decomposed)
    head_did TEXT,
    head_collection TEXT,
    head_rkey TEXT,

    updated_at TEXT NOT NULL,

    PRIMARY KEY (resource_did, resource_collection, resource_rkey)
);
