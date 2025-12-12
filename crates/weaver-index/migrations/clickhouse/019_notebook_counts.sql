-- Notebook engagement counts
-- Updated by MVs from likes, bookmarks, subscriptions (added later with graph tables)
-- Joined with notebooks at query time

CREATE TABLE IF NOT EXISTS notebook_counts (
    did String,
    rkey String,

    -- Signed for increment/decrement from MVs
    like_count Int64 DEFAULT 0,
    bookmark_count Int64 DEFAULT 0,
    subscriber_count Int64 DEFAULT 0,

    indexed_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = SummingMergeTree((like_count, bookmark_count, subscriber_count))
ORDER BY (did, rkey)
