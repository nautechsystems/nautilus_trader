# OKX

Founded in 2017, OKX is a leading cryptocurrency exchange offering spot, perpetual swap,
futures, and options trading. This integration supports live market data ingest and order
execution on OKX.

## Overview

This adapter is implemented in Rust, with optional Python bindings for ease of use in Python-based workflows.
It does not require external OKX client libraries—the core components are compiled as a static library and linked automatically during the build.

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/okx/).

### Product support

| Product Type      | Data Feed | Trading | Notes                                          |
|-------------------|-----------|---------|------------------------------------------------|
| Spot              | ✓         | ✓       | Use for index prices.                          |
| Perpetual Swaps   | ✓         | ✓       | Linear and inverse contracts.                  |
| Futures           | ✓         | ✓       | Specific expiration dates.                     |
| Margin            | -         | -       | *Not yet supported*.                           |
| Options           | ✓         | -       | *Data feed supported, trading coming soon*.    |

:::info
**Options support**: While you can subscribe to options market data and receive price updates, order execution for options is not yet implemented. You can use the symbology format shown above to subscribe to options data feeds.
:::

The OKX adapter includes multiple components, which can be used separately or together depending on your use case.

- `OKXHttpClient`: Low-level HTTP API connectivity.
- `OKXWebSocketClient`: Low-level WebSocket API connectivity.
- `OKXInstrumentProvider`: Instrument parsing and loading functionality.
- `OKXDataClient`: Market data feed manager.
- `OKXExecutionClient`: Account management and trade execution gateway.
- `OKXLiveDataClientFactory`: Factory for OKX data clients (used by the trading node builder).
- `OKXLiveExecClientFactory`: Factory for OKX execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (as shown below),
and won’t need to work directly with these lower-level components.
:::

## Symbology

OKX uses specific symbol conventions for different instrument types. All instrument IDs should include the `.OKX` suffix when referencing them (e.g., `BTC-USDT.OKX` for spot Bitcoin).

### Symbol format by instrument type

#### SPOT

Format: `{BaseCurrency}-{QuoteCurrency}`

Examples:

- `BTC-USDT` - Bitcoin against USDT (Tether)
- `BTC-USDC` - Bitcoin against USDC
- `ETH-USDT` - Ethereum against USDT
- `SOL-USDT` - Solana against USDT

To subscribe to spot Bitcoin USD in your strategy:

```python
InstrumentId.from_str("BTC-USDT.OKX")  # For USDT-quoted spot
InstrumentId.from_str("BTC-USDC.OKX")  # For USDC-quoted spot
```

#### SWAP (Perpetual Futures)

Format: `{BaseCurrency}-{QuoteCurrency}-SWAP`

Examples:

- `BTC-USDT-SWAP` - Bitcoin perpetual swap (linear, USDT-margined)
- `BTC-USD-SWAP` - Bitcoin perpetual swap (inverse, coin-margined)
- `ETH-USDT-SWAP` - Ethereum perpetual swap (linear)
- `ETH-USD-SWAP` - Ethereum perpetual swap (inverse)

Linear vs Inverse contracts:

- **Linear** (USDT-margined): Uses stablecoins like USDT as margin.
- **Inverse** (coin-margined): Uses the base cryptocurrency as margin.

#### FUTURES (Dated Futures)

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}`

Examples:

- `BTC-USD-251226` - Bitcoin futures expiring December 26, 2025
- `ETH-USD-251226` - Ethereum futures expiring December 26, 2025
- `BTC-USD-250328` - Bitcoin futures expiring March 28, 2025

Note: Futures are typically inverse contracts (coin-margined).

#### OPTIONS

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}-{Strike}-{Type}`

Examples:

- `BTC-USD-250328-100000-C` - Bitcoin call option, $100,000 strike, expiring March 28, 2025
- `BTC-USD-250328-100000-P` - Bitcoin put option, $100,000 strike, expiring March 28, 2025
- `ETH-USD-250328-4000-C` - Ethereum call option, $4,000 strike, expiring March 28, 2025

Where:

- `C` = Call option
- `P` = Put option

### Common questions

**Q: How do I subscribe to spot Bitcoin USD?**
A: Use `BTC-USDT.OKX` for USDT-margined spot or `BTC-USDC.OKX` for USDC-margined spot.

**Q: What's the difference between BTC-USDT-SWAP and BTC-USD-SWAP?**
A: `BTC-USDT-SWAP` is a linear perpetual (USDT-margined), while `BTC-USD-SWAP` is an inverse perpetual (BTC-margined).

**Q: How do I know which contract type to use?**
A: Check the `contract_types` parameter in the configuration:

