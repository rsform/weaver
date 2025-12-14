-- Notebook entries mapping (denormalized for reverse lookup)
-- Maps entries to the notebooks that contain them

CREATE TABLE IF NOT EXISTS notebook_entries (
    -- Entry being referenced
    entry_did String,
    entry_rkey String,

    -- Notebook containing this entry
    notebook_did String,
    notebook_rkey String,

    -- Position in entry list (for ordering)
    position UInt32,

    -- Timestamps
    indexed_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(indexed_at)
-- Primary lookup: find notebooks for an entry
ORDER BY (entry_did, entry_rkey, notebook_did, notebook_rkey)
