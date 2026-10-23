-- The existing migration runner backs up the database and runs this once, offline.
CREATE TEMP TABLE native_provider_order AS
SELECT id, ROW_NUMBER() OVER (ORDER BY priority, id) * 10 AS position
FROM market_data_providers;

CREATE TEMP TABLE native_provider_previous AS
SELECT COALESCE((SELECT enabled FROM market_data_providers WHERE id = 'HTTP_API'), 0) AS enabled,
       COALESCE((SELECT position FROM native_provider_order WHERE id = 'HTTP_API'),
                (SELECT COALESCE(MAX(position), 0) + 10 FROM native_provider_order)) AS position;

INSERT INTO market_data_providers (id, name, description, url, priority, enabled, provider_type)
SELECT 'EASTMONEY', 'Eastmoney', 'Stocks, funds and indices from Eastmoney and Tiantian Fund',
       'https://quote.eastmoney.com', position, enabled, 'builtin' FROM native_provider_previous
UNION ALL
SELECT 'TENCENT', 'Tencent Finance', 'Stocks, listed funds and indices from Tencent Finance',
       'https://gu.qq.com', position + 1, enabled, 'builtin' FROM native_provider_previous
UNION ALL
SELECT 'SINA', 'Sina Finance', 'Stocks, listed funds and indices from Sina Finance',
       'https://finance.sina.com.cn', position + 2, enabled, 'builtin' FROM native_provider_previous;

INSERT INTO native_provider_order
SELECT id, priority FROM market_data_providers WHERE id IN ('EASTMONEY', 'TENCENT', 'SINA');
DELETE FROM native_provider_order WHERE id = 'HTTP_API';
WITH ordered AS (
    SELECT id, ROW_NUMBER() OVER (ORDER BY position, id) AS priority FROM native_provider_order
)
UPDATE market_data_providers SET priority = (SELECT priority FROM ordered WHERE ordered.id = market_data_providers.id)
WHERE id != 'HTTP_API';

CREATE TEMP TABLE native_provider_assets AS
SELECT id FROM assets
WHERE json_extract(provider_config, '$.preferred_provider') = 'HTTP_API'
   OR json_type(provider_config, '$.overrides.HTTP_API') IS NOT NULL;

UPDATE quote_sync_state SET data_source = '', error_count = 0, last_error = NULL,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE asset_id IN (SELECT id FROM native_provider_assets) OR data_source = 'HTTP_API';

UPDATE assets SET provider_config = json_remove(provider_config, '$.preferred_provider')
WHERE json_extract(provider_config, '$.preferred_provider') = 'HTTP_API';
UPDATE assets SET provider_config = json_remove(provider_config, '$.overrides.HTTP_API')
WHERE id IN (SELECT id FROM native_provider_assets);
UPDATE assets SET provider_config = json_remove(provider_config, '$.overrides')
WHERE id IN (SELECT id FROM native_provider_assets)
  AND json_extract(provider_config, '$.overrides') = '{}';
UPDATE assets SET provider_config = NULL
WHERE id IN (SELECT id FROM native_provider_assets) AND provider_config = '{}';

-- Historical quote provenance and prices deliberately remain unchanged.
DELETE FROM market_data_providers WHERE id = 'HTTP_API';
DROP TABLE native_provider_assets;
DROP TABLE native_provider_previous;
DROP TABLE native_provider_order;
