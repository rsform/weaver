-- Entry engagement counts
-- Updated by MVs from likes, bookmarks (added later with graph tables)
-- Joined with entries at query time

CREATE TABLE IF NOT EXISTS entry_counts (
    did String,
    rkey String,

    -- Signed for increment/decrement from MVs
    like_count Int64 DEFAULT 0,
    bookmark_count Int64 DEFAULT 0,

    indexed_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = SummingMergeTree((like_count, bookmark_count))
ORDER BY (did, rkey)
