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

# %% [markdown]
# Note: Use the jupytext python package to be able to open this python file in jupyter as a notebook.
# Also run `jupytext-config set-default-viewer` to open jupytext python files as notebooks by default.

# %%
from nautilus_trader.adapters.databento import DATABENTO
from nautilus_trader.adapters.databento import DatabentoDataClientConfig
from nautilus_trader.adapters.databento import DatabentoLiveDataClientFactory
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.trading.strategy import Strategy


# %% [markdown]
# ## parameters

# %%
catalog_folder = "live_catalog"
catalog = load_catalog(catalog_folder)


# %% [markdown]
# ## strategy


# %%
class DataSubscriberConfig(StrategyConfig, frozen=True):
    instrument_ids: list[InstrumentId] | None = None


class DataSubscriber(Strategy):
    def __init__(self, config: DataSubscriberConfig) -> None:
        super().__init__(config)

    def on_start(self) -> None:
        start_time = time_object_to_dt("2024-05-09T10:00")
        end_time = time_object_to_dt("2024-05-09T10:05")

        self.request_quote_ticks(
            InstrumentId.from_str("ESM4.XCME"),  # or "ESM4.GLBX"
            start_time,
            end_time,
            params={"schema": "bbo-1m"},
            update_catalog=True,
        )

        # for instrument_id in self.config.instrument_ids:
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

        # self.subscribe_quote_ticks(instrument_id, client_id=DATABENTO_CLIENT_ID)
        # self.subscribe_trade_ticks(instrument_id, client_id=DATABENTO_CLIENT_ID)
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
        # Databento does not support live data unsubscribing
        pass

    def on_historical_data(self, data) -> None:
        self.log.info(repr(data), LogColor.CYAN)

    def on_order_book_deltas(self, deltas) -> None:
        self.log.info(repr(deltas), LogColor.CYAN)

    def on_order_book(self, order_book) -> None:
        self.log.info(f"\n{order_book.instrument_id}\n{order_book.pprint(10)}", LogColor.CYAN)

    def on_quote_tick(self, tick) -> None:
        self.log.info(repr(tick), LogColor.CYAN)

    def on_trade_tick(self, tick) -> None:
        self.log.info(repr(tick), LogColor.CYAN)


# %% [markdown]
# ## backtest node

# %%
# For correct subscription operation, you must specify all instruments to be immediately
# subscribed for as part of the data client configuration
instrument_ids = None

# %%
strat_config = DataSubscriberConfig(instrument_ids=instrument_ids)
strategy = DataSubscriber(config=strat_config)

catalogs = [
    DataCatalogConfig(
        path=catalog.path,
    ),
]

exec_engine = LiveExecEngineConfig(
    reconciliation=False,  # Not applicable
    inflight_check_interval_ms=0,  # Not applicable
)

logging = LoggingConfig(
    log_level="INFO",
)

data_clients: dict[str, LiveDataClientConfig] = {
    DATABENTO: DatabentoDataClientConfig(
        api_key=None,  # 'DATABENTO_API_KEY' env var
        http_gateway=None,
        instrument_provider=InstrumentProviderConfig(load_all=True),
        instrument_ids=instrument_ids,
        parent_symbols={"GLBX.MDP3": {"ES.FUT"}},
        mbo_subscriptions_delay=10.0,
    ),
}

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    catalogs=catalogs,
    exec_engine=exec_engine,
    logging=logging,
    data_clients=data_clients,
    # other settings
    timeout_connection=20.0,
    timeout_reconciliation=10.0,  # Not applicable
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=0.0,  # Not required as no order state
)

# %%
node = TradingNode(config=config_node)
node.trader.add_strategy(strategy)
node.add_data_client_factory(DATABENTO, DatabentoLiveDataClientFactory)
node.build()

# %%
node.run()

# %%
node.stop()

# %%
node.dispose()

# %%
