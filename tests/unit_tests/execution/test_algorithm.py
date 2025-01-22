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

from datetime import timedelta
from decimal import Decimal

import pytest

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.component import TimeEventHandler
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import ImportableExecAlgorithmConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.algorithms.twap import TWAPExecAlgorithm
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()
FAUX_AAPL_BINANCE = TestInstrumentProvider.equity("AAPL", "BINANCE")


class TestExecAlgorithm:
    def setup(self) -> None:
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache_db = MockCacheDatabase()
        self.cache = Cache(database=self.cache_db)
        self.cache.add_instrument(ETHUSDT_PERP_BINANCE)
        self.cache.add_instrument(FAUX_AAPL_BINANCE)

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=DataEngineConfig(debug=True),
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

        self.emulator = OrderEmulator(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.exchange.add_instrument(ETHUSDT_PERP_BINANCE)

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.cache.add_instrument(ETHUSDT_PERP_BINANCE)

        update = TestEventStubs.margin_account_state(account_id=AccountId("BINANCE-001"))
        self.portfolio.update_account(update)

        config = StrategyConfig(manage_gtd_expiry=True)
        self.strategy = Strategy(config)
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine.start()
        self.risk_engine.start()
        self.exec_engine.start()
        self.emulator.start()
        self.strategy.start()

    def test_exec_algorithm_reset(self) -> None:
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exec_algorithm.start()
        exec_algorithm.stop()

        # Act, Assert
        exec_algorithm.reset()

    def test_exec_algorithm_to_importable_config(self) -> None:
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()

        # Act
        config = exec_algorithm.to_importable_config()

        # Assert
        assert isinstance(config, ImportableExecAlgorithmConfig)
        assert config.dict() == {
            "exec_algorithm_path": "nautilus_trader.examples.algorithms.twap:TWAPExecAlgorithm",
            "config_path": "nautilus_trader.examples.algorithms.twap:TWAPExecAlgorithmConfig",
            "config": {
                "exec_algorithm_id": ExecAlgorithmId("TWAP"),
                "log_events": True,
                "log_commands": True,
            },
        }

    def test_exec_algorithm_spawn_market_order_with_quantity_too_high(self) -> None:
        """
        Test that an exception is raised when more than the primary quantity attempts to
        be spawned.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exec_algorithm.start()

        primary_order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(Decimal("1")),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params={"horizon_secs": 2, "interval_secs": 1},
        )

        # Act, Assert
        with pytest.raises(ValueError):
            exec_algorithm.spawn_market(
                primary=primary_order,
                quantity=ETHUSDT_PERP_BINANCE.make_qty(Decimal("2")),  # <-- Greater than primary
                time_in_force=TimeInForce.FOK,
                reduce_only=True,
                tags=["EXIT"],
            )

    def test_exec_algorithm_spawn_market_order(self) -> None:
        """
        Test that the primary order was reduced and the spawned order has the expected
        properties.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
            tags=["EXIT"],
        )

        # Assert
        assert primary_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        assert primary_order.is_active_local
        assert spawned_order.is_active_local
        assert spawned_order.client_order_id.value == primary_order.client_order_id.value + "-E1"
        assert spawned_order.order_type == OrderType.MARKET
        assert spawned_order.quantity == spawned_qty
        assert spawned_order.time_in_force == TimeInForce.FOK
        assert spawned_order.is_reduce_only
        assert spawned_order.tags == ["EXIT"]

    def test_exec_algorithm_spawn_limit_order(self) -> None:
        """
        Test that the primary order was reduced and the spawned order has the expected
        properties.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
            tags=["ENTRY"],
        )

        # Assert
        assert primary_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        assert primary_order.is_active_local
        assert spawned_order.is_active_local
        assert spawned_order.client_order_id.value == primary_order.client_order_id.value + "-E1"
        assert spawned_order.order_type == OrderType.LIMIT
        assert spawned_order.quantity == spawned_qty
        assert spawned_order.time_in_force == TimeInForce.DAY
        assert not spawned_order.is_reduce_only
        assert spawned_order.tags == ["ENTRY"]
        assert primary_order.is_primary
        assert not primary_order.is_spawned
        assert not spawned_order.is_primary
        assert spawned_order.is_spawned

    def test_exec_algorithm_spawn_market_to_limit_order(self) -> None:
        """
        Test that the primary order was reduced and the spawned order has the expected
        properties.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
            tags=["ENTRY"],
        )

        # Assert
        assert primary_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(Decimal("0.5"))
        assert primary_order.is_active_local
        assert spawned_order.is_active_local
        assert spawned_order.client_order_id.value == primary_order.client_order_id.value + "-E1"
        assert spawned_order.order_type == OrderType.MARKET_TO_LIMIT
        assert spawned_order.quantity == spawned_qty
        assert spawned_order.time_in_force == TimeInForce.GTD
        assert spawned_order.expire_time_ns == 3_600_000_000_000
        assert not spawned_order.is_reduce_only
        assert spawned_order.tags == ["ENTRY"]

    def test_exec_algorithm_modify_order_in_place(self) -> None:
        """
        Test that the primary order is modified in place.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
            tags=["ENTRY"],
        )

        new_price = ETHUSDT_PERP_BINANCE.make_price(Decimal("5001.0"))
        exec_algorithm.modify_order_in_place(primary_order, price=new_price)

        # Assert
        assert isinstance(primary_order.last_event, OrderUpdated)
        assert primary_order.price == new_price

    def test_exec_algorithm_modify_order(self) -> None:
        """
        Test that the primary order is modified.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
        """
        Test that the primary order is canceled.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
        assert not primary_order.is_active_local

    def test_exec_algorithm_on_order(self) -> None:
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
            "O-19700101-000000-000-None-1",
            "O-19700101-000000-000-None-1-E1",
            "O-19700101-000000-000-None-1-E2",
            "O-19700101-000000-000-None-1-E3",
            "O-19700101-000000-000-None-1-E4",
            "O-19700101-000000-000-None-1-E5",
            "O-19700101-000000-000-None-1-E6",
        ]

    def test_exec_algorithm_on_order_with_small_interval_and_size_precision_zero(self) -> None:
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exec_algorithm.start()

        order = self.strategy.order_factory.market(
            instrument_id=FAUX_AAPL_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("2"),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params={"horizon_secs": 0.5, "interval_secs": 0.1},
        )

        # Act
        self.strategy.submit_order(order)

        events: list[TimeEventHandler] = self.clock.advance_time(secs_to_nanos(2.0))
        for event in events:
            event.handle()

        # Assert
        spawned_orders = self.cache.orders_for_exec_spawn(order.client_order_id)
        assert self.risk_engine.command_count == 1
        assert self.exec_engine.command_count == 1
        assert len(spawned_orders) == 1
        assert [o.client_order_id.value for o in spawned_orders] == ["O-19700101-000000-000-None-1"]

    def test_exec_algorithm_on_order_list_emulated_with_entry_exec_algorithm(self) -> None:
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exec_algorithm.start()

        tick1: QuoteTick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5005.0,
            ask_price=5005.0,
            bid_size=10.000,
            ask_size=10.000,
        )

        tick2: QuoteTick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
            bid_size=10.000,
            ask_size=10.000,
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

        exec_spawn_id = original_entry_order.client_order_id

        # Act
        self.strategy.submit_order_list(bracket)

        # Trigger ENTRY order release
        self.data_engine.process(tick2)

        events: list[TimeEventHandler] = self.clock.advance_time(secs_to_nanos(3.0))
        for event in events:
            event.handle()

        transformed_entry_order = self.cache.order(original_entry_order.client_order_id)

        # Assert
        spawned_orders = self.cache.orders_for_exec_spawn(exec_spawn_id)
        assert transformed_entry_order.status == OrderStatus.SUBMITTED
        assert sl_order.status == OrderStatus.INITIALIZED
        assert tp_order.status == OrderStatus.INITIALIZED
        assert self.risk_engine.command_count == 7
        assert self.exec_engine.command_count == 7
        assert len(spawned_orders) == 7
        assert [o.client_order_id.value for o in spawned_orders] == [
            "O-19700101-000000-000-None-1",
            "O-19700101-000000-000-None-1-E1",
            "O-19700101-000000-000-None-1-E2",
            "O-19700101-000000-000-None-1-E3",
            "O-19700101-000000-000-None-1-E4",
            "O-19700101-000000-000-None-1-E5",
            "O-19700101-000000-000-None-1-E6",
        ]
        # Assert final scheduled order quantity
        assert transformed_entry_order.quantity == ETHUSDT_PERP_BINANCE.make_qty(0.004)
        assert sl_order.quantity == quantity
        assert tp_order.quantity == quantity
        assert self.cache.exec_spawn_total_quantity(exec_spawn_id) == Quantity.from_str("1.000")
        assert self.cache.exec_spawn_total_filled_qty(exec_spawn_id) == Quantity.from_str("0.000")
        assert self.cache.exec_spawn_total_leaves_qty(exec_spawn_id) == Quantity.from_str("1.000")

    def test_exec_algorithm_on_emulated_bracket_with_exec_algo_entry(self) -> None:
        """
        Test that the OTO contingent orders update as the primary order is filled.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exec_algorithm.start()

        tick1: QuoteTick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5005.0,
            ask_price=5005.0,
            bid_size=10.000,
            ask_size=10.000,
        )

        tick2: QuoteTick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
            bid_size=10.000,
            ask_size=10.000,
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
            entry_exec_algorithm_params={"horizon_secs": 2, "interval_secs": 0.5},
        )

        entry_order = bracket.orders[0]
        sl_order = bracket.orders[1]
        tp_order = bracket.orders[2]

        exec_spawn_id = entry_order.client_order_id

        # Act
        self.strategy.submit_order_list(bracket)

        # Trigger ENTRY order release
        self.data_engine.process(tick2)
        self.exchange.process(0)

        # Assert
        spawned_orders = self.cache.orders_for_exec_spawn(exec_spawn_id)
        transformed_entry_order = self.cache.order(entry_order.client_order_id)
        assert transformed_entry_order.status == OrderStatus.RELEASED
        assert sl_order.status == OrderStatus.EMULATED
        assert tp_order.status == OrderStatus.EMULATED
        assert sl_order.is_active_local
        assert tp_order.is_active_local
        assert self.exec_engine.command_count == 1
        assert self.risk_engine.command_count == 1
        assert len(spawned_orders) == 2
        assert [o.client_order_id.value for o in spawned_orders] == [
            "O-19700101-000000-000-None-1",
            "O-19700101-000000-000-None-1-E1",
        ]
        # Assert final scheduled order quantity
        assert sl_order.quantity == Quantity.from_str("0.250")
        assert tp_order.quantity == Quantity.from_str("0.250")
        assert self.cache.exec_spawn_total_quantity(exec_spawn_id) == Quantity.from_str("1.000")
        assert self.cache.exec_spawn_total_filled_qty(exec_spawn_id) == Quantity.from_str("0.250")
        assert self.cache.exec_spawn_total_leaves_qty(exec_spawn_id) == Quantity.from_str("0.750")

        # Fill more SL size
        events: list[TimeEventHandler] = self.clock.advance_time(secs_to_nanos(0.5))
        for event in events:
            event.handle()
        self.exchange.process(0)

        assert sl_order.quantity == Quantity.from_str("0.500")
        assert tp_order.quantity == Quantity.from_str("0.500")
        assert self.cache.exec_spawn_total_quantity(exec_spawn_id) == Quantity.from_str("1.000")
        assert self.cache.exec_spawn_total_filled_qty(exec_spawn_id) == Quantity.from_str("0.500")
        assert self.cache.exec_spawn_total_leaves_qty(exec_spawn_id) == Quantity.from_str("0.500")
        assert self.exec_engine.command_count == 2

        # Fill remaining SL size
        events = self.clock.advance_time(secs_to_nanos(2.0))
        for event in events:
            event.handle()
        self.exchange.process(0)

        assert sl_order.status == OrderStatus.EMULATED
        assert tp_order.status == OrderStatus.EMULATED
        assert sl_order.quantity == Quantity.from_str("1.000")
        assert tp_order.quantity == Quantity.from_str("1.000")
        assert self.cache.exec_spawn_total_quantity(exec_spawn_id) == Quantity.from_str("1.000")
        assert self.cache.exec_spawn_total_filled_qty(exec_spawn_id) == Quantity.from_str("1.000")
        assert self.cache.exec_spawn_total_leaves_qty(exec_spawn_id) == Quantity.from_str("0.000")
        assert self.exec_engine.command_count == 4

    def test_exec_algorithm_on_emulated_bracket_with_partially_multi_filled_sl(self) -> None:
        """
        Test that the TP order in an OUO contingent relationship with the SL should have
        its size reduced to the total size of the execution spawns leaves quantity.
        """
        # Arrange
        exec_algorithm = TWAPExecAlgorithm()
        exec_algorithm.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exec_algorithm.start()

        tick1: QuoteTick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5005.0,
            ask_price=5005.0,
            bid_size=10.000,
            ask_size=10.000,
        )

        tick2: QuoteTick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=5000.0,
            ask_price=5000.0,
            bid_size=10.000,
            ask_size=10.000,
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
            sl_exec_algorithm_id=exec_algorithm.id,
            tp_exec_algorithm_id=exec_algorithm.id,
            sl_exec_algorithm_params={"horizon_secs": 2, "interval_secs": 0.5},
            tp_exec_algorithm_params={"horizon_secs": 2, "interval_secs": 0.5},
        )

        entry_order = bracket.orders[0]
        sl_order = bracket.orders[1]
        tp_order = bracket.orders[2]

        # Act
        self.strategy.submit_order_list(bracket)

        # Trigger ENTRY order release
        self.data_engine.process(tick2)
        self.exchange.process(0)

        tick3: QuoteTick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=4090.0,
            ask_price=4090.0,
            bid_size=10.000,
            ask_size=10.000,
        )

        # Trigger SL order release
        self.data_engine.process(tick3)
        self.exchange.process(0)

        # Assert
        spawned_orders = self.cache.orders_for_exec_spawn(sl_order.exec_spawn_id)
        transformed_entry_order = self.cache.order(entry_order.client_order_id)
        sl_order = self.cache.order(sl_order.client_order_id)
        assert transformed_entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.RELEASED
        assert tp_order.status == OrderStatus.EMULATED
        assert self.exec_engine.command_count == 2
        assert self.risk_engine.command_count == 1
        assert len(spawned_orders) == 2
        assert [o.client_order_id.value for o in spawned_orders] == [
            "O-19700101-000000-000-None-2",
            "O-19700101-000000-000-None-2-E1",
        ]
        # Assert final scheduled order quantity
        assert sl_order.quantity == Quantity.from_str("0.750")
        assert sl_order.leaves_qty == Quantity.from_str("0.750")
        assert tp_order.quantity == Quantity.from_str("0.750")
        assert tp_order.leaves_qty == Quantity.from_str("0.750")
        assert self.cache.exec_spawn_total_quantity(sl_order.exec_spawn_id) == Quantity.from_str(
            "1.000",
        )
        assert self.cache.exec_spawn_total_filled_qty(sl_order.exec_spawn_id) == Quantity.from_str(
            "0.250",
        )
        assert self.cache.exec_spawn_total_leaves_qty(sl_order.exec_spawn_id) == Quantity.from_str(
            "0.750",
        )
        assert self.cache.exec_spawn_total_quantity(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.750",
        )
        assert self.cache.exec_spawn_total_filled_qty(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.000",
        )
        assert self.cache.exec_spawn_total_leaves_qty(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.750",
        )

        # Fill more SL size
        events: list[TimeEventHandler] = self.clock.advance_time(secs_to_nanos(0.5))
        for event in events:
            event.handle()
        self.exchange.process(0)

        assert sl_order.quantity == Quantity.from_str("0.500")
        assert sl_order.leaves_qty == Quantity.from_str("0.500")
        assert tp_order.quantity == Quantity.from_str("0.500")
        assert tp_order.leaves_qty == Quantity.from_str("0.500")
        assert self.cache.exec_spawn_total_quantity(sl_order.exec_spawn_id) == Quantity.from_str(
            "1.000",
        )
        assert self.cache.exec_spawn_total_filled_qty(sl_order.exec_spawn_id) == Quantity.from_str(
            "0.500",
        )
        assert self.cache.exec_spawn_total_leaves_qty(sl_order.exec_spawn_id) == Quantity.from_str(
            "0.500",
        )
        assert self.cache.exec_spawn_total_quantity(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.500",
        )
        assert self.cache.exec_spawn_total_filled_qty(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.000",
        )
        assert self.cache.exec_spawn_total_leaves_qty(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.500",
        )
        assert self.exec_engine.command_count == 3

        # Fill remaining SL size
        events = self.clock.advance_time(secs_to_nanos(2.0))
        for event in events:
            event.handle()
        self.exchange.process(0)

        assert sl_order.status == OrderStatus.FILLED
        assert tp_order.status == OrderStatus.CANCELED
        assert self.cache.exec_spawn_total_quantity(sl_order.exec_spawn_id) == Quantity.from_str(
            "1.000",
        )
        assert self.cache.exec_spawn_total_filled_qty(sl_order.exec_spawn_id) == Quantity.from_str(
            "1.000",
        )
        assert self.cache.exec_spawn_total_leaves_qty(sl_order.exec_spawn_id) == Quantity.from_str(
            "0.000",
        )
        assert self.cache.exec_spawn_total_quantity(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.250",
        )
        assert self.cache.exec_spawn_total_filled_qty(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.000",
        )
        assert self.cache.exec_spawn_total_leaves_qty(tp_order.exec_spawn_id) == Quantity.from_str(
            "0.250",
        )
        assert self.cache.exec_spawn_total_quantity(
            sl_order.exec_spawn_id,
            active_only=True,
        ) == Quantity.from_str("0.000")
        assert self.cache.exec_spawn_total_filled_qty(
            sl_order.exec_spawn_id,
            active_only=True,
        ) == Quantity.from_str("0.000")
        assert self.cache.exec_spawn_total_leaves_qty(
            sl_order.exec_spawn_id,
            active_only=True,
        ) == Quantity.from_str("0.000")
        assert self.cache.exec_spawn_total_quantity(
            tp_order.exec_spawn_id,
            active_only=True,
        ) == Quantity.from_str("0.000")
        assert self.cache.exec_spawn_total_filled_qty(
            tp_order.exec_spawn_id,
            active_only=True,
        ) == Quantity.from_str("0.000")
        assert self.cache.exec_spawn_total_leaves_qty(
            tp_order.exec_spawn_id,
            active_only=True,
        ) == Quantity.from_str("0.000")
        assert self.exec_engine.command_count == 5
