# AX Exchange

[AX Exchange](https://architect.exchange) is a centralized and regulated derivatives exchange for
traditional underlying asset classes. Operated by Architect Bermuda Ltd. and licensed by the
[Bermuda Monetary Authority (BMA)](https://www.bma.bm/), AX lists perpetual contracts in production
and also exposes dated futures in its sandbox catalog.

This integration supports live market data ingest and order execution with AX Exchange.

## Overview

This adapter is implemented in Rust and exposed to Python through PyO3 bindings. It does not
require external AX client libraries.

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The AX Exchange adapter includes multiple components, which can be used together or separately
depending on the use case.

- `AxHttpClient`: Low-level HTTP API connectivity.
- `AxMdWebSocketClient`: Market data WebSocket connectivity.
- `AxOrdersWebSocketClient`: Orders WebSocket connectivity.
- `AxDataClient`: A market data feed manager.
- `AxExecutionClient`: An account management and trade execution gateway.
- `AxDataClientFactory`: Factory for AX data clients.
- `AxExecutionClientFactory`: Factory for AX execution clients.

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/architect_ax/examples/)

## AX Exchange documentation

AX Exchange provides documentation for users at the
[Architect documentation site](https://docs.architect.exchange/).
Refer to the AX Exchange documentation in conjunction with this NautilusTrader integration guide.

## Products

The production catalog contains perpetual contracts across these venue categories:

| Venue category   | Examples                     | Nautilus asset class |
| ---------------- | ---------------------------- | -------------------- |
| Foreign exchange | `EURUSD-PERP`, `JPYUSD-PERP` | FX                   |
| Equities         | `AAPL-PERP`, `NVDA-PERP`     | Equity               |
| Energy ETFs      | `USO-PERP`, `UNG-PERP`       | Equity               |
| Metals           | `XAU-PERP`, `XAG-PERP`       | Commodity            |
| Energy           | `WTI-PERP`                   | Commodity            |
| Treasuries       | `UST10Y-PERP`                | Debt                 |
| Compute          | `OCPI-H100-PERP`             | Alternative          |

The sandbox also lists dated gold contracts such as `XAU-2026-SEP` and `XAU-2026-DEC`.

The adapter maps a `crypto` venue category to the `CRYPTOCURRENCY` asset class, and any category it
does not recognize to `ALTERNATIVE`.

### Perpetual contracts

A perpetual contract (perpetual swap) is a derivative that tracks the price of an underlying
asset without expiring. Unlike standard futures, there is no settlement date, which eliminates
rollover costs and simplifies position management. A funding rate mechanism keeps the contract
price aligned with the underlying index price through periodic payments between long and short
holders. See the [Architect documentation](https://docs.architect.exchange/) for details on
funding rate mechanics and contract specifications.

Characteristics of AX perpetual contracts:

- **Cash-settled in USD**: No physical delivery. All profit and loss is settled in USD.
- **Funding rates**: Periodic payments keep the contract price aligned with the underlying.
- **Multiplier of 1**: Each contract represents one unit of exposure to the underlying.
- **Whole contracts only**: Fractional quantities are not supported.
- **Margin**: Initial margin is required to open a position; maintenance margin to keep it open.

The adapter represents an AX instrument without an expiration as `PerpetualContract` and an
instrument with an expiration as `FuturesContract`. The venue category determines the Nautilus
asset class. The adapter uses `MARGIN` account type and `NETTING` order management.

## Symbology

The adapter preserves each AX symbol and appends the Nautilus venue identifier `.AX`. Perpetual
symbols use the `-PERP` suffix. Dated symbols include their year and contract month.

| Contract     | AX Symbol      | Nautilus InstrumentId |
| ------------ | -------------- | --------------------- |
| EUR/USD perp | `EURUSD-PERP`  | `EURUSD-PERP.AX`      |
| Gold perp    | `XAU-PERP`     | `XAU-PERP.AX`         |
| Dated gold   | `XAU-2026-SEP` | `XAU-2026-SEP.AX`     |

The venue identifier is `AX`. To construct a Nautilus `InstrumentId`:

```python
from nautilus_trader.model import InstrumentId

instrument_id = InstrumentId.from_str("EURUSD-PERP.AX")
```

## Environments

AX Exchange provides two trading environments. Configure the appropriate environment using the
`environment` parameter in your client configuration.

| Environment    | Config                                 | Description                            |
| -------------- | -------------------------------------- | -------------------------------------- |
| **Sandbox**    | `environment=AxEnvironment.SANDBOX`    | Test environment with simulated funds. |
| **Production** | `environment=AxEnvironment.PRODUCTION` | Live trading with real funds.          |

### Sandbox

The default environment for development and testing with simulated funds.
All sandbox endpoints are resolved automatically when `environment=AxEnvironment.SANDBOX`.

#### 1. Create a sandbox account

Follow the [Architect documentation](https://docs.architect.exchange/) to create a sandbox
account. An invite code is required during registration.

#### 2. Create API keys and fund the account

Use the AX sandbox UI to generate API keys and deposit simulated funds into your account.
Store the `api_key` and `api_secret` securely.

#### 3. Set environment variables

```bash
export AX_API_KEY="your-sandbox-api-key"
export AX_API_SECRET="your-sandbox-api-secret"
```

#### 4. Configure the live node

Set `environment=AxEnvironment.SANDBOX` on the data and execution client configs. See the
[Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax/)
for complete `LiveNode` setup.

### Production

For live trading with real funds. Requires a verified AX Exchange account.

```python
config = AxExecutionClientConfig(
    environment=AxEnvironment.PRODUCTION,
)
```

:::warning
Ensure you are using the correct environment before placing orders.
Sandbox is the default to prevent accidental live trading.
:::

## Market data

The adapter provides real-time market data via WebSocket subscriptions, with HTTP endpoints
for historical data backfill.

### Data types

| AX Data           | Nautilus Data Type  | Notes                                                          |
| ----------------- | ------------------- | -------------------------------------------------------------- |
| Order book (L1)   | `QuoteTick`         | Best bid/ask top-of-book from L1 book subscription.            |
| Order book (L2)   | `OrderBookDelta`    | Aggregated price levels.                                       |
| Order book (L3)   | `OrderBookDelta`    | Per-snapshot order quantities with synthetic IDs.              |
| Trades            | `TradeTick`         | Real-time trade events from trade-only WebSocket subscription. |
| Mark price        | `MarkPriceUpdate`   | Extracted from L1 ticker subscription.                         |
| Bars/candles      | `Bar`               | OHLCV data (total volume only, no buy/sell breakdown).         |
| Funding rates     | `FundingRateUpdate` | Polled via HTTP; interval configurable.                        |
| Instrument status | `InstrumentStatus`  | State changes from L1 ticker subscription.                     |

AX instrument states map to `MarketStatusAction` as follows:

| AX state                            | `MarketStatusAction`        |
| ----------------------------------- | --------------------------- |
| Pre-open                            | `PRE_OPEN`                  |
| Open                                | `TRADING`                   |
| Closed, closed-frozen               | `CLOSE`                     |
| Halted                              | `HALT`                      |
| Match-and-close auction             | `CROSS`                     |
| Suspended                           | `SUSPEND`                   |
| Delisted, or any unrecognized state | `NOT_AVAILABLE_FOR_TRADING` |

:::note
Historical quote tick requests are not supported by AX Exchange. Only real-time quote
data is available via WebSocket L1 book subscriptions. AX also publishes no index prices and no
instrument close events, so those subscriptions log a warning and yield no data.
:::

:::note
AX L3 snapshots contain per-order quantities but no venue order IDs. The adapter assigns synthetic
IDs within each snapshot. It cannot track the same individual order across snapshots.
:::

:::note
AX publishes no trade identifier for market data, so the adapter derives `TradeTick.trade_id` from the
trade's timestamp, price, size, and aggressor side. REST and WebSocket agree on the same trade whenever
both report its aggressor side. Prints that AX reports identically share an ID; only consumers that
deduplicate market data on `trade_id` are affected, since fills carry the venue's own trade IDs.
:::

### WebSocket subscription behavior

AX market data WebSocket subscriptions use one active stream per symbol. The adapter selects the
smallest stream that covers the active Nautilus subscriptions:

- A trades-only subscription uses AX `level: "TRADES"`, which delivers trade prints only.
- Book-only and quote-only subscriptions set AX `trades: false` and `ticker: false` to suppress
  unrequested trade and ticker events.
- Mark price and instrument status subscriptions require AX ticker events, so the adapter enables
  ticker delivery on the active book stream, opening an L1 stream when no book subscription exists.
- Book deltas subscribe at the AX level matching the Nautilus book type. `L1_MBP` has no
  delta-capable AX equivalent, so the adapter logs a warning and subscribes at L2 instead.
- If multiple Nautilus data types are active for a symbol, the adapter resubscribes only when the
  required AX level or delivery flags change.

AX documents estimated funding rates on ticker events and an estimated-funding request on the orders
WebSocket. Nautilus exposes settled funding-rate updates through HTTP polling; the adapter does not
parse or emit the venue's estimated funding fields as a separate Nautilus data type.

### HTTP API behavior

- `GET /tickers` returns limit/offset page metadata and supports `limit`, `offset`, and `sort`
  query parameters.
- `GET /ticker` returns the ticker under a top-level `ticker` response field.
- `GET /open-orders` uses limit/offset pagination. Open-order reconciliation traverses all pages
  and validates totals, offsets, duplicates, and completeness so detected response drift fails the
  request.
- `GET /fills` and `GET /funding-rates` use cursor pagination. The adapter traverses each cursor
  chain as a best-effort historical read; AX corrections during traversal are not an atomic
  snapshot.
- `GET /orders` exposes cursor metadata and supports `order_id`, `order_ids`, `account_id`, and
  optional timestamp filters. Startup mass-status reconciliation traverses its cursor chain,
  accepts partial pages, and rejects repeated cursors or duplicate order IDs.
- Open-order, historical-order, fill, and position report requests resolve an uncached symbol
  through `GET /instrument` and cache the result. An instrument request or parse failure fails that
  entire report request instead of dropping venue state.
- `GET /transactions` requires `start_timestamp_ns` and `end_timestamp_ns` with a range no wider
  than 7 days. The low-level client exposes its cursor and account selectors.
- `GET /order-status` can include `reject_reason` and `reject_message` for rejected orders.
- When an account selector is omitted, AX uses the primary account. The high-level execution client
  owns one primary account; low-level request models expose documented account selectors.

### Bar intervals

| Interval | Description |
| -------- | ----------- |
| `1s`     | 1-second    |
| `5s`     | 5-second    |
| `1m`     | 1-minute    |
| `5m`     | 5-minute    |
| `15m`    | 15-minute   |
| `1h`     | 1-hour      |
| `1d`     | 1-day       |

## Orders capability

The AX order-entry API has no order-type selector. Its single native order shape requires a price,
which the adapter maps to a Nautilus `LIMIT` order. The adapter simulates a Nautilus `MARKET` order
by previewing an aggressive price and submitting that priced shape with IOC.

The official [REST place-order](https://docs.architect.exchange/api-reference/order-management/place-order)
and [orders WebSocket](https://docs.architect.exchange/api-reference/order-management/orders-ws)
request schemas contain no `order_type` or `trigger_price` field, and sandbox stop-limit submissions
with unbreached triggers executed immediately at the active limit price. With conditional execution
unconfirmed, the adapter rejects venue-native stop-limit orders before sending them.

Nautilus can still emulate a stop-limit order locally. The common order emulator waits for the
configured trigger, then sends a plain limit order to this adapter.

### Order types

| Order Type             | Supported | Notes                                           |
| ---------------------- | --------- | ----------------------------------------------- |
| `MARKET`               | ✓         | Adapter-simulated with an aggressive IOC price. |
| `LIMIT`                | ✓         | Maps to the native AX priced order shape.       |
| `STOP_LIMIT`           | -         | *Not supported by AX Exchange*.                 |
| `LIMIT_IF_TOUCHED`     | -         | *Not supported by AX Exchange*.                 |
| `STOP_MARKET`          | -         | *Not supported by AX Exchange*.                 |
| `MARKET_IF_TOUCHED`    | -         | *Not supported by AX Exchange*.                 |
| `TRAILING_STOP_MARKET` | -         | *Not supported by AX Exchange*.                 |

### Execution instructions

| Instruction      | Supported | Notes                                                         |
| ---------------- | --------- | ------------------------------------------------------------- |
| `post_only`      | ✓         | Maker-only; rejected if the order would take.                 |
| `reduce_only`    | -         | Rejected locally; AX exposes no reduce-only field.            |
| `quote_quantity` | -         | Rejected locally; the adapter wire path encodes base only.    |
| `display_qty`    | -         | Rejected locally; the adapter wire path has no display field. |

The reduce-only boundary matters because AX has no reduce-only field. In sandbox, an order whose
reduce-only instruction was dropped from the wire payload was accepted and filled as an ordinary
order, which can open or increase exposure instead of closing it; production behavior was not
verified. The adapter therefore denies reduce-only orders before submission rather than sending an
instruction the venue cannot honor.

The adapter also rejects quote-quantity and display-quantity instructions because its AX wire path
cannot encode those semantics. This is an adapter boundary, not a claim that AX Exchange rejects
equivalent venue-native features.

### Time in force

| Time in Force  | Supported | Notes                            |
| -------------- | --------- | -------------------------------- |
| `GTC`          | ✓         | Good Till Canceled.              |
| `GTD`          | -         | Rejected locally by the adapter. |
| `DAY`          | ✓         | Valid until end of trading day.  |
| `IOC`          | ✓         | Immediate or Cancel.             |
| `FOK`          | -         | Rejected locally by the adapter. |
| `AT_THE_OPEN`  | -         | Rejected locally by the adapter. |
| `AT_THE_CLOSE` | -         | Rejected locally by the adapter. |

The venue deprecates `DAY` and recommends `GTC` instead.

### Advanced order features

| Feature            | Supported | Notes                                                              |
| ------------------ | --------- | ------------------------------------------------------------------ |
| Order modification | ✓         | Atomic replace; AX returns a new venue order ID.                   |
| Cancel order       | ✓         | Single order cancellation.                                         |
| Cancel all orders  | ✓         | Cancel all open orders for an instrument.                          |
| Batch cancel       | -         | The adapter sends individual cancels.                              |
| Order lists        | ✓         | Sequential submission (orders submitted individually, non-atomic). |

### Position management

| Feature         | Supported | Notes                                |
| --------------- | --------- | ------------------------------------ |
| Query positions | ✓         | Real-time position updates.          |
| Position mode   | -         | Netting mode only.                   |
| Cross margin    | ✓         | Cross-margin across all instruments. |

### Order querying

| Feature              | Supported | Notes                                                   |
| -------------------- | --------- | ------------------------------------------------------- |
| Query open orders    | ✓         | List all active orders.                                 |
| Query single order   | ✓         | By venue order ID or client order ID (any order state). |
| Order status reports | ✓         | Open-order checks and historical startup mass status.   |
| Fill reports         | ✓         | Execution and fill history.                             |

:::note
Bulk open-order checks use `/open-orders` when `open_check_open_only` is enabled, which is the
default. Otherwise, they use `/orders`. Startup mass-status reconciliation uses `/orders`, so its
snapshot includes historical terminal orders such as filled and canceled orders. Single-order
queries via `query_order` use the dedicated `/order-status` endpoint, which works for any order
state.

AX open and historical order payloads do not expose a stop order type or trigger price.
REST-derived reconciliation therefore reports every visible external order as a limit order. The
adapter does not submit venue-native conditional orders.
:::

## Authentication

AX Exchange uses bearer token authentication:

1. API key and secret obtain a session token via `/authenticate`.
2. The session token is used as a bearer token for subsequent REST and WebSocket requests.
3. The adapter requests one-hour session tokens and refreshes them every 30 minutes.
4. A refresh updates REST authentication and the token used by the next WebSocket reconnect without
   interrupting the active connection.

## Configuration

### Environments and endpoints

| Environment | HTTP API                                         | HTTP API (orders)                                   | Market Data WS                                   | Orders WS                                            |
| ----------- | ------------------------------------------------ | --------------------------------------------------- | ------------------------------------------------ | ---------------------------------------------------- |
| Sandbox     | `https://gateway.sandbox.architect.exchange/api` | `https://gateway.sandbox.architect.exchange/orders` | `wss://gateway.sandbox.architect.exchange/md/ws` | `wss://gateway.sandbox.architect.exchange/orders/ws` |
| Production  | `https://gateway.architect.exchange/api`         | `https://gateway.architect.exchange/orders`         | `wss://gateway.architect.exchange/md/ws`         | `wss://gateway.architect.exchange/orders/ws`         |

:::info
Order management endpoints (place, cancel, replace, cancel-all, order status, open orders,
historical orders, and initial margin requirement) use the orders base URL. Every other REST
endpoint, including authentication, account state, fills, transactions, and market data, uses the
API base URL. The adapter resolves both from the configured environment.
:::

### Data client configuration options

| Option                             | Default   | Description                                                         |
| ---------------------------------- | --------- | ------------------------------------------------------------------- |
| `api_key`                          | `None`    | API key; loaded from `AX_API_KEY` env var when omitted.             |
| `api_secret`                       | `None`    | API secret; loaded from `AX_API_SECRET` env var when omitted.       |
| `environment`                      | `SANDBOX` | Trading environment (`SANDBOX` or `PRODUCTION`).                    |
| `base_url_http`                    | `None`    | Override for the REST base URL.                                     |
| `base_url_ws_public`               | `None`    | Override for the market data WebSocket URL.                         |
| `base_url_ws_private`              | `None`    | Override for the private orders WebSocket URL.                      |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket transports.               |
| `http_timeout_secs`                | `60`      | Timeout (seconds) for REST requests.                                |
| `max_retries`                      | `3`       | Maximum retry attempts for REST requests.                           |
| `retry_delay_initial_ms`           | `1,000`   | Initial delay (milliseconds) between retries.                       |
| `retry_delay_max_ms`               | `10,000`  | Maximum delay (milliseconds) between retries (exponential backoff). |
| `heartbeat_interval_secs`          | `20`      | Heartbeat interval (seconds) for WebSocket connections.             |
| `recv_window_ms`                   | `5,000`   | Reserved; AX uses bearer tokens and the adapter sends no window.    |
| `update_instruments_interval_mins` | `60`      | Interval (minutes) between instrument catalog refreshes.            |
| `funding_rate_poll_interval_mins`  | `15`      | Interval (minutes) between funding rate poll requests.              |
| `transport_backend`                | `Sockudo` | WebSocket transport backend.                                        |

### Execution client configuration options

| Option                    | Default   | Description                                                         |
| ------------------------- | --------- | ------------------------------------------------------------------- |
| `account_id`              | `AX-001`  | Account ID for the execution client.                                |
| `api_key`                 | `None`    | API key; loaded from `AX_API_KEY` env var when omitted.             |
| `api_secret`              | `None`    | API secret; loaded from `AX_API_SECRET` env var when omitted.       |
| `environment`             | `SANDBOX` | Trading environment (`SANDBOX` or `PRODUCTION`).                    |
| `base_url_http`           | `None`    | Override for the API REST base URL.                                 |
| `base_url_orders`         | `None`    | Override for the orders REST base URL.                              |
| `base_url_ws_private`     | `None`    | Override for the orders WebSocket URL.                              |
| `proxy_url`               | `None`    | Optional proxy URL for HTTP and WebSocket transports.               |
| `http_timeout_secs`       | `60`      | Timeout (seconds) for REST requests.                                |
| `max_retries`             | `3`       | Maximum retry attempts for REST requests.                           |
| `retry_delay_initial_ms`  | `1,000`   | Initial delay (milliseconds) between retries.                       |
| `retry_delay_max_ms`      | `10,000`  | Maximum delay (milliseconds) between retries (exponential backoff). |
| `heartbeat_interval_secs` | `30`      | Heartbeat interval (seconds) for WebSocket connections.             |
| `recv_window_ms`          | `5,000`   | Reserved; AX uses bearer tokens and the adapter sends no window.    |
| `cancel_on_disconnect`    | `False`   | Cancel this WebSocket session's open orders on disconnect.          |
| `transport_backend`       | `Sockudo` | WebSocket transport backend.                                        |

When `transport_backend=None`, the compiled Rust default selects Sockudo when the
`transport-sockudo` Cargo feature is enabled and Tungstenite otherwise.

Use `AxDataClientConfig` with `AxDataClientFactory` and `AxExecutionClientConfig` with
`AxExecutionClientFactory`. The Python examples show the complete `LiveNode.builder(...)`
configuration for data and execution clients.

### API credentials

There are two options for supplying your credentials to the AX Exchange clients.
Either pass the corresponding `api_key` and `api_secret` values to the configuration objects, or
set the following environment variables:

- `AX_API_KEY`
- `AX_API_SECRET`

:::tip
We recommend using environment variables to manage your credentials.
:::

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.

## Implementation notes

- **Whole contracts only**: AX uses integer contract quantities. The adapter models a one-contract
  size increment and lot size, while enforcing each instrument's separate `minimum_order_size`.
  Fractional quantities generate `OrderDenied` locally.
- **Dated futures activation**: AX publishes expiration but not activation timestamps. The adapter
  uses zero for the unknown activation time and preserves that limitation in instrument metadata.
- **Rate limiting**: The adapter applies a conservative rate limit of 10 requests/second with
  automatic exponential backoff on rate limit responses.
- **Market orders**: AX does not support native market orders. The adapter uses a preview endpoint
  to determine the take-through price and submits an aggressive IOC limit order. Because the book
  can move between the preview and the submission, a simulated market order may fill partially.
- **Stop-limit orders**: The adapter rejects venue-native stop-limit submissions because sandbox
  testing did not confirm conditional semantics. Use local order emulation when a strategy
  requires a stop-limit order.
- **Order modification**: AX supports atomic order replacement via `POST /replace-order`. The
  execution client maps `modify_order` to this endpoint and records the new venue order ID it
  returns. A modification is rejected locally when it carries a trigger price, which AX has no
  field for, or when the order has no venue order ID yet.
- **Funding rate polling**: The data client polls `GET /funding-rates` per subscribed instrument on
  `funding_rate_poll_interval_mins`, requesting a seven-day lookback so a rate is still found across
  weekends and holidays, and emits the latest rate only when it differs from the last one emitted.
- **Cancel on disconnect**: Set `cancel_on_disconnect=True` in the execution client config
  to have the exchange cancel all open orders if the orders WebSocket disconnects.
- **Instrument fee rates**: AX reports maker and taker rates per account on `GET /whoami`, so the
  adapter resolves them after authenticating and applies them to every instrument. A client with
  credentials fails to connect if that lookup fails, rather than reporting zero fees for the process
  lifetime. A data client configured without credentials cannot read the rates and reports zero fees.
- **Fill commissions**: Real-time fill events from the WebSocket do not include fee data.
  Commission is reported as zero for streaming fills. During reconciliation, the REST
  `/fills` endpoint provides accurate fee information.
- **Fill reconciliation window**: The `/fills` endpoint requires a bounded time range and
  caps the span at seven days. Reconciliation requests the most recent seven days of fills;
  fills older than that are not reconciled.
- **Fill order identity**: AX can omit `order_id` for block trades and final settlement fills. The
  adapter derives a deterministic reconciliation order ID from `trade_id` for those classified
  records. Classification fields are optional for regular fills with a valid `order_id`. The adapter
  rejects rows with neither an order ID nor explicit special-fill classification, and rejects
  inconsistent classification.
- **Unfilled IOC/FOK**: AX reports an unfilled immediate order as an expiry; the adapter maps
  it to `OrderCanceled` to match NautilusTrader semantics.
- **One-tick quotes**: Example testers place post-only limits one tick from top of book.
  Those quotes can still fill. Flatten leftovers with
  `cargo run --bin ax-flatten -p nautilus-architect-ax` (`AX_IS_SANDBOX` defaults to true).
  That binary cancels all open orders on the account, then closes every position.

## Contributing

:::info
For additional features or to contribute to the AX Exchange adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
