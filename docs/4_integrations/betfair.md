# Betfair

NautilusTrader offers adapters for integrating with the Betfair REST API and 
Exchange Streaming API. Under the hood it leverages the excellent [betfairlightweight](https://github.com/liampauling/betfair) library to handle some of the formatting of requests to Betfair.

The following integration classes are available:
- `BetfairInstrumentProvider` which allows querying the Betfair market catalogue for betting markets, which are then converted into Nautilus "instruments".
- `BetfairDataClient` which connects to the Exchange Stream API and streams market data.
- `BetfairExecutionClient` which allows the retrieval of account information and execution and updates for orders (or bets).

The Betfair adapter currently uses environment variables for authentication. To use the adapter, 
simply pass the following config to the TradingNode, indicating the names of the environment variables 
to look for when connecting:

```python
config = {
    ... # Omitted 
    "data_clients": {
        "BETFAIR": {
            "username": "YOUR_BETFAIR_USERNAME",
            "password": "YOUR_BETFAIR_PASSWORD",
            "app_key": "YOUR_BETFAIR_APP_KEY",
            "cert_dir": "YOUR_BETFAIR_CERT_DIR",
        },
    },
    "exec_clients": {
        "BETFAIR": {
            "username": "YOUR_BETFAIR_USERNAME",
            "password": "YOUR_BETFAIR_PASSWORD",
            "app_key": "YOUR_BETFAIR_APP_KEY",
            "cert_dir": "YOUR_BETFAIR_CERT_DIR",
            "base_currency": "AUD",
        },
    }
```

Then, create a `TradingNode` and add the factory clients:

```python
# Instantiate the node passing a list of strategies and configuration
node = TradingNode(config=config)
node.trader.add_strategies([strategy])

# Register your client factories with the node (can also take user defined factories)
node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
node.add_exec_client_factory("BETFAIR", BetfairLiveExecutionClientFactory)
```
