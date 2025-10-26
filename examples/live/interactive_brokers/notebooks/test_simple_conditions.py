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
from nautilus_trader.core.uuid import UUID4

# fmt: on
from nautilus_trader.live.config import LiveDataEngineConfig
from nautilus_trader.live.config import RoutingConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model import BarType
from nautilus_trader.model import TraderId
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.trading import Strategy
from nautilus_trader.trading.config import StrategyConfig


# %%
class SimpleConditionsConfig(StrategyConfig, frozen=True):
    tradable_instrument_id: str | None = "ESZ5.CME"


class SimpleConditionsStrategy(Strategy):
    def __init__(self, config: SimpleConditionsConfig) -> None:
        super().__init__(config)
        self.bar_type_m1: dict[InstrumentId, BarType] = {}
        self.tradable_instrument_id = config.tradable_instrument_id
        self.order_count = 0

    def on_start(self) -> None:
        self.log.info(f"instrument_id in cache : {self.cache.instrument_ids()}")
        self.log.info(f"instruments in cache : {self.cache.instruments()}")

        for instrument in self.cache.instruments():
            if str(instrument.id) == self.tradable_instrument_id:
                self.log.info(
                    f"instrument {instrument.info['contract']['tradingClass']}: \n{instrument}",
                )

                # Test all condition types
                self.test_volume_condition_order(instrument)
                self.test_time_condition_order(instrument)
                self.test_execution_condition_order(instrument)
                self.test_margin_condition_order(instrument)
                self.test_percent_change_condition_order(instrument)
                self.test_price_condition_order(instrument)

    def test_price_condition_order(self, instrument):
        """
        Test a simple limit order with price condition.
        """
        self.order_count += 1

        # Get the actual contract ID from the instrument
        contract_id = instrument.info.get("contract", {}).get("conId", 495512563)

        # Price condition: trigger when ES goes above 6000
        price_condition = {
            "type": "price",
            "conId": contract_id,  # Use actual ES contract ID
            "exchange": "CME",
            "isMore": True,
            "price": 6000.0,
            "triggerMethod": 0,
            "conjunction": "and",
        }

        order_tags = IBOrderTags(
            conditions=[price_condition],
            conditionsCancelOrder=False,  # Transmit order when condition is met
        )

        order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument.id,
            client_order_id=self.order_factory.generate_client_order_id(),
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(5950),  # Below current market
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            tags=[order_tags.value],
        )

        self.log.info(f"Submitting PRICE CONDITION order: {order}")
        self.submit_order(order)

    def test_time_condition_order(self, instrument):
        """
        Test a simple limit order with time condition.
        """
        self.order_count += 1

        # Time condition: trigger 5 minutes from now
        # IB accepts two formats:
        # 1. "yyyymmdd hh:mm:ss US/Eastern" (with timezone)
        # 2. "yyyymmddd-hh:mm:ss" (UTC with dash)
        future_time = datetime.datetime.now() + datetime.timedelta(minutes=5)

        # Try UTC format with dash (as mentioned in IB error message)
        time_str = future_time.strftime("%Y%m%d-%H:%M:%S")
        self.log.info(f"Time condition string (UTC format): '{time_str}'")

        time_condition = {
            "type": "time",
            "time": time_str,
            "isMore": True,
            "conjunction": "and",
        }

        order_tags = IBOrderTags(
            conditions=[time_condition],
            conditionsCancelOrder=False,
        )

        order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument.id,
            client_order_id=self.order_factory.generate_client_order_id(),
            order_side=OrderSide.SELL,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(6100),  # Above current market
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            tags=[order_tags.value],
        )

        self.log.info(f"Submitting TIME CONDITION order (triggers at {time_str}): {order}")
        self.submit_order(order)

    def test_volume_condition_order(self, instrument):
        """
        Test a simple limit order with volume condition.
        """
        self.order_count += 1

        # Get the actual contract ID from the instrument
        contract_id = instrument.info.get("contract", {}).get("conId", 495512563)

        # Volume condition: trigger when volume exceeds 100,000
        volume_condition = {
            "type": "volume",
            "conId": contract_id,  # Use actual ES contract ID
            "exchange": "CME",
            "isMore": True,
            "volume": 100000,
            "conjunction": "and",
        }

        order_tags = IBOrderTags(
            conditions=[volume_condition],
            conditionsCancelOrder=True,  # Cancel order when condition is met
        )

        order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument.id,
            client_order_id=self.order_factory.generate_client_order_id(),
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(5900),  # Below current market
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            tags=[order_tags.value],
        )

        self.log.info(f"Submitting VOLUME CONDITION order: {order}")
        self.submit_order(order)

    def test_execution_condition_order(self, instrument):
        """
        Test a simple limit order with execution condition.
        """
        self.order_count += 1

        # Execution condition: trigger when another symbol executes
        execution_condition = {
            "type": "execution",
            "symbol": "SPY",
            "secType": "STK",
            "exchange": "SMART",
            "conjunction": "and",
        }

        order_tags = IBOrderTags(
            conditions=[execution_condition],
            conditionsCancelOrder=False,
        )

        order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument.id,
            client_order_id=self.order_factory.generate_client_order_id(),
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(5800),  # Below current market
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            tags=[order_tags.value],
        )

        self.log.info(f"Submitting EXECUTION CONDITION order: {order}")
        self.submit_order(order)

    def test_margin_condition_order(self, instrument):
        """
        Test a simple limit order with margin condition.
        """
        self.order_count += 1

        # Margin condition: trigger when margin cushion is greater than 75%
        margin_condition = {
            "type": "margin",
            "percent": 75,
            "isMore": True,
            "conjunction": "and",
        }

        order_tags = IBOrderTags(
            conditions=[margin_condition],
            conditionsCancelOrder=False,
        )

        order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument.id,
            client_order_id=self.order_factory.generate_client_order_id(),
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(5700),  # Below current market
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            tags=[order_tags.value],
        )

        self.log.info(f"Submitting MARGIN CONDITION order: {order}")
        self.submit_order(order)

    def test_percent_change_condition_order(self, instrument):
        """
        Test a simple limit order with percent change condition.
        """
        self.order_count += 1

        # Get contract ID from instrument
        contract_id = instrument.info.get("contract", {}).get("conId", 495512563)

        # Percent change condition: trigger when contract increases by 5%
        percent_change_condition = {
            "type": "percent_change",
            "conId": contract_id,
            "exchange": "CME",
            "changePercent": 5.0,
            "isMore": True,
            "conjunction": "and",
        }

        order_tags = IBOrderTags(
            conditions=[percent_change_condition],
            conditionsCancelOrder=False,
        )

        order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument.id,
            client_order_id=self.order_factory.generate_client_order_id(),
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(5600),  # Below current market
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            tags=[order_tags.value],
        )

        self.log.info(f"Submitting PERCENT CHANGE CONDITION order: {order}")
        self.submit_order(order)

    def on_order_submitted(self, event):
        self.log.info(f"Order submitted: {event}")

    def on_order_accepted(self, event):
        self.log.info(f"Order accepted: {event}")

    def on_order_rejected(self, event):
        self.log.error(f"Order rejected: {event}")

    def on_order_canceled(self, event):
        self.log.info(f"Order canceled: {event}")

    def on_order_filled(self, event):
        self.log.info(f"Order filled: {event}")


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
    trader_id=TraderId("CONDITIONS-TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        log_level_file="INFO",
        log_file_name=datetime.datetime.strftime(
            datetime.datetime.now(datetime.UTC),
            "%Y-%m-%d_%H-%M",
        )
        + "_simple_conditions_test.log",
        log_directory="./logs/",
        print_config=True,
    ),
    data_clients={
        IB: InteractiveBrokersDataClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=7497,
            ibg_client_id=9005,  # Different client ID
            market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,
            instrument_provider=instrument_provider,
            use_regular_trading_hours=False,
        ),
    },
    exec_clients={
        IB: InteractiveBrokersExecClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=7497,
            ibg_client_id=9005,  # Different client ID
            account_id=os.environ.get("TWS_ACCOUNT"),
            instrument_provider=instrument_provider,
            routing=RoutingConfig(
                default=True,
            ),
        ),
    },
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,
        validate_data_sequence=True,
        time_bars_build_with_no_updates=False,
    ),
)

strat_config = SimpleConditionsConfig(tradable_instrument_id=tradable_instrument_id)
strategy = SimpleConditionsStrategy(config=strat_config)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node
node.add_data_client_factory(IB, InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory(IB, InteractiveBrokersLiveExecClientFactory)
node.build()

# %%
node.run()

# %%
# node.stop()

# %%
# node.dispose()

# %%
