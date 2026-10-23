# Futu market data provider

FUTU is an opt-in, built-in REST quote provider for US, Hong Kong, Shanghai,
Shenzhen and Beijing stocks and ETFs. It provides latest OHLCV and historical
daily bars. Securities search and profiles remain with existing providers.
`FUND:005827` continues to identify an off-exchange fund served by Eastmoney; it
is never routed to FUTU. Exchange-qualified searches such as `XSHG:600519`,
`XSHE:161725` and `XHKG:00700` keep their existing identities. Supported
exchange securities can use FUTU for quotes after creation.

## Configuration

Create an AppKey and upload its public key in the Futunn dashboard. In market
data settings, open Futu, select **Show / edit credentials**, enter AppKeyId and
the matching private key, save, then enable the provider. Supported private keys
are unencrypted PEM: Ed25519 PKCS#8, or RSA PKCS#8/PKCS#1. The key type
determines the signing algorithm. Save and delete refresh the existing client; a
refresh failure is shown separately from a secret-storage failure.

Both fields are stored together under the `FUTU` SecretStore identifier as
`{"appKeyId":"...","privateKey":"..."}`. Desktop uses native credentials and the
web runtime uses its configured secret store. The UI loads credentials only on
explicit reveal and keeps them out of query and mutation caches.

## Architecture impact

Enabling FUTU allows backend requests to `https://webapi.futunn.com`, carrying
requested security codes and date ranges. Redirects are disabled. Credentials
and raw API responses are not logged. Initialization and symbol resolution do
not make network requests. A migration appends a disabled provider record;
existing settings, assets, and stored quotes are preserved.

Market-data owns signing, parsing and transport; core owns SecretStore access
and initialization. Existing synchronization, storage, events, override
precedence and provider failure isolation remain in place. Each HTTP request
uses the existing RateLimiter at 60 requests/minute with burst capacity one;
this also covers historical subrequests that the outer registry cannot count.
The limit is per client instance, while Futunn's quota is shared per IP.

Latest quotes use `/api/v1.0/quote/stock-quote`. History uses daily, unadjusted
bars (`ktype=2`, `autype=0`), in nonoverlapping windows of at most 300 calendar
days. A response indicating more pages fails rather than silently truncating
history. Any failed window fails the entire request. Dates use the market's
trading day, not the UTC date of an exchange timestamp.

No automatic clock correction or independent retry loop is added. On
authentication errors check the AppKey, key algorithm, quote permissions and
system clock (the documented default tolerance is five seconds).

## Verification

Unit tests verify both signature algorithms, exact transmitted bytes, market
mapping, date/decimal conversion, per-request throttling, history windows,
partial failures and configuration behavior. Local HTTP tests use synthetic
keys. The focused E2E specification runs in both LTR and RTL with an isolated
database and FUTU fixture quotes; it never calls Futunn production endpoints.
These checks do not establish that a user's AppKey has working quote access.

Reference:
[Futunn authentication](https://open.futunn.com/zh-cn/api/overview/getting-started).
