# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.16.4
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# ## imports

# %%
# Note: Use the python extension jupytext to be able to open this python file in jupyter as a notebook

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
from nautilus_trader.core.datetime import unix_nanos_to_str
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarAggregation
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.trading.strategy import Strategy


# %% [markdown]
# ## parameters

# %%
# import nautilus_trader.adapters.databento.data_utils as db_data_utils
# from nautilus_trader.adapters.databento.data_utils import init_databento_client
# from option_trader import DATA_PATH, DATABENTO_API_KEY # personal library, use your own values especially for DATABENTO_API_KEY
# db_data_utils.DATA_PATH = DATA_PATH

catalog_folder = "historical_bars_catalog"
catalog = load_catalog(catalog_folder)

future_symbols = ["ESU4"]

# small amount of data to download for testing, very cheap
start_time = "2024-07-01T23:40"
end_time = "2024-07-02T00:10"

# a valid databento key can be entered here, the example below runs with already saved test data
# db_data_utils.DATABENTO_API_KEY = DATABENTO_API_KEY
# init_databento_client()

# https://databento.com/docs/schemas-and-data-formats/whats-a-schema
futures_data_bars = databento_data(
    future_symbols,
    start_time,
    end_time,
    "ohlcv-1m",
    "futures",
    catalog_folder,
)

futures_data_quotes = databento_data(
    future_symbols,
    "2024-07-01T23:58",
    "2024-07-02T00:02",
    "mbp-1",
    "futures",
    catalog_folder,
)

futures_data_trades = databento_data(
    future_symbols,
    "2024-07-01T23:58",
    "2024-07-02T00:02",
    "trades",
    "futures",
    catalog_folder,
)

# %% [markdown]
# ## strategy


# %%
class TestHistoricalAggConfig(StrategyConfig, frozen=True):
    symbol_id: InstrumentId
    historical_start_delay: int = 10
    historical_end_delay: int = 1


class TestHistoricalAggStrategy(Strategy):
    def __init__(self, config: TestHistoricalAggConfig):
        super().__init__(config=config)

        self._symbol_id = config.symbol_id
        self._historical_start_delay = config.historical_start_delay
        self._historical_end_delay = config.historical_end_delay
        # self.external_sma = SimpleMovingAverage(2)
        # self.composite_sma = SimpleMovingAverage(2)

    def on_start(self):
        ######### for testing bars
        utc_now = self._clock.utc_now()
        start_historical_bars = utc_now - pd.Timedelta(minutes=self._historical_start_delay)
        end_historical_bars = utc_now - pd.Timedelta(minutes=self._historical_end_delay)
        self.user_log(f"on_start: {start_historical_bars=}, {end_historical_bars=}")

        # external_bar_type = BarType.from_str(f"{self._symbol_id}-1-MINUTE-LAST-EXTERNAL")
        # self.subscribe_bars(external_bar_type)

        bar_type_1 = BarType.from_str(f"{self._symbol_id}-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")
        bar_type_2 = BarType.from_str(f"{self._symbol_id}-4-MINUTE-LAST-INTERNAL@2-MINUTE-INTERNAL")
        bar_type_3 = BarType.from_str(f"{self._symbol_id}-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")

        self.subscribe_bars(bar_type_1)
        self.subscribe_bars(bar_type_2)
        self.subscribe_bars(bar_type_3)

        self.request_aggregated_bars(
            [bar_type_1, bar_type_2, bar_type_3],
            start=start_historical_bars,
            end=end_historical_bars,
            update_existing_subscriptions=True,
            include_external_data=False,
        )

        #### for testing indicators with bars
        # self.register_indicator_for_bars(external_bar_type, self.external_sma)
        # self.register_indicator_for_bars(composite_bar_type, self.composite_sma)

        ######### for testing quote ticks
        # utc_now = self._clock.utc_now()
        # start_historical_bars = utc_now - pd.Timedelta(minutes=self._historical_start_delay)
        # end_historical_bars = utc_now - pd.Timedelta(
        #     minutes=self._historical_end_delay,
        #     milliseconds=1,
        # )
        # self.user_log(f"on_start: {start_historical_bars=}, {end_historical_bars=}")

        # bar_type_1 = BarType.from_str(f"{self._symbol_id}-1-MINUTE-BID-INTERNAL")
        # bar_type_2 = BarType.from_str(f"{self._symbol_id}-2-MINUTE-BID-INTERNAL@1-MINUTE-INTERNAL")

        # self.subscribe_bars(bar_type_1)
        # self.subscribe_bars(bar_type_2)

        # self.request_aggregated_bars(
        #     [bar_type_1, bar_type_2],
        #     start=start_historical_bars,
        #     end=end_historical_bars,
        #     update_existing_subscriptions=True,
        #     include_external_data=False,
        # )

        ######### for testing trade ticks
        # utc_now = self._clock.utc_now()
        # start_historical_bars = utc_now - pd.Timedelta(minutes=self._historical_start_delay)
        # end_historical_bars = utc_now - pd.Timedelta(
        #     minutes=self._historical_end_delay,
        #     milliseconds=1,
        # )
        # self.user_log(f"on_start: {start_historical_bars=}, {end_historical_bars=}")

        # bar_type_1 = BarType.from_str(f"{self._symbol_id}-1-MINUTE-LAST-INTERNAL")
        # bar_type_2 = BarType.from_str(f"{self._symbol_id}-2-MINUTE-LAST-INTERNAL@1-MINUTE-INTERNAL")

        # self.subscribe_bars(bar_type_1)
        # self.subscribe_bars(bar_type_2)

        # self.request_aggregated_bars(
        #     [bar_type_1, bar_type_2],
        #     start=start_historical_bars,
        #     end=end_historical_bars,
        #     update_existing_subscriptions=True,
        #     include_external_data=False,
        # )

    def on_historical_data(self, data):
        if type(data) is Bar:
            self.user_log(f"historical bar ts_init = {unix_nanos_to_str(data.ts_init)}")
            self.user_log(data)

            # self.user_log(f"{self.external_sma.value=}, {self.external_sma.initialized=}")
            # self.user_log(f"{self.composite_sma.value=}, {self.composite_sma.initialized=}")

    def on_bar(self, bar):
        self.user_log(f"bar ts_init = {unix_nanos_to_str(bar.ts_init)}")
        self.user_log(bar)

        # self.user_log(f"{self.external_sma.value=}, {self.external_sma.initialized=}")
        # self.user_log(f"{self.composite_sma.value=}, {self.composite_sma.initialized=}")

    def user_log(self, msg):
        self.log.warning(str(msg), color=LogColor.GREEN)


