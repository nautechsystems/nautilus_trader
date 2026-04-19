# Coinbase

Founded in 2012, Coinbase is one of the largest US-regulated cryptocurrency
exchanges, offering trading across spot, perpetual swaps, and dated futures via
the Advanced Trade API. This adapter currently supports live market data
ingest; order execution is planned (see the component status table below).

## Overview

The Coinbase adapter is implemented in Rust and consumed by the v2 system.
The adapter does not ship a legacy Python `TradingNode` integration; only
configuration and enum types are exported through PyO3 so v2 entry points can
construct them from Python.

Current components:

| Component                          | Status   | Notes                                                |
|------------------------------------|----------|------------------------------------------------------|
| `CoinbaseHttpClient`               | Built    | Low‑level HTTP connectivity with ES256 JWT signing.  |
| `CoinbaseWebSocketClient`          | Built    | Low‑level WebSocket connectivity.                    |
| `CoinbaseInstrumentProvider`       | Built    | Instrument parsing and loading.                      |
| `CoinbaseDataClient`               | Built    | Rust market data feed manager.                       |
| `CoinbaseDataClientFactory`        | Built    | Rust data client factory.                            |
| `CoinbaseExecutionClient`          | Pending  | Rust execution client; tracked for follow‑up work.   |
| `CoinbaseExecutionClientFactory`   | Pending  | Rust execution client factory; tracked for follow‑up.|

PyO3 surface available from `nautilus_trader.core.nautilus_pyo3.coinbase`:

- `CoinbaseDataClientConfig`, `CoinbaseExecClientConfig`
- `CoinbaseEnvironment`
- `COINBASE` venue constant

## Coinbase documentation

Coinbase provides documentation for the Advanced Trade API:

