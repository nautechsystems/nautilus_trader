# AX Exchange

[AX Exchange](https://architect.exchange) is a centralized and regulated derivatives exchange for
traditional underlying asset classes. Operated by Architect Bermuda Ltd. and licensed by the
[Bermuda Monetary Authority (BMA)](https://www.bma.bm/), AX lists perpetual contracts in production
and also exposes dated futures in its sandbox catalog.

This integration supports live market data ingest and order execution with AX Exchange.

## Examples

You can find live example scripts in the [examples/live/architect_ax](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax/) directory.

## Overview

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The AX Exchange adapter includes multiple components, which can be used together or separately
depending on the use case.

- `AxHttpClient`: Low-level HTTP API connectivity.
- `AxMdWebSocketClient`: Market data WebSocket connectivity.
- `AxOrdersWebSocketClient`: Orders WebSocket connectivity.
- `AxInstrumentProvider`: Instrument parsing and loading functionality.
- `AxDataClient`: A market data feed manager.
- `AxExecutionClient`: An account management and trade execution gateway.
- `AxLiveDataClientFactory`: Factory for AX data clients (used by the trading node builder).
- `AxLiveExecClientFactory`: Factory for AX execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## AX Exchange documentation

AX Exchange provides documentation for users which can be found at the
[Architect documentation site](https://docs.architect.exchange/).
It's recommended you also refer to the AX Exchange documentation in conjunction with this
NautilusTrader integration guide.

## Products

The production catalog currently contains perpetual contracts across these venue categories:

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
from nautilus_trader.model.identifiers import InstrumentId

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

#### 4. Configure the trading node

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        AX: AxDataClientConfig(
            environment=AxEnvironment.SANDBOX,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        AX: AxExecClientConfig(
            environment=AxEnvironment.SANDBOX,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
)
```

### Production

For live trading with real funds. Requires a verified AX Exchange account.

```python
config = AxExecClientConfig(
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

| AX Data           | Nautilus Data Type  | Notes                                                             |
| ----------------- | ------------------- | ----------------------------------------------------------------- |
| Order book (L1)   | `QuoteTick`         | Best bid/ask top‑of‑book from L1 book subscription.               |
| Order book (L2)   | `OrderBookDelta`    | Aggregated price levels.                                          |
| Order book (L3)   | `OrderBookDelta`    | Per‑snapshot order quantities with synthetic IDs.                 |
| Trades            | `TradeTick`         | Real‑time trade events from trade‑only WebSocket subscription.    |
| Mark price        | `MarkPriceUpdate`   | Extracted from L1 ticker subscription.                            |
| Bars/candles      | `Bar`               | OHLCV data (total volume only, no buy/sell breakdown).            |
| Funding rates     | `FundingRateUpdate` | Polled via HTTP; interval configurable.                           |
| Instrument status | `InstrumentStatus`  | State changes (open, halted, closed) from L1 ticker subscription. |

:::note
Historical quote tick requests are not supported by AX Exchange. Only real-time quote
data is available via WebSocket L1 book subscriptions.
:::

:::note
AX L3 snapshots contain per-order quantities but no venue order IDs. The adapter assigns synthetic
IDs within each snapshot. It cannot track the same individual order across snapshots.
:::

:::note
AX publishes no trade identifier for market data, so the adapter derives `TradeTick.trade_id` from the
trade's own timestamp and content. REST and WebSocket agree on the same trade whenever both report its
aggressor side. Prints that AX reports identically share an ID; only consumers that deduplicate market
data on `trade_id` are affected, since fills carry the venue's own trade IDs.
:::

### WebSocket subscription behavior

AX market data WebSocket subscriptions use one active stream per symbol. The adapter selects the
smallest stream that covers the active Nautilus subscriptions:

- `subscribe_trades` uses AX `level: "TRADES"`, which delivers trade prints only.
- Book-only and quote-only subscriptions set AX `trades: false` and `ticker: false` to suppress
  unrequested trade and ticker events.
- Mark price and instrument status subscriptions require AX ticker events, so the adapter enables
  ticker delivery on the L1 stream when either data type is active.
- If multiple Nautilus data types are active for a symbol, the adapter resubscribes only when the
  required AX level or delivery flags change.

AX release notes also describe estimated funding rates on ticker events and an order WebSocket
estimated-funding request. Nautilus currently exposes settled funding-rate updates through HTTP
polling; the adapter does not parse or emit the venue's estimated funding fields as a separate
Nautilus data type.

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

The current AX order-entry API has no order-type selector. Its single native order shape requires
a price, which the adapter maps to a Nautilus `LIMIT` order. The adapter simulates a Nautilus
`MARKET` order by previewing an aggressive price and submitting that priced shape with IOC.

The current official [REST place-order](https://docs.architect.exchange/api-reference/order-management/place-order)
and [orders WebSocket](https://docs.architect.exchange/api-reference/order-management/orders-ws)
request schemas do not contain `order_type` or `trigger_price` fields. A sandbox test on 2026-07-18
submitted buy and sell stop-limit requests whose triggers were not breached. Both requests executed
immediately at the active limit prices. This did not confirm conditional execution semantics, so the
adapter rejects venue-native stop-limit orders before sending them.

Nautilus can still emulate a stop-limit order locally. The common order emulator waits for the
configured trigger, then sends a plain limit order to this adapter.

### Nautilus order types

| Order Type             | Supported | Notes                                           |
| ---------------------- | --------- | ----------------------------------------------- |
| `MARKET`               | ✓         | Adapter‑simulated with an aggressive IOC price. |
| `LIMIT`                | ✓         | Maps to the native AX priced order shape.       |
| `STOP_LIMIT`           | -         | *Not supported by AX Exchange*.                 |
| `LIMIT_IF_TOUCHED`     | -         | *Not supported by AX Exchange*.                 |
| `STOP_MARKET`          | -         | *Not supported by AX Exchange*.                 |
| `MARKET_IF_TOUCHED`    | -         | *Not supported by AX Exchange*.                 |
| `TRAILING_STOP_MARKET` | -         | *Not supported by AX Exchange*.                 |

### Execution instructions

| Instruction      | Supported | Notes                                                         |
| ---------------- | --------- | ------------------------------------------------------------- |
| `post_only`      | ✓         | Maker‑only; rejected if the order would take.                 |
| `reduce_only`    | -         | Rejected locally; AX exposes no reduce‑only field.            |
| `quote_quantity` | -         | Rejected locally; the adapter wire path encodes base only.    |
| `display_qty`    | -         | Rejected locally; the adapter wire path has no display field. |

A sandbox test on 2026-07-18 confirmed why this boundary is required. AX accepted and filled a
reduce-only order as an ordinary order when the instruction was omitted from the wire payload. The
adapter now denies reduce-only orders before submission. The sandbox account was returned to a flat
position after the test; production behavior was not tested.

The adapter also rejects quote-quantity and display-quantity instructions because its current AX
wire path cannot encode those semantics. This is an adapter boundary, not a claim that AX Exchange
rejects equivalent venue-native features.

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
| Order modification | ✓         | Rust client only; the Python client rejects modification requests. |
| Cancel order       | ✓         | Single order cancellation.                                         |
| Cancel all orders  | ✓         | Cancel all open orders for an instrument.                          |
| Batch cancel       | -         | The adapter sends individual cancels.                              |
| Order lists        | ✓         | Sequential submission (orders submitted individually, non‑atomic). |

### Position management

| Feature         | Supported | Notes                                |
| --------------- | --------- | ------------------------------------ |
| Query positions | ✓         | Real‑time position updates.          |
| Position mode   | -         | Netting mode only.                   |
| Cross margin    | ✓         | Cross‑margin across all instruments. |

### Order querying

| Feature              | Supported | Notes                                                   |
| -------------------- | --------- | ------------------------------------------------------- |
| Query open orders    | ✓         | List all active orders.                                 |
| Query single order   | ✓         | By venue order ID or client order ID (any order state). |
| Order status reports | ✓         | Open‑order checks and historical startup mass status.   |
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

| Environment | HTTP API (market data)                           | HTTP API (orders)                                   | Market Data WS                                   | Orders WS                                            |
| ----------- | ------------------------------------------------ | --------------------------------------------------- | ------------------------------------------------ | ---------------------------------------------------- |
| Sandbox     | `https://gateway.sandbox.architect.exchange/api` | `https://gateway.sandbox.architect.exchange/orders` | `wss://gateway.sandbox.architect.exchange/md/ws` | `wss://gateway.sandbox.architect.exchange/orders/ws` |
| Production  | `https://gateway.architect.exchange/api`         | `https://gateway.architect.exchange/orders`         | `wss://gateway.architect.exchange/md/ws`         | `wss://gateway.architect.exchange/orders/ws`         |

:::info
Order management HTTP endpoints (place, cancel, order status) use a separate base URL
from market data endpoints. This is handled automatically by the adapter configuration.
:::

### Data client configuration options

| Option                             | Default   | Description                                                         |
| ---------------------------------- | --------- | ------------------------------------------------------------------- |
| `api_key`                          | `None`    | API key; loaded from `AX_API_KEY` env var when omitted.             |
| `api_secret`                       | `None`    | API secret; loaded from `AX_API_SECRET` env var when omitted.       |
| `environment`                      | `SANDBOX` | Trading environment (`SANDBOX` or `PRODUCTION`).                    |
| `base_url_http`                    | `None`    | Override for the REST base URL.                                     |
| `base_url_ws`                      | `None`    | Override for the market data WebSocket URL.                         |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket transports.               |
| `transport_backend`                | `None`    | Override the compiled WebSocket transport default.                  |
| `http_timeout_secs`                | `60`      | Timeout (seconds) for REST requests.                                |
| `max_retries`                      | `3`       | Maximum retry attempts for REST requests.                           |
| `retry_delay_initial_ms`           | `1000`    | Initial delay (milliseconds) between retries.                       |
| `retry_delay_max_ms`               | `10000`   | Maximum delay (milliseconds) between retries (exponential backoff). |
| `heartbeat_interval_secs`          | `20`      | Heartbeat interval (seconds) for WebSocket connections.             |
| `update_instruments_interval_mins` | `60`      | Interval (minutes) between instrument catalog refreshes.            |
| `funding_rate_poll_interval_mins`  | `15`      | Interval (minutes) between funding rate poll requests.              |

### Execution client configuration options

| Option                    | Default   | Description                                                         |
| ------------------------- | --------- | ------------------------------------------------------------------- |
| `api_key`                 | `None`    | API key; loaded from `AX_API_KEY` env var when omitted.             |
| `api_secret`              | `None`    | API secret; loaded from `AX_API_SECRET` env var when omitted.       |
| `environment`             | `SANDBOX` | Trading environment (`SANDBOX` or `PRODUCTION`).                    |
| `base_url_http`           | `None`    | Override for the market data REST base URL.                         |
| `base_url_orders`         | `None`    | Override for the orders REST base URL.                              |
| `base_url_ws`             | `None`    | Override for the orders WebSocket URL.                              |
| `proxy_url`               | `None`    | Optional proxy URL for HTTP and WebSocket transports.               |
| `transport_backend`       | `None`    | Override the compiled WebSocket transport default.                  |
| `http_timeout_secs`       | `60`      | Timeout (seconds) for REST requests.                                |
| `max_retries`             | `3`       | Maximum retry attempts for REST requests.                           |
| `retry_delay_initial_ms`  | `1000`    | Initial delay (milliseconds) between retries.                       |
| `retry_delay_max_ms`      | `10000`   | Maximum delay (milliseconds) between retries (exponential backoff). |
| `heartbeat_interval_secs` | `30`      | Heartbeat interval (seconds) for WebSocket connections.             |
| `cancel_on_disconnect`    | `false`   | Cancel this WebSocket session's open orders on disconnect.          |

When `transport_backend=None`, the compiled Rust default selects Sockudo when the
`transport-sockudo` Cargo feature is enabled and Tungstenite otherwise.

The most common use case is to configure a live `TradingNode` to include AX Exchange
data and execution clients. To achieve this, add an `AX` section to your client
configuration(s):

```python
from nautilus_trader.adapters.architect_ax import AX
from nautilus_trader.adapters.architect_ax import AxDataClientConfig
from nautilus_trader.adapters.architect_ax import AxEnvironment
from nautilus_trader.adapters.architect_ax import AxExecClientConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        AX: AxDataClientConfig(
            environment=AxEnvironment.SANDBOX,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        AX: AxExecClientConfig(
            environment=AxEnvironment.SANDBOX,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.architect_ax import AX
from nautilus_trader.adapters.architect_ax import AxLiveDataClientFactory
from nautilus_trader.adapters.architect_ax import AxLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory(AX, AxLiveDataClientFactory)
node.add_exec_client_factory(AX, AxLiveExecClientFactory)

# Finally build the node
node.build()
```

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
  to determine the take-through price and submits an aggressive IOC limit order.
- **Stop-limit orders**: The adapter rejects venue-native stop-limit submissions because live
  sandbox testing did not confirm conditional semantics. Use local order emulation when a strategy
  requires a stop-limit order.
- **Order modification**: AX supports atomic order replacement via `POST /replace-order`. The Rust
  client maps `modify_order` to this endpoint and receives a new order ID. The Python client rejects
  modification requests; cancel and resubmit instead.
- **Cancel on disconnect**: Set `cancel_on_disconnect=True` in the execution client config
  to have the exchange cancel all open orders if the orders WebSocket disconnects.
- **Instrument fee rates**: AX reports maker and taker rates per account on `GET /whoami`, so the
  adapter resolves them after authenticating and applies them to every instrument. The execution
  client fails to connect if that lookup fails, rather than reporting zero fees for the process
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

## Contributing

:::info
For additional features or to contribute to the AX Exchange adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
