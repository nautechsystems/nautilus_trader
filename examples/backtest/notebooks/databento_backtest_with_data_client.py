# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.17.3
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# # Databento Data Client with Backtest Node
#
# This example demonstrates how to use the Databento data client with a backtest node.

# %% [markdown]
# ## Imports

# %%
# Note: Use the jupytext python extension to be able to open this python file in jupyter as a notebook

# %%
import asyncio

import nautilus_trader.adapters.databento.data_utils as db_data_utils
from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.factories import DatabentoLiveDataClientFactory
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.trading.strategy import Strategy


# %%
# We need to use nest_asyncio in a jupyter notebook to be able to run async code as sync for market data
# requests in a backtest
try:
    asyncio.get_running_loop()
except RuntimeError:
    pass  # No loop running
else:
    import nest_asyncio

    nest_asyncio.apply()

# %% [markdown]
# ## Parameters

# %%
# Set the data path for Databento data
# DATA_PATH = "/path/to/your/data"  # Use your own value here
# db_data_utils.DATA_PATH = DATA_PATH

catalog_folder = "futures_catalog"
catalog = db_data_utils.load_catalog(catalog_folder)

future_symbols = ["ESM4"]

# Small amount of data for testing
start_time = "2024-05-09T10:00"
end_time = "2024-05-09T10:01"

# A valid databento key can be entered here (or as an env variable of the same name)
# DATABENTO_API_KEY = None
# db_data_utils.init_databento_client(DATABENTO_API_KEY)

# # # Ensure data is available
# futures_data = databento_data(
#     future_symbols,
#     start_time,
#     end_time,
#     "definition",  # "ohlcv-1m"
#     "futures",
#     catalog_folder,
# )

# %% [markdown]
# ## Strategy


# %%
class FuturesStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for the FuturesStrategy.
    """

    future_id: InstrumentId


class FuturesStrategy(Strategy):
    """
    A simple futures trading strategy that subscribes to bar data.
    """

    def __init__(self, config: FuturesStrategyConfig) -> None:
        super().__init__(config=config)
        self.bar_type: BarType | None = None
        self.position_opened = False
        self.n_depths = 0

    def on_start(self) -> None:
        self.bar_type = BarType.from_str(f"{self.config.future_id}-1-MINUTE-LAST-EXTERNAL")

        # Request instrument
        now = self.clock.utc_now()
        self.request_instrument(self.bar_type.instrument_id, end=now, update_catalog=True)
        # instrument = self.cache.instrument(self.bar_type.instrument_id)
        # self.log.warning(f"{instrument=}")

        # Subscribe to bar data
        self.subscribe_bars(self.bar_type, update_catalog=True)

        # Subscribe order book depth
        self.subscribe_order_book_depth(self.config.future_id, depth=10, update_catalog=True)

        self.user_log(f"Strategy started, subscribed to {self.bar_type}")

    def on_order_book_depth(self, depth):
        if self.n_depths > 0:
            return

        self.user_log(
            f"Depth received: ts_init={unix_nanos_to_iso8601(depth.ts_init)}, {depth=}",
        )
        self.n_depths += 1

    def on_bar(self, bar: Bar) -> None:
        self.user_log(
            f"Bar received: ts_init={unix_nanos_to_iso8601(bar.ts_init)}, close={bar.close}",
        )

        # Simple strategy: open a position on the first bar
        if not self.position_opened:
            self.user_log("Opening a position")
            self.submit_market_order(self.config.future_id, 1)
            self.position_opened = True

    def submit_market_order(self, instrument_id: InstrumentId, quantity: int) -> None:
        order = self.order_factory.market(
            instrument_id=instrument_id,
            order_side=(OrderSide.BUY if quantity > 0 else OrderSide.SELL),
            quantity=Quantity.from_int(abs(quantity)),
        )
        self.submit_order(order)
        self.user_log(f"Submitted order: {order}")

    def user_log(self, msg: str) -> None:
        self.log.warning(str(msg), color=LogColor.GREEN)

    def on_stop(self) -> None:
        self.unsubscribe_bars(self.bar_type)
        self.user_log("Strategy stopped")


# %% [markdown]
# ## Backtest Configuration

# %%
# Create BacktestEngineConfig
strategies = [
    ImportableStrategyConfig(
        strategy_path=FuturesStrategy.fully_qualified_name(),
        config_path=FuturesStrategyConfig.fully_qualified_name(),
        config={
            "future_id": InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
        },
    ),
]

logging = LoggingConfig(
    bypass_logging=False,
    log_colors=True,
    log_level="WARN",
    log_level_file="WARN",
    log_directory=".",
    log_file_format=None,
    log_file_name="databento_backtest_with_data_client",
    clear_log_file=True,
    print_config=False,
    use_pyo3=False,
)

# Configure the data catalog
catalogs = [
    DataCatalogConfig(
        path=catalog.path,
    ),
]

engine_config = BacktestEngineConfig(
    logging=logging,
    strategies=strategies,
    catalogs=catalogs,
)

# Create BacktestRunConfig
venues = [
    BacktestVenueConfig(
        name="XCME",
        oms_type="NETTING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1_000_000 USD"],
    ),
]

data_clients: dict = {
    "databento-001": DatabentoDataClientConfig(
        api_key=None,  # 'DATABENTO_API_KEY' env var is used
        routing=RoutingConfig(
            default=False,
            venues=frozenset(["XCME"]),
        ),
    ),
}

config = BacktestRunConfig(
    engine=engine_config,
    venues=venues,
    data=[],  # Empty data list since we're using data clients
    start=start_time,
    end=end_time,
    data_clients=data_clients,
)

configs = [config]

# Create the backtest node
node = BacktestNode(configs=configs)

# Register the Databento data client factory
node.add_data_client_factory("databento", DatabentoLiveDataClientFactory)

# Build the node (this will create and register the data clients)
node.build()

# node.get_engine(configs[0].id).kernel.data_engine.default_client
# node.get_engine(configs[0].id).kernel.data_engine.routing_map

# %%
# Run the backtest
node.run()

# %%
# # Display results
# engine = node.get_engine(configs[0].id)
# engine.trader.generate_order_fills_report()
# engine.trader.generate_positions_report()
# engine.trader.generate_account_report(Venue("GLBX"))

# %%
# # Clean up
# node.dispose()