- [REST API reference](https://docs.cdp.coinbase.com/advanced-trade/reference)
- [WebSocket channels](https://docs.cdp.coinbase.com/advanced-trade/docs/ws-channels)
- [API key authentication](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication)
- [Rate limits](https://docs.cdp.coinbase.com/advanced-trade/docs/rate-limits)

It's recommended you also refer to the Coinbase documentation in conjunction
with this NautilusTrader integration guide.

:::info
This adapter targets the Coinbase Advanced Trade API. The separate
[Coinbase International Exchange (INTX)](https://international.coinbase.com)
venue is supported by the dedicated `coinbase_intx` adapter.
:::

## Products

A product is an umbrella term for a group of related instrument types.

The following product types are supported:

| Product Type        | Supported | Notes                                                    |
|---------------------|-----------|----------------------------------------------------------|
| Spot                | ✓         | USD, USDC, and USDT-quoted spot pairs.                   |
| Perpetual contracts | ✓         | USD-margined perpetual swaps on the FCM venue.           |
| Futures contracts   | ✓         | Dated delivery futures (nano BTC, nano ETH, etc).        |

## Symbology

Coinbase uses the venue's native `product_id` field directly as the Nautilus
symbol. The instrument ID is `{product_id}.COINBASE`.

| Product          | Format                             | Examples                           |
|------------------|------------------------------------|------------------------------------|
| Spot             | `{base}-{quote}`                   | `BTC-USD`, `ETH-USDC`, `SOL-USDT`. |
| Perpetual        | `{contract_code}-{ddMMMyy}-CDE`    | `BIP-20DEC30-CDE` (BTC PERP).      |
| Dated future     | `{contract_code}-{ddMMMyy}-CDE`    | `BIT-24APR26-CDE` (BTC Apr 2026).  |

The `-CDE` suffix denotes the Coinbase Derivatives Exchange (FCM venue).
Perpetuals carry an exchange-assigned far-future expiry (e.g. `20DEC30`) but
are classified as `CryptoPerpetual` based on the presence of an ongoing
funding rate. Dated futures are classified as `CryptoFuture`.

The adapter resolves the product type structurally from API metadata
(`future_product_details.perpetual_details.funding_rate` plus
`contract_expiry_type`); the fallback heuristic checks `display_name` for
`PERP` or `Perpetual` substrings.

Examples of full Nautilus instrument IDs:

- `BTC-USD.COINBASE` (spot Bitcoin/USD).
- `ETH-USDC.COINBASE` (spot Ether/USDC).
- `BIP-20DEC30-CDE.COINBASE` (BTC perpetual swap).
- `BIT-24APR26-CDE.COINBASE` (BTC dated future, Apr 2026).

## Environments

Coinbase provides two trading environments. Configure the appropriate
environment using the `environment` field in your client configuration.

| Environment | `environment` value             | REST base URL                      |
|-------------|---------------------------------|------------------------------------|
| Live        | `CoinbaseEnvironment.LIVE`      | `https://api.coinbase.com`         |
| Sandbox     | `CoinbaseEnvironment.SANDBOX`   | `https://api-sandbox.coinbase.com` |

### Live (production)

The default environment for live trading with real funds.

```python
config = CoinbaseExecClientConfig(
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    # environment=CoinbaseEnvironment.LIVE (default)
)
```

Environment variables: `COINBASE_API_KEY`, `COINBASE_API_SECRET`.

### Sandbox

A static-mock test environment for integration plumbing, per the
[Sandbox docs](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/sandbox).

```python
config = CoinbaseExecClientConfig(
    environment=CoinbaseEnvironment.SANDBOX,
    # API credentials are not required by sandbox.
)
```

:::warning
**Sandbox is not a parallel trading venue:**

- All responses are static and pre-defined; there is no live market or
  dynamic pricing.
- Only Accounts and Orders endpoints are available; other resources are not.
- Authentication is not required (and not enforced).
- A custom `X-Sandbox` request header can trigger predefined error scenarios.

Use sandbox to wire up your client and verify request/response shape; use
production (with real funds and care) for any realistic behaviour testing.
:::

## Authentication

Coinbase Advanced Trade uses ES256 JWT authentication. Each REST request and
each WebSocket subscription generates a short-lived JWT signed with your EC
private key. The adapter resolves credentials from environment variables or
from the config fields.

### Creating an API key

Coinbase has several key types. The adapter requires a **Coinbase App Secret
API key** with the **ECDSA** signature algorithm (not Ed25519).

<Steps>
<Step>
Go to the CDP portal API keys page:
[portal.cdp.coinbase.com/projects/api-keys](https://portal.cdp.coinbase.com/projects/api-keys).
</Step>
<Step>
Select the **Secret API Keys** tab and click **Create API key**.
</Step>
<Step>
Enter a nickname (e.g. `nautilus-trading`).
</Step>
<Step>
Expand **API restrictions** and set permissions to **View** and **Trade**.
</Step>
<Step>
Expand **Advanced Settings** and change the signature algorithm from Ed25519
to **ECDSA**. This step is required: Ed25519 keys do not work with the
Advanced Trade API.
</Step>
<Step>
Click **Create API key**. Save the key name and private key from the modal.
The key name looks like `organizations/{org_id}/apiKeys/{key_id}`. The
private key is a PEM-encoded EC key (SEC1 format).
</Step>
</Steps>

:::warning
Coinbase no longer auto-downloads the key file. Copy the values from the
creation modal or click the download button before closing it. You cannot
retrieve the private key afterward.
:::

:::info
Do not use legacy API keys from coinbase.com/settings/api (UUID format with
HMAC-SHA256 signing). Those use a different auth scheme (`CB-ACCESS-*`
headers) that the adapter does not support.
:::

For full details see the Coinbase
[API key authentication guide](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication).

### Environment variables

| Variable              | Description                                               |
|-----------------------|-----------------------------------------------------------|
| `COINBASE_API_KEY`    | Key name (`organizations/{org_id}/apiKeys/{key_id}`).     |
| `COINBASE_API_SECRET` | PEM‑encoded EC private key (full multi‑line string).      |

Example:

```bash
export COINBASE_API_KEY="organizations/abc-123/apiKeys/def-456"
export COINBASE_API_SECRET="$(cat ~/path/to/cdp_api_key.pem)"
```

:::tip
We recommend using environment variables to manage your credentials.
:::

### JWT lifetime

Coinbase JWTs expire after 120 seconds. Per the
[WebSocket overview](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-overview),
a different JWT must be generated for each authenticated WebSocket message
(i.e. for each subscribe). The adapter regenerates a fresh JWT for every
signed REST request and for every authenticated subscribe message; no
manual rotation is required.

## Orders capability

The tables below describe the Coinbase **venue** order surface that the planned
execution client will target. Order routing through this adapter is not
available yet; see the component status table under [Overview](#overview).
Coinbase order capabilities differ between Spot and Derivatives (perpetuals
and dated futures share the same FCM order surface).

### Order types

| Order Type             | Spot | Perpetual | Future | Notes                                                                |
|------------------------|------|-----------|--------|----------------------------------------------------------------------|
| `MARKET`               | ✓    | ✓         | ✓      | IOC on Spot; IOC or FOK on Perpetual.                                |
| `LIMIT`                | ✓    | ✓         | ✓      |                                                                      |
| `STOP_MARKET`          | -    | -         | -      | Not exposed by the venue.                                            |
| `STOP_LIMIT`           | -    | ✓         | ✓      | Not available on Spot.                                               |
| `MARKET_IF_TOUCHED`    | -    | -         | -      | Not exposed by the venue.                                            |
| `LIMIT_IF_TOUCHED`     | -    | -         | -      | Not exposed by the venue.                                            |
| `TRAILING_STOP_MARKET` | -    | -         | -      | Not exposed by the venue.                                            |

### Execution instructions

| Instruction   | Spot | Perpetual | Future | Notes                                                              |
|---------------|------|-----------|--------|--------------------------------------------------------------------|
| `post_only`   | ✓    | ✓         | ✓      | LIMIT GTC and LIMIT GTD only.                                      |
| `reduce_only` | -    | ✓         | ✓      | Derivatives only.                                                  |

### Time in force

| Time in force | Spot | Perpetual | Future | Notes                                                |
|---------------|------|-----------|--------|------------------------------------------------------|
| `GTC`         | ✓    | ✓         | ✓      | Good Till Canceled.                                  |
| `GTD`         | ✓    | ✓         | ✓      | LIMIT and STOP_LIMIT (perp/future).                  |
| `IOC`         | ✓    | ✓         | ✓      | MARKET only.                                         |
| `FOK`         | ✓    | ✓         | -      | LIMIT (Spot) and MARKET (Perpetual).                 |

### Advanced order features

| Feature            | Spot | Perpetual | Future | Notes                                                                              |
|--------------------|------|-----------|--------|------------------------------------------------------------------------------------|
| Order Modification | ✓    | ✓         | ✓      | GTC variants only (LIMIT, STOP_LIMIT, Bracket); other types use cancel‑replace.    |
| Bracket Orders     | -    | ✓         | ✓      | Native bracket on perp/future.                                                     |
| OCO Orders         | -    | -         | -      | Not exposed as a distinct order type.                                              |
| Iceberg Orders     | -    | -         | -      | Not documented.                                                                    |
| TWAP Orders        | ✓    | -         | -      | Spot only.                                                                         |
| Scaled Orders      | ✓    | -         | -      | Spot only; ladders one parent across a price range.                                |

See the [Create Order reference](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order)
and [Edit Order reference](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/edit-order)
for the underlying venue specification.

### Position controls (derivatives)

| Control       | Notes                                                                |
|---------------|----------------------------------------------------------------------|
| Leverage      | Set per order; default `1.0`.                                        |
| Margin type   | Set per order: cross (default) or isolated.                          |
| Position mode | One‑way only; hedge mode is not exposed.                             |

### Batch operations

| Operation     | Notes                                                                                              |
|---------------|----------------------------------------------------------------------------------------------------|
| Batch Submit  | Not supported. Each order is one `Create Order` request.                                           |
| Batch Modify  | Not supported. Each edit is one `Edit Order` request.                                              |
| Batch Cancel  | `POST /api/v3/brokerage/orders/batch_cancel` accepts an `order_ids` array. No documented max size; per‑order success/failure in the response. |

### Order querying

| Feature              | Spot | Perpetual | Future | Notes                                       |
|----------------------|------|-----------|--------|---------------------------------------------|
| Query open orders    | ✓    | ✓         | ✓      | List all active orders.                     |
| Query order history  | ✓    | ✓         | ✓      | Historical order data with cursor paging.   |
| Order status updates | ✓    | ✓         | ✓      | Real‑time state changes via `user` channel. |
| Trade history        | ✓    | ✓         | ✓      | Execution and fill reports.                 |

### Spot trading limitations

- `reduce_only` is not supported (the instruction applies to derivatives).
- Trailing stop orders are not supported.
- Native stop‑limit and bracket orders are not available on Spot.
- Quote‑denominated MARKET orders are supported; LIMIT orders are sized in
  base units.

### Derivatives trading

Coinbase derivatives trade through the FCM (Futures Commission Merchant)
venue. The adapter receives funding rates and mark prices through the public
WebSocket `ticker` channel today; futures balance updates through the
authenticated `futures_balance_summary` channel are planned alongside the
execution client.

#### Funding rates

The adapter receives funding rate data from the WebSocket `ticker` channel for
perpetual contracts. The `funding_rate` and `funding_time` fields are
populated when present; partial ticker updates that omit the fields fall back
to the cached last-known value per symbol. Funding interval is sourced from
the `funding_interval` field on the FCM `future_product_details` payload
(typically 3600s, i.e. hourly funding).

For historical funding rate requests, the adapter reads from the REST
products endpoint and computes the interval from consecutive funding
timestamps.

#### Position reconciliation (planned)

When the execution client lands, it will reconcile open orders and positions
from REST on connect and after a WebSocket reconnect. The reconciliation
lookback window will be configurable via the standard
`reconciliation_lookback_mins` setting on `LiveExecEngineConfig`.

#### Fill deduplication (planned)

The user-channel WebSocket can replay events on reconnect. The execution
client will deduplicate fills by `(venue_order_id, trade_id)`. After very
long disconnections (beyond the in-memory dedup window) replayed fills may
emit duplicate `OrderFilled` events; strategies should rely on REST
reconciliation to recover canonical state in that case.

## Rate limiting

Coinbase publishes the following limits for the Advanced Trade APIs:

| Surface                           | Limit                                                | Source                                                |
|-----------------------------------|------------------------------------------------------|-------------------------------------------------------|
| WebSocket connections             | 8 per second per IP address                          | Advanced Trade WebSocket Rate Limits                  |
| WebSocket unauthenticated msgs    | 8 per second per IP address                          | Advanced Trade WebSocket Rate Limits                  |
| WebSocket subscribe deadline      | First subscribe message must arrive within 5 s of connect or the server disconnects | Advanced Trade WebSocket Overview |
| Authenticated WebSocket JWT       | 120 s; a fresh JWT must be generated for every authenticated subscribe message | Advanced Trade WebSocket Overview |
| REST per‑key quota                | 10,000 requests per hour per API key (Coinbase App general policy) | Coinbase App Rate Limiting       |

When the REST limit is exceeded, Coinbase returns HTTP `429` with this body:

```json
{
  "errors": [
    {
      "id": "rate_limit_exceeded",
      "message": "Too many requests"
    }
  ]
}
```

:::info
The Advanced Trade-specific REST quota (per-second ceilings, per-portfolio
limits) is not separately published in the Advanced Trade docs at the time of
writing; the Coinbase App per-hour quota above is the most specific
documented value. References:
[REST rate limits](https://docs.cdp.coinbase.com/advanced-trade/docs/rest-api-rate-limits/),
[WebSocket rate limits](https://docs.cdp.coinbase.com/advanced-trade/docs/ws-rate-limits),
[Coinbase App rate limiting](https://docs.cdp.coinbase.com/coinbase-app/api-architecture/rate-limiting).
:::

## Reconnect and resubscribe

The WebSocket client uses exponential backoff with a base of 250ms and a cap
of 30s on reconnect. After reconnect, subscriptions are restored automatically
in the order they were created. Coinbase requires a subscribe message within
5 seconds of connection or the server disconnects; the adapter sends queued
subscriptions immediately after the WebSocket handshake completes.

For authenticated channels (`user`, `futures_balance_summary`, planned), the
adapter generates a fresh JWT for every subscribe message; per the Coinbase
docs, "you must generate a different JWT for each websocket message sent,
since the JWTs will expire after 120 seconds." Once a subscription is
accepted the data flow continues for the lifetime of the WebSocket
connection without further authentication.

## Configuration

### Data client configuration options

| Option                             | Default | Description                                   |
|------------------------------------|---------|-----------------------------------------------|
| `api_key`                          | `None`  | Falls back to `COINBASE_API_KEY` env var.     |
| `api_secret`                       | `None`  | Falls back to `COINBASE_API_SECRET` env var.  |
| `base_url_rest`                    | `None`  | Override for the REST base URL.               |
| `base_url_ws`                      | `None`  | Override for the WebSocket market data URL.   |
| `http_proxy_url`                   | `None`  | Optional HTTP proxy URL.                      |
| `ws_proxy_url`                     | `None`  | Optional WebSocket proxy URL.                 |
| `environment`                      | `Live`  | `Live` or `Sandbox`.                          |
| `http_timeout_secs`                | `10`    | HTTP request timeout (seconds).               |
| `ws_timeout_secs`                  | `30`    | WebSocket timeout (seconds).                  |
| `update_instruments_interval_mins` | `60`    | Interval between instrument catalogue refreshes. |

### Execution client configuration options (planned)

`CoinbaseExecClientConfig` is exported and constructable today, but no
execution client consumes it yet (see component status table above).

| Option                   | Default | Description                                            |
|--------------------------|---------|--------------------------------------------------------|
| `api_key`                | `None`  | Falls back to `COINBASE_API_KEY` env var.              |
| `api_secret`             | `None`  | Falls back to `COINBASE_API_SECRET` env var.           |
| `base_url_rest`          | `None`  | Override for the REST base URL.                        |
| `base_url_ws`            | `None`  | Override for the user data WebSocket URL.              |
| `http_proxy_url`         | `None`  | Optional HTTP proxy URL.                               |
| `ws_proxy_url`           | `None`  | Optional WebSocket proxy URL.                          |
| `environment`            | `Live`  | `Live` or `Sandbox`.                                   |
| `http_timeout_secs`      | `10`    | HTTP request timeout (seconds).                        |
| `max_retries`            | `3`     | Maximum retry attempts for HTTP requests.              |
| `retry_delay_initial_ms` | `100`   | Initial retry delay (milliseconds).                    |
| `retry_delay_max_ms`     | `5000`  | Maximum retry delay (milliseconds).                    |

Configurations are constructed from Python via the PyO3-exported types:

```python
from nautilus_trader.core.nautilus_pyo3 import CoinbaseDataClientConfig
from nautilus_trader.core.nautilus_pyo3 import CoinbaseExecClientConfig
from nautilus_trader.core.nautilus_pyo3 import CoinbaseEnvironment

data_config = CoinbaseDataClientConfig(
    api_key="YOUR_COINBASE_API_KEY",
    api_secret="YOUR_COINBASE_API_SECRET",
    environment=CoinbaseEnvironment.LIVE,
)

exec_config = CoinbaseExecClientConfig(
    api_key="YOUR_COINBASE_API_KEY",
    api_secret="YOUR_COINBASE_API_SECRET",
    environment=CoinbaseEnvironment.LIVE,
)
```

The v2 system instantiates the Rust factories directly from these configs;
no Python factory wiring is required.

## Known limitations

- Order modification is restricted to GTC orders (LIMIT, STOP_LIMIT, Bracket);
  other types must use cancel‑replace.
- OCO orders are not exposed as a distinct order type.
- Trailing stop, MARKET_IF_TOUCHED, LIMIT_IF_TOUCHED, and iceberg orders are
  not exposed by the venue.
- Batch submit and batch modify are not available; only batch cancel is.
- Sandbox is a static‑mock environment (Accounts and Orders endpoints only,
  pre‑defined responses, no real market data).

## Contributing

:::info
For additional features or to contribute to the Coinbase adapter, please see
our [contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
