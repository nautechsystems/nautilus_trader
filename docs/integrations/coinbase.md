# Coinbase

Founded in 2012, Coinbase is one of the largest US-regulated cryptocurrency
exchanges, offering trading across spot, perpetual swaps, and dated futures via
the Advanced Trade API. This adapter supports live market data ingest and
**spot** order execution; perpetual and dated futures execution is deferred
to a future derivatives-specific exec client (see the
[Spot-only execution scope](#spot-only-execution-scope) note).

## Overview

The Coinbase adapter is implemented in Rust and consumed by the v2 system.
The adapter does not ship a legacy Python `TradingNode` integration; only
configuration and enum types are exported through PyO3 so v2 entry points can
construct them from Python.

Current components:

| Component                          | Status | Notes                                                          |
|------------------------------------|--------|----------------------------------------------------------------|
| `CoinbaseHttpClient`               | Built  | Two‑layer REST client: raw endpoint methods + domain wrapper.  |
| `CoinbaseWebSocketClient`          | Built  | Low‑level WebSocket connectivity with JWT subscribe auth.      |
| `CoinbaseInstrumentProvider`       | Built  | Instrument parsing and loading.                                |
| `CoinbaseDataClient`               | Built  | Rust market data feed manager.                                 |
| `CoinbaseDataClientFactory`        | Built  | Rust data client factory.                                      |
| `CoinbaseExecutionClient`          | Built  | Spot‑only Rust execution client (REST orders + WS user feed).  |
| `CoinbaseExecutionClientFactory`   | Built  | Rust execution client factory.                                 |

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

The tables below describe the Coinbase **venue** order surface. The shipped
[`CoinbaseExecutionClient`](#spot-only-execution-scope) routes spot orders;
perpetual and dated futures rows describe what the venue supports, not what
this client currently submits. Coinbase order capabilities differ between
Spot and Derivatives (perpetuals and dated futures share the same FCM order
surface).

### Spot-only execution scope

The `CoinbaseExecutionClient` factory hardcodes `AccountType::Cash` and
`OmsType::Netting`, and `generate_position_status_reports` returns empty.
Margin / position bookkeeping for derivatives is therefore not represented.
To prevent silent inconsistencies, three guards are in place:

1. The connect-time instrument bootstrap loads only `CoinbaseProductType::Spot`
   products.
2. `submit_order` denies any order whose instrument is not present in the
   spot-only cache.
3. `generate_order_status_report(s)` and `generate_fill_reports` post-filter
   their output through the same spot cache, so a Coinbase account that holds
   both spot and derivative activity will not surface derivative reports
   through this client.

A separate derivatives execution client variant is planned.

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
WebSocket `ticker` channel today. **Order routing for derivatives is not
supported by the current `CoinbaseExecutionClient`** (see
[Spot-only execution scope](#spot-only-execution-scope)). Futures balance
updates through the authenticated `futures_balance_summary` channel and
margin/position reconciliation are deferred to a future derivatives-specific
exec client.

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

#### Position reconciliation

The execution client returns no position reports today (Coinbase spot has no
positions; futures position reporting is not yet implemented). Open orders
and historical fills are still reconciled from REST via
`generate_order_status_report(s)` and `generate_fill_reports` on connect and
on the standard reconciliation interval set by `LiveExecEngineConfig`.

#### Fill deduplication

The user-channel WebSocket can replay events on reconnect. The execution
client maintains a 10,000-entry FIFO dedup keyed on
`(venue_order_id, trade_id)` and drops any fill whose synthesized trade ID
matches a recently-seen one. After very long disconnections (beyond the
in-memory dedup window) replayed fills may emit duplicate `OrderFilled`
events; strategies should rely on REST reconciliation to recover canonical
state in that case.

## Execution client behaviour

This section documents how `CoinbaseExecutionClient` translates Nautilus
order commands and Coinbase venue events into Nautilus execution events.

### Order submission

`submit_order` builds the Coinbase `order_configuration` shape directly from
Nautilus order fields:

- `MARKET` -> `market_market_ioc`. Only `TimeInForce::Ioc` and `Gtc` (the
  Nautilus default) are accepted; any explicit `Fok`, `Day`, or `Gtd` on a
  market order is rejected before the HTTP call so callers do not silently
  receive IOC semantics. A `MARKET` order built with `Gtc` executes as IOC
  at the venue; strategies that require strict backtest/live parity should
  construct `MarketOrder` with `Ioc` explicitly.
- `LIMIT` GTC -> `limit_limit_gtc`, GTD -> `limit_limit_gtd` (requires
  `expire_time`), FOK -> `limit_limit_fok`.
- `STOP_LIMIT` GTC -> `stop_limit_stop_limit_gtc`, GTD ->
  `stop_limit_stop_limit_gtd`. Stop direction is derived from the order
  side (`Buy` -> `STOP_DIRECTION_STOP_UP`, `Sell` -> `STOP_DIRECTION_STOP_DOWN`).
- `STOP_MARKET`, `MARKET_IF_TOUCHED`, `LIMIT_IF_TOUCHED`, and trailing-stop
  variants are rejected with `OrderDenied` (not exposed by the venue).

On a successful HTTP create, an `OrderAccepted` is emitted carrying the
venue order ID returned in `success_response.order_id`. On a `success=false`
response or HTTP error, `OrderRejected` is emitted with the formatted
failure reason.

### Order modification

`modify_order` posts to `/orders/edit` with the typed `EditOrderRequest`.
Coinbase restricts edits to GTC variants (LIMIT, STOP_LIMIT, Bracket); other
order types must use cancel-replace. The exec client forwards `price`,
`quantity`, and `trigger_price` (mapped to the venue's `stop_price` field).
Failures emit `OrderModifyRejected` with the typed `EditOrderResponse`
failure reason (preferring `edit_failure_reason`, falling back to
`preview_failure_reason`).

### Cancellation

- `cancel_order` posts a single-id `batch_cancel`. Per-order failure surfaces
  as `OrderCancelRejected`.
- `cancel_all_orders` lists open orders via REST without the `OPEN`-only
  filter (because Coinbase's `OPEN` filter excludes `PENDING` and `QUEUED`
  orders that are still cancelable), filters locally to
  `{Submitted, Accepted, Triggered, PendingUpdate, PartiallyFilled}` and
  the requested side, then chunks `batch_cancel` calls in groups of 100.
  Per-order and transport failures emit `OrderCancelRejected` for every
  affected order.
- `batch_cancel_orders` chunks the same way and surfaces both per-order
  failures and transport errors as `OrderCancelRejected`.

### User WebSocket channel

`CoinbaseExecutionClient` subscribes to the `user` channel with no
`product_ids` filter (returns events for all products) and to a fresh JWT.
Each user event is parsed into an `OrderStatusReport` and fed to the
execution event stream. Coinbase reports cumulative state per order rather
than per-trade fills, so the exec client tracks
`(filled_qty, total_fees, avg_price, max_quantity)` per venue order and:

1. Synthesizes a `FillReport` from the cumulative delta. The per-fill price
   is derived as `(avg_now * qty_now - avg_prev * qty_prev) / delta_qty` so
   multi-fill orders carry the correct trade price rather than the
   cumulative weighted average.
2. Restores the original quantity on terminal updates (`CANCELLED`,
   `EXPIRED`, `FAILED`) where the venue zeroes `leaves_quantity` and
   cum+leaves would otherwise collapse to `filled_qty`.
3. Suppresses fill synthesis on `snapshot` events but uses them to seed
   the cumulative-state baseline so subsequent live updates compute correct
   deltas.
4. Persists cumulative state across WebSocket reconnects via
   `Arc<Mutex<...>>` owned by the exec client (not the feed handler).

On reconnect, account state is re-fetched via REST so balance changes during
the disconnect window are recovered.

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

For authenticated channels (`user` today, `futures_balance_summary` deferred
with the derivatives exec client), the adapter generates a fresh JWT for
every subscribe message; per the Coinbase docs, "you must generate a
different JWT for each websocket message sent, since the JWTs will expire
after 120 seconds." Once a subscription is accepted the data flow continues
for the lifetime of the WebSocket connection without further authentication.

When the exec client's WebSocket reconnects, the inner client is rebuilt
from scratch (rather than relying on the existing connection's state
machine) to guarantee a fresh `cmd_tx`/`out_rx`/signal trio even if the
prior session's `Disconnect` command lost a race with the shutdown signal.
Cumulative per-order tracking persists across reconnects so synthesized
fill deltas remain correct.

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

### Execution client configuration options

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

### Venue-side

- Order modification is restricted to GTC orders (LIMIT, STOP_LIMIT, Bracket);
  other types must use cancel-replace.
- OCO orders are not exposed as a distinct order type.
- Trailing stop, MARKET_IF_TOUCHED, LIMIT_IF_TOUCHED, and iceberg orders are
  not exposed by the venue.
- Batch submit and batch modify are not available; only batch cancel is.
- Sandbox is a static-mock environment (Accounts and Orders endpoints only,
  pre-defined responses, no real market data).
- The user-channel WebSocket reports cumulative per-order state, not
  per-trade fills. The exec client derives per-fill quantity, price, and
  commission from the cumulative delta; per-trade `trade_id`s are
  synthesized from `(venue_order_id, cumulative_quantity)`.

### Adapter-side

- **Spot only.** Submission, modification, cancellation, and report
  generation are filtered to spot products. Derivatives orders submitted
  through this client are denied. See
  [Spot-only execution scope](#spot-only-execution-scope).
- **Position reports return empty.** Coinbase spot has no positions; futures
  position reporting awaits the derivatives exec client variant.
- **External-order reconciliation from the WS user channel is unsafe for
  LIMIT and STOP_LIMIT.** The Coinbase user channel does not include
  `price`, `stop_price`, or `trigger_type` on order updates. If the engine's
  `LiveExecEngineConfig.filter_unclaimed_external_orders` is `false`
  (the default), an `OrderStatusReport` for an order this client did not
  submit will reach the engine's external-order reconcile path, which can
  panic when reconstructing a `LimitOrder`/`StopLimitOrder` without those
  fields. **Set `filter_unclaimed_external_orders = true` when running this
  adapter alongside other clients on the same Coinbase account.** A
  REST-enrichment fix is tracked for a follow-up.
- **Cancel-all and batch-cancel REST list failures are logged only.** If the
  list-open-orders REST call fails, no per-order `OrderCancelRejected` is
  emitted; orders remain in `PendingCancel` until the next reconciliation
  recovers them. Mirrors the Bybit adapter pattern.
- **Newly listed spot products require a reconnect to be tradeable.** The
  spot instrument cache is populated on connect; products listed after that
  are not in the cache and `submit_order` will deny them.
- **MARKET orders execute as IOC even when constructed with the Nautilus
  default `TimeInForce::Gtc`.** Coinbase's only MARKET wrapper is
  `market_market_ioc`. Strategies needing strict backtest/live parity for
  MARKET orders should construct `MarketOrder` with `TimeInForce::Ioc`
  explicitly. Explicit `Fok`, `Day`, or `Gtd` on a MARKET order is rejected.

## Contributing

:::info
For additional features or to contribute to the Coinbase adapter, please see
our [contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
