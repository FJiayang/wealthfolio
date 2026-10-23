INSERT INTO market_data_providers (id, name, description, url, priority, enabled, provider_type)
SELECT 'IFIND', 'Tonghuashun iFinD', 'Stocks, funds and indices from Tonghuashun iFinD',
       'https://mcp.51ifind.com', COALESCE(MAX(priority), 0) + 1, 0, 'builtin'
FROM market_data_providers;
