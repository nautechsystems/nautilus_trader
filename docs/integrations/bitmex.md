# BitMEX

Founded in 2014, BitMEX (Bitcoin Mercantile Exchange) is a cryptocurrency derivatives
trading platform offering spot, perpetual contracts, traditional futures, prediction
markets, and other advanced trading products. This integration supports live market data
ingest and order execution with BitMEX.

## Overview

This adapter is implemented in Rust, with optional Python bindings for ease of use in Python-based workflows.
It does not require external BitMEX client libraries—the core components are compiled as a static library and linked automatically during the build.

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/bitmex/).

## Components

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The BitMEX adapter includes multiple components, which can be used together or separately depending
on the use case.

- `BitmexHttpClient`: Low-level HTTP API connectivity.
- `BitmexWebSocketClient`: Low-level WebSocket API connectivity.
- `BitmexInstrumentProvider`: Instrument parsing and loading functionality.
- `BitmexDataClient`: A market data feed manager.
- `BitmexExecutionClient`: An account management and trade execution gateway.
- `BitmexLiveDataClientFactory`: Factory for BitMEX data clients (used by the trading node builder).
- `BitmexLiveExecClientFactory`: Factory for BitMEX execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## BitMEX documentation

BitMEX provides extensive documentation for users:

- [BitMEX API Explorer](https://www.bitmex.com/app/restAPI) - Interactive API documentation.
- [BitMEX API Documentation](https://www.bitmex.com/app/apiOverview) - Complete API reference.
- [BitMEX Exchange Rules](https://www.bitmex.com/exchange-rules) - Official exchange rules and regulations.
- [Contract Guides](https://www.bitmex.com/app/contract) - Detailed contract specifications.
- [Spot Trading Guide](https://www.bitmex.com/app/spotGuide) - Spot trading overview.
- [Perpetual Contracts Guide](https://www.bitmex.com/app/perpetualContractsGuide) - Perpetual swaps explained.
- [Futures Contracts Guide](https://www.bitmex.com/app/futuresGuide) - Traditional futures information.

It's recommended you refer to the BitMEX documentation in conjunction with this
NautilusTrader integration guide.

## Product support

| Product Type      | Data Feed | Trading | Notes                                               |
|-------------------|-----------|---------|-----------------------------------------------------|
| Spot              | ✓         | ✓       | Limited pairs, unified wallet with derivatives.     |
| Perpetual Swaps   | ✓         | ✓       | Inverse and linear contracts available.             |
| Futures           | ✓         | ✓       | Traditional fixed expiration contracts.             |
| Quanto Futures    | ✓         | ✓       | Settled in different currency than underlying.      |
| Prediction Markets| ✓         | ✓       | Event-based contracts, 0-100 pricing, USDT settled. |
| Options           | -         | -       | *Not provided by BitMEX*.                           |

:::note
BitMEX has discontinued their options products to focus on their core derivatives and spot offerings.
:::

### Spot trading

- Direct token/coin trading with immediate settlement.
- Major pairs including XBT/USDT, ETH/USDT, ETH/XBT.
- Additional altcoin pairs (LINK, SOL, UNI, APE, AXS, BMEX against USDT).

### Derivatives

- **Perpetual contracts**: Inverse (e.g., XBTUSD) and linear (e.g., ETHUSDT).
- **Traditional futures**: Fixed expiration date contracts.
- **Quanto futures**: Contracts settled in a different currency than the underlying.
- **Prediction markets**: Event-based derivatives (e.g., P_FTXZ26, P_SBFJAILZ26) allowing traders to speculate on outcomes across crypto, finance, and other events. No leverage, priced 0-100, settled in USDT.

## Symbology

BitMEX uses a specific naming convention for its trading symbols. Understanding this
convention is crucial for correctly identifying and trading instruments.

### Symbol format

BitMEX symbols typically follow these patterns:

- **Spot pairs**: Base currency + Quote currency (e.g., `XBT/USDT`, `ETH/USDT`).
- **Perpetual contracts**: Base currency + Quote currency (e.g., `XBTUSD`, `ETHUSD`).
- **Futures contracts**: Base currency + Expiry code (e.g., `XBTM24`, `ETHH25`).
- **Quanto contracts**: Special naming for non-USD settled contracts.
- **Prediction markets**: `P_` prefix + Event identifier + Expiry code (e.g., `P_POWELLK26`, `P_FTXZ26`).

:::info
BitMEX uses `XBT` as the symbol for Bitcoin instead of `BTC`. This follows the ISO 4217
currency code standard where "X" denotes non-national currencies. XBT and BTC refer to
the same asset - Bitcoin.
:::

### Expiry codes

Futures contracts use standard futures month codes:

- `F` = January
- `G` = February
- `H` = March
- `J` = April
- `K` = May
- `M` = June
- `N` = July
- `Q` = August
- `U` = September
- `V` = October
- `X` = November
- `Z` = December

Followed by the year (e.g., `24` for 2024, `25` for 2025).

### NautilusTrader instrument IDs

Within NautilusTrader, BitMEX instruments are identified using the native BitMEX symbol
directly, combined with the venue identifier:

```python
from nautilus_trader.model.identifiers import InstrumentId

# Spot pairs (note: no slash in the symbol)
spot_id = InstrumentId.from_str("XBTUSDT.BITMEX")  # XBT/USDT spot
eth_spot_id = InstrumentId.from_str("ETHUSDT.BITMEX")  # ETH/USDT spot

# Perpetual contracts
perp_id = InstrumentId.from_str("XBTUSD.BITMEX")  # Bitcoin perpetual (inverse)
linear_perp_id = InstrumentId.from_str("ETHUSDT.BITMEX")  # Ethereum perpetual (linear)

# Futures contract (June 2024)
futures_id = InstrumentId.from_str("XBTM24.BITMEX")  # Bitcoin futures expiring June 2024

# Prediction market contracts
prediction_id = InstrumentId.from_str("P_XBTETFV23.BITMEX")  # Bitcoin ETF SEC approval prediction expiring October 2023
```

:::note
BitMEX spot symbols in NautilusTrader don't include the slash (/) that appears in the
BitMEX UI. Use `XBTUSDT` instead of `XBT/USDT`.
:::

### Quantity scaling

BitMEX reports spot and derivative quantities in *contract* units. The actual asset size per
contract is exchange-specific and published on the instrument definition:

- `lotSize` – minimum number of contracts you can trade.
- `underlyingToPositionMultiplier` – number of contracts per unit of the underlying asset.

For example, the SOL/USDT spot instrument (`SOLUSDT`) exposes `lotSize = 1000` and
`underlyingToPositionMultiplier = 10000`, meaning one contract represents `1 / 10000 = 0.0001`
SOL, and the minimum order (`lotSize * contract_size`) is `0.1` SOL. The adapter now derives the
contract size directly from these fields and scales both inbound market data and outbound orders
accordingly, so quantities in Nautilus are always expressed in base units (SOL, ETH, etc.).

See the BitMEX API documentation for details on these fields: <https://www.bitmex.com/app/apiOverview#Instrument-Properties>.

## Orders capability

The BitMEX integration supports the following order types and execution features.

### Order types

| Order Type             | Supported | Notes                                         |
|------------------------|-----------|-----------------------------------------------|
| `MARKET`               | ✓         | Executed immediately at current market price. Quote quantity not supported. |
| `LIMIT`                | ✓         | Executed only at specified price or better.   |
| `STOP_MARKET`          | ✓         | Supported (set `trigger_price`).              |
| `STOP_LIMIT`           | ✓         | Supported (set `price` and `trigger_price`).  |
| `MARKET_IF_TOUCHED`    | ✓         | Supported (set `trigger_price`).              |
| `LIMIT_IF_TOUCHED`     | ✓         | Supported (set `price` and `trigger_price`).  |
| `TRAILING_STOP_MARKET` | -         | *Not implemented* (supported by BitMEX).      |

### Execution instructions

| Instruction   | Supported | Notes                                                                             |
|---------------|-----------|-----------------------------------------------------------------------------------|
| `post_only`   | ✓         | Supported via `ParticipateDoNotInitiate` execution instruction on `LIMIT` orders. |
| `reduce_only` | ✓         | Supported via `ReduceOnly` execution instruction.                                 |

:::note
Post-only orders that would cross the spread are canceled by BitMEX rather than rejected. The
integration surfaces these as rejections with `due_post_only=True` so strategies can handle them
consistently.
:::

### Trigger types

BitMEX supports multiple reference prices to evaluate stop/conditional order triggers for:

- `STOP_MARKET`
- `STOP_LIMIT`
- `MARKET_IF_TOUCHED`
- `LIMIT_IF_TOUCHED`

Choose the trigger type that matches your strategy and/or risk preferences.

| Reference price | Nautilus `TriggerType` | BitMEX value  | Notes                                                                           |
|-----------------|------------------------|---------------|---------------------------------------------------------------------------------|
| Last trade      | `LAST_PRICE`           | `LastPrice`   | BitMEX default; triggers on the last traded price.                              |
| Mark price      | `MARK_PRICE`           | `MarkPrice`   | Recommended for many stop-loss use cases to reduce stop-outs from price spikes. |
| Index price     | `INDEX_PRICE`          | `IndexPrice`  | Tracks the external index; useful for some contracts.                           |

- If no `trigger_type` is provided, BitMEX uses its venue default (`LastPrice`).
- These trigger references are exchange-evaluated; the order remains resting at the venue until triggered.

**Example**:

```python
from nautilus_trader.model.enums import TriggerType

order = self.order_factory.stop_market(
    instrument_id=instrument_id,
    order_side=order_side,
    quantity=qty,
    trigger_price=trigger,
    trigger_type=TriggerType.MARK_PRICE,  # Use BitMEX Mark Price as reference
)
```

`ExecTester` example configuration also demonstrates setting `stop_trigger_type=TriggerType.MARK_PRICE`
in `examples/live/bitmex/bitmex_exec_tester.py`.

### Time in force

| Time in force  | Supported | Notes                                               |
|----------------|-----------|-----------------------------------------------------|
| `GTC`          | ✓         | Good Till Canceled (default).                       |
| `GTD`          | -         | *Not supported by BitMEX*.                          |
| `FOK`          | ✓         | Fill or Kill - fills entire order or cancels.       |
| `IOC`          | ✓         | Immediate or Cancel - partial fill allowed.         |
| `DAY`          | ✓         | Expires at 00:00 UTC (BitMEX trading day boundary). |

:::note
`DAY` orders expire at 12:00am UTC, which marks the BitMEX trading day boundary (end of trading hours for that day).
See the [BitMEX Exchange Rules](https://www.bitmex.com/exchange-rules) and [API documentation](https://www.bitmex.com/api/explorer/) for complete details.
:::

### Advanced order features

| Feature            | Supported | Notes                                          |
|--------------------|-----------|------------------------------------------------|
| Order Modification | ✓         | Modify price, quantity, and trigger price.     |
| Bracket Orders     | -         | Use `contingency_type` and `linked_order_ids`. |
| Iceberg Orders     | ✓         | Use `display_qty`.                             |
| Trailing Stops     | -         | *Not implemented* (supported by BitMEX).       |
| Pegged Orders      | -         | *Not implemented* (supported by BitMEX).       |

### Batch operations

| Operation          | Supported | Notes                                       |
|--------------------|-----------|---------------------------------------------|
| Batch Submit       | -         | *Not supported by BitMEX*.                  |
| Batch Modify       | -         | *Not supported by BitMEX*.                  |
| Batch Cancel       | ✓         | Cancel multiple orders in a single request. |

### Position management

| Feature             | Supported | Notes                                              |
|---------------------|-----------|----------------------------------------------------|
| Query positions     | ✓         | REST and real-time position updates via WebSocket. |
| Cross margin        | ✓         | Default margin mode.                               |
| Isolated margin     | ✓         |                                                    |

### Order querying

| Feature              | Supported | Notes                                        |
|----------------------|-----------|----------------------------------------------|
| Query open orders    | ✓         | List all active orders.                      |
| Query order history  | ✓         | Historical order data.                       |
| Order status updates | ✓         | Real-time order state changes via WebSocket. |
| Trade history        | ✓         | Execution and fill reports.                  |

## Market data

- Order book deltas: `L2_MBP` only; `depth` 0 (full book) or 25.
- Order book snapshots: `L2_MBP` only; `depth` 0 (default 10) or 10.
- Quotes, trades, and instrument updates are supported via WebSocket.
- Funding rates, mark prices, and index prices are supported where applicable.
- Historical requests via REST:
  - Trade ticks with optional `start`, `end`, and `limit` filters (up to 1,000 results per call).
  - Time bars (`1m`, `5m`, `1h`, `1d`) for externally aggregated LAST prices, including optional partial bins.

:::note
BitMEX caps each REST response at 1,000 rows and requires manual pagination via `start`/`startTime`. The current adapter returns only the
first page; wider pagination support is scheduled for a future update.
:::

## Connection management

### HTTP Keep-Alive

The BitMEX adapter utilizes HTTP keep-alive for optimal performance:

- **Connection pooling**: Connections are automatically pooled and reused.
- **Keep-alive timeout**: 90 seconds (matches BitMEX server-side timeout).
- **Automatic reconnection**: Failed connections are automatically re-established.
- **SSL session caching**: Reduces handshake overhead for subsequent requests.

This configuration ensures low-latency communication with BitMEX servers by maintaining
persistent connections and avoiding the overhead of establishing new connections for each request.

### Request authentication and expiration

BitMEX uses an `api-expires` header for request authentication to prevent replay attacks:

- Signed requests include an `api-expires` Unix timestamp set `recv_window_ms / 1000` seconds ahead (10 seconds by default).
- BitMEX rejects any request once that timestamp has passed, so keep latency within your configured window.

## Rate limiting

BitMEX implements a dual-layer rate limiting system:

### REST limits

- **Burst limit**: 10 requests per second for authenticated users (applies to order placement, modification, and cancel endpoints).
- **Rolling minute limit**: 120 requests per minute for authenticated users (30 requests per minute for unauthenticated users).
- **Order caps**: 200 open orders and 10 stop orders per symbol; exceeding these caps triggers exchange-side rejections.

The adapter enforces these quotas automatically and surfaces the rate-limit headers BitMEX returns with each response.

### WebSocket limits

- Connection requests: follow the exchange guidance (currently 3 connections per second per IP).
- Private streams require authentication; the adapter reconnects automatically if a limit is exceeded.

:::warning
Exceeding BitMEX rate limits returns HTTP 429 and may trigger temporary IP bans; persistent 4xx/5xx errors can extend the lockout period.
:::

### Configurable rate limits

The rate limits can be configured if your account has different limits than the defaults:

| Parameter                  | Default (authenticated) | Default (unauthenticated) | Description                                         |
|----------------------------|-------------------------|---------------------------|-----------------------------------------------------|
| `max_requests_per_second`  | 10                      | 10                        | Maximum requests per second (burst limit).          |
| `max_requests_per_minute`  | 120                     | 30                        | Maximum requests per minute (rolling window).       |

:::info
For more details on rate limiting, see the [BitMEX API documentation on rate limits](https://www.bitmex.com/app/restAPI#Limits).
:::

:::warning
**Cancel Broadcaster Rate Limit Considerations**

The cancel broadcaster (when `canceller_pool_size > 1`) fans out each cancel request to multiple independent HTTP clients in parallel. Each client maintains its own rate limiter, which means the effective request rate is multiplied by the pool size.

**Example**: With `canceller_pool_size=3` (default) and `max_requests_per_second=10`, a single cancel operation consumes **3 requests** (one per client), potentially reaching **30 requests/second** if canceling rapidly.

Since BitMEX enforces rate limits **at the account level** (not per connection), the broadcaster can push you over the exchange's default limits of 10 req/s burst and 120 req/min rolling window.

**Mitigations**: Reduce `max_requests_per_second` and `max_requests_per_minute` proportionally (divide by `canceller_pool_size`), or adjust the pool size itself (see [Cancel broadcaster configuration](#cancel-broadcaster)).
Future versions may support shared rate limiters across the pool.
:::

### Rate-limit headers

BitMEX exposes the current allowance via response headers:

- `x-ratelimit-limit`: total requests permitted in the current window.
- `x-ratelimit-remaining`: remaining requests before throttling occurs.
- `x-ratelimit-reset`: UNIX timestamp when the allowance resets.
- `retry-after`: seconds to wait after a 429 response.

## Cancel broadcaster

The BitMEX execution client includes a cancel broadcaster that provides fault-tolerant order cancellation through parallel request fanout.

### Concepts

Order cancellations are time-critical operations - when a strategy decides to cancel an order, any delay or failure can result in unintended fills, slippage, or unwanted position exposure. The cancel broadcaster addresses this by:

- **Parallel fanout**: Cancel requests are simultaneously broadcast to multiple independent HTTP client instances.
- **First-success short-circuiting**: The first successful response wins, and remaining in-flight requests are immediately aborted.
- **Fault tolerance**: If one HTTP client experiences network issues, DNS failures, or connection timeouts, other clients in the pool continue processing.
- **Idempotent success handling**: Responses indicating the order was already canceled (such as "orderID not found" or similar idempotent states) are treated as success rather than failure, preventing unnecessary error propagation.

This architecture ensures that a single network path failure or slow connection doesn't block critical cancel operations, improving the reliability of risk management and position control in live trading.

### Health monitoring

Each HTTP client in the broadcaster pool maintains health metrics:

- Successful cancellations mark a client as healthy.
- Failed requests increment error counters.
- Background health checks periodically verify client connectivity.
- Degraded clients are tracked but remain in the pool to maintain fault tolerance.

The broadcaster exposes metrics including total cancels, successful cancels, failed cancels, expected rejects (already canceled orders), and idempotent successes for operational monitoring and debugging.

#### Tracked metrics

| Metric                   | Type   | Description                                                                                                           |
|--------------------------|--------|-----------------------------------------------------------------------------------------------------------------------|
| `total_cancels`          | `u64`  | Total number of cancel operations initiated (includes single, batch, and cancel-all requests).                        |
| `successful_cancels`     | `u64`  | Number of cancel operations that successfully received acknowledgement from BitMEX.                                   |
| `failed_cancels`         | `u64`  | Number of cancel operations where all HTTP clients in the pool failed (no healthy clients or all requests failed).    |
| `expected_rejects`       | `u64`  | Number of expected rejection patterns detected (e.g., post-only order rejections).                                    |
| `idempotent_successes`   | `u64`  | Number of idempotent success responses (order already cancelled, order not found, unable to cancel due to state).     |
| `healthy_clients`        | `usize`| Current number of healthy HTTP clients in the pool (clients that passed recent health checks).                        |
| `total_clients`          | `usize`| Total number of HTTP clients configured in the pool (`canceller_pool_size`).                                          |

These metrics can be accessed programmatically via the `get_metrics()` and `get_metrics_async()` methods on the `CancelBroadcaster` instance.

### Configuration

The cancel broadcaster is configured via the execution client configuration:

| Option                | Default | Description                                                                               |
|-----------------------|---------|-------------------------------------------------------------------------------------------|
| `canceller_pool_size` | `3`     | Size of the HTTP client pool for the broadcaster. Higher values increase fault tolerance but consume more resources. |

**Example configuration**:

```python
from nautilus_trader.adapters.bitmex.config import BitmexExecClientConfig

exec_config = BitmexExecClientConfig(
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    canceller_pool_size=3,  # Default pool size
)
```

:::tip
For HFT strategies without higher rate limits, consider reducing `canceller_pool_size=1` to minimize rate limit consumption.
The default pool size of 3 broadcasts each cancel request to 3 parallel HTTP clients for fault tolerance, which consumes 3× the rate limit quota per cancel operation.
Single-client mode still benefits from the broadcaster's idempotent success handling but uses standard rate limits.
:::

The broadcaster is automatically started when the execution client connects and stopped when it disconnects. All cancel operations (`cancel_order`, `cancel_all_orders`, `batch_cancel_orders`) are automatically routed through the broadcaster without requiring any changes to strategy code.

## Configuration

### API credentials

BitMEX API credentials can be provided either directly in the configuration or via environment variables:

- `BITMEX_API_KEY`: Your BitMEX API key for production.
- `BITMEX_API_SECRET`: Your BitMEX API secret for production.
- `BITMEX_TESTNET_API_KEY`: Your BitMEX API key for testnet (when `testnet=True`).
- `BITMEX_TESTNET_API_SECRET`: Your BitMEX API secret for testnet (when `testnet=True`).

To generate API keys:

1. Log in to your BitMEX account.
2. Navigate to Account & Security → API Keys.
3. Create a new API key with appropriate permissions.
4. For testnet, use [testnet.bitmex.com](https://testnet.bitmex.com).

:::note
**Testnet API endpoints**:

- REST API: `https://testnet.bitmex.com/api/v1`
- WebSocket: `wss://ws.testnet.bitmex.com/realtime`

The adapter automatically routes requests to the correct endpoints when `testnet=True` is configured.
:::

### Data client configuration options

The BitMEX data client provides the following configuration options:

| Option                            | Default  | Description |
|-----------------------------------|----------|-------------|
| `api_key`                         | `None`   | Optional API key; if `None`, loaded from `BITMEX_API_KEY`. |
| `api_secret`                      | `None`   | Optional API secret; if `None`, loaded from `BITMEX_API_SECRET`. |
| `base_url_http`                   | `None`   | Override for the REST base URL (defaults to production). |
| `base_url_ws`                     | `None`   | Override for the WebSocket base URL (defaults to production). |
| `testnet`                         | `False`  | Route requests to the BitMEX testnet when `True`. |
| `http_timeout_secs`               | `60`     | Request timeout applied to HTTP calls. |
| `max_retries`                     | `None`   | Maximum retry attempts for HTTP calls (disabled when `None`). |
| `retry_delay_initial_ms`          | `1,000`  | Initial backoff delay (milliseconds) between retries. |
| `retry_delay_max_ms`              | `5,000`  | Maximum backoff delay (milliseconds) between retries. |
| `recv_window_ms`                  | `10,000` | Expiration window (milliseconds) for signed requests. See [Request authentication](#request-authentication-and-expiration). |
| `update_instruments_interval_mins`| `60`     | Interval (minutes) between instrument catalogue refreshes. |
| `max_requests_per_second`         | `10`     | Burst rate limit enforced by the adapter for REST calls. |
| `max_requests_per_minute`         | `120`    | Rolling minute rate limit enforced by the adapter for REST calls. |

### Execution client configuration options

The BitMEX execution client provides the following configuration options:

| Option                   | Default  | Description |
|--------------------------|----------|-------------|
| `api_key`                | `None`   | Optional API key; if `None`, loaded from `BITMEX_API_KEY`. |
| `api_secret`             | `None`   | Optional API secret; if `None`, loaded from `BITMEX_API_SECRET`. |
| `base_url_http`          | `None`   | Override for the REST base URL (defaults to production). |
| `base_url_ws`            | `None`   | Override for the WebSocket base URL (defaults to production). |
| `testnet`                | `False`  | Route orders to the BitMEX testnet when `True`. |
| `http_timeout_secs`      | `60`     | Request timeout applied to HTTP calls. |
| `max_retries`            | `None`   | Maximum retry attempts for HTTP calls (disabled when `None`). |
| `retry_delay_initial_ms` | `1,000`  | Initial backoff delay (milliseconds) between retries. |
| `retry_delay_max_ms`     | `5,000`  | Maximum backoff delay (milliseconds) between retries. |
| `recv_window_ms`         | `10,000` | Expiration window (milliseconds) for signed requests. See [Request authentication](#request-authentication-and-expiration). |
| `max_requests_per_second`| `10`     | Burst rate limit enforced by the adapter for REST calls. |
| `max_requests_per_minute`| `120`    | Rolling minute rate limit enforced by the adapter for REST calls. |
| `canceller_pool_size`    | `3`      | Number of redundant HTTP clients in the cancel broadcaster pool. See [Cancel broadcaster](#cancel-broadcaster). |

### Configuration examples

A typical BitMEX configuration for live trading includes both testnet and mainnet options:

```python
from nautilus_trader.adapters.bitmex.config import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex.config import BitmexExecClientConfig

# Using environment variables (recommended)
testnet_data_config = BitmexDataClientConfig(
    testnet=True,  # API credentials loaded from BITMEX_API_KEY and BITMEX_API_SECRET
)

# Using explicit credentials
mainnet_data_config = BitmexDataClientConfig(
    api_key="YOUR_API_KEY",  # Or use os.getenv("BITMEX_API_KEY")
    api_secret="YOUR_API_SECRET",  # Or use os.getenv("BITMEX_API_SECRET")
    testnet=False,
)

mainnet_exec_config = BitmexExecClientConfig(
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    testnet=False,
)
```

## Trading considerations

### Contingent orders

The BitMEX execution adapter now maps Nautilus contingent order lists to the exchange's
native `clOrdLinkID`/`contingencyType` mechanics. When the engine submits
`ContingencyType::Oco` or `ContingencyType::Oto` orders the adapter will:

- Create/maintain the linked order group on BitMEX so child stops and targets inherit the
  parent order status.
- Propagate order list updates and cancellations so that contingent peers stay aligned with
  the current position state.
- Surface execution reports with the appropriate contingency metadata, enabling strategy-level
  tracking without additional manual wiring.

This means common bracket flows (entry + stop + take-profit) and multi-leg stop structures can
now be managed directly by BitMEX instead of being emulated client-side. When defining
strategies, continue to use Nautilus `OrderList`/`ContingencyType` abstractions—the adapter
handles the required BitMEX wiring automatically.

### Contract specifications

- **Inverse contracts**: Settled in cryptocurrency (e.g., XBTUSD settled in XBT).
- **Linear contracts**: Settled in stablecoin (e.g., ETHUSDT settled in USDT).
- **Contract size**: Varies by instrument, check specifications carefully.
- **Tick size**: Minimum price increment varies by contract.

### Margin requirements

- Initial margin requirements vary by contract and market conditions.
- Maintenance margin is typically lower than initial margin.
- Liquidation occurs when maintenance margin requirement is not satisfied.
- BitMEX supports both isolated margin and cross margin modes.
- Risk limits can be adjusted based on position size per the [Exchange Rules](https://www.bitmex.com/exchange-rules).

### Fees

- **Maker fees**: Typically negative (rebate) for providing liquidity.
- **Taker fees**: Positive fee for taking liquidity.
- **Funding rates**: Apply to perpetual contracts every 8 hours.
- **Prediction market fees**: Maker 0.00%, Taker 0.25% (no leverage allowed).

:::info
For additional features or to contribute to the BitMEX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
