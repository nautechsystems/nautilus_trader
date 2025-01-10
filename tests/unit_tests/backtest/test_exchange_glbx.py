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
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading import Strategy


_ESH4_GLBX = TestInstrumentProvider.es_future(2024, 3)


class TestSimulatedExchangeGlbx:
    def setup(self) -> None:
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
            venue=Venue("GLBX"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(10),
            leverages={},
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            latency_model=LatencyModel(0),
        )
        self.exchange.add_instrument(_ESH4_GLBX)

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.cache.add_instrument(_ESH4_GLBX)

        # Create mock strategy
        self.strategy = Strategy()
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

    def test_repr(self) -> None:
        # Arrange, Act, Assert
        assert (
            repr(self.exchange)
            == "SimulatedExchange(id=GLBX, oms_type=HEDGING, account_type=MARGIN)"
        )

    def test_process_order_within_expiration_submits(self) -> None:
        # Arrange: Prepare market
        one_nano_past_activation = _ESH4_GLBX.activation_ns + 1
        tick = TestDataStubs.quote_tick(
            instrument=_ESH4_GLBX,
            bid_price=4010.00,
            ask_price=4011.00,
            ts_init=one_nano_past_activation,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            _ESH4_GLBX.id,
            OrderSide.BUY,
            Quantity.from_int(10),
            Price.from_str("4000.00"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(one_nano_past_activation)

        # Assert
        assert self.clock.timestamp_ns() == 1_630_704_600_000_000_001
        assert order.status == OrderStatus.ACCEPTED

    def test_process_order_prior_to_activation_rejects(self) -> None:
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=_ESH4_GLBX,
            bid_price=4010.00,
            ask_price=4011.00,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            _ESH4_GLBX.id,
            OrderSide.BUY,
            Quantity.from_int(10),
            Price.from_str("4000.00"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert (
            order.last_event.reason
            == "Contract ESH4.GLBX not yet active, activation 2021-09-03T21:30:00.000000000Z"
        )

    def test_process_order_after_expiration_rejects(self) -> None:
        # Arrange: Prepare market
        one_nano_past_expiration = _ESH4_GLBX.expiration_ns + 1

        tick = TestDataStubs.quote_tick(
            instrument=_ESH4_GLBX,
            bid_price=4010.00,
            ask_price=4011.00,
            ts_init=one_nano_past_expiration,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            _ESH4_GLBX.id,
            OrderSide.BUY,
            Quantity.from_int(10),
            Price.from_str("4000.00"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(one_nano_past_expiration)

        # Assert
        assert self.clock.timestamp_ns() == 1_710_513_000_000_000_001
        assert order.status == OrderStatus.REJECTED
        assert (
            order.last_event.reason
            == "Contract ESH4.GLBX has expired, expiration 2024-03-15T14:30:00.000000000Z"
        )

    def test_process_exchange_past_instrument_expiration_cancels_open_order(self) -> None:
        # Arrange: Prepare market
        one_nano_past_activation = _ESH4_GLBX.activation_ns + 1
        tick = TestDataStubs.quote_tick(
            instrument=_ESH4_GLBX,
            bid_price=4010.00,
            ask_price=4011.00,
            ts_init=one_nano_past_activation,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            _ESH4_GLBX.id,
            OrderSide.BUY,
            Quantity.from_int(10),
            Price.from_str("4000.00"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(one_nano_past_activation)

        # Act
        self.exchange.get_matching_engine(_ESH4_GLBX.id).iterate(_ESH4_GLBX.expiration_ns)

        # Assert
        assert self.clock.timestamp_ns() == _ESH4_GLBX.expiration_ns == 1_710_513_000_000_000_000
        assert order.status == OrderStatus.CANCELED

    def test_process_exchange_past_instrument_expiration_closed_open_position(self) -> None:
        # Arrange: Prepare market
        one_nano_past_activation = _ESH4_GLBX.activation_ns + 1
        tick = TestDataStubs.quote_tick(
            instrument=_ESH4_GLBX,
            bid_price=4010.00,
            ask_price=4011.00,
            ts_init=one_nano_past_activation,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market(
            _ESH4_GLBX.id,
            OrderSide.BUY,
            Quantity.from_int(10),
        )

        self.strategy.submit_order(order)
        self.exchange.process(one_nano_past_activation)

        # Act
        self.exchange.get_matching_engine(_ESH4_GLBX.id).iterate(_ESH4_GLBX.expiration_ns)

        # Assert
        assert self.clock.timestamp_ns() == _ESH4_GLBX.expiration_ns == 1_710_513_000_000_000_000
        assert order.status == OrderStatus.FILLED
        position = self.cache.positions()[0]
        assert position.is_closed

    def test_process_exchange_after_expiration_not_raise_exception_when_no_open_position(
        self,
    ) -> None:
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=_ESH4_GLBX,
            bid_price=4010.00,
            ask_price=4011.00,
            ts_init=_ESH4_GLBX.expiration_ns,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market(
            _ESH4_GLBX.id,
            OrderSide.BUY,
            Quantity.from_int(10),
        )
        self.strategy.submit_order(order)
        self.exchange.process(_ESH4_GLBX.expiration_ns)
        self.strategy.close_all_positions(instrument_id=_ESH4_GLBX.id)  # <- Close position for test
        self.exchange.process(_ESH4_GLBX.expiration_ns)

        # Assert test prerequisite
        assert self.cache.positions_open_count() == 0
        assert self.cache.positions_total_count() == 1

        # Act
        one_nano_past_expiration = _ESH4_GLBX.expiration_ns + 1
        self.exchange.process(one_nano_past_expiration)
        self.exchange.get_matching_engine(_ESH4_GLBX.id).iterate(_ESH4_GLBX.expiration_ns)

        # Assert
        assert self.clock.timestamp_ns() == _ESH4_GLBX.expiration_ns == 1_710_513_000_000_000_000
