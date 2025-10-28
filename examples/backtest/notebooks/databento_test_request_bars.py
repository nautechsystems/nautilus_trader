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
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
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
# Set the data path for Databento data
# import nautilus_trader.adapters.databento.data_utils as db_data_utils
# DATA_PATH = "/path/to/your/data"  # Use your own value here
# db_data_utils.DATA_PATH = DATA_PATH

catalog_folder = "historical_bars_catalog"
catalog = load_catalog(catalog_folder)

future_symbols = ["ESU4"]

# small amount of data to download for testing, very cheap
# Note that the example below doesn't need any download as the test data is included in the repository
start_time = "2024-07-01T23:40"
end_time = "2024-07-02T00:10"

# A valid databento key can be entered here (or as an env variable of the same name)
# DATABENTO_API_KEY = None
# db_data_utils.init_databento_client(DATABENTO_API_KEY)

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
    data_type: str = "bars"


class TestHistoricalAggStrategy(Strategy):
    def __init__(self, config: TestHistoricalAggConfig):
        super().__init__(config=config)

        # self.external_sma = SimpleMovingAverage(2)
        # self.composite_sma = SimpleMovingAverage(2)

    def on_start(self):
        if self.config.data_type == "bars":
            ######### for testing bars
            utc_now = self.clock.utc_now()
            start_historical_bars = utc_now - pd.Timedelta(
                minutes=self.config.historical_start_delay,
            )
            end_historical_bars = utc_now - pd.Timedelta(minutes=self.config.historical_end_delay)
            self.user_log(f"on_start: {start_historical_bars=}, {end_historical_bars=}")

            symbol_id = self.config.symbol_id
            self.external_bar_type = BarType.from_str(f"{symbol_id}-1-MINUTE-LAST-EXTERNAL")
            self.bar_type_1 = BarType.from_str(
                f"{symbol_id}-2-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL",
            )
            self.bar_type_2 = BarType.from_str(
                f"{symbol_id}-4-MINUTE-LAST-INTERNAL@2-MINUTE-INTERNAL",
            )
            self.bar_type_3 = BarType.from_str(
                f"{symbol_id}-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL",
            )

            self.request_instrument(symbol_id)
            # self.subscribe_instruments(symbol_id.venue)

            # registering bar types with indicators, request_aggregated_bars below will update both indicators
            # self.register_indicator_for_bars(self.external_bar_type, self.external_sma)
            # self.register_indicator_for_bars(self.bar_type_1, self.composite_sma)

            self.request_aggregated_bars(
                [self.bar_type_1, self.bar_type_2, self.bar_type_3],
                start=start_historical_bars,
                end=end_historical_bars,
                update_subscriptions=True,
                # includes external bars in the response, not just internally aggregated ones
                include_external_data=True,
            )

            self.user_log("request_aggregated_bars done")

            self.subscribe_bars(self.external_bar_type)
            self.subscribe_bars(self.bar_type_1)
            self.subscribe_bars(self.bar_type_2)
            self.subscribe_bars(self.bar_type_3)

            self.user_log("subscribe_bars done")
        elif self.config.data_type == "quotes":
            ######## for testing quotes
            utc_now = self.clock.utc_now()
            start_historical_bars = utc_now - pd.Timedelta(
                minutes=self.config.historical_start_delay,
            )
            end_historical_bars = utc_now - pd.Timedelta(
                minutes=self.config.historical_end_delay,
                milliseconds=1,
            )
            self.user_log(f"on_start: {start_historical_bars=}, {end_historical_bars=}")

            self.bar_type_1 = BarType.from_str(f"{self.config.symbol_id}-1-MINUTE-BID-INTERNAL")
            self.bar_type_2 = BarType.from_str(
                f"{self.config.symbol_id}-2-MINUTE-BID-INTERNAL@1-MINUTE-INTERNAL",
            )

            self.request_aggregated_bars(
                [self.bar_type_1, self.bar_type_2],
                start=start_historical_bars,
                end=end_historical_bars,
                update_subscriptions=True,
                include_external_data=False,
            )

            self.subscribe_bars(self.bar_type_1)
            self.subscribe_bars(self.bar_type_2)
        if self.config.data_type == "trades":
            ######## for testing trades
            utc_now = self.clock.utc_now()
            start_historical_bars = utc_now - pd.Timedelta(
                minutes=self.config.historical_start_delay,
            )
            end_historical_bars = utc_now - pd.Timedelta(
                minutes=self.config.historical_end_delay,
                milliseconds=1,
            )
            self.user_log(f"on_start: {start_historical_bars=}, {end_historical_bars=}")

            self.bar_type_1 = BarType.from_str(f"{self.config.symbol_id}-1-MINUTE-LAST-INTERNAL")
            self.bar_type_2 = BarType.from_str(
                f"{self.config.symbol_id}-2-MINUTE-LAST-INTERNAL@1-MINUTE-INTERNAL",
            )

            self.request_aggregated_bars(
                [self.bar_type_1, self.bar_type_2],
                start=start_historical_bars,
                end=end_historical_bars,
                update_subscriptions=True,
                include_external_data=False,
            )

            self.subscribe_bars(self.bar_type_1)
            self.subscribe_bars(self.bar_type_2)

    def on_historical_data(self, data):
        if type(data) is Bar:
            self.user_log(
                f"historical bar ts_init = {unix_nanos_to_iso8601(data.ts_init)}, {data.ts_init}",
            )
            self.user_log(data)

            # self.user_log(f"{self.external_sma.value=}, {self.external_sma.initialized=}")
            # self.user_log(f"{self.composite_sma.value=}, {self.composite_sma.initialized=}")

    def on_bar(self, bar):
        self.user_log(f"bar ts_init = {unix_nanos_to_iso8601(bar.ts_init)}, {bar.ts_init}")
        self.user_log(bar)

        # self.user_log(f"{self.external_sma.value=}, {self.external_sma.initialized=}")
        # self.user_log(f"{self.composite_sma.value=}, {self.composite_sma.initialized=}")

    def user_log(self, msg):
        self.log.warning(str(msg), color=LogColor.GREEN)

    def on_stop(self):
        if self.config.data_type == "bars":
            self.unsubscribe_bars(self.external_bar_type)
            self.unsubscribe_bars(self.bar_type_1)
            self.unsubscribe_bars(self.bar_type_2)
            self.unsubscribe_bars(self.bar_type_3)
        elif self.config.data_type in ["quote", "trades"]:
            self.unsubscribe_bars(self.bar_type_1)
            self.unsubscribe_bars(self.bar_type_2)


