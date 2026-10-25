-- Local query outcomes are not financial records and are not device-synced.
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

-- Benchmarks can exist without user assets. Never insert synthetic assets to
-- satisfy the normal quotes foreign key and never include these in quote lists.
CREATE TABLE benchmark_history (
    identity TEXT NOT NULL,
    day TEXT NOT NULL,
    quote_json TEXT NOT NULL,
    PRIMARY KEY (identity, day)
);

-- Catalog deletion may cascade prices without going through QuoteStore.
CREATE TRIGGER invalidate_asset_history_queries AFTER DELETE ON assets
BEGIN
    DELETE FROM benchmark_history WHERE identity IN
        (SELECT identity FROM market_history_queries WHERE asset_id = OLD.id);
    DELETE FROM market_history_queries WHERE asset_id = OLD.id;
END;

-- Custom query identities use only eligible ordered source revisions. Updating
-- one custom provider must not purge unrelated provider/benchmark caches.
-- Obsolete identities remain unreachable until normal explicit cache cleanup.