- For linear contracts: `OKXContractType.LINEAR`.
- For inverse contracts: `OKXContractType.INVERSE`.

## Orders capability

Below are the order types, execution instructions, and time-in-force options supported
for linear perpetual swap products on OKX.

### Client order ID requirements

:::warning
OKX has specific requirements for client order IDs:

- **No hyphens allowed**: OKX does not accept hyphens (`-`) in client order IDs.
- Maximum length: 32 characters.
- Allowed characters: alphanumeric characters and underscores only.

When configuring your strategy, ensure you set:

```python
use_hyphens_in_client_order_ids=False
```

:::

### Order types

| Order Type          | Linear Perpetual Swap | Notes                                |
|---------------------|-----------------------|--------------------------------------|
| `MARKET`            | ✓                     | Immediate execution at market price. Supports quote quantity. |
| `LIMIT`             | ✓                     | Execution at specified price or better. |
| `STOP_MARKET`       | ✓                     | Conditional market order (OKX algo order). |
| `STOP_LIMIT`        | ✓                     | Conditional limit order (OKX algo order). |
| `MARKET_IF_TOUCHED` | ✓                     | Conditional market order (OKX algo order). |
| `LIMIT_IF_TOUCHED`  | ✓                     | Conditional limit order (OKX algo order). |
| `TRAILING_STOP`     | -                     | *Not yet supported*. |

:::info
**Conditional orders**: `STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`, and `LIMIT_IF_TOUCHED` are implemented as OKX algo orders, providing advanced trigger capabilities with multiple price sources.
:::

### Execution instructions

| Instruction    | Linear Perpetual Swap | Notes                  |
|----------------|-----------------------|------------------------|
| `post_only`    | ✓                     | Only for LIMIT orders. |
| `reduce_only`  | ✓                     | Only for derivatives.  |

### Time in force

| Time in force | Linear Perpetual Swap | Notes                                             |
|---------------|-----------------------|---------------------------------------------------|
| `GTC`         | ✓                     | Good Till Canceled.                               |
| `FOK`         | ✓                     | Fill or Kill.                                     |
| `IOC`         | ✓                     | Immediate or Cancel.                              |
| `GTD`         | ✗                     | *Not supported by OKX API.* |

:::info
**GTD (Good Till Date) time in force**: OKX does not support native GTD functionality through their API.

If you need GTD functionality, you must use Nautilus's strategy-managed GTD feature, which will handle the order expiration by canceling the order at the specified expiry time.
:::

### Batch operations

| Operation          | Linear Perpetual Swap | Notes                                     |
|--------------------|-----------------------|-------------------------------------------|
| Batch Submit       | ✓                     | Submit multiple orders in single request. |
| Batch Modify       | ✓                     | Modify multiple orders in single request. |
| Batch Cancel       | ✓                     | Cancel multiple orders in single request. |

### Position management

| Feature           | Linear Perpetual Swap | Notes                                                |
|-------------------|-----------------------|------------------------------------------------------|
| Query positions   | ✓                     | Real-time position updates.                          |
| Position mode     | ✓                     | Net vs Long/Short mode.                              |
| Leverage control  | ✓                     | Dynamic leverage adjustment per instrument.          |
| Margin mode       | ✓                     | Supports cash, isolated, cross, spot_isolated modes. |

### Margin modes

OKX's unified account system allows trading with different margin modes on a per-order basis. The adapter supports specifying the trade mode (`td_mode`) when submitting orders.

:::note
**Important**: Account modes must be initially configured via the OKX Web/App interface. The API cannot set the account mode for the first time.
:::

For more details on OKX's account modes and margin system, see the [OKX Account Mode documentation](https://www.okx.com/docs-v5/en/#overview-account-mode).

#### Available margin modes

- **`cash`**: Spot trading without margin (default for spot trading).
- **`isolated`**: Isolated margin mode where risk is confined to specific positions.
- **`cross`**: Cross margin mode where all positions share the same margin pool.
- **`spot_isolated`**: Isolated margin for spot trading.

#### Setting margin mode per order

You can specify the margin mode for individual orders using the `params` parameter:

```python
# Submit an order with isolated margin mode
strategy.submit_order(
    order=order,
    params={"td_mode": "isolated"}
)

# Submit an order with cross margin mode
strategy.submit_order(
    order=order,
    params={"td_mode": "cross"}
)
```

If no `td_mode` is specified in params, the adapter will use the default mode configured for your account type:

- Cash accounts default to `cash` mode.
- Margin accounts default to `isolated` mode.

This flexibility allows you to:

- Run multiple strategies with different risk profiles simultaneously.
- Isolate high-risk trades while using cross margin for capital-efficient positions.
- Mix spot and margin trades within the same account.

