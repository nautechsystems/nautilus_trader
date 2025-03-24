# Coinbase International

:::warning
The Coinbase International integration is currently in a beta testing phase.
Exercise caution and report any issues on GitHub.
:::

[Coinbase International Exchange](https://www.coinbase.com/en/international-exchange) provides non-US institutional clients with access to cryptocurrency perpetual futures and spot markets.
The exchange serves European and international traders by providing leveraged crypto derivatives, often restricted or unavailable in these regions.

This guide will walk you through using Coinbase International with NautilusTrader for data ingest and/or live trading.

Coinbase International brings a high standard of customer protection, a robust risk management framework and high-performance trading technology, including:

- Real-time 24/7/365 risk management.
- Liquidity from external market makers (no proprietary trading).
- Dynamic margin requirements and collateral assessments.
- Liquidation framework that meets rigorous compliance standards.
- Well-capitalized exchange to support tail market events.
- Collaboration with top-tier global regulators.

See the [Introducing Coinbase International Exchange](https://www.coinbase.com/en-au/blog/introducing-coinbase-international-exchange) blog article for more details.

## Examples

You can find functional live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/coinbase_intx).
These examples demonstrate how to set up live market data feeds and execution clients for trading on Coinbase International, providing practical starting points for your integration.

## Overview

The following products are supported on the Coinbase International exchange:

- Perpetual Futures contracts
- Spot cryptocurrencies

:::info
No additional `coinbase_intx` installation is required; the adapter’s core components, written in Rust, are automatically compiled and linked during the build.
:::

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The Coinbase International adapter includes multiple components, which can be used together or
separately depending on the use case. These components work together to connect to Coinbase International’s APIs,
fetch market data, manage instruments, and execute trades, all orchestrated through a trading node configuration.

- `CoinbaseIntxHttpClient`: REST API connectivity.
- `CoinbaseIntxWebSocketClient`: WebSocket API connectivity.
- `CoinbaseIntxInstrumentProvider`: Instrument parsing and loading functionality.
- `CoinbaseIntxDataClient`: A market data feed manager.
- `CoinbaseIntxExecutionClient`: An account management and trade execution gateway.
- `CoinbaseIntxLiveDataClientFactory`: Factory for Coinbase International data clients (used by the trading node builder).
- `CoinbaseIntxLiveExecClientFactory`: Factory for Coinbase International execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (described below),
and won't necessarily need to work with the above components directly.
:::

## Coinbase documentation

Coinbase International provides extensive API documentation for users which can be found in the [Coinbase Developer Platform](https://docs.cdp.coinbase.com/intx/docs/welcome).
We recommend also referring to the Coinbase International documentation in conjunction with this NautilusTrader integration guide.

## Data

### Instruments

The adapter automatically loads all available instruments from Coinbase International on startup.
It also supports periodic reloading of instrument definitions (configurable via `update_instruments_interval_mins`) to ensure your trading system always has the latest instrument definitions and status, including:

- Updated price/size increments.
- New instruments.
- Trading state changes.

### WebSocket market data

The data client connects to Coinbase International's WebSocket feed to stream real-time market data, including:

- Order book updates (L2 market-by-price).
- Quote ticks (top-of-book best bid/ask).
- Trade ticks.
- Bar (candlestick) data.

The WebSocket client handles automatic reconnection and subscription management, ensuring consistent market data delivery even during temporary connection issues.

## Execution

The adapter is built to trade one Coinbase International portfolio per execution client.

To specify the portfolio, set the `COINBASE_INTX_PORTFOLIO_ID` environment variable to the desired
portfolio ID. Alternatively, if using multiple execution clients, define the `portfolio_id` in the
execution configuration for each client.

### Order types

Coinbase International offers market, limit, and stop order types, enabling a broad range of strategies.
The table below indicates which order types are supported (✓) for derivatives and spot markets.

|                        | Derivatives          | Spot                     |
|------------------------|----------------------|--------------------------|
| `MARKET`               | ✓                    | ✓                        |
| `LIMIT`                | ✓                    | ✓                        |
| `STOP_MARKET`          | ✓                    | ✓                        |
| `STOP_LIMIT`           | ✓                    | ✓                        |

:::note
`MARKET` orders must be submitted with either `IOC` or `FOK` time in force.
:::

### Advanced order features

Coinbase International supports several advanced order features that can be accessed through the adapter:

- **Post-Only**: Limit orders can be specified as post-only (`post_only=True`) to ensure they only provide liquidity and never take liquidity.
- **Reduce-Only**: Orders can be specified as reduce-only (`reduce_only=True`) to ensure they only reduce existing positions and never increase exposure.
- **Time-In-Force**: All standard time-in-force options are supported (GTC, GTD, IOC, FOK).

### FIX Drop Copy integration

The Coinbase International adapter includes a FIX (Financial Information eXchange) [drop copy](https://docs.cdp.coinbase.com/intx/docs/fix-msg-drop-copy) client.
This provides reliable, low-latency execution updates directly from Coinbase's matching engine.

:::note
This approach is necessary because execution messages are not provided over the WebSocket feed, and delivers faster and more reliable order execution updates than polling the REST API.
:::

The FIX client:

- Establishes a secure TCP/TLS connection and logs on automatically when the trading node starts.
- Handles connection monitoring and automatic reconnection if the connection is interrupted.
- Properly logs out and closes the connection when the trading node stops.

The client processes several types of execution messages:

- Order status reports (canceled, expired, triggered).
- Fill reports (both partial fills and complete fills).

The FIX credentials are automatically managed using the same API credentials as the REST and WebSocket clients.
No additional configuration is required beyond providing valid API credentials, as the FIX connection is fully set up and managed by the adapter.

### Account and position management

On startup, the execution client loads your current account and execution state including:

- Available balances across all assets.
- Open positions.
- Open orders.

This provides your trading strategies with a complete picture of your account before placing new orders.
Position status reports are generated both on startup and periodically to ensure accurate risk management.

## Configuration

### Strategies

:::warning
Coinbase International has a strict specification for client order IDs.
Nautilus can meet the spec by using UUID4 values for client order IDs.
To comply, set the `use_uuid_client_order_ids=True` config option in your strategy configuration (otherwise, order submission will trigger an API error).

See the Coinbase International [Create order](https://docs.cdp.coinbase.com/intx/reference/createorder) REST API documentation for further details.
:::

An example configuration could be:

```python
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Further config omitted
    data_clients={
        COINBASE_INTX: CoinbaseIntxDataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        COINBASE_INTX: CoinbaseIntxExecClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
            http_timeout_secs=10,
        ),
    },
)

strat_config = TOBQuoterConfig(
    use_uuid_client_order_ids=True,  # <-- Necessary for Coinbase Intx
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    ...,  # Further config omitted
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX
from nautilus_trader.adapters.coinbase_intx.factories import CoinbaseIntxLiveDataClientFactory
from nautilus_trader.adapters.coinbase_intx.factories import CoinbaseIntxLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory(COINBASE_INTX, CoinbaseIntxLiveDataClientFactory)
node.add_exec_client_factory(COINBASE_INTX, CoinbaseIntxLiveExecClientFactory)

# Finally build the node
node.build()
```

### API credentials

Provide credentials to the clients using one of the following methods.

Either pass values for the following configuration options:

- `api_key`
- `api_secret`
- `api_passphrase`
- `portfolio_id`

Or, set the following environment variables:

- `COINBASE_INTX_API_KEY`
- `COINBASE_INTX_API_SECRET`
- `COINBASE_INTX_API_PASSPHRASE`
- `COINBASE_INTX_PORTFOLIO_ID`

:::tip
We recommend using environment variables to manage your credentials.
:::

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.

## Implementation Notes

- **Heartbeats**: The adapter maintains heartbeats on both the WebSocket and FIX connections to ensure reliable connectivity.
- **Graceful Shutdown**: The adapter properly handles graceful shutdown, ensuring all pending messages are processed before disconnecting.
- **Thread Safety**: All adapter components are thread-safe, allowing them to be used from multiple threads concurrently.
- **Execution Model**: The adapter can be configured with a single Coinbase International portfolio per execution client. For trading multiple portfolios, you can create multiple execution clients.
