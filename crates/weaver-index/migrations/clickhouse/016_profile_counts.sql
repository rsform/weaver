-- Profile counts aggregated from graph tables
-- Updated by MVs from follows, notebooks, entries (added later with those tables)
-- Joined with profiles at query time

CREATE TABLE IF NOT EXISTS profile_counts (
    did String,

    -- Signed for increment/decrement from MVs
    follower_count Int64 DEFAULT 0,
    following_count Int64 DEFAULT 0,
    notebook_count Int64 DEFAULT 0,
    entry_count Int64 DEFAULT 0,

    indexed_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = SummingMergeTree((follower_count, following_count, notebook_count, entry_count))
ORDER BY did
