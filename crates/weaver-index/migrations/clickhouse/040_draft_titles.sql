-- Draft titles extracted from Loro snapshots
-- Updated by background task when edit_heads changes

CREATE TABLE IF NOT EXISTS draft_titles (
    -- Draft identity (matches drafts table)
    did String,
    rkey String,

    -- Extracted title from Loro doc
    title String DEFAULT '',

    -- Head used for extraction (stale if doesn't match edit_heads)
    head_did String DEFAULT '',
    head_rkey String DEFAULT '',
    head_cid String DEFAULT '',

    -- Timestamps
    updated_at DateTime64(3) DEFAULT now64(3),
    indexed_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(indexed_at)
ORDER BY (did, rkey)