# %% [markdown]
# ## backtest node

# %%
# BacktestEngineConfig

strategies = [
    ImportableStrategyConfig(
        strategy_path=TestHistoricalAggStrategy.fully_qualified_name(),
        config_path=TestHistoricalAggConfig.fully_qualified_name(),
        config={
            "symbol_id": InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
            # for bars
            "historical_start_delay": 10,
            "historical_end_delay": 1,
            # for quotes
            # "historical_start_delay": 2,
            # "historical_end_delay": 0,
        },
    ),
]

logging = LoggingConfig(
    bypass_logging=False,
    log_colors=True,
    log_level="WARN",
    log_level_file="WARN",
    log_directory=".",
    log_file_format=None,  # 'json' or None
    log_file_name="databento_option_greeks",
    clear_log_file=True,
)

catalogs = [
    DataCatalogConfig(
        path=catalog.path,
    ),
]

data_engine = DataEngineConfig(
    time_bars_origins={
        BarAggregation.MINUTE: pd.Timedelta(seconds=0),
    },
)

engine_config = BacktestEngineConfig(
    strategies=strategies,
    logging=logging,
    catalogs=catalogs,
    data_engine=data_engine,
)

# BacktestRunConfig

data = [
    BacktestDataConfig(
        data_cls=Bar,
        catalog_path=catalog.path,
        instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
        bar_spec="1-MINUTE-LAST",
        start_time="2024-07-01T23:40",
        end_time="2024-07-02T00:10",
    ),
    BacktestDataConfig(
        data_cls=QuoteTick,
        catalog_path=catalog.path,
        instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
        start_time="2024-07-01T23:58",
        end_time="2024-07-02T00:02",
    ),
    BacktestDataConfig(
        data_cls=TradeTick,
        catalog_path=catalog.path,
        instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
        start_time="2024-07-01T23:58",
        end_time="2024-07-02T00:02",
    ),
]

venues = [
    BacktestVenueConfig(
        name="GLBX",
        oms_type="NETTING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1_000_000 USD"],
    ),
]

configs = [
    BacktestRunConfig(
        engine=engine_config,
        data=data,
        venues=venues,
        chunk_size=None,  # use None when loading custom data
        # for bars
        start="2024-07-01T23:55",
        end="2024-07-02T00:10",
        # for quote or trade ticks
        # start="2024-07-02T00:00",
        # end="2024-07-02T00:02",
    ),
]

node = BacktestNode(configs=configs)

# %%
results = node.run(raise_exception=True)
