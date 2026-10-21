INSERT OR IGNORE INTO market_data_providers (id, name, description, priority, enabled, provider_type)
VALUES ('HTTP_API', 'Standard HTTP API', 'Exact instrument lookup, profiles and prices from a self-hosted HTTP service', 10, 0, 'builtin');
