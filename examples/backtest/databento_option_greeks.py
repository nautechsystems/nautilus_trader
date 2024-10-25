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
from nautilus_trader.adapters.databento.data_utils import data_path
from nautilus_trader.adapters.databento.data_utils import databento_data
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import StreamingConfig
from nautilus_trader.core.datetime import unix_nanos_to_str
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.greeks import GreeksData
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.risk.greeks import GreeksCalculator
from nautilus_trader.risk.greeks import GreeksCalculatorConfig
from nautilus_trader.risk.greeks import InterestRateProvider
from nautilus_trader.risk.greeks import InterestRateProviderConfig
from nautilus_trader.trading.strategy import Strategy


# %% [markdown]
# ## parameters

# %%
# import nautilus_trader.adapters.databento.data_utils as db_data_utils
# from nautilus_trader.adapters.databento.data_utils import init_databento_client
# from option_trader import DATA_PATH, DATABENTO_API_KEY # personal library, use your own values especially for DATABENTO_API_KEY
# db_data_utils.DATA_PATH = DATA_PATH

catalog_folder = "options_catalog"
catalog = load_catalog(catalog_folder)

future_symbols = ["ESM4"]
option_symbols = ["ESM4 P5230", "ESM4 P5250"]

start_time = "2024-05-09T10:00"
end_time = "2024-05-09T10:05"

# a valid databento key can be entered here, the example below runs with already saved test data
# db_data_utils.DATABENTO_API_KEY = DATABENTO_API_KEY
# init_databento_client()

# https://databento.com/docs/schemas-and-data-formats/whats-a-schema
futures_data = databento_data(
    future_symbols,
    start_time,
    end_time,
    "ohlcv-1m",
    "futures",
    catalog_folder,
)
options_data = databento_data(
    option_symbols,
    start_time,
    end_time,
    "mbp-1",
    "options",
    catalog_folder,
)


# %% [markdown]
# ## strategy


# %%
class OptionConfig(StrategyConfig, frozen=True):
    future_id: InstrumentId
    option_id: InstrumentId
    option_id2: InstrumentId
    load_greeks: bool = False


class OptionStrategy(Strategy):
    """
    An options trading strategy that calculates and displays the portfolio greeks.

    The strategy subscribes to quote ticks for two options and a futures contract, and
    initializes a portfolio with some trades. It can optionally load greeks from a
    catalog, or compute them on the fly. The strategy logs the portfolio greeks at
    regular intervals.

    """

    def __init__(self, config: OptionConfig):
        super().__init__(config=config)

        self.future_id = config.future_id
        self.option_id = config.option_id
        self.option_id2 = config.option_id2
        self.load_greeks = config.load_greeks

        self.start_orders_done = False

    def on_start(self):
        self.subscribe_quote_ticks(self.option_id)
        self.subscribe_quote_ticks(self.option_id2)

        bar_type = BarType.from_str(f"{self.future_id}-1-MINUTE-LAST-EXTERNAL")
        self.subscribe_bars(bar_type)

    def init_portfolio(self):
        self.submit_market_order(instrument_id=self.option_id, quantity=-10)
        self.submit_market_order(instrument_id=self.option_id2, quantity=10)
        self.submit_market_order(instrument_id=self.future_id, quantity=1)

        self.start_orders_done = True

    def on_bar(self, bar):
        self.user_log(f"bar ts_init = {unix_nanos_to_str(bar.ts_init)}")

        if not self.start_orders_done:
            self.user_log("Initializing the portfolio with some trades")
            self.init_portfolio()
            return

        if self.load_greeks:
            # when greeks are loaded from a catalog a small delay is needed so all greeks are updated
            # note that loading greeks is not required, it's actually faster to just compute them every time
            self.clock.set_time_alert(
                "display greeks",
                self.clock.utc_now().replace(microsecond=100),
                self.display_greeks,
                override=True,
            )
        else:
            self.display_greeks()

    def display_greeks(self, alert=None):
        portfolio_greeks = self.portfolio_greeks()
        self.user_log(f"{portfolio_greeks=}")

    def submit_market_order(self, instrument_id, quantity):
        order = self.order_factory.market(
            instrument_id=instrument_id,
            order_side=(OrderSide.BUY if quantity > 0 else OrderSide.SELL),
            quantity=Quantity.from_int(abs(quantity)),
        )

        self.submit_order(order)

    def submit_limit_order(self, instrument_id, price, quantity):
        order = self.order_factory.limit(
            instrument_id=instrument_id,
            order_side=(OrderSide.BUY if quantity > 0 else OrderSide.SELL),
            quantity=Quantity.from_int(abs(quantity)),
            price=Price(price),
        )

        self.submit_order(order)

    def user_log(self, msg):
        self.log.warning(str(msg), color=LogColor.GREEN)


