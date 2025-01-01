# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestSimulatedExchange:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            clock=self.clock,
            cache=self.cache,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=RiskEngineConfig(debug=True),
        )

        self.exchange = SimulatedExchange(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={AUDUSD_SIM.id: Decimal(10)},
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            latency_model=LatencyModel(0),
            reject_stop_orders=False,
        )
        self.exchange.add_instrument(USDJPY_SIM)

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.cache.add_instrument(USDJPY_SIM)

        # Create mock strategy
        self.strategy = MockStrategy(bar_type=TestDataStubs.bartype_usdjpy_1min_bid())
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Start components
        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_submit_stop_limit_buy_order_when_marketable_then_fills(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=90.002,
            ask_price=90.005,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.100"),  # <-- immediately filled
            trigger_price=Price.from_str("90.000"),  # <-- immediately triggered
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.avg_px == 90.005  # <-- fills at ASK
        assert order.liquidity_side == LiquiditySide.TAKER
        assert len(self.exchange.get_open_orders()) == 0

    def test_submit_stop_limit_sell_order_when_marketable_then_fills(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=90.002,
            ask_price=90.005,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            price=Price.from_str("90.000"),  # <-- immediately filled
            trigger_price=Price.from_str("90.010"),  # <-- immediately triggered
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.avg_px == 90.002  # <-- fills at BID
        assert order.liquidity_side == LiquiditySide.TAKER
        assert len(self.exchange.get_open_orders()) == 0

    def test_process_quote_tick_fills_buy_stop_limit_order_passively(self):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=90.002,
            ask_price=90.005,
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.100"),
            trigger_price=Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid_price=Price.from_str("90.010"),
            ask_price=Price.from_str("90.011"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0
        assert order.avg_px == 90.010  # <-- fills at triggered price
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)

    def test_process_quote_tick_fills_sell_stop_limit_order_passively(self):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=90.002,
            ask_price=90.005,
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            price=Price.from_str("89.900"),
            trigger_price=Price.from_str("90.000"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid_price=Price.from_str("89.998"),
            ask_price=Price.from_str("89.999"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0
        assert order.avg_px == 90.000  # <-- fills at triggered price
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)
