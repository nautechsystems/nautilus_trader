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
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import OrderList
from nautilus_trader.trading import Strategy
from nautilus_trader.trading.config import StrategyConfig


# %%
class Strat_mre_config(StrategyConfig, frozen=True):
    tradable_instrument_id: str | None = "NQZ5.CME"


class Strat_mre(Strategy):
    def __init__(self, config: Strat_mre_config) -> None:
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

                self.buy_bracket(instrument.id, instrument.make_price(6650))

                self.clock.set_time_alert(
                    "sl",
                    self.clock.utc_now() + pd.Timedelta(seconds=10),
                    lambda event: self.modify_sl(instrument.id, instrument.make_price(6600)),
                )

    def buy_bracket(self, instrument_id, low):
        instrument = self.cache.instrument(instrument_id)

        if not instrument:
            self.log.error(f"No instrument loaded for instrument id : {instrument_id}")
            sys.exit(1)

        order_list: OrderList = self.order_factory.bracket(
            instrument_id=instrument_id,
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(2),
            time_in_force=TimeInForce.GTC,
            entry_post_only=False,
            contingency_type=ContingencyType.OCO,
            sl_trigger_price=instrument.make_price(low - 10),
            sl_tags=[IBOrderTags(outsideRth=True).value],
            tp_order_type=OrderType.LIMIT,
            tp_price=instrument.make_price(low * 2),
            tp_post_only=False,
            entry_order_type=OrderType.MARKET,
            emulation_trigger=TriggerType.NO_TRIGGER,
        )
        self.log.info(f"orderlist : {order_list}")
        self.submit_order_list(order_list)

    def modify_sl(self, instrument_id, low):
        sl_order = self.get_sl_order(instrument_id)
        self.log.info(f"modifying sl order for {instrument_id} to : {low}")
        self.modify_order(sl_order, trigger_price=low)

    def get_sl_order(self, instrument_id):
        list_orders_for_instrument = self.cache.orders(instrument_id=instrument_id)

        for _order in list_orders_for_instrument:
            if _order.is_open and _order.order_type == 3:
                return _order

        self.log.error(
            f"Error : sl not found for instrument {instrument_id}\n list of orders found : {list_orders_for_instrument}",
        )
        sys.exit(1)

    # def on_bar(self, bar: Bar) -> None:
    #     dt_utc_now = self.clock().utc_now()

    #     if dt_utc_now - pd.Timedelta(seconds=90) > unix_nanos_to_dt(bar.ts_event):
    #         self.log.error(
    #             f"histo bar reception \n{bar}\n dt now : {dt_utc_now} bar dt: {unix_nanos_to_dt(bar.ts_event)}")
    #         return

    #     self.log.info(f"bar : {bar}")
    #     instrument_id = bar.bar_type.instrument_id

    #     if str(instrument_id) != self.tradable_instrument_id:
    #         return

    #     if not self.portfolio.is_flat(instrument_id):
    #         self.modify_sl(instrument_id, bar.low)
    #     else:
    #         self.buy_bracket(instrument_id, bar.low)


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
        + "_mre_modify_order.log",
        log_directory="./logs/",
        print_config=True,
    ),
    data_clients={
        IB: InteractiveBrokersDataClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=7497,
            ibg_client_id=9003,
            market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,
            instrument_provider=instrument_provider,
            use_regular_trading_hours=False,
        ),
    },
    exec_clients={
        IB: InteractiveBrokersExecClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=7497,
            ibg_client_id=9003,
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
strat_config = Strat_mre_config(tradable_instrument_id=tradable_instrument_id)
# Instantiate your strategy
strategy = Strat_mre(config=strat_config)

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
