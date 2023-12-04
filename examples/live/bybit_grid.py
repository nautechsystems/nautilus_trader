from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.factories import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit.factories import BybitLiveExecClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import CacheDatabaseConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.grid import GridConfig
from nautilus_trader.examples.strategies.grid import GridStrategy
from nautilus_trader.live.node import TradingNode


bybit_testnet_api_key = "puoVYU45dIfelFgOon"
bybit_testnet_api_secret = "b1qY5GDzPR9RgcQvbnHhIT5W2iWmqTJJSRvT"

config_node = TradingNodeConfig(
    trader_id="FILIP-001",
    environment=Environment.LIVE,
    logging=LoggingConfig(log_level="INFO"),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
    ),
    cache_database=CacheDatabaseConfig(type="redis"),
    data_clients={
        "BYBIT": BybitDataClientConfig(
            api_key=bybit_testnet_api_key,
            api_secret=bybit_testnet_api_secret,
            instrument_types=[BybitInstrumentType.LINEAR],
            testnet=True,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        "BYBIT": BybitExecClientConfig(
            api_key=bybit_testnet_api_key,
            api_secret=bybit_testnet_api_secret,
            instrument_types=[BybitInstrumentType.LINEAR],
            testnet=True,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

node = TradingNode(config=config_node)

instrument_id = "ETHUSDT-LINEAR.BYBIT"
grid_config = GridConfig(
    instrument_id=instrument_id,
    value="test",
)
grid_strategy = GridStrategy(config=grid_config)

node.trader.add_strategy(grid_strategy)
node.add_data_client_factory("BYBIT", BybitLiveDataClientFactory)
node.add_exec_client_factory("BYBIT", BybitLiveExecClientFactory)
node.build()

if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
