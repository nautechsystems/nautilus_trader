# AX Exchange

[AX Exchange](https://architect.exchange) is the world's first centralized and regulated exchange
for perpetual futures on traditional underlying asset classes. Operated by Architect Bermuda Ltd.
and licensed by the [Bermuda Monetary Authority (BMA)](https://www.bma.bm/), AX brings crypto-style
perpetual contracts to traditional financial markets including foreign exchange, metals, energy,
equity indices, and interest rates.

This integration supports live market data ingest and order execution with AX Exchange.

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax/).

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
[Architect documentation site](https://docs.architect.co/).
It's recommended you also refer to the AX Exchange documentation in conjunction with this
NautilusTrader integration guide.

## Products

AX Exchange specializes in perpetual futures contracts on traditional asset classes. Perpetual
contracts never expire, eliminating rollover costs associated with standard futures.

| Asset Class      | Examples                            | Notes                            |
|------------------|-------------------------------------|----------------------------------|
| Foreign exchange | GBPUSD-PERP, EURUSD-PERP.           | Major and minor FX pairs.        |
| Stock indices    | Equity index perpetuals.            |                                  |
| Metals           | XAU-PERP (gold), XAG-PERP (silver). | Precious metals perpetuals.      |
| Energy           | Crude oil, natural gas.             | Energy commodity perpetuals.     |
| Interest rates   | SOFR, treasury yields.              | Rate perpetuals.                 |

:::info
All instruments on AX Exchange are perpetual futures using a netting account model with
cross-margin. The adapter uses `MARGIN` account type and `NETTING` order management.
:::

## Symbology

AX Exchange uses a straightforward naming convention. All instruments are perpetual futures
identified by the `-PERP` suffix appended to the underlying asset symbol.

**Format**: `{SYMBOL}-PERP`

| Underlying     | AX Symbol      | Nautilus InstrumentId |
|----------------|----------------|-----------------------|
| GBP/USD        | `GBPUSD-PERP`  | `GBPUSD-PERP.AX`      |
| EUR/USD        | `EURUSD-PERP`  | `EURUSD-PERP.AX`      |
| Gold           | `XAU-PERP`     | `XAU-PERP.AX`         |
| Silver         | `XAG-PERP`     | `XAG-PERP.AX`         |

The venue identifier is `AX`. To construct a Nautilus `InstrumentId`:

```python
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("GBPUSD-PERP.AX")
```

## Environments

AX Exchange provides two trading environments. Configure the appropriate environment using the
`environment` parameter in your client configuration.

| Environment    | Config                                 | Description                            |
|----------------|----------------------------------------|----------------------------------------|
| **Sandbox**    | `environment=AxEnvironment.SANDBOX`    | Test environment with simulated funds. |
| **Production** | `environment=AxEnvironment.PRODUCTION` | Live trading with real funds.          |

### Sandbox

The default environment for development and testing with simulated funds.

```python
config = AxExecClientConfig(
    environment=AxEnvironment.SANDBOX,
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
The sandbox environment is the default to prevent accidental live trading.
:::

## Market data

The adapter provides comprehensive real-time market data via WebSocket subscriptions.

### Data types

| AX Data         | Nautilus Data Type  | Notes                                        |
|-----------------|---------------------|----------------------------------------------|
| Order book (L1) | `QuoteTick`         | Best bid/ask top-of-book.                    |
| Order book (L2) | `OrderBookDelta`    | Aggregated price levels.                     |
| Order book (L3) | `OrderBookDelta`    | Individual order quantities.                 |
| Trades          | `TradeTick`         | Real-time trade events from L1 subscription. |
| Tickers         | `QuoteTick`         | Mark price, index price, 24h statistics.     |
| Bars/candles    | `Bar`               | OHLCV data with buy/sell volume.             |
| Funding rates   | `FundingRateUpdate` | Current funding rate and settlement info.    |

### Bar intervals

| Interval | Description |
|----------|-------------|
| `1s`     | 1-second    |
| `5s`     | 5-second    |
| `1m`     | 1-minute    |
| `5m`     | 5-minute    |
| `15m`    | 15-minute   |
| `1h`     | 1-hour      |
| `1d`     | 1-day       |

## Orders capability

AX Exchange supports market and limit order types with stop triggers.

### Order types

| Order Type             | Supported | Notes                                              |
|------------------------|-----------|----------------------------------------------------|
| `MARKET`               | ✓         | Execute immediately at best available price.       |
| `LIMIT`                | ✓         | Execute at specified price or better.              |
| `STOP_LIMIT`           | ✓         | Trigger a limit order when stop price is breached. |
| `LIMIT_IF_TOUCHED`     | -         | *Not currently implemented by AX Exchange*.        |
| `STOP_MARKET`          | -         | *Not supported*.                                   |
| `MARKET_IF_TOUCHED`    | -         | *Not supported*.                                   |
| `TRAILING_STOP_MARKET` | -         | *Not supported*.                                   |

### Execution instructions

| Instruction   | Supported | Notes                                                    |
|---------------|-----------|----------------------------------------------------------|
| `post_only`   | ✓         | Maker-only; rejected if order would take liquidity.      |
| `reduce_only` | -         | *Not supported*.                                         |

### Time in force

| Time in Force | Supported | Notes                                        |
|---------------|-----------|----------------------------------------------|
| `GTC`         | ✓         | Good Till Canceled.                          |
| `GTD`         | ✓         | Good Till Date.                              |
| `DAY`         | ✓         | Valid until end of trading day.              |
| `IOC`         | ✓         | Immediate or Cancel.                         |
| `FOK`         | ✓         | Fill or Kill.                                |
| `AT_THE_OPEN` | ✓         | Execute at market open or expire.            |
| `AT_THE_CLOSE`| ✓         | Execute at market close or expire.           |

### Advanced order features

| Feature            | Supported | Notes                                     |
|--------------------|-----------|-------------------------------------------|
| Order modification | -         | Not supported; use cancel and resubmit.   |
| Cancel order       | ✓         | Single order cancellation.                |
| Cancel all orders  | ✓         | Cancel all open orders for an instrument. |
| Batch cancel       | ✓         | Cancel multiple specified orders.         |
| Order lists        | ✓         | Submit multiple orders in a single list.  |

### Position management

| Feature          | Supported | Notes                                  |
|------------------|-----------|----------------------------------------|
| Query positions  | ✓         | Real-time position updates.            |
| Position mode    | -         | Netting mode only.                     |
| Cross margin     | ✓         | Cross-margin across all instruments.   |

### Order querying

| Feature              | Supported | Notes                             |
|----------------------|-----------|-----------------------------------|
| Query open orders    | ✓         | List all active orders.           |
| Order status reports | ✓         | Full order lifecycle reporting.   |
| Fill reports         | ✓         | Execution and fill history.       |

## Authentication

AX Exchange uses bearer token authentication:

1. API key and secret obtain a session token via `/authenticate`.
2. The session token is used as a bearer token for subsequent REST and WebSocket requests.
3. Session tokens expire after a configurable period (default: 86400 seconds).

## Configuration

### Environments and endpoints

| Environment | HTTP API                                         | Market Data WS                                   | Orders WS                                            |
|-------------|--------------------------------------------------|--------------------------------------------------|------------------------------------------------------|
| Sandbox     | `https://gateway.sandbox.architect.exchange/api` | `wss://gateway.sandbox.architect.exchange/md/ws` | `wss://gateway.sandbox.architect.exchange/orders/ws` |
| Production  | `https://gateway.architect.exchange/api`         | `wss://gateway.architect.exchange/md/ws`         | `wss://gateway.architect.exchange/orders/ws`         |

### Data client configuration options

| Option                             | Default   | Description                                                         |
|------------------------------------|-----------|---------------------------------------------------------------------|
| `api_key`                          | `None`    | API key; loaded from `AX_API_KEY` env var when omitted.             |
| `api_secret`                       | `None`    | API secret; loaded from `AX_API_SECRET` env var when omitted.       |
| `environment`                      | `SANDBOX` | Trading environment (`SANDBOX` or `PRODUCTION`).                    |
| `base_url_http`                    | `None`    | Override for the REST base URL.                                     |
| `base_url_ws`                      | `None`    | Override for the WebSocket URL.                                     |
| `http_proxy_url`                   | `None`    | Optional HTTP proxy URL.                                            |
| `http_timeout_secs`                | `60`      | Timeout (seconds) for REST requests.                                |
| `max_retries`                      | `3`       | Maximum retry attempts for REST requests.                           |
| `retry_delay_initial_ms`           | `1000`    | Initial delay (milliseconds) between retries.                       |
| `retry_delay_max_ms`               | `10000`   | Maximum delay (milliseconds) between retries (exponential backoff). |
| `update_instruments_interval_mins` | `60`      | Interval (minutes) between instrument catalog refreshes.            |

### Execution client configuration options

| Option                   | Default   | Description                                                         |
|--------------------------|-----------|---------------------------------------------------------------------|
| `api_key`                | `None`    | API key; loaded from `AX_API_KEY` env var when omitted.             |
| `api_secret`             | `None`    | API secret; loaded from `AX_API_SECRET` env var when omitted.       |
| `environment`            | `SANDBOX` | Trading environment (`SANDBOX` or `PRODUCTION`).                    |
| `base_url_http`          | `None`    | Override for the REST base URL.                                     |
| `base_url_ws`            | `None`    | Override for the orders WebSocket URL.                              |
| `http_proxy_url`         | `None`    | Optional HTTP proxy URL.                                            |
| `http_timeout_secs`      | `60`      | Timeout (seconds) for REST requests.                                |
| `max_retries`            | `3`       | Maximum retry attempts for REST requests.                           |
| `retry_delay_initial_ms` | `1000`    | Initial delay (milliseconds) between retries.                       |
| `retry_delay_max_ms`     | `10000`   | Maximum delay (milliseconds) between retries (exponential backoff). |

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

- **Whole contracts only**: AX Exchange uses integer contract quantities. Fractional quantities
  are not supported and will be rejected.
- **Rate limiting**: The adapter applies a conservative rate limit of 10 requests/second with
  automatic exponential backoff on rate limit responses.
- **Market orders**: AX does not support native market orders. The adapter uses a preview endpoint
  to determine the take-through price and submits an aggressive IOC limit order.

## Contributing

:::info
For additional features or to contribute to the AX Exchange adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
