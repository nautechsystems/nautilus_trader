# Betfair

Founded in 2000, Betfair operates the world’s largest online betting exchange,
with its headquarters in London and satellite offices across the globe.

NautilusTrader provides an adapter for integrating with the Betfair REST API and
Exchange Streaming API.

## Installation

Install NautilusTrader with Betfair support via pip:

```bash
pip install --upgrade "nautilus_trader[betfair]"
```

To build from source with Betfair extras:

```bash
uv sync --all-extras
```

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/betfair/).

## Betfair documentation

For API details and troubleshooting, see the official [Betfair Developer Documentation](https://developer.betfair.com/en/get-started/).

## Application keys

Betfair requires an Application Key to authenticate API requests. After registering and funding your account, obtain your key using the [API-NG Developer AppKeys Tool](https://apps.betfair.com/visualisers/api-ng-account-operations/).

:::info
See also the [Betfair Getting Started - Application Keys](https://betfair-developer-docs.atlassian.net/wiki/spaces/1smk3cen4v3lu3yomq5qye0ni/pages/2687105/Application+Keys) guide.
:::

## API credentials

Supply your Betfair credentials via environment variables or client configuration:

```bash
export BETFAIR_USERNAME=<your_username>
export BETFAIR_PASSWORD=<your_password>
export BETFAIR_APP_KEY=<your_app_key>
export BETFAIR_CERTS_DIR=<path_to_certificate_dir>
```

:::tip
We recommend using environment variables to manage your credentials.
:::

## Overview

The Betfair adapter provides three primary components:

- `BetfairInstrumentProvider`: loads Betfair markets and converts them into Nautilus instruments.
- `BetfairDataClient`: streams real-time market data from the Exchange Streaming API.
- `BetfairExecutionClient`: submits orders (bets) and tracks execution status via the REST API.

## Orders capability

Betfair operates as a betting exchange with unique characteristics compared to traditional financial exchanges:

### Order types

| Order Type             | Supported | Notes                               |
|------------------------|-----------|-------------------------------------|
| `MARKET`               | -         | Not applicable to betting exchange. |
| `LIMIT`                | ✓         | Orders placed at specific odds.     |
| `STOP_MARKET`          | -         | *Not supported*.                    |
| `STOP_LIMIT`           | -         | *Not supported*.                    |
| `MARKET_IF_TOUCHED`    | -         | *Not supported*.                    |
| `LIMIT_IF_TOUCHED`     | -         | *Not supported*.                    |
| `TRAILING_STOP_MARKET` | -         | *Not supported*.                    |

### Execution instructions

| Instruction   | Supported | Notes                               |
|---------------|-----------|-------------------------------------|
| `post_only`   | -         | Not applicable to betting exchange. |
| `reduce_only` | -         | Not applicable to betting exchange. |

### Time in force options

| Time in force | Supported | Notes                               |
|---------------|-----------|-------------------------------------|
| `GTC`         | -         | Betting exchange uses different model. |
| `GTD`         | -         | Betting exchange uses different model. |
| `FOK`         | -         | Betting exchange uses different model. |
| `IOC`         | -         | Betting exchange uses different model. |

### Advanced order features

| Feature            | Supported | Notes                                    |
|--------------------|-----------|------------------------------------------|
| Order Modification | ✓         | Limited to non-exposure changing fields. |
| Bracket/OCO Orders | -         | *Not supported*.                         |
| Iceberg Orders     | -         | *Not supported*.                         |

### Batch operations

| Operation          | Supported | Notes                |
|--------------------|-----------|----------------------|
| Batch Submit       | -         | *Not supported*.     |
| Batch Modify       | -         | *Not supported*.     |
| Batch Cancel       | -         | *Not supported*.     |

### Position management

| Feature             | Supported | Notes                                   |
|---------------------|-----------|-----------------------------------------|
| Query positions     | -         | Betting exchange model differs.         |
| Position mode       | -         | Not applicable to betting exchange.     |
| Leverage control    | -         | No leverage in betting exchange.        |
| Margin mode         | -         | No margin in betting exchange.          |

### Order querying

| Feature              | Supported | Notes                                   |
|----------------------|-----------|-----------------------------------------|
| Query open orders    | ✓         | List all active bets.                   |
| Query order history  | ✓         | Historical betting data.                |
| Order status updates | ✓         | Real-time bet state changes.            |
| Trade history        | ✓         | Bet matching and settlement reports.    |

### Contingent orders

| Feature             | Supported | Notes                                  |
|---------------------|-----------|------------------------------------------|
| Order lists         | -         | *Not supported*.                        |
| OCO orders          | -         | *Not supported*.                        |
| Bracket orders      | -         | *Not supported*.                        |
| Conditional orders  | -         | Basic bet conditions only.              |

## Configuration

### Data client configuration options

| Option                    | Default   | Description |
|---------------------------|-----------|-------------|
| `account_currency`        | Required  | Betfair account currency for data and price feeds. |
| `username`                | `None`    | Betfair account username; taken from environment when omitted. |
| `password`                | `None`    | Betfair account password; taken from environment when omitted. |
| `app_key`                 | `None`    | Betfair application key used for API authentication. |
| `certs_dir`               | `None`    | Directory containing Betfair SSL certificates for login. |
| `instrument_config`       | `None`    | Optional `BetfairInstrumentProviderConfig` to scope available markets. |
| `subscription_delay_secs` | `3`       | Delay (seconds) before initial market subscription request is sent. |
| `keep_alive_secs`         | `36,000`  | Keep-alive interval (seconds) for the Betfair session. |
| `stream_conflate_ms`      | `None`    | Explicit stream conflation interval in milliseconds (`0` disables conflation). |

### Execution client configuration options

| Option                       | Default  | Description |
|------------------------------|----------|-------------|
| `account_currency`           | Required | Betfair account currency for order placement and balances. |
| `username`                   | `None`   | Betfair account username; taken from environment when omitted. |
| `password`                   | `None`   | Betfair account password; taken from environment when omitted. |
| `app_key`                    | `None`   | Betfair application key used for API authentication. |
| `certs_dir`                  | `None`   | Directory containing Betfair SSL certificates for login. |
| `instrument_config`          | `None`   | Optional `BetfairInstrumentProviderConfig` to scope reconciliation. |
| `calculate_account_state`    | `True`   | Calculate account state locally from events when `True`. |
| `request_account_state_secs` | `300`    | Interval (seconds) to poll Betfair for account state (`0` disables). |
| `reconcile_market_ids_only`  | `False`  | When `True`, reconciliation requests only cover configured market IDs. |
| `ignore_external_orders`     | `False`  | When `True`, ignore stream orders missing from the local cache. |

Here is a minimal example showing how to configure a live `TradingNode` with Betfair clients:

```python
from nautilus_trader.adapters.betfair import BETFAIR
from nautilus_trader.adapters.betfair import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair import BetfairLiveExecClientFactory
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode

# Configure Betfair data and execution clients (using AUD account currency)
config = TradingNodeConfig(
    data_clients={BETFAIR: {"account_currency": "AUD"}},
    exec_clients={BETFAIR: {"account_currency": "AUD"}},
)

# Build the TradingNode with Betfair adapter factories
node = TradingNode(config)
node.add_data_client_factory(BETFAIR, BetfairLiveDataClientFactory)
node.add_exec_client_factory(BETFAIR, BetfairLiveExecClientFactory)
node.build()
```

:::info
For additional features or to contribute to the Betfair adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