# %% [markdown]
# ## backtest node

# %%
# BacktestEngineConfig
tested_market_data = "bars"  # "bars" | "quotes" | "trades"

historical_start_delay = 10 if tested_market_data == "bars" else 2
historical_end_delay = 1 if tested_market_data == "bars" else 0

backtest_start = "2024-07-01T23:55" if tested_market_data == "bars" else "2024-07-02T00:00"
backtest_end = "2024-07-02T00:10" if tested_market_data == "bars" else "2024-07-02T00:02"

strategies = [
    ImportableStrategyConfig(
        strategy_path=TestHistoricalAggStrategy.fully_qualified_name(),
        config_path=TestHistoricalAggConfig.fully_qualified_name(),
        config={
            "symbol_id": InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
            "historical_start_delay": historical_start_delay,
            "historical_end_delay": historical_end_delay,
            "data_type": tested_market_data,
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
    log_file_name="databento_test_request_bars",
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
    time_bars_origin_offset={
        BarAggregation.MINUTE: pd.Timedelta(seconds=0),
    },
    time_bars_build_delay=20,
    # default is 15 when using composite bars aggregating internal bars
    # also useful in live context to account for network delay
)

engine_config = BacktestEngineConfig(
    strategies=strategies,
    logging=logging,
    catalogs=catalogs,
    data_engine=data_engine,
)

# BacktestRunConfig

data = []

if tested_market_data == "bars":
    data.append(
        BacktestDataConfig(
            data_cls=Bar,
            catalog_path=catalog.path,
            instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
            bar_spec="1-MINUTE-LAST",
            start_time=backtest_start,
            end_time=backtest_end,
        ),
    )
elif tested_market_data == "quotes":
    data.append(
        BacktestDataConfig(
            data_cls=QuoteTick,
            catalog_path=catalog.path,
            instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
            start_time=backtest_start,
            end_time=backtest_end,
        ),
    )
elif tested_market_data == "trades":
    data.append(
        BacktestDataConfig(
            data_cls=TradeTick,
            catalog_path=catalog.path,
            instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.XCME"),
            start_time=backtest_start,
            end_time=backtest_end,
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
        data=data,
        venues=venues,
        chunk_size=None,  # use None when loading custom data
        start=backtest_start,
        end=backtest_end,
    ),
]

node = BacktestNode(configs=configs)

# %%
results = node.run()

# %%
