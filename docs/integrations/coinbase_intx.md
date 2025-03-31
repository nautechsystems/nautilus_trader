# Coinbase International

**This guide will walk you through using Coinbase International with NautilusTrader for data ingest and/or live trading**.

:::warning
The Coinbase International integration is currently in a beta testing phase.
Exercise caution and report any issues on GitHub.
:::

[Coinbase International Exchange](https://www.coinbase.com/en/international-exchange) provides non-US institutional clients with access to cryptocurrency perpetual futures and spot markets.
The exchange serves European and international traders by providing leveraged crypto derivatives, often restricted or unavailable in these regions.

Coinbase International brings a high standard of customer protection, a robust risk management framework and high-performance trading technology, including:

- Real-time 24/7/365 risk management.
- Liquidity from external market makers (no proprietary trading).
- Dynamic margin requirements and collateral assessments.
- Liquidation framework that meets rigorous compliance standards.
- Well-capitalized exchange to support tail market events.
- Collaboration with top-tier global regulators.

:::info
See the [Introducing Coinbase International Exchange](https://www.coinbase.com/en-au/blog/introducing-coinbase-international-exchange) blog article for more details.
:::

## Installation

:::note
No additional `coinbase_intx` installation is required; the adapter’s core components, written in Rust, are automatically compiled and linked during the build.
:::

## Examples

You can find functional live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/coinbase_intx).
These examples demonstrate how to set up live market data feeds and execution clients for trading on Coinbase International.

## Overview

The following products are supported on the Coinbase International exchange:

- Perpetual Futures contracts
- Spot cryptocurrencies

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The Coinbase International adapter includes multiple components, which can be used together or
separately depending on the use case. These components work together to connect to Coinbase International’s APIs
for market data and execution.

- `CoinbaseIntxHttpClient`: REST API connectivity.
- `CoinbaseIntxWebSocketClient`: WebSocket API connectivity.
- `CoinbaseIntxInstrumentProvider`: Instrument parsing and loading functionality.
- `CoinbaseIntxDataClient`: A market data feed manager.
- `CoinbaseIntxExecutionClient`: An account management and trade execution gateway.
- `CoinbaseIntxLiveDataClientFactory`: Factory for Coinbase International data clients.
- `CoinbaseIntxLiveExecClientFactory`: Factory for Coinbase International execution clients.

:::note
Most users will simply define a configuration for a live trading node (described below),
and won't necessarily need to work with the above components directly.
:::

## Coinbase documentation

Coinbase International provides extensive API documentation for users which can be found in the [Coinbase Developer Platform](https://docs.cdp.coinbase.com/intx/docs/welcome).
We recommend also referring to the Coinbase International documentation in conjunction with this NautilusTrader integration guide.

## Data

### Instruments

On startup, the adapter automatically loads all available instruments from the Coinbase International REST API
and subscribes to the `INSTRUMENTS` WebSocket channel for updates. This ensures that the cache and clients requiring
up-to-date definitions for parsing always have the latest instrument data.

Available instrument types include:

- `CurrencyPair` (Spot cryptocurrencies)
- `CryptoPerpetual`

:::note
Index products have not yet been implemented.
:::

The following data types are available:

- `OrderBookDelta` (L2 market-by-price)
- `QuoteTick` (L1 top-of-book best bid/ask)
- `TradeTick`
- `Bar`
- `MarkPriceUpdate`
- `IndexPriceUpdate`

:::note
Historical data requests have not yet been implemented.
:::

### WebSocket market data

The data client connects to Coinbase International's WebSocket feed to stream real-time market data.
The WebSocket client handles automatic reconnection and re-subscribes to active subscriptions upon reconnecting.

## Execution

**The adapter is designed to trade one Coinbase International portfolio per execution client.**

### Selecting a portfolio

To identify your available portfolios and their IDs, use the REST client by running the following script:

```bash
python nautilus_trader/adapters/coinbase_intx/scripts/list_portfolios.py
```

This will output a list of portfolio details, similar to the example below:

```bash
[{'borrow_disabled': False,
  'cross_collateral_enabled': False,
  'is_default': False,
  'is_lsp': False,
  'maker_fee_rate': '-0.00008',
  'name': 'hrp5587988499',
  'portfolio_id': '3mnk59ap-1-22',  # Your portfolio ID
  'portfolio_uuid': 'dd0958ad-0c9d-4445-a812-1870fe40d0e1',
  'pre_launch_trading_enabled': False,
  'taker_fee_rate': '0.00012',
  'trading_lock': False,
  'user_uuid': 'd4fbf7ea-9515-1068-8d60-4de91702c108'}]
```

### Configuring the portfolio

To specify a portfolio for trading, set the `COINBASE_INTX_PORTFOLIO_ID` environment variable to
the desired `portfolio_id`. If you're using multiple execution clients, you can alternatively define
the `portfolio_id` in the execution configuration for each client.

### Order types

Coinbase International offers market, limit, and stop order types, enabling a broad range of strategies.
The table below indicates which order types are supported (✓).

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
- Handles connection monitoring and automatic reconnection and logon if the connection is interrupted.
- Properly logs out and closes the connection when the trading node stops.

The client processes several types of execution messages:

- Order status reports (canceled, expired, triggered).
- Fill reports (partial and complete fills).

The FIX credentials are automatically managed using the same API credentials as the REST and WebSocket clients.
No additional configuration is required beyond providing valid API credentials.

:::note
The REST client handles processing `REJECTED` and `ACCEPTED` status execution messages on order submission.
:::

### Account and position management

On startup, the execution client requests and loads your current account and execution state including:

- Available balances across all assets.
- Open orders.
- Open positions.

This provides your trading strategies with a complete picture of your account before placing new orders.

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

## Implementation notes

- **Heartbeats**: The adapter maintains heartbeats on both the WebSocket and FIX connections to ensure reliable connectivity.
- **Rate Limits**: The REST API client is configured to limit requests to the 40/second, as specified by Coinbase International.
- **Graceful Shutdown**: The adapter properly handles graceful shutdown, ensuring all pending messages are processed before disconnecting.
- **Thread Safety**: All adapter components are thread-safe, allowing them to be used from multiple threads concurrently.
- **Execution Model**: The adapter can be configured with a single Coinbase International portfolio per execution client. For trading multiple portfolios, you can create multiple execution clients.
