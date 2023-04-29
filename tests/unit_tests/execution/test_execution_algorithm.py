# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import timedelta
from decimal import Decimal

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.timer import TimeEventHandler
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.algorithms.twap import TWAPExecAlgorithm
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestExecAlgorithm:
    def setup(self) -> None:
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.INFO,
            bypass=True,
        )

        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )
        self.cache.add_instrument(ETHUSDT_PERP_BINANCE)

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=DataEngineConfig(debug=True),
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=RiskEngineConfig(debug=True),
        )

        self.emulator = OrderEmulator(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("BINANCE")
        self.exchange = SimulatedExchange(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-asset wallet
            starting_balances=[Money(200, ETH), Money(1_000_000, USDT)],
            default_leverage=Decimal(10),
            leverages={},
            instruments=[ETHUSDT_PERP_BINANCE],
            modules=[],
            fill_model=FillModel(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.cache.add_instrument(ETHUSDT_PERP_BINANCE)

        update = TestEventStubs.margin_account_state(account_id=AccountId("BINANCE-001"))
        self.portfolio.update_account(update)

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine.start()
        self.risk_engine.start()
        self.exec_engine.start()
        self.emulator.start()
        self.strategy.start()

    def test_exec_algorithm_spawn_market_order(self) -> None:
        """Test that the primary order was reduced and the spawned order has the expected properties."""
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        exec_algorithm.start()

        primary_order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(Decimal("1")),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params={"horizon_secs": 2, "interval_secs": 1},
        )

        # Act
        spawned_qty = ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        spawned_order = exec_algorithm.spawn_market(
            primary=primary_order,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(spawned_qty),
            time_in_force=TimeInForce.FOK,
            reduce_only=True,
            tags="EXIT",
        )

        # Assert
        assert primary_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        assert spawned_order.client_order_id.value == primary_order.client_order_id.value + "-E1"
        assert spawned_order.order_type == OrderType.MARKET
        assert spawned_order.quantity == spawned_qty
        assert spawned_order.time_in_force == TimeInForce.FOK
        assert spawned_order.is_reduce_only
        assert spawned_order.tags == "EXIT"

    def test_exec_algorithm_spawn_limit_order(self) -> None:
        """Test that the primary order was reduced and the spawned order has the expected properties."""
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        exec_algorithm.start()

        primary_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(Decimal("1")),
            price=ETHUSDT_PERP_BINANCE.make_price(Decimal("5000.25")),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params={"horizon_secs": 2, "interval_secs": 1},
        )

        # Act
        spawned_qty = ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        spawned_order = exec_algorithm.spawn_limit(
            primary=primary_order,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(spawned_qty),
            price=ETHUSDT_PERP_BINANCE.make_price(Decimal("5000.25")),
            time_in_force=TimeInForce.DAY,
            reduce_only=False,
            tags="ENTRY",
        )

        # Assert
        assert primary_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        assert spawned_order.client_order_id.value == primary_order.client_order_id.value + "-E1"
        assert spawned_order.order_type == OrderType.LIMIT
        assert spawned_order.quantity == spawned_qty
        assert spawned_order.time_in_force == TimeInForce.DAY
        assert not spawned_order.is_reduce_only
        assert spawned_order.tags == "ENTRY"

    def test_exec_algorithm_spawn_market_to_limit_order(self) -> None:
        """Test that the primary order was reduced and the spawned order has the expected properties."""
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        exec_algorithm.start()

        primary_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(Decimal("1")),
            price=ETHUSDT_PERP_BINANCE.make_price(Decimal("5000.25")),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params={"horizon_secs": 2, "interval_secs": 1},
        )

        # Act
        spawned_qty = ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        spawned_order = exec_algorithm.spawn_market_to_limit(
            primary=primary_order,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(spawned_qty),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=60),
            reduce_only=False,
            tags="ENTRY",
        )

        # Assert
        assert primary_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        assert spawned_order.client_order_id.value == primary_order.client_order_id.value + "-E1"
        assert spawned_order.order_type == OrderType.MARKET_TO_LIMIT
        assert spawned_order.quantity == spawned_qty
        assert spawned_order.time_in_force == TimeInForce.GTD
        assert spawned_order.expire_time_ns == 3_600_000_000_000
        assert not spawned_order.is_reduce_only
        assert spawned_order.tags == "ENTRY"

    def test_exec_algorithm_modify_order_in_place(self) -> None:
        """Test that the primary order is modified in place."""
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        exec_algorithm.start()

        primary_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(Decimal("1")),
            price=ETHUSDT_PERP_BINANCE.make_price(Decimal("5000.25")),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params={"horizon_secs": 2, "interval_secs": 1},
        )

        # Act
        spawned_qty = ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        exec_algorithm.spawn_limit(
            primary=primary_order,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(spawned_qty),
            price=ETHUSDT_PERP_BINANCE.make_price(Decimal("5000.25")),
            time_in_force=TimeInForce.DAY,
            reduce_only=False,
            tags="ENTRY",
        )

        new_price = ETHUSDT_PERP_BINANCE.make_price(Decimal("5001.0"))
        exec_algorithm.modify_order_in_place(primary_order, price=new_price)

        # Assert
        assert isinstance(primary_order.last_event, OrderUpdated)
        assert primary_order.price == new_price

    def test_exec_algorithm_modify_order(self) -> None:
        """Test that the primary order is modified."""
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        exec_algorithm.start()

        primary_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(Decimal("1.000")),
            price=ETHUSDT_PERP_BINANCE.make_price(Decimal("5000.25")),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params={"horizon_secs": 2, "interval_secs": 1},
        )
        self.strategy.submit_order(primary_order)
        self.exchange.process(0)

        new_qty = ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.900"))
        new_price = ETHUSDT_PERP_BINANCE.make_price(Decimal("5001.0"))

        # Act
        exec_algorithm.modify_order_in_place(
            primary_order,
            quantity=new_qty,
            price=new_price,
        )
        self.exchange.process(0)

        # Assert
        assert isinstance(primary_order.last_event, OrderUpdated)
        assert primary_order.status == OrderStatus.INITIALIZED
        assert primary_order.price == new_price
        assert primary_order.quantity == new_qty

    def test_exec_algorithm_cancel_order(self) -> None:
        """Test that the primary order is canceled."""
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        exec_algorithm.start()

        primary_order = self.strategy.order_factory.limit(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(Decimal("1.000")),
            price=ETHUSDT_PERP_BINANCE.make_price(Decimal("5000.25")),
        )
        self.strategy.submit_order(primary_order)
        self.exchange.process(0)

        # Act
        exec_algorithm.cancel_order(primary_order)
        self.exchange.process(0)

        # Assert
        assert primary_order.status == OrderStatus.CANCELED

    def test_exec_algorithm_on_order(self) -> None:
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        exec_algorithm.start()

        order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(1),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params={"horizon_secs": 3, "interval_secs": 0.5},
        )

        # Act
        self.strategy.submit_order(order)

        events: list[TimeEventHandler] = self.clock.advance_time(secs_to_nanos(3.0))
        for event in events:
            event.handle()

        # Assert
        spawned_orders = self.cache.orders_for_exec_spawn(order.client_order_id)
        assert self.risk_engine.command_count == 7
        assert self.exec_engine.command_count == 7
        assert len(spawned_orders) == 7
        assert [o.client_order_id.value for o in spawned_orders] == [
            "O-19700101-0000-000-None-1",
            "O-19700101-0000-000-None-1-E1",
            "O-19700101-0000-000-None-1-E2",
            "O-19700101-0000-000-None-1-E3",
            "O-19700101-0000-000-None-1-E4",
            "O-19700101-0000-000-None-1-E5",
            "O-19700101-0000-000-None-1-E6",
        ]

    def test_exec_algorithm_on_order_list_emulated_with_entry_exec_algorithm(self) -> None:
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        exec_algorithm.start()

        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid=ETHUSDT_PERP_BINANCE.make_price(5005.0),
            ask=ETHUSDT_PERP_BINANCE.make_price(5005.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            ask=ETHUSDT_PERP_BINANCE.make_price(5000.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        quantity = ETHUSDT_PERP_BINANCE.make_qty(1)
        bracket: OrderList = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=quantity,
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + timedelta(seconds=30),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(5000.00),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(4090.00),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(5010.00),
            emulation_trigger=TriggerType.BID_ASK,
            entry_order_type=OrderType.MARKET_IF_TOUCHED,
            entry_exec_algorithm_id=exec_algorithm.id,
            entry_exec_algorithm_params={"horizon_secs": 3, "interval_secs": 0.5},
        )

        original_entry_order = bracket.orders[0]
        sl_order = bracket.orders[1]
        tp_order = bracket.orders[2]

        # Act
        self.strategy.submit_order_list(bracket, manage_gtd_expiry=True)

        # Trigger order from emulator
        self.data_engine.process(tick2)

        events: list[TimeEventHandler] = self.clock.advance_time(secs_to_nanos(3.0))
        for event in events:
            event.handle()

        transformed_entry_order = self.cache.order(original_entry_order.client_order_id)

        # Assert
        spawned_orders = self.cache.orders_for_exec_spawn(original_entry_order.client_order_id)
        assert transformed_entry_order.status == OrderStatus.SUBMITTED
        assert sl_order.status == OrderStatus.INITIALIZED
        assert tp_order.status == OrderStatus.INITIALIZED
        assert self.risk_engine.command_count == 7
        assert self.exec_engine.command_count == 7
        assert len(spawned_orders) == 7
        assert [o.client_order_id.value for o in spawned_orders] == [
            "O-19700101-0000-000-None-1",
            "O-19700101-0000-000-None-1-E1",
            "O-19700101-0000-000-None-1-E2",
            "O-19700101-0000-000-None-1-E3",
            "O-19700101-0000-000-None-1-E4",
            "O-19700101-0000-000-None-1-E5",
            "O-19700101-0000-000-None-1-E6",
        ]
        # Assert final scheduled order quantity
        assert transformed_entry_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(0.004)
        assert sl_order.quantity == quantity
        assert tp_order.quantity == quantity
