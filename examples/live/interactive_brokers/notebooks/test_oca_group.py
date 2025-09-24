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
# Note: Use the jupytext python package to be able to open this python file in jupyter as a notebook.
# Also run `jupytext-config set-default-viewer` to open jupytext python files as notebooks by default.

# %%
import datetime
import os
import sys

import pandas as pd

# fmt: off
from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags
from nautilus_trader.adapters.interactive_brokers.config import IBMarketDataTypeEnum
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig

# fmt: on
# from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.live.config import LiveDataEngineConfig
from nautilus_trader.live.config import RoutingConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model import BarType
from nautilus_trader.model import TraderId
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading import Strategy
from nautilus_trader.trading.config import StrategyConfig


# %%
class Strat_oca_test_config(StrategyConfig, frozen=True):
    tradable_instrument_id: str | None = "ESZ5.CME"


class Strat_oca_test(Strategy):
    def __init__(self, config: Strat_oca_test_config) -> None:
        super().__init__(config)
        self.bar_type_m1: dict[InstrumentId, BarType] = {}
        self.tradable_instrument_id = config.tradable_instrument_id

    def on_start(self) -> None:
        self.log.info(f"instrument_id in cache : {self.cache.instrument_ids()}")
        self.log.info(f"instruments in cache : {self.cache.instruments()}")

        for instrument in self.cache.instruments():
            if str(instrument.id) == self.tradable_instrument_id:
                self.log.info(
                    f"instrument {instrument.info['contract']['tradingClass']}: \n{instrument}",
                )
                self.bar_type_m1[instrument.id] = BarType.from_str(
                    str(instrument.id) + "-1-MINUTE-LAST-EXTERNAL",
                )
                self.log.info(f"subscribing to : {self.bar_type_m1[instrument.id]}")
                # self.subscribe_bars(self.bar_type_m1[instrument.id])

                # Test explicit OCA groups - create two separate orders with explicit OCA group
                self.create_oca_orders(instrument.id)

                self.clock.set_time_alert(
                    "modify_test",
                    self.clock.utc_now() + pd.Timedelta(seconds=10),
                    lambda event: self.test_oca_modification(instrument.id),
                )

    def create_oca_orders(self, instrument_id):
        """
        Create two separate orders with explicit OCA group to test OCA functionality.
        """
        instrument = self.cache.instrument(instrument_id)

        if not instrument:
            self.log.error(f"No instrument loaded for instrument id : {instrument_id}")
            sys.exit(1)

        # Create explicit OCA group name
        oca_group_name = f"TEST_OCA_{self.clock.utc_now().strftime('%H%M%S')}"

        # Create first order with explicit OCA group
        order1 = self.order_factory.stop_market(
            instrument_id=instrument_id,
            order_side=OrderSide.SELL,
            quantity=instrument.make_qty(1),
            trigger_price=instrument.make_price(6600),
            time_in_force=TimeInForce.GTC,
            tags=[
                IBOrderTags(ocaGroup=oca_group_name, ocaType=1).value,
            ],  # ocaType=1 means cancel all others
        )

        # Create second order with same OCA group
        order2 = self.order_factory.limit(
            instrument_id=instrument_id,
            order_side=OrderSide.SELL,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(6800),
            time_in_force=TimeInForce.GTC,
            tags=[IBOrderTags(ocaGroup=oca_group_name, ocaType=1).value],  # Same OCA group
        )

        self.log.info(f"Creating OCA orders with group: {oca_group_name}")
        self.log.info(f"Order 1 (Stop): {order1}")
        self.log.info(f"Order 2 (Limit): {order2}")

        # Submit both orders
        self.submit_order(order1)
        self.submit_order(order2)

    def test_oca_modification(self, instrument_id):
        """
        Test if we can modify orders that are part of explicit OCA groups.
        """
        instrument = self.cache.instrument(instrument_id)
        list_orders_for_instrument = self.cache.orders(instrument_id=instrument_id)

        self.log.info(f"Testing OCA modification for {instrument_id}")
        self.log.info(f"Found {len(list_orders_for_instrument)} orders")

        # Find the stop order to modify
        stop_order = None
        for order in list_orders_for_instrument:
            if order.is_open and order.order_type == OrderType.STOP_MARKET:
                stop_order = order
                break

        if stop_order:
            new_trigger_price = instrument.make_price(6550)
            self.log.info(
                f"Attempting to modify OCA stop order from {stop_order.trigger_price} to {new_trigger_price}",
            )
            try:
                self.modify_order(stop_order, trigger_price=new_trigger_price)
                self.log.info("OCA order modification command sent successfully")
            except Exception as e:
                self.log.error(f"Failed to modify OCA order: {e}")
        else:
            self.log.error("No stop order found to modify")


# %%
es_contract = IBContract(
    secType="FUT",
    exchange="CME",
    localSymbol="ESZ5",
    lastTradeDateOrContractMonth="20251219",
)

contracts = [es_contract]
tradable_instrument_id = "ESZ5.CME"


# Configure the trading node
instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    load_contracts=frozenset(contracts),
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
)

config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        log_level_file="INFO",
        log_file_name=datetime.datetime.strftime(
            datetime.datetime.now(datetime.UTC),
            "%Y-%m-%d_%H-%M",
        )
        + "_oca_test.log",
        log_directory="./logs/",
        print_config=True,
    ),
    data_clients={
        IB: InteractiveBrokersDataClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=7497,
            ibg_client_id=9004,  # Different client ID to avoid conflicts
            market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,
            instrument_provider=instrument_provider,
            use_regular_trading_hours=False,
        ),
    },
    exec_clients={
        IB: InteractiveBrokersExecClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=7497,
            ibg_client_id=9004,  # Different client ID to avoid conflicts
            account_id=os.environ.get("TWS_ACCOUNT"),
            instrument_provider=instrument_provider,
            routing=RoutingConfig(
                default=True,
            ),
        ),
    },
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,  # Will use opening time as `ts_event` (same like IB)
        validate_data_sequence=True,  # Will make sure DataEngine discards any Bars received out of sequence
        time_bars_build_with_no_updates=False,
    ),
)
strat_config = Strat_oca_test_config(tradable_instrument_id=tradable_instrument_id)
# Instantiate your strategy
strategy = Strat_oca_test(config=strat_config)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(IB, InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory(IB, InteractiveBrokersLiveExecClientFactory)
node.build()

# %%
node.run()

# %%
node.stop()

# %%
node.dispose()

# %%
