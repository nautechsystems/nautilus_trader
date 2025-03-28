# Betfair

Founded in 2000, Betfair operates the world’s largest online betting exchange,
with its headquarters in London and satellite offices across the globe.

NautilusTrader provides an adapter for integrating with the Betfair REST API and
Exchange Streaming API.

## Installation

To install the latest `nautilus_trader` package along with the `betfair` dependencies using pip:

```
pip install -U "nautilus_trader[betfair]"
```

To install from source using uv:

```
uv sync --extra betfair
```

## Examples

You can find functional live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/betfair/).

## Betfair documentation

Betfair provides extensive [documentation](https://developer.betfair.com/en/get-started/) for developers integrating with their exchange APIs.
This resource is valuable for gaining background information, understanding the APIs, and troubleshooting integration issues.

## Application Keys

Betfair uses Application Keys (App keys) to manage interactions with its APIs.
Initially, you will be given a "Delayed" App key (data delayed 1-180 seconds), later you can apply for a "Live" App key.

After setting up a funded Betfair account, you will need to obtain your App key.
You can do this through the [Accounts API Demo Tool](https://apps.betfair.com/visualisers/api-ng-account-operations/). Follow these steps:

1. Log in to your Betfair account. With your browser's developer tools open, inspect the initial POST request to https://identitysso.betfair.com.au/api/login, and find the `ssoid` in the response headers (set in the cookie).
2. Open the Betfair API tool and enter your `ssoid` into the Session Token (ssoid) field.
3. In the left-hand navigation, select `getDeveloperAppKeys`, then click the Execute button at the bottom to retrieve your App key.

:::info
See also the [Betfair Getting Started - Application Keys](https://betfair-developer-docs.atlassian.net/wiki/spaces/1smk3cen4v3lu3yomq5qye0ni/pages/2687105/Application+Keys) guide.
:::

## API credentials

There are two options for supplying your credentials to the Betfair clients.
Either pass the corresponding values to the config dictionaries, or
set the following environment variables:
- `BETFAIR_USERNAME`
- `BETFAIR_PASSWORD`
- `BETFAIR_APP_KEY`
- `BETFAIR_CERTS_DIR`

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.

:::tip
We recommend using environment variables to manage your credentials.
:::

## Overview

The following adapter classes are available:

- `BetfairInstrumentProvider` which enables querying the Betfair market catalogue for betting markets, which are then converted into Nautilus "instruments".
- `BetfairDataClient` which connects to the Exchange Stream API and streams market data.
- `BetfairExecutionClient` which enables the retrieval of account information and execution and updates for orders (or bets).

## Configuration

The most common use case is to configure a live `TradingNode` to include Betfair
data and execution clients. To achieve this, add a `BETFAIR` section to your client
configuration(s):

```python
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "BETFAIR": {
            "account_currency": "AUD",
            # username=None, # 'BETFAIR_USERNAME' env var
            # password=None, # 'BETFAIR_PASSWORD' env var
            # app_key=None, # 'BETFAIR_APP_KEY' env var
            # certs_dir=None, # 'BETFAIR_CERTS_DIR' env var
        },
    },
    exec_clients={
        "BETFAIR": {
            "account_currency": "AUD",
            # username=None, # 'BETFAIR_USERNAME' env var
            # password=None, # 'BETFAIR_PASSWORD' env var
            # app_key=None, # 'BETFAIR_APP_KEY' env var
            # certs_dir=None, # 'BETFAIR_CERTS_DIR' env var
        },
    }
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
node.add_exec_client_factory("BETFAIR", BetfairLiveExecClientFactory)

# Finally build the node
node.build()
```