### Order querying

| Feature              | Linear Perpetual Swap | Notes                                     |
|----------------------|-----------------------|-------------------------------------------|
| Query open orders    | ✓                     | List all active orders.                   |
| Query order history  | ✓                     | Historical order data.                    |
| Order status updates | ✓                     | Real-time order state changes.            |
| Trade history        | ✓                     | Execution and fill reports.               |

### Contingent orders

| Feature             | Linear Perpetual Swap | Notes                                     |
|---------------------|-----------------------|---------------------------------------------|
| Order lists         | -                     | *Not supported*.                           |
| OCO orders          | ✓                     | One-Cancels-Other orders.                  |
| Bracket orders      | ✓                     | Stop loss + take profit combinations.      |
| Conditional orders  | ✓                     | Stop and limit-if-touched orders.          |

#### Conditional order architecture

Conditional orders (OKX algo orders) use a hybrid architecture for optimal performance and reliability:

- **Submission**: Via HTTP REST API (`/api/v5/trade/order-algo`)
- **Status updates**: Via WebSocket business endpoint (`/ws/v5/business`) on the `orders-algo` channel
- **Cancellation**: Via HTTP REST API using algo order ID tracking

This design ensures:

- Immediate submission acknowledgment through HTTP.
- Real-time status updates through WebSocket.
- Proper order lifecycle management with algo order ID mapping.

#### Supported conditional order types

| Order Type          | Trigger Types          | Notes                                     |
|---------------------|------------------------|-------------------------------------------|
| `STOP_MARKET`       | Last, Mark, Index      | Market execution when triggered.          |
| `STOP_LIMIT`        | Last, Mark, Index      | Limit order placement when triggered.     |
| `MARKET_IF_TOUCHED` | Last, Mark, Index      | Market execution when price touched.      |
| `LIMIT_IF_TOUCHED`  | Last, Mark, Index      | Limit order placement when price touched. |

#### Trigger price types

Conditional orders support different trigger price sources:

- **Last Price** (`TriggerType.LAST_PRICE`): Uses the last traded price (default).
- **Mark Price** (`TriggerType.MARK_PRICE`): Uses the mark price (recommended for derivatives).
- **Index Price** (`TriggerType.INDEX_PRICE`): Uses the underlying index price.

```python
# Example: Stop loss using mark price trigger
stop_order = order_factory.stop_market(
    instrument_id=instrument_id,
    order_side=OrderSide.SELL,
    quantity=Quantity.from_str("0.1"),
    trigger_price=Price.from_str("45000.0"),
    trigger_type=TriggerType.MARK_PRICE,  # Use mark price for trigger
)
strategy.submit_order(stop_order)
```

## Authentication

To use the OKX adapter, you'll need to create API credentials in your OKX account:

1. Log into your OKX account and navigate to the API management page.
2. Create a new API key with the required permissions for trading and data access.
3. Note down your API key, secret key, and passphrase.

You can provide these credentials through environment variables:

```bash
export OKX_API_KEY="your_api_key"
export OKX_API_SECRET="your_api_secret"
export OKX_API_PASSPHRASE="your_passphrase"
```

Or pass them directly in the configuration (not recommended for production).

## Rate limiting

The adapter enforces OKX’s per-endpoint quotas while keeping sensible defaults for both REST and WebSocket calls.

### REST limits

- Global cap: 250 requests per second (matches 500 requests / 2 seconds IP allowance).
- Endpoint-specific quotas appear in the table below and mirror OKX’s published limits where available.

### WebSocket limits

- Subscription operations: 3 requests per second.
- Order actions (place/cancel/amend): 250 requests per second.

:::warning
OKX enforces per-endpoint and per-account quotas; exceeding them leads to HTTP 429 responses and temporary throttling on that key.
:::

| Key / Endpoint                   | Limit (req/sec) | Notes                                                   |
|----------------------------------|-----------------|---------------------------------------------------------|
| `okx:global`                     | 250             | Matches 500 req / 2 s IP allowance.                     |
| `/api/v5/public/instruments`     | 10              | Matches OKX 20 req / 2 s docs.                          |
| `/api/v5/market/candles`         | 50              | Higher allowance for streaming candles.                 |
| `/api/v5/market/history-candles` | 20              | Conservative quota for large historical pulls.          |
| `/api/v5/market/history-trades`  | 30              | Trade history pulls.                                    |
| `/api/v5/account/balance`        | 5               | OKX guidance: 10 req / 2 s.                             |
| `/api/v5/trade/order`            | 30              | 60 requests / 2 seconds per-instrument limit.           |
| `/api/v5/trade/orders-pending`   | 20              | Open order fetch.                                       |
| `/api/v5/trade/orders-history`   | 20              | Historical orders.                                      |
| `/api/v5/trade/fills`            | 30              | Execution reports.                                      |
| `/api/v5/trade/order-algo`       | 10              | Algo placements (conditional orders).                   |
| `/api/v5/trade/cancel-algos`     | 10              | Algo cancellation.                                      |

