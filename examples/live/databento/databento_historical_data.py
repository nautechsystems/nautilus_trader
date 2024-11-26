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

# %%

# %% [markdown]
# ## imports

# %%
# Note: Use the python extension jupytext to be able to open this python file in jupyter as a notebook

# %%
# from nautilus_trader.backtest.node import BacktestNode
# from nautilus_trader.common.enums import LogColor
# from nautilus_trader.config import BacktestDataConfig
# from nautilus_trader.config import BacktestEngineConfig
# from nautilus_trader.config import BacktestRunConfig
# from nautilus_trader.config import BacktestVenueConfig
# from nautilus_trader.config import ImportableActorConfig
# from nautilus_trader.config import ImportableStrategyConfig
# from nautilus_trader.config import LoggingConfig
# from nautilus_trader.config import StrategyConfig
# from nautilus_trader.config import StreamingConfig
# from nautilus_trader.core.datetime import unix_nanos_to_str
# from nautilus_trader.model.data import Bar
# from nautilus_trader.model.data import BarType
# from nautilus_trader.model.data import QuoteTick
# from nautilus_trader.model.enums import OrderSide
# from nautilus_trader.model.greeks import GreeksData
# from nautilus_trader.model.identifiers import InstrumentId
# from nautilus_trader.model.identifiers import Venue
# from nautilus_trader.model.objects import Price
# from nautilus_trader.model.objects import Quantity
# from nautilus_trader.risk.greeks import GreeksCalculator
# from nautilus_trader.risk.greeks import GreeksCalculatorConfig
# from nautilus_trader.risk.greeks import InterestRateProvider
# from nautilus_trader.risk.greeks import InterestRateProviderConfig
# from nautilus_trader.trading.strategy import Strategy
from typing import Any

from nautilus_trader.adapters.databento import DATABENTO
from nautilus_trader.adapters.databento import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento import DatabentoDataClientConfig
from nautilus_trader.adapters.databento import DatabentoLiveDataClientFactory
from nautilus_trader.adapters.databento.data_utils import databento_data
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
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
class DataSubscriberConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``DataSubscriber`` instances.

    Parameters
    ----------
    instrument_ids : list[InstrumentId]
        The instrument IDs to subscribe to.

    """

    instrument_ids: list[InstrumentId]


class DataSubscriber(Strategy):
    """
    An example strategy which subscribes to live data.

    Parameters
    ----------
    config : DataSubscriberConfig
        The configuration for the instance.

    """

    def __init__(self, config: DataSubscriberConfig) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_ids = config.instrument_ids

    def on_start(self) -> None:
        """
        Actions to be performed when the strategy is started.

        Here we specify the 'DATABENTO' client_id for subscriptions.

        """
        for instrument_id in self.instrument_ids:
            # from nautilus_trader.model.enums import BookType

            # self.subscribe_order_book_deltas(
            #     instrument_id=instrument_id,
            #     book_type=BookType.L3_MBO,
            #     client_id=DATABENTO_CLIENT_ID,
            # )
            # self.subscribe_order_book_at_interval(
            #     instrument_id=instrument_id,
            #     book_type=BookType.L2_MBP,
            #     depth=10,
            #     client_id=DATABENTO_CLIENT_ID,
            #     interval_ms=1000,
            # )

            self.subscribe_quote_ticks(instrument_id, client_id=DATABENTO_CLIENT_ID)
            self.subscribe_trade_ticks(instrument_id, client_id=DATABENTO_CLIENT_ID)
            # self.subscribe_instrument_status(instrument_id, client_id=DATABENTO_CLIENT_ID)
            # self.request_quote_ticks(instrument_id)
            # self.request_trade_ticks(instrument_id)

            # from nautilus_trader.model.data import DataType
            # from nautilus_trader.model.data import InstrumentStatus
            #
            # status_data_type = DataType(
            #     type=InstrumentStatus,
            #     metadata={"instrument_id": instrument_id},
            # )
            # self.request_data(status_data_type, client_id=DATABENTO_CLIENT_ID)

            # from nautilus_trader.model.data import BarType
            # self.request_bars(BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"))

            # # Imbalance
            # from nautilus_trader.adapters.databento import DatabentoImbalance
            #
            # metadata = {"instrument_id": instrument_id}
            # self.request_data(
            #     data_type=DataType(type=DatabentoImbalance, metadata=metadata),
            #     client_id=DATABENTO_CLIENT_ID,
            # )

            # # Statistics
            # from nautilus_trader.adapters.databento import DatabentoStatistics
            #
            # metadata = {"instrument_id": instrument_id}
            # self.subscribe_data(
            #     data_type=DataType(type=DatabentoStatistics, metadata=metadata),
            #     client_id=DATABENTO_CLIENT_ID,
            # )
            # self.request_data(
            #     data_type=DataType(type=DatabentoStatistics, metadata=metadata),
            #     client_id=DATABENTO_CLIENT_ID,
            # )

        # self.request_instruments(venue=Venue("GLBX"), client_id=DATABENTO_CLIENT_ID)
        # self.request_instruments(venue=Venue("XCHI"), client_id=DATABENTO_CLIENT_ID)
        # self.request_instruments(venue=Venue("XNAS"), client_id=DATABENTO_CLIENT_ID)

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        # Databento does not support live data unsubscribing

    def on_historical_data(self, data: Any) -> None:
        self.log.info(repr(data), LogColor.CYAN)

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when the strategy is running and receives order book
        deltas.

        Parameters
        ----------
        deltas : OrderBookDeltas
            The order book deltas received.

        """
        self.log.info(repr(deltas), LogColor.CYAN)

    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when an order book update is received.
        """
        self.log.info(f"\n{order_book.instrument_id}\n{order_book.pprint(10)}", LogColor.CYAN)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        self.log.info(repr(tick), LogColor.CYAN)

    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        """
        self.log.info(repr(tick), LogColor.CYAN)


# %% [markdown]
# ## backtest node

# %%
# For correct subscription operation, you must specify all instruments to be immediately
# subscribed for as part of the data client configuration
instrument_ids = [
    InstrumentId.from_str("ES.c.0.GLBX"),
    # InstrumentId.from_str("ES.FUT.GLBX"),
    # InstrumentId.from_str("CL.FUT.GLBX"),
    # InstrumentId.from_str("LO.OPT.GLBX"),
    # InstrumentId.from_str("AAPL.XNAS"),
]

# %%
# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Not applicable
        inflight_check_interval_ms=0,  # Not applicable
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    data_clients={
        DATABENTO: DatabentoDataClientConfig(
            api_key=None,  # 'DATABENTO_API_KEY' env var
            http_gateway=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_ids=instrument_ids,
            parent_symbols={"GLBX.MDP3": {"ES.FUT"}},
            mbo_subscriptions_delay=10.0,
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,  # Not applicable
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=0.0,  # Not required as no order state
)

# %%
# Instantiate the node with a configuration
node = TradingNode(config=config_node)

strat_config = DataSubscriberConfig(instrument_ids=instrument_ids)
strategy = DataSubscriber(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(DATABENTO, DatabentoLiveDataClientFactory)
node.build()

# %%
node.run()

# %%
node.dispose()

# %%