# %% [markdown]
# ## backtest node

# %%
# BacktestEngineConfig

# for saving and loading custom data greeks, use False, True then True, False below
load_greeks, stream_data = False, False

actors = [
    ImportableActorConfig(
        actor_path=GreeksCalculator.fully_qualified_name(),
        config_path=GreeksCalculatorConfig.fully_qualified_name(),
        config={
            "load_greeks": load_greeks,
        },
    ),
    ImportableActorConfig(
        actor_path=InterestRateProvider.fully_qualified_name(),
        config_path=InterestRateProviderConfig.fully_qualified_name(),
        config={
            "interest_rates_file": str(data_path(catalog_folder, "usd_short_term_rate.xml")),
        },
    ),
]

strategies = [
    ImportableStrategyConfig(
        strategy_path=OptionStrategy.fully_qualified_name(),
        config_path=OptionConfig.fully_qualified_name(),
        config={
            "future_id": InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
            "option_id": InstrumentId.from_str(f"{option_symbols[0]}.GLBX"),
            "option_id2": InstrumentId.from_str(f"{option_symbols[1]}.GLBX"),
            "load_greeks": load_greeks,
        },
    ),
]

streaming = StreamingConfig(
    catalog_path=catalog.path,
    fs_protocol="file",
    include_types=[GreeksData],
)

logging = LoggingConfig(
    bypass_logging=False,
    log_colors=True,
    log_level="WARN",
    log_level_file="WARN",
    log_directory=".",
    log_file_format=None,  # 'json' or None
    log_file_name="databento_option_greeks",
)

engine_config = BacktestEngineConfig(
    actors=actors,
    strategies=strategies,
    streaming=(streaming if stream_data else None),
    logging=logging,
)

# BacktestRunConfig

data = [
    BacktestDataConfig(
        data_cls=Bar,
        catalog_path=catalog.path,
        instrument_id=InstrumentId.from_str(f"{future_symbols[0]}.GLBX"),
        bar_spec="1-MINUTE-LAST",
        # start_time=start_time,
        # end_time=end_time,
    ),
    BacktestDataConfig(
        data_cls=QuoteTick,
        catalog_path=catalog.path,
    ),
]

if load_greeks:
    data.append(
        BacktestDataConfig(
            data_cls=GreeksData.fully_qualified_name(),
            catalog_path=catalog.path,
            client_id="GreeksDataProvider",
            metadata={"instrument_id": "ES"},
        ),
    )

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
    ),
]

node = BacktestNode(configs=configs)

# %%
results = node.run(raise_exception=True)

# %%
if stream_data:
    # 'overwrite_or_ignore' keeps existing data intact, 'delete_matching' overwrites everything, see in pyarrow/dataset.py
    catalog.convert_stream_to_data(
        results[0].instance_id,
        GreeksData,
        basename_template="part-{i}.parquet",
        partitioning=["date"],
        existing_data_behavior="overwrite_or_ignore",
    )

# %% [markdown]
# ## backtest results

# %%
engine = node.get_engine(configs[0].id)
engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()

# %%
engine.trader.generate_account_report(Venue("GLBX"))
