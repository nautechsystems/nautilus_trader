# Betfair

NautilusTrader offers adapters for integrating with the Betfair REST API and 
Exchange Streaming API.

## Overview

The following integration classes are available:
- `BetfairInstrumentProvider` which allows querying the Betfair market catalogue for betting markets, which are then converted into Nautilus "instruments".
- `BetfairDataClient` which connects to the Exchange Stream API and streams market data.
- `BetfairExecutionClient` which allows the retrieval of account information and execution and updates for orders (or bets).

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
            "username": "YOUR_BETFAIR_USERNAME",
            "password": "YOUR_BETFAIR_PASSWORD",
            "app_key": "YOUR_BETFAIR_APP_KEY",
            "cert_dir": "YOUR_BETFAIR_CERT_DIR",
        },
    },
    exec_clients={
        "BETFAIR": {
            "username": "YOUR_BETFAIR_USERNAME",
            "password": "YOUR_BETFAIR_PASSWORD",
            "app_key": "YOUR_BETFAIR_APP_KEY",
            "cert_dir": "YOUR_BETFAIR_CERT_DIR",
            "base_currency": "AUD",
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

### API credentials
There are two options for supplying your credentials to the Betfair clients.
Either pass the corresponding `api_key` and `api_secret` values to the config dictionaries, or
set the following environment variables: 
- `BETFAIR_API_KEY`
- `BETFAIR_API_SECRET`
- `BETFAIR_APP_KEY`
- `BETFAIR_CERT_DIR`

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.