All keys automatically include the `okx:global` bucket. URLs are normalised (query strings removed) before rate limiting, so requests with different filters share the same quota.

:::info
For more details on rate limiting, see the official documentation: <https://www.okx.com/docs-v5/en/#rest-api-rate-limit>.
:::

## Configuration

### Configuration options

The OKX data client provides the following configuration options:

#### Data client

| Option                               | Default                         | Description |
|--------------------------------------|---------------------------------|-------------|
| `instrument_types`                   | `(OKXInstrumentType.SPOT,)`     | Controls which OKX instrument families are loaded (spot, swap, futures, options). |
| `contract_types`                     | `None`                          | Restricts loading to specific contract styles when combined with `instrument_types`. |
| `base_url_http`                      | `None`                          | Override for the OKX REST endpoint; defaults to the production URL resolved at runtime. |
| `base_url_ws`                        | `None`                          | Override for the market data WebSocket endpoint. |
| `api_key` / `api_secret` / `api_passphrase` | `None`                  | When omitted, pulled from the `OKX_API_KEY`, `OKX_API_SECRET`, and `OKX_PASSPHRASE` environment variables. |
| `is_demo`                            | `False`                         | Connects to the OKX demo environment when `True`. |
| `http_timeout_secs`                  | `60`                            | Request timeout (seconds) for REST market data calls. |
| `update_instruments_interval_mins`   | `60`                            | Interval, in minutes, between background instrument refreshes. |
| `vip_level`                          | `None`                          | Enables higher-depth order book channels when set to the matching OKX VIP tier. |

The OKX execution client provides the following configuration options:

#### Execution client

| Option                     | Default     | Description |
|----------------------------|-------------|-------------|
| `instrument_types`         | `(OKXInstrumentType.SPOT,)` | Instrument families that should be tradable for this client. |
| `contract_types`           | `None`      | Restricts tradable contracts (linear, inverse, options) when paired with `instrument_types`. |
| `base_url_http`            | `None`      | Override for the OKX trading REST endpoint. |
| `base_url_ws`              | `None`      | Override for the private WebSocket endpoint. |
| `api_key` / `api_secret` / `api_passphrase` | `None` | Fall back to `OKX_API_KEY`, `OKX_API_SECRET`, and `OKX_PASSPHRASE` environment variables when unset. |
| `margin_mode`              | `None`      | Forces the OKX account margin mode (cross or isolated) when specified. |
| `is_demo`                  | `False`     | Connects to the OKX demo trading environment. |
| `http_timeout_secs`        | `60`        | Request timeout (seconds) for REST trading calls. |
| `use_fills_channel`        | `False`     | Subscribes to the dedicated fills channel (VIP5+ required) for lower-latency fill reports. |
| `use_mm_mass_cancel`       | `False`     | Uses the market-maker bulk cancel endpoint when available; otherwise falls back to per-order cancels. |
| `max_retries`              | `3`         | Maximum retry attempts for recoverable REST errors. |
| `retry_delay_initial_ms`   | `1,000`     | Initial delay (milliseconds) applied before retrying a failed request. |
| `retry_delay_max_ms`       | `10,000`    | Upper bound for the exponential backoff delay between retries. |

Below is an example configuration for a live trading node using OKX data and execution clients:

```python
from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig, OKXExecClientConfig
from nautilus_trader.adapters.okx.factories import OKXLiveDataClientFactory, OKXLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig, LiveExecEngineConfig, LoggingConfig, TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,
    data_clients={
        OKX: OKXDataClientConfig(
            api_key=None,           # Will use OKX_API_KEY env var
            api_secret=None,        # Will use OKX_API_SECRET env var
            api_passphrase=None,    # Will use OKX_API_PASSPHRASE env var
            base_url_http=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.SWAP,),
            contract_types=(OKXContractType.LINEAR,),
            is_demo=False,
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            api_key=None,
            api_secret=None,
            api_passphrase=None,
            base_url_http=None,
            base_url_ws=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.SWAP,),
            contract_types=(OKXContractType.LINEAR,),
            is_demo=False,
        ),
    },
)
node = TradingNode(config=config)
node.add_data_client_factory(OKX, OKXLiveDataClientFactory)
node.add_exec_client_factory(OKX, OKXLiveExecClientFactory)
node.build()
```

:::info
For additional features or to contribute to the OKX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
