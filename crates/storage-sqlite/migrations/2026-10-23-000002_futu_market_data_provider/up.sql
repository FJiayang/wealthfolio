INSERT INTO market_data_providers (id, name, description, url, priority, enabled, provider_type)
SELECT 'FUTU', 'Futu', 'Stock and ETF quotes from Futunn OpenAPI',
       'https://open.futunn.com', COALESCE(MAX(priority), 0) + 1, 0, 'builtin'
FROM market_data_providers;
