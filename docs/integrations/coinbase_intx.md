# Coinbase International

[Coinbase International Exchange](https://www.coinbase.com/en/international-exchange) provides non-US institutional clients with access to cryptocurrency perpetual futures and spot markets.
The exchange serves European and international traders by providing leveraged crypto derivatives, often restricted or unavailable in these regions.

Coinbase International brings a high standard of customer protection, a robust risk management framework and high-performance trading technology, including:

- Real-time 24/7/365 risk management
- Liquidity from external market makers (no proprietary trading)
- Dynamic margin requirements and collateral assessments
- Liquidation framework that meets rigorous compliance standards
- Well-capitalized exchange to support tail market events
- Collaboration with top-tier global regulators

See the [Introducing Coinbase International Exchange](https://www.coinbase.com/en-au/blog/introducing-coinbase-international-exchange) blog article for more details.

:::info
We are currently working on this integration guide.
:::

:::warning
The Coinbase International integration is currently in a beta testing phase.
Exercise caution and report any issues on GitHub.
:::

## Examples

You can find functional live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/coinbase_intx).

## Overview

The following products are supported on the Coinbase International exchange:

- Perpetual Futures contracts
- Spot cryptocurrencies

:::info
No additional `coinbase_intx` installation is required; the adapter’s core components, written in Rust, are automatically compiled and linked during the build.
:::

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The Coinbase International adapter includes multiple components, which can be used together or
separately depending on the use case.

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

## Coinbase International documentation

Coinbase International provides extensive API documentation for users which can be found in the [Coinbase Developer Platform](https://docs.cdp.coinbase.com/intx/docs/welcome).
We recommend also referring to the Coinbase International documentation in conjunction with this NautilusTrader integration guide.

## Order types

Coinbase International offers market, limit, and stop order types, enabling a broad range of strategies.

|                        | Derivatives          | Spot                     |
|------------------------|----------------------|--------------------------|
| `MARKET`               | ✓                    | ✓                        |
| `LIMIT`                | ✓                    | ✓                        |
| `STOP_MARKET`          | ✓                    | ✓                        |
| `STOP_LIMIT`           | ✓                    | ✓                        |

:::note
`MARKET` orders must be submitted with either `IOC` or `FOK` time in force.
:::

## Execution

The adapter is built to trade one Coinbase International portfolio per execution client.

To specify the portfolio, set the `COINBASE_INTX_PORTFOLIO_ID` environment variable to the desired
portfolio ID. Alternatively, if using multiple execution clients, define the `portfolio_id` in the
execution configuration for each client.

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

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.
