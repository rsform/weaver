-- Migration tracking table
-- Tracks which migrations have been applied

CREATE TABLE IF NOT EXISTS _migrations (
    -- Migration filename (e.g., '001_raw_records.sql')
    name String,

    -- When this migration was applied
    applied_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
ORDER BY (name);
