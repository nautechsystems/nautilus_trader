# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.19.0
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# ## imports

# %%
# Note: Use the jupytext python extension to be able to open this python file in jupyter as a notebook

# %%

import pandas as pd

from nautilus_trader.adapters.databento.data_utils import databento_data
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.trading.strategy import Strategy


# %% [markdown]
# ## parameters

# %%
# Set the data path for Databento data
# import nautilus_trader.adapters.databento.data_utils as db_data_utils
# DATA_PATH = "/path/to/your/data"  # Use your own value here
# db_data_utils.DATA_PATH = DATA_PATH
# A valid databento key can be entered here (or as an env variable of the same name)

# DATABENTO_API_KEY = None
# db_data_utils.init_databento_client(DATABENTO_API_KEY)

catalog_folder = "order_book_deltas_catalog"
catalog = load_catalog(catalog_folder)

future_symbols = ["ESM4"]

start_time = "2024-05-08T00:00:00"
end_time = "2024-05-08T00:00:02"

order_book_deltas = databento_data(
    future_symbols,
    start_time,
    end_time,
    "mbo",
    "orderbooks",
    catalog_folder,
    as_legacy_cython=True,
    load_databento_files_if_exist=True,
)

# deltas = catalog.order_book_deltas()
# deltas_batched = OrderBookDeltas.batch(deltas)
# len(deltas_batched)

# %% [markdown]
# ## strategy


# %%
class TestOrderBookDeltasConfig(StrategyConfig, frozen=True):
    symbol_id: InstrumentId


class TestOrderBookDeltasStrategy(Strategy):
    def __init__(self, config: TestOrderBookDeltasConfig):
        super().__init__(config=config)
        self._deltas_count = 0

    def on_start(self):
        self.request_instrument(self.config.symbol_id)

        # Set time alert for 1 second after start
        alert_time = self.clock.utc_now() + pd.Timedelta(seconds=1)
        self.clock.set_time_alert(
            "subscribe_alert",
            alert_time,
            self.on_subscribe_timer,
        )

    def on_subscribe_timer(self, event):
        self.user_log(
            f"Subscribing to order book deltas after 1 second delay, clock={self.clock.utc_now()}",
        )
        self.subscribe_order_book_deltas(self.config.symbol_id)

    def on_order_book_deltas(self, deltas):
        if self._deltas_count % 50 == 0:
            order_book = self.cache.order_book(self.config.symbol_id)
            self.user_log(f"{order_book}, ts_init={deltas.ts_init}")

        self._deltas_count += 1

    def on_stop(self):
        order_book = self.cache.order_book(self.config.symbol_id)
        self.user_log(f"Final OrderBook: {order_book}")

    def user_log(self, msg, color=LogColor.GREEN):
        self.log.warning(f"{msg}", color=color)


# %% [markdown]
# ## backtest node

# %%
# BacktestEngineConfig

strategies = [
    ImportableStrategyConfig(
        strategy_path=TestOrderBookDeltasStrategy.fully_qualified_name(),
        config_path=TestOrderBookDeltasConfig.fully_qualified_name(),
        config={
            "symbol_id": InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
        },
    ),
]

logging = LoggingConfig(
    bypass_logging=False,
    log_colors=True,
    log_level="WARN",
    log_level_file="WARN",
    log_directory=".",
    log_file_format=None,  # "json" or None
    log_file_name="databento_order_book_deltas",
    clear_log_file=True,
    print_config=False,
    use_pyo3=False,
)

catalogs = [
    DataCatalogConfig(
        path=catalog.path,
    ),
]

data_engine = DataEngineConfig(
    buffer_deltas=True,
)

engine_config = BacktestEngineConfig(
    strategies=strategies,
    logging=logging,
    catalogs=catalogs,
    data_engine=data_engine,
)

# BacktestRunConfig

data = []
data.append(
    BacktestDataConfig(
        data_cls=OrderBookDeltas,
        catalog_path=catalog.path,
        instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
        start_time=start_time,
        end_time=end_time,
    ),
)

venues = [
    BacktestVenueConfig(
        name="XCME",
        oms_type="NETTING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1_000_000 USD"],
    ),
]

configs = [
    BacktestRunConfig(
        engine=engine_config,
        data=[],  # data,
        venues=venues,
        chunk_size=None,  # use None when loading custom data
        start=start_time,
        end=end_time,
    ),
]

node = BacktestNode(configs=configs)

# %%
results = node.run()

# %%

# %%
