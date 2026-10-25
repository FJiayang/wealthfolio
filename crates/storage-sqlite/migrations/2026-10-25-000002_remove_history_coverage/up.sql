-- Query coverage proofs are no longer used. Actual asset and benchmark prices
-- remain untouched; automatic supplementation now uses local date boundaries.
DROP TRIGGER IF EXISTS invalidate_asset_history_queries;
DROP TABLE IF EXISTS market_history_queries;
