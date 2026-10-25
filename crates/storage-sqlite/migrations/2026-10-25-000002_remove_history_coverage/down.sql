-- Restore only the obsolete schema for downgrade compatibility, never invent
-- proofs from existing prices. Benchmark history remains untouched.
CREATE TABLE market_history_queries (
    identity TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    start_day TEXT NOT NULL,
    end_day TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('complete', 'empty', 'partial')),
    PRIMARY KEY (identity, start_day, end_day),
    CHECK (start_day <= end_day)
);
CREATE INDEX market_history_queries_asset ON market_history_queries(asset_id);
CREATE TRIGGER invalidate_asset_history_queries AFTER DELETE ON assets
BEGIN
    DELETE FROM benchmark_history WHERE identity IN
        (SELECT identity FROM market_history_queries WHERE asset_id = OLD.id);
    DELETE FROM market_history_queries WHERE asset_id = OLD.id;
END;
