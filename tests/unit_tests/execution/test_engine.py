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

import pytest

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import InvalidConfiguration
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.messages import TradingCommand
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestExecutionEngine:
    def setup(self) -> None:
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache_db = MockCacheDatabase()

        self.cache = Cache(
            database=self.cache_db,
        )

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config = ExecEngineConfig(
            snapshot_orders=True,
            snapshot_positions=True,
            snapshot_positions_interval_secs=10,
            debug=True,
        )
        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Prepare components
        self.cache.add_instrument(AUDUSD_SIM)

        self.venue = Venue("SIM")
        self.exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.portfolio.update_account(TestEventStubs.margin_account_state())
        self.exec_engine.register_client(self.exec_client)

    def test_registered_clients_returns_expected(self) -> None:
        # Arrange, Act
        result = self.exec_engine.registered_clients

        # Assert
        assert result == [ClientId("SIM")]
        assert self.exec_engine.default_client is None

    def test_register_exec_client_for_routing(self) -> None:
        # Arrange
        exec_client = MockExecutionClient(
            client_id=ClientId("IB"),
            venue=None,  # Multi-venue
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        self.exec_engine.register_client(exec_client)

        # Assert
        assert self.exec_engine.default_client == exec_client.id
        assert self.exec_engine.registered_clients == [
            exec_client.id,
            self.exec_client.id,
        ]

    def test_execute_skips_commands_for_external_clients(self):
        # Arrange
        ext_client_id = ClientId("EXT_EXEC")

        msgbus = MessageBus(trader_id=self.trader_id, clock=self.clock)
        cache = Cache(database=MockCacheDatabase())

        engine = ExecutionEngine(
            msgbus=msgbus,
            cache=cache,
            clock=self.clock,
            config=ExecEngineConfig(external_clients=[ext_client_id], debug=True),
        )

        order = self.order_factory.market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )

        cmd = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            order=order,
            position_id=None,
            client_id=ext_client_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act / Assert: no error even though no client registered for EXT_EXEC
        engine.execute(cmd)

        assert engine.get_external_client_ids() == {ext_client_id}

    def test_register_venue_routing(self) -> None:
        # Arrange
        exec_client = MockExecutionClient(
            client_id=ClientId("IB"),
            venue=None,  # Multi-venue
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        self.exec_engine.register_venue_routing(exec_client, Venue("NYMEX"))

        # Assert
        assert self.exec_engine.default_client is None
        assert self.exec_engine.registered_clients == [
            exec_client.id,
            self.exec_client.id,
        ]

    def test_register_strategy_with_external_order_claims_when_claim(self) -> None:
        # Arrange
        config = StrategyConfig(external_order_claims=["ETHUSDT-PERP.DYDX"])
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        expected_instrument_id = InstrumentId.from_str("ETHUSDT-PERP.DYDX")

        # Act
        self.exec_engine.register_external_order_claims(strategy)

        # Assert
        assert self.exec_engine.get_external_order_claim(expected_instrument_id) == strategy.id
        assert self.exec_engine.get_external_order_claims_instruments() == {expected_instrument_id}

    def test_register_strategy_with_external_order_claims_when_no_claim(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument_id = InstrumentId.from_str("ETHUSDT-PERP.DYDX")

        # Act
        self.exec_engine.register_external_order_claims(strategy)

        # Assert
        assert self.exec_engine.get_external_order_claim(instrument_id) is None
        assert self.exec_engine.get_external_order_claims_instruments() == set()

    def test_register_external_order_claims_conflict(self) -> None:
        # Arrange
        config1 = StrategyConfig(
            order_id_tag="000",
            external_order_claims=["ETHUSDT-PERP.DYDX"],
        )
        strategy1 = Strategy(config=config1)
        strategy1.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        config2 = StrategyConfig(
            order_id_tag="001",
            external_order_claims=["ETHUSDT-PERP.DYDX"],  # <-- Already claimed
        )
        strategy2 = Strategy(config=config2)
        strategy2.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine.register_external_order_claims(strategy1)

        # Act, Assert
        with pytest.raises(InvalidConfiguration):
            self.exec_engine.register_external_order_claims(strategy2)

    def test_deregister_client_removes_client(self) -> None:
        # Arrange, Act
        self.exec_engine.deregister_client(self.exec_client)

        # Assert
        assert self.exec_engine.registered_clients == []

    def test_check_connected_when_client_disconnected_returns_false(self) -> None:
        # Arrange
        self.exec_client.start()
        self.exec_client.stop()

        # Act
        result = self.exec_engine.check_connected()

        # Assert
        assert not result

    def test_check_connected_when_client_connected_returns_true(self) -> None:
        # Arrange
        self.exec_client.start()

        # Act
        result = self.exec_engine.check_connected()

        # Assert
        assert result

    def test_check_disconnected_when_client_disconnected_returns_true(self) -> None:
        # Arrange, Act
        result = self.exec_engine.check_disconnected()

        # Assert
        assert result

    def test_check_disconnected_when_client_connected_returns_false(self) -> None:
        # Arrange
        self.exec_client.start()

        # Act
        result = self.exec_engine.check_disconnected()

        # Assert
        assert not result

    def test_check_integrity_calls_check_on_cache(self) -> None:
        # Arrange, Act
        result = self.exec_engine.check_integrity()

        # Assert
        assert result  # No exceptions raised

    def test_setting_of_position_id_counts(self) -> None:
        # Arrange
        strategy_id = StrategyId("S-001")
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.000"),
        )

        order.apply(TestEventStubs.order_submitted(order))

        fill1 = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-1-001"),
            strategy_id=strategy_id,
            last_px=Price.from_str("50000.00000000"),
        )

        order.apply(fill1)
        position = Position(instrument=BTCUSDT_BINANCE, fill=fill1)

        self.cache_db.add_order(order)
        self.cache_db.update_order(order)
        self.cache_db.add_position(position)

        # Act
        self.portfolio.reset()
        self.exec_engine.load_cache()

        # Assert
        assert self.exec_engine.position_id_count(strategy_id) == 1

    def test_given_random_command_logs_and_continues(self) -> None:
        # Arrange
        random = TradingCommand(
            client_id=None,
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=AUDUSD_SIM.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(random)

    def test_submit_order_with_duplicate_client_order_id_logs(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.risk_engine.execute(submit_order)  # Duplicate command

        # Assert
        assert order.status == OrderStatus.SUBMITTED

    def test_submit_order_for_random_venue_logs(self) -> None:
        # Arrange
        self.cache.add_instrument(BTCUSDT_BINANCE)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(10),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.exec_engine.command_count == 1
        assert order.status == OrderStatus.INITIALIZED

    def test_order_filled_with_unrecognized_strategy_id(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(
            TestEventStubs.order_filled(
                order,
                AUDUSD_SIM,
                strategy_id=StrategyId("RANDOM-001"),
            ),
        )

        # Assert (does not send to strategy)
        assert order.status == OrderStatus.FILLED

    def test_submit_bracket_order_list_with_all_duplicate_client_order_id_logs_does_not_submit(
        self,
    ) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        entry = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        stop_loss = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("0.50000"),
        )

        take_profit = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        bracket1 = OrderList(
            order_list_id=OrderListId("1"),
            orders=[entry, stop_loss, take_profit],
        )

        submit_order_list = SubmitOrderList(
            self.trader_id,
            strategy.id,
            bracket1,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order_list)
        self.exec_engine.process(TestEventStubs.order_submitted(entry))
        self.exec_engine.process(TestEventStubs.order_submitted(stop_loss))
        self.exec_engine.process(TestEventStubs.order_submitted(take_profit))
        self.risk_engine.execute(submit_order_list)  # <-- Duplicate command

        # Assert
        assert entry.status == OrderStatus.SUBMITTED  # Did not invalidate originals
        assert stop_loss.status == OrderStatus.SUBMITTED  # Did not invalidate originals
        assert take_profit.status == OrderStatus.SUBMITTED  # Did not invalidate originals
        assert self.exec_engine.command_count == 2

    def test_submit_order(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert submit_order in self.exec_client.commands
        assert self.cache.order_exists(order.client_order_id)

    def test_submit_order_with_cleared_cache_logs_error(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.cache.reset()
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        # Assert
        assert order.status == OrderStatus.INITIALIZED

    def test_when_applying_event_to_order_with_invalid_state_trigger_logs(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act (event attempts to fill order before its submitted)
        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        # Assert
        assert order.status == OrderStatus.INITIALIZED

    def test_duplicate_order_accepted_event_logs_debug_not_warning(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        # Order is now in ACCEPTED state
        assert order.status == OrderStatus.ACCEPTED
        initial_event_count = order.event_count

        # Process duplicate OrderAccepted event
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        # Assert
        assert order.status == OrderStatus.ACCEPTED  # Status unchanged
        assert order.event_count == initial_event_count  # Event not applied

    def test_order_filled_event_when_order_not_found_in_cache_logs(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act (event attempts to fill order before its submitted)
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        # Assert
        assert self.exec_engine.event_count == 1
        assert order.status == OrderStatus.INITIALIZED

    def test_cancel_order_for_already_closed_order_logs_and_does_nothing(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Push to OrderStatus.FILLED (closed)
        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        cancel_order = CancelOrder(
            self.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(cancel_order)

        # Assert
        assert order.status == OrderStatus.FILLED

    def test_cancel_order_then_filled_reopens_order(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Push to OrderStatus.CANCELED (closed)
        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_canceled(order))

        # Act
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.is_closed

    def test_cancel_order_then_partially_filled_reopens_order(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Push to OrderStatus.CANCELED (closed)
        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_canceled(order))

        # Act
        self.exec_engine.process(
            TestEventStubs.order_filled(order, AUDUSD_SIM, last_qty=Quantity.from_int(50_000)),
        )

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.is_open
        assert order in self.cache.orders_open()

    def test_process_event_with_no_venue_order_id_logs_and_does_nothing(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            AUDUSD_SIM.make_price(1.00000),
            emulation_trigger=TriggerType.BID_ASK,
        )

        self.cache.add_order(order, position_id=None)

        self.exec_engine.process(TestEventStubs.order_submitted(order))

        self.cache.reset()  # <-- reset cache so execution engine has to go looking

        canceled = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=None,
            event_id=UUID4(),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(canceled)

        # Assert
        assert order.status == OrderStatus.SUBMITTED

    def test_modify_order_for_already_closed_order_logs_and_does_nothing(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Push to OrderStatus.FILLED (closed)
        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("0.85101"),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        modify = ModifyOrder(
            self.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            Quantity.from_int(200_000),
            None,
            order.trigger_price,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(modify)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.quantity == Quantity.from_int(100_000)

    def test_handle_order_event_with_random_client_order_id_and_order_id_cached(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        canceled = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("web_001"),  # Random ID from a web UI
            order.venue_order_id,
            self.account_id,
            UUID4(),
            self.clock.timestamp_ns(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(canceled)

        # Assert (order was found and OrderCanceled event was applied)
        assert order.status == OrderStatus.CANCELED

    def test_handle_order_event_with_random_client_order_id_and_order_id_not_cached(
        self,
    ) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        canceled = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("web_001"),  # Random ID from a web UI
            VenueOrderId("RANDOM_001"),  # Also a random order id the engine won't find
            self.account_id,
            UUID4(),
            self.clock.timestamp_ns(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(canceled)

        # Assert (order was not found, engine did not crash)
        assert order.status == OrderStatus.ACCEPTED

    def test_handle_duplicate_order_events_logs_error_and_does_not_apply(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        canceled = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("web_001"),  # Random ID from a web UI
            order.venue_order_id,
            self.account_id,
            UUID4(),
            self.clock.timestamp_ns(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(canceled)
        self.exec_engine.process(canceled)

        # Assert (order was found and OrderCanceled event was applied)
        assert order.status == OrderStatus.CANCELED
        assert order.event_count == 4

    def test_handle_order_fill_event_with_no_position_id_correctly_handles_fill(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(
            TestEventStubs.order_filled(
                order=order,
                instrument=AUDUSD_SIM,
            ),
        )

        expected_position_id = PositionId("P-19700101-000000-000-None-1")

        # Assert
        assert self.cache.position_exists(expected_position_id)
        assert self.cache.is_position_open(expected_position_id)
        assert not self.cache.is_position_closed(expected_position_id)
        assert isinstance(self.cache.position(expected_position_id), Position)
        assert expected_position_id in self.cache.position_ids()
        assert expected_position_id not in self.cache.position_closed_ids(strategy_id=strategy.id)
        assert expected_position_id not in self.cache.position_closed_ids()
        assert expected_position_id in self.cache.position_open_ids(strategy_id=strategy.id)
        assert expected_position_id in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_handle_order_fill_event(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        expected_position_id = PositionId("P-19700101-000000-000-None-1")

        # Assert
        assert self.cache.position_exists(expected_position_id)
        assert self.cache.is_position_open(expected_position_id)
        assert not self.cache.is_position_closed(expected_position_id)
        assert isinstance(self.cache.position(expected_position_id), Position)
        assert expected_position_id in self.cache.position_ids()
        assert expected_position_id not in self.cache.position_closed_ids(strategy_id=strategy.id)
        assert expected_position_id not in self.cache.position_closed_ids()
        assert expected_position_id in self.cache.position_open_ids(strategy_id=strategy.id)
        assert expected_position_id in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_handle_multiple_partial_fill_events(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        # Act
        expected_position_id = PositionId("P-19700101-000000-000-None-1")

        self.exec_engine.process(
            TestEventStubs.order_filled(
                order=order,
                instrument=AUDUSD_SIM,
                last_qty=Quantity.from_int(20_100),
            ),
        )

        self.exec_engine.process(
            TestEventStubs.order_filled(
                order=order,
                instrument=AUDUSD_SIM,
                last_qty=Quantity.from_int(19_900),
            ),
        )

        self.exec_engine.process(
            TestEventStubs.order_filled(
                order=order,
                instrument=AUDUSD_SIM,
                last_qty=Quantity.from_int(60_000),
            ),
        )

        # Assert
        assert self.cache.position_exists(expected_position_id)
        assert self.cache.is_position_open(expected_position_id)
        assert not self.cache.is_position_closed(expected_position_id)
        assert isinstance(self.cache.position(expected_position_id), Position)
        assert expected_position_id in self.cache.position_ids()
        assert expected_position_id not in self.cache.position_closed_ids(strategy_id=strategy.id)
        assert expected_position_id not in self.cache.position_closed_ids()
        assert expected_position_id in self.cache.position_open_ids(strategy_id=strategy.id)
        assert expected_position_id in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_handle_position_opening_with_position_id_none(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        expected_id = PositionId("P-19700101-000000-000-None-1")  # Generated inside engine

        # Assert
        assert self.cache.position_exists(expected_id)
        assert self.cache.is_position_open(expected_id)
        assert not self.cache.is_position_closed(expected_id)
        assert isinstance(self.cache.position(expected_id), Position)
        assert expected_id in self.cache.position_ids()
        assert expected_id not in self.cache.position_closed_ids(strategy_id=strategy.id)
        assert expected_id not in self.cache.position_closed_ids()
        assert expected_id in self.cache.position_open_ids(strategy_id=strategy.id)
        assert expected_id in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_add_to_existing_position_on_order_fill(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(TestEventStubs.order_filled(order1, AUDUSD_SIM))

        expected_position_id = PositionId("P-19700101-000000-000-None-1")

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=expected_position_id,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order2, AUDUSD_SIM, position_id=expected_position_id),
        )

        # Assert
        assert self.cache.position_exists(expected_position_id)
        assert self.cache.is_position_open(expected_position_id)
        assert not self.cache.is_position_closed(expected_position_id)
        assert isinstance(self.cache.position(expected_position_id), Position)
        assert len(self.cache.positions_closed(strategy_id=strategy.id)) == 0
        assert len(self.cache.positions_closed()) == 0
        assert len(self.cache.positions_open(strategy_id=strategy.id)) == 1
        assert len(self.cache.positions_open()) == 1
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_close_position_on_order_fill(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order2 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-1")

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(
            TestEventStubs.order_filled(order1, AUDUSD_SIM, position_id=position_id),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=position_id,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order2, AUDUSD_SIM, position_id=position_id),
        )

        # # Assert
        assert self.cache.position_exists(position_id)
        assert not self.cache.is_position_open(position_id)
        assert self.cache.is_position_closed(position_id)
        assert self.cache.position(position_id).id == position_id
        assert self.cache.positions(strategy_id=strategy.id)[0].id == position_id
        assert self.cache.positions()[0].id == position_id
        assert len(self.cache.positions_open(strategy_id=strategy.id)) == 0
        assert len(self.cache.positions_open()) == 0
        assert self.cache.positions_closed(strategy_id=strategy.id)[0].id == position_id
        assert self.cache.positions_closed()[0].id == position_id
        assert position_id not in self.cache.position_open_ids(strategy_id=strategy.id)
        assert position_id not in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 0
        assert self.cache.positions_closed_count() == 1

    def test_multiple_strategy_positions_opened(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy1 = Strategy()
        strategy1.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        strategy2 = Strategy(StrategyConfig(order_id_tag="002"))
        strategy2.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order2 = strategy2.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy1.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy2.id,
            position_id=None,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        position1_id = PositionId("P-1")
        position2_id = PositionId("P-2")

        # Act
        self.risk_engine.execute(submit_order1)
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(
            TestEventStubs.order_filled(order1, AUDUSD_SIM, position_id=position1_id),
        )
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order2, AUDUSD_SIM, position_id=position2_id),
        )

        # # Assert
        assert self.cache.position_exists(position1_id)
        assert self.cache.position_exists(position2_id)
        assert self.cache.is_position_open(position1_id)
        assert self.cache.is_position_open(position2_id)
        assert not self.cache.is_position_closed(position1_id)
        assert not self.cache.is_position_closed(position2_id)
        assert isinstance(self.cache.position(position1_id), Position)
        assert isinstance(self.cache.position(position2_id), Position)
        assert position1_id in self.cache.position_ids(strategy_id=strategy1.id)
        assert position2_id in self.cache.position_ids(strategy_id=strategy2.id)
        assert position1_id in self.cache.position_ids()
        assert position2_id in self.cache.position_ids()
        assert len(self.cache.position_open_ids()) == 2
        assert len(self.cache.positions_open(strategy_id=strategy1.id)) == 1
        assert len(self.cache.positions_open(strategy_id=strategy2.id)) == 1
        assert len(self.cache.positions_open(strategy_id=strategy2.id)) == 1
        assert len(self.cache.positions_open()) == 2
        assert len(self.cache.positions_open(strategy_id=strategy1.id)) == 1
        assert len(self.cache.positions_open(strategy_id=strategy2.id)) == 1
        assert position1_id in self.cache.position_open_ids(strategy_id=strategy1.id)
        assert position2_id in self.cache.position_open_ids(strategy_id=strategy2.id)
        assert position1_id in self.cache.position_open_ids()
        assert position2_id in self.cache.position_open_ids()
        assert position1_id not in self.cache.position_closed_ids(strategy_id=strategy1.id)
        assert position2_id not in self.cache.position_closed_ids(strategy_id=strategy2.id)
        assert position1_id not in self.cache.position_closed_ids()
        assert position2_id not in self.cache.position_closed_ids()
        assert self.cache.positions_total_count() == 2
        assert self.cache.positions_open_count() == 2
        assert self.cache.positions_closed_count() == 0

    def test_multiple_strategy_positions_one_active_one_closed(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy1 = Strategy()
        strategy1.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        strategy2 = Strategy(StrategyConfig(order_id_tag="002"))
        strategy2.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order2 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order3 = strategy2.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy1.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        position_id1 = PositionId("P-1")

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy1.id,
            position_id=position_id1,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        submit_order3 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy2.id,
            position_id=None,
            order=order3,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        position_id2 = PositionId("P-2")

        # Act
        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(
            TestEventStubs.order_filled(order1, AUDUSD_SIM, position_id=position_id1),
        )

        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order2, AUDUSD_SIM, position_id=position_id1),
        )

        self.risk_engine.execute(submit_order3)
        self.exec_engine.process(TestEventStubs.order_submitted(order3))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order3, venue_order_id=VenueOrderId("3")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order3, AUDUSD_SIM, position_id=position_id2),
        )

        # Assert
        # Already tested .is_position_active and .is_position_closed above
        assert self.cache.position_exists(position_id1)
        assert self.cache.position_exists(position_id2)
        assert position_id1 in self.cache.position_ids(strategy_id=strategy1.id)
        assert position_id2 in self.cache.position_ids(strategy_id=strategy2.id)
        assert position_id1 in self.cache.position_ids()
        assert position_id2 in self.cache.position_ids()
        assert len(self.cache.positions_open(strategy_id=strategy1.id)) == 0
        assert len(self.cache.positions_open(strategy_id=strategy2.id)) == 1
        assert len(self.cache.positions_open()) == 1
        assert len(self.cache.positions_closed()) == 1
        assert len(self.cache.positions()) == 2
        assert position_id1 not in self.cache.position_open_ids(strategy_id=strategy1.id)
        assert position_id2 in self.cache.position_open_ids(strategy_id=strategy2.id)
        assert position_id1 not in self.cache.position_open_ids()
        assert position_id2 in self.cache.position_open_ids()
        assert position_id1 in self.cache.position_closed_ids(strategy_id=strategy1.id)
        assert position_id2 not in self.cache.position_closed_ids(strategy_id=strategy2.id)
        assert position_id1 in self.cache.position_closed_ids()
        assert position_id2 not in self.cache.position_closed_ids()
        assert self.cache.positions_total_count() == 2
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 1

    def test_flip_position_on_opposite_filled_same_position_sell(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(150000),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-19700101-000000-000-000-1")

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(
            TestEventStubs.order_filled(order1, AUDUSD_SIM, position_id=position_id),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=position_id,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order2, AUDUSD_SIM, position_id=position_id),
        )

        # Assert
        position_id_flipped = PositionId("P-19700101-000000-000-None-1F")
        position_flipped = self.cache.position(position_id_flipped)

        assert position_flipped.signed_qty == -50_000
        assert position_flipped.last_event.last_qty == 50_000
        assert self.cache.position_exists(position_id)
        assert self.cache.position_exists(position_id_flipped)
        assert self.cache.is_position_closed(position_id)
        assert self.cache.is_position_open(position_id_flipped)
        assert position_id in self.cache.position_ids()
        assert position_id in self.cache.position_ids(strategy_id=strategy.id)
        assert position_id_flipped in self.cache.position_ids()
        assert position_id_flipped in self.cache.position_ids(strategy_id=strategy.id)
        assert self.cache.positions_total_count() == 2
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 1

    def test_flip_position_on_opposite_filled_same_position_buy(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(150000),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-19700101-000000-000-None-1")

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(
            TestEventStubs.order_filled(order1, AUDUSD_SIM, position_id=position_id),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=position_id,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order2, AUDUSD_SIM, position_id=position_id),
        )

        # Assert
        position_id_flipped = PositionId("P-19700101-000000-000-None-1F")
        position_flipped = self.cache.position(position_id_flipped)

        assert position_flipped.signed_qty == 50_000
        assert position_flipped.last_event.last_qty == 50_000
        assert self.cache.position_exists(position_id)
        assert self.cache.position_exists(position_id_flipped)
        assert self.cache.is_position_closed(position_id)
        assert self.cache.is_position_open(position_id_flipped)
        assert position_id in self.cache.position_ids()
        assert position_id in self.cache.position_ids(strategy_id=strategy.id)
        assert position_id_flipped in self.cache.position_ids()
        assert position_id_flipped in self.cache.position_ids(strategy_id=strategy.id)
        assert self.cache.positions_total_count() == 2
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 1

    def test_flip_position_on_flat_position_then_filled_reusing_position_id(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order3 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-19700101-000000-000-001-1")

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(
            TestEventStubs.order_filled(order1, AUDUSD_SIM, position_id=position_id),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=position_id,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        submit_order3 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=position_id,
            order=order3,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        position = self.cache.position(position_id)

        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order2, AUDUSD_SIM, position_id=position_id),
        )
        assert position.signed_qty == 0

        # Reuse same position_id
        self.risk_engine.execute(submit_order3)

        # Assert
        assert order3.status == OrderStatus.INITIALIZED

    def test_flip_position_when_netting_oms(self) -> None:
        # Arrange
        self.exec_engine.start()

        config = StrategyConfig(oms_type="NETTING")
        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(200_000),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-19700101-000000-000-None-1")

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(
            TestEventStubs.order_filled(order1, AUDUSD_SIM, position_id=position_id),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=position_id,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(
            TestEventStubs.order_filled(order2, AUDUSD_SIM, position_id=position_id),
        )

        # Assert
        position_id_flipped = PositionId("P-19700101-000000-000-None-1F")
        position = self.cache.position(position_id)
        position_flipped = self.cache.position(position_id_flipped)
        assert position.id == position_id
        assert position.quantity == Quantity.from_int(0)
        assert position_flipped.quantity == Quantity.from_int(100_000)
        assert position_flipped.side == PositionSide.LONG

    def test_handle_updated_order_event(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            quantity=Quantity.from_int(10_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_pending_update(order))

        # Get order, check venue_order_id
        cached_order = self.cache.order(order.client_order_id)
        assert cached_order.venue_order_id == order.venue_order_id

        # Act
        new_venue_id = VenueOrderId("1")
        order_updated = OrderUpdated(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=AUDUSD_SIM.id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("1"),
            account_id=self.account_id,
            quantity=order.quantity,
            price=order.price,
            trigger_price=None,
            ts_event=self.clock.timestamp_ns(),
            event_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.exec_engine.process(order_updated)

        # Order should have new venue_order_id
        # TODO: This test was updated as the venue order ID currently does not change once assigned
        cached_order = self.cache.order(order.client_order_id)
        assert cached_order.venue_order_id == new_venue_id

    def test_submit_order_with_quote_quantity_and_no_prices_denies(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            quantity=Quantity.from_int(100_000),
            quote_quantity=True,  # <-- Quantity denominated in quote currency
        )

        # Act
        strategy.submit_order(order)

        # Assert
        assert order.quantity == Quantity.from_int(100_000)
        assert order.is_closed
        assert isinstance(order.last_event, OrderDenied)

    def test_submit_bracket_order_with_quote_quantity_and_no_prices_denies(self) -> None:
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bracket = strategy.order_factory.bracket(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            tp_price=Price.from_str("20.0"),
            sl_trigger_price=Price.from_str("10.0"),
            quantity=Quantity.from_int(100_000),
            quote_quantity=True,  # <-- Quantity denominated in quote currency
        )

        # Act
        strategy.submit_order_list(bracket)

        # Assert
        assert bracket.orders[0].quantity == Quantity.from_int(100_000)
        assert bracket.orders[1].quantity == Quantity.from_int(100_000)
        assert bracket.orders[2].quantity == Quantity.from_int(100_000)
        assert bracket.orders[0].is_quote_quantity
        assert bracket.orders[1].is_quote_quantity
        assert bracket.orders[2].is_quote_quantity
        assert isinstance(bracket.orders[0].last_event, OrderDenied)
        assert isinstance(bracket.orders[1].last_event, OrderDenied)
        assert isinstance(bracket.orders[2].last_event, OrderDenied)

    @pytest.mark.parametrize(
        ("order_side", "expected_quantity"),
        [
            [OrderSide.BUY, Quantity.from_str("124984")],
            [OrderSide.SELL, Quantity.from_str("125000")],
        ],
    )
    def test_submit_order_with_quote_quantity_and_quote_tick_converts_to_base_quantity(
        self,
        order_side: OrderSide,
        expected_quantity: Quantity,
    ) -> None:
        # Arrange
        self.exec_engine.start()

        # Set up market
        tick = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.80000"),
            ask_price=Price.from_str("0.80010"),
            bid_size=Quantity.from_int(10_000_000),
            ask_size=Quantity.from_int(10_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_quote_tick(tick)

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=order_side,
            price=Price.from_str("10.0"),
            quantity=Quantity.from_int(100_000),
            quote_quantity=True,  # <-- Quantity denominated in quote currency
        )

        strategy.submit_order(order)

        # Act
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        # Assert
        assert order.quantity == expected_quantity
        assert not order.is_quote_quantity

    @pytest.mark.parametrize(
        ("order_side", "expected_quantity"),
        [
            [OrderSide.BUY, Quantity.from_str("124992")],
            [OrderSide.SELL, Quantity.from_str("124992")],
        ],
    )
    def test_submit_order_with_quote_quantity_and_trade_ticks_converts_to_base_quantity(
        self,
        order_side: OrderSide,
        expected_quantity: Quantity,
    ) -> None:
        # Arrange
        self.exec_engine.start()

        # Set up market
        tick = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("0.80005"),
            size=Quantity.from_int(100_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_trade_tick(tick)

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=order_side,
            price=Price.from_str("10.0"),
            quantity=Quantity.from_int(100_000),
            quote_quantity=True,  # <-- Quantity denominated in quote currency
        )

        strategy.submit_order(order)

        # Act
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        # Assert
        assert order.quantity == expected_quantity
        assert not order.is_quote_quantity

    def test_submit_order_with_quote_quantity_and_conversion_disabled_keeps_quote_quantity(
        self,
    ) -> None:
        # Arrange
        local_clock = TestClock()
        msgbus = MessageBus(trader_id=self.trader_id, clock=local_clock)
        cache = Cache(database=MockCacheDatabase())
        portfolio = Portfolio(msgbus=msgbus, cache=cache, clock=local_clock)
        portfolio.update_account(TestEventStubs.margin_account_state())

        config = ExecEngineConfig(convert_quote_qty_to_base=False, debug=True)
        exec_engine = ExecutionEngine(
            msgbus=msgbus,
            cache=cache,
            clock=local_clock,
            config=config,
        )

        exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            msgbus=msgbus,
            cache=cache,
            clock=local_clock,
        )
        exec_engine.register_client(exec_client)
        exec_engine.start()

        cache.add_instrument(AUDUSD_SIM)

        tick = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.80000"),
            ask_price=Price.from_str("0.80010"),
            bid_size=Quantity.from_int(10_000_000),
            ask_size=Quantity.from_int(10_000_000),
            ts_event=0,
            ts_init=0,
        )
        cache.add_quote_tick(tick)

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            clock=local_clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            quantity=Quantity.from_int(100_000),
            quote_quantity=True,
        )
        original_qty = order.quantity

        strategy.submit_order(order)

        # Act
        exec_engine.process(TestEventStubs.order_submitted(order))
        exec_engine.process(TestEventStubs.order_accepted(order))

        # Assert
        assert order.is_quote_quantity
        assert order.quantity == original_qty

    @pytest.mark.parametrize(
        ("order_side", "expected_quantity"),
        [
            [OrderSide.BUY, Quantity.from_str("124984")],
            [OrderSide.SELL, Quantity.from_str("125000")],
        ],
    )
    def test_submit_bracket_order_with_quote_quantity_and_ticks_converts_expected(
        self,
        order_side: OrderSide,
        expected_quantity: Quantity,
    ) -> None:
        # Arrange
        self.exec_engine.start()

        trade_tick = TradeTick(
            instrument_id=AUDUSD_SIM.id,
            price=Price.from_str("0.80005"),
            size=Quantity.from_int(100_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_trade_tick(trade_tick)

        quote_tick = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.80000"),
            ask_price=Price.from_str("0.80010"),
            bid_size=Quantity.from_int(10_000_000),
            ask_size=Quantity.from_int(10_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_quote_tick(quote_tick)

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bracket = strategy.order_factory.bracket(
            instrument_id=AUDUSD_SIM.id,
            order_side=order_side,
            tp_price=Price.from_str("20.0"),
            sl_trigger_price=Price.from_str("10.0"),
            quantity=Quantity.from_int(100_000),
            quote_quantity=True,  # <-- Quantity denominated in quote currency
        )

        # Act
        strategy.submit_order_list(bracket)

        # Assert
        assert bracket.orders[0].quantity == expected_quantity
        assert bracket.orders[1].quantity == expected_quantity
        assert bracket.orders[2].quantity == expected_quantity
        assert not bracket.orders[0].is_quote_quantity
        assert not bracket.orders[1].is_quote_quantity
        assert not bracket.orders[2].is_quote_quantity

    def test_submit_market_should_not_add_to_own_book(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
        )

        # Act
        strategy.submit_order(order)

        # Assert
        assert self.cache.own_order_book(order.instrument_id) is None

    @pytest.mark.parametrize(
        ("time_in_force"),
        [
            TimeInForce.FOK,
            TimeInForce.IOC,
        ],
    )
    def test_submit_ioc_fok_should_not_add_to_own_book(self, time_in_force: TimeInForce) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("10.0"),
            time_in_force=time_in_force,
        )

        # Act
        strategy.submit_order(order)

        # Assert
        assert self.cache.own_order_book(order.instrument_id) is None

    def test_submit_order_adds_to_own_book_bid(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("10.0"),
        )

        # Act
        strategy.submit_order(order)

        # Assert
        own_book = self.cache.own_order_book(order.instrument_id)
        assert own_book.update_count == 1
        assert len(own_book.asks_to_dict()) == 0
        assert len(own_book.bids_to_dict()) == 1
        assert len(own_book.bids_to_dict()[Decimal("10.0")]) == 1
        own_order = own_book.bids_to_dict()[Decimal("10.0")][0]
        assert own_order.client_order_id.value == order.client_order_id.value
        assert own_order.price == Decimal("10.0")
        assert own_order.size == Decimal(100_000)
        assert own_order.status == nautilus_pyo3.OrderStatus.INITIALIZED
        assert self.cache.own_bid_orders(order.instrument_id) == {Decimal("10.0"): [order]}
        assert self.cache.own_ask_orders(order.instrument_id) == {}

    def test_submit_order_adds_to_own_book_ask(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11.0"),
        )

        # Act
        strategy.submit_order(order)

        # Assert
        own_book = self.cache.own_order_book(order.instrument_id)
        assert own_book.update_count == 1
        assert len(own_book.asks_to_dict()) == 1
        assert len(own_book.bids_to_dict()) == 0
        assert len(own_book.asks_to_dict()[Decimal("11.0")]) == 1
        own_order = own_book.asks_to_dict()[Decimal("11.0")][0]
        assert own_order.client_order_id.value == order.client_order_id.value
        assert own_order.price == Decimal("11.0")
        assert own_order.size == Decimal(100_000)
        assert own_order.status == nautilus_pyo3.OrderStatus.INITIALIZED
        assert self.cache.own_ask_orders(order.instrument_id) == {Decimal("11.0"): [order]}
        assert self.cache.own_bid_orders(order.instrument_id) == {}

    def test_cancel_order_removes_from_own_book(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        order_bid = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("10.0"),
        )

        order_ask = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11.0"),
        )

        strategy.submit_order(order_bid)
        self.exec_engine.process(TestEventStubs.order_submitted(order_bid))
        self.exec_engine.process(TestEventStubs.order_accepted(order_bid))

        strategy.submit_order(order_ask)
        self.exec_engine.process(TestEventStubs.order_submitted(order_ask))
        self.exec_engine.process(TestEventStubs.order_accepted(order_ask))

        # Act
        strategy.cancel_order(order_bid)
        strategy.cancel_order(order_ask)
        self.exec_engine.process(TestEventStubs.order_canceled(order_bid))
        self.exec_engine.process(TestEventStubs.order_canceled(order_ask))

        # Assert
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book.update_count == 10
        assert len(own_book.asks_to_dict()) == 0
        assert len(own_book.bids_to_dict()) == 0
        assert self.cache.own_bid_orders(instrument.id) == {}
        assert self.cache.own_ask_orders(instrument.id) == {}

    def test_rejected_order_removed_from_own_book(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("10.0"),
        )

        strategy.submit_order(order)

        # Assert order was added to own book
        own_book = self.cache.own_order_book(order.instrument_id)
        assert len(own_book.bids_to_dict()) == 1

        # Act - reject the order
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_rejected(order))

        # Assert - order should be removed from own book
        assert len(own_book.bids_to_dict()) == 0
        assert self.cache.own_bid_orders(order.instrument_id) == {}

    @pytest.mark.parametrize(
        ("time_in_force"),
        [
            TimeInForce.FOK,
            TimeInForce.IOC,
        ],
    )
    def test_ioc_fok_not_added_to_existing_own_book(self, time_in_force: TimeInForce) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # First, create a normal limit order to establish an own book for this instrument
        limit_order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("10.0"),
        )
        strategy.submit_order(limit_order)

        # Assert own book exists
        own_book = self.cache.own_order_book(limit_order.instrument_id)
        assert own_book is not None
        assert len(own_book.bids_to_dict()) == 1

        # Act - submit IOC/FOK order for same instrument
        ioc_fok_order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(50_000),
            price=Price.from_str("10.5"),
            time_in_force=time_in_force,
        )
        strategy.submit_order(ioc_fok_order)

        # Assert - IOC/FOK order should NOT be in own book
        assert len(own_book.bids_to_dict()) == 1  # Still just the limit order
        assert Decimal("10.0") in own_book.bids_to_dict()
        assert Decimal("10.5") not in own_book.bids_to_dict()

        # Simulate rejection and ensure it doesn't cause issues
        self.exec_engine.process(TestEventStubs.order_submitted(ioc_fok_order))
        self.exec_engine.process(TestEventStubs.order_rejected(ioc_fok_order))

        # Assert - still only the limit order in book
        assert len(own_book.bids_to_dict()) == 1
        assert Decimal("10.0") in own_book.bids_to_dict()

    def test_own_book_status_filtering(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        order_bid = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("10.0"),
        )

        order_ask = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11.0"),
        )

        strategy.submit_order(order_bid)
        self.exec_engine.process(TestEventStubs.order_submitted(order_bid))
        self.exec_engine.process(TestEventStubs.order_accepted(order_bid))

        strategy.submit_order(order_ask)
        self.exec_engine.process(TestEventStubs.order_submitted(order_ask))
        self.exec_engine.process(TestEventStubs.order_accepted(order_ask))

        # Act
        strategy.cancel_order(order_bid)
        strategy.cancel_order(order_ask)

        # Assert
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book.update_count == 8
        assert len(own_book.asks_to_dict()) == 1  # Order is there with no filtering
        assert len(own_book.bids_to_dict()) == 1  # Order is there with no filtering
        assert (
            self.cache.own_bid_orders(
                instrument.id,
                status={OrderStatus.ACCEPTED, OrderStatus.PARTIALLY_FILLED},
            )
            == {}
        )
        assert (
            self.cache.own_ask_orders(
                instrument.id,
                status={OrderStatus.ACCEPTED, OrderStatus.PARTIALLY_FILLED},
            )
            == {}
        )

    def test_filled_order_removes_from_own_book(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        order_bid = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("10.0"),
        )

        order_ask = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11.0"),
        )

        strategy.submit_order(order_bid)
        self.exec_engine.process(TestEventStubs.order_submitted(order_bid))
        self.exec_engine.process(TestEventStubs.order_accepted(order_bid))

        strategy.submit_order(order_ask)
        self.exec_engine.process(TestEventStubs.order_submitted(order_ask))
        self.exec_engine.process(TestEventStubs.order_accepted(order_ask))

        # Act
        self.exec_engine.process(TestEventStubs.order_filled(order_bid, instrument))
        self.exec_engine.process(TestEventStubs.order_filled(order_ask, instrument))

        # Assert
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book.update_count == 8
        assert len(own_book.asks_to_dict()) == 0
        assert len(own_book.bids_to_dict()) == 0
        assert self.cache.own_bid_orders(instrument.id) == {}
        assert self.cache.own_ask_orders(instrument.id) == {}

    def test_order_updates_in_own_book(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        order_bid = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("10.0"),
        )

        order_ask = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11.0"),
        )

        strategy.submit_order(order_bid)
        self.exec_engine.process(TestEventStubs.order_submitted(order_bid))
        self.exec_engine.process(TestEventStubs.order_accepted(order_bid))

        strategy.submit_order(order_ask)
        self.exec_engine.process(TestEventStubs.order_submitted(order_ask))
        self.exec_engine.process(TestEventStubs.order_accepted(order_ask))

        # Act
        new_bid_price = Price.from_str("9.0")
        new_ask_price = Price.from_str("12.0")
        self.exec_engine.process(TestEventStubs.order_updated(order_bid, price=new_bid_price))
        self.exec_engine.process(TestEventStubs.order_updated(order_ask, price=new_ask_price))

        # Assert
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book.update_count == 8
        assert len(own_book.asks_to_dict()) == 1
        assert len(own_book.bids_to_dict()) == 1
        assert len(own_book.asks_to_dict()[Decimal("12.0")]) == 1
        assert len(own_book.bids_to_dict()[Decimal("9.0")]) == 1

        own_order_bid = own_book.bids_to_dict()[Decimal("9.0")][0]
        assert own_order_bid.client_order_id.value == order_bid.client_order_id.value
        assert own_order_bid.price == new_bid_price
        assert own_order_bid.status == nautilus_pyo3.OrderStatus.ACCEPTED

        own_order_ask = own_book.asks_to_dict()[Decimal("12.0")][0]
        assert own_order_ask.client_order_id.value == order_ask.client_order_id.value
        assert own_order_ask.price == new_ask_price
        assert own_order_ask.status == nautilus_pyo3.OrderStatus.ACCEPTED

        assert self.cache.own_bid_orders(instrument.id, status={OrderStatus.ACCEPTED}) == {
            Decimal("9.0"): [order_bid],
        }
        assert self.cache.own_ask_orders(instrument.id, status={OrderStatus.ACCEPTED}) == {
            Decimal("12.0"): [order_ask],
        }
        self.cache.audit_own_order_books()

    def test_position_flip_with_own_order_book(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # Create initial long position
        buy_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.0"),
        )

        strategy.submit_order(buy_order)
        self.exec_engine.process(TestEventStubs.order_submitted(buy_order))
        self.exec_engine.process(TestEventStubs.order_accepted(buy_order))
        self.exec_engine.process(TestEventStubs.order_filled(buy_order, instrument))

        # The position ID should be generated by the execution engine
        position_id = PositionId("P-19700101-000000-000-None-1")

        # Check that the position was created
        original_position = self.cache.position(position_id)
        assert original_position is not None, "Original position not created"

        # Create larger sell order to flip position
        sell_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),  # Twice the size to flip position
            price=Price.from_str("1.1"),
        )

        # Act
        strategy.submit_order(sell_order, position_id=position_id)
        self.exec_engine.process(TestEventStubs.order_submitted(sell_order))
        self.exec_engine.process(TestEventStubs.order_accepted(sell_order))
        self.exec_engine.process(
            TestEventStubs.order_filled(sell_order, instrument, position_id=position_id),
        )

        # Instead of assuming a specific ID for the flipped position,
        # let's find which positions exist in the cache
        positions = self.cache.positions()
        assert len(positions) == 2, f"Expected 2 positions, found {len(positions)}"

        # The flipped position should be the one that's open
        flipped_position = None
        for pos in positions:
            if pos.id != position_id and pos.is_open:
                flipped_position = pos
                break

        assert flipped_position is not None, "Flipped position not found"

        # Assert
        # Verify position has flipped
        assert original_position.is_closed
        assert flipped_position.is_open
        assert flipped_position.side == PositionSide.SHORT
        assert flipped_position.quantity == Quantity.from_int(100_000)

        # Verify own order book state
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book.update_count > 0
        assert len(own_book.asks_to_dict()) == 0  # Orders should be removed after fill
        assert len(own_book.bids_to_dict()) == 0  # Orders should be removed after fill

    def test_own_book_with_crossed_orders(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # Create a bid above current best ask
        buy_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.05"),  # Buy at 1.05
        )

        # Create an ask below the bid
        sell_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.04"),  # Sell at 1.04 (below the bid)
        )

        # Act
        strategy.submit_order(buy_order)
        self.exec_engine.process(TestEventStubs.order_submitted(buy_order))
        self.exec_engine.process(TestEventStubs.order_accepted(buy_order))

        strategy.submit_order(sell_order)
        self.exec_engine.process(TestEventStubs.order_submitted(sell_order))
        self.exec_engine.process(TestEventStubs.order_accepted(sell_order))

        # Assert
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book.update_count > 0

        # Verify both orders exist in the book, even though they're "crossed"
        assert len(own_book.asks_to_dict()) == 1
        assert len(own_book.bids_to_dict()) == 1

        # Verify by price
        assert len(own_book.asks_to_dict()[Decimal("1.04")]) == 1
        assert len(own_book.bids_to_dict()[Decimal("1.05")]) == 1

        # The own book doesn't enforce market integrity rules like not allowing crossed books
        # because it's just tracking the orders, not matching them

        # Check order status by status filtering
        active_orders = self.cache.own_bid_orders(instrument.id, status={OrderStatus.ACCEPTED})
        assert len(active_orders) == 1
        assert Decimal("1.05") in active_orders

        active_orders = self.cache.own_ask_orders(instrument.id, status={OrderStatus.ACCEPTED})
        assert len(active_orders) == 1
        assert Decimal("1.04") in active_orders

    def test_own_book_with_contingent_orders(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # Create a bracket order with limit entry, limit TP and limit SL
        bracket = strategy.order_factory.bracket(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            entry_order_type=OrderType.LIMIT,
            entry_price=Price.from_str("1.00"),  # Limit entry price
            tp_price=Price.from_str("1.10"),  # Take profit at 1.10
            sl_trigger_price=Price.from_str(
                "0.90",
            ),  # Stop loss at 0.90 (using limit price not trigger)
            quantity=Quantity.from_int(100_000),
        )

        # Act
        strategy.submit_order_list(bracket)

        # Process entry order events
        entry_order = bracket.orders[0]
        tp_order = bracket.orders[1]
        sl_order = bracket.orders[2]

        self.exec_engine.process(TestEventStubs.order_submitted(entry_order))
        self.exec_engine.process(TestEventStubs.order_accepted(entry_order))

        # Assert - before entry fill
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book.update_count > 1

        # Entry order should be in the book as a bid
        assert len(own_book.bids_to_dict()) == 1
        assert Decimal("1.00") in own_book.bids_to_dict()
        assert len(own_book.bids_to_dict()[Decimal("1.00")]) == 1

        # TP order should be in the book as an ask (submitted)
        assert len(own_book.asks_to_dict()) == 1
        assert Decimal("1.10") in own_book.asks_to_dict()
        assert len(own_book.asks_to_dict()[Decimal("1.10")]) == 1

        # Now fill the entry order
        self.exec_engine.process(TestEventStubs.order_filled(entry_order, instrument))

        # Process TP and SL orders - they should be submitted and accepted now
        self.exec_engine.process(TestEventStubs.order_submitted(tp_order))
        self.exec_engine.process(TestEventStubs.order_accepted(tp_order))
        self.exec_engine.process(TestEventStubs.order_submitted(sl_order))
        self.exec_engine.process(TestEventStubs.order_accepted(sl_order))

        # Assert - after entry fill
        own_book = self.cache.own_order_book(instrument.id)

        # Entry order should be removed from the book as it's filled
        assert len(own_book.bids_to_dict()) == 0

        # TP should still be in the book
        assert len(own_book.asks_to_dict()) == 1
        assert Decimal("1.10") in own_book.asks_to_dict()

        # Test that contingent orders are linked to the same position
        position_id = self.cache.position_id(entry_order.client_order_id)
        assert position_id is not None
        assert self.cache.position_id(tp_order.client_order_id) == position_id
        assert self.cache.position_id(sl_order.client_order_id) == position_id

    @pytest.mark.parametrize(
        ("status, price, process_steps, expected_in_book"),
        [
            (
                OrderStatus.INITIALIZED,
                "1.00",
                [],  # No processing steps beyond submission
                True,  # Should be in book
            ),
            (
                OrderStatus.SUBMITTED,
                "1.01",
                [OrderStatus.SUBMITTED],  # Process submission
                True,  # Should be in book
            ),
            (
                OrderStatus.ACCEPTED,
                "1.02",
                [OrderStatus.SUBMITTED, OrderStatus.ACCEPTED],  # Process submission and acceptance
                True,  # Should be in book
            ),
            (
                OrderStatus.PARTIALLY_FILLED,
                "1.03",
                [
                    OrderStatus.SUBMITTED,
                    OrderStatus.ACCEPTED,
                    OrderStatus.PARTIALLY_FILLED,
                ],  # Partial fill
                True,  # Should be in book
            ),
            (
                OrderStatus.FILLED,
                "1.04",
                [OrderStatus.SUBMITTED, OrderStatus.ACCEPTED, OrderStatus.FILLED],  # Complete fill
                False,  # Should not be in book (filled)
            ),
            (
                OrderStatus.CANCELED,
                "1.05",
                [OrderStatus.SUBMITTED, OrderStatus.ACCEPTED, OrderStatus.CANCELED],  # Cancellation
                False,  # Should not be in book (canceled)
            ),
        ],
    )
    def test_own_book_order_status_filtering_parameterized(
        self,
        status: OrderStatus,
        price: str,
        process_steps: list[OrderStatus],
        expected_in_book: bool,
    ) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # Create the order
        order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str(price),
        )

        # Submit the order
        strategy.submit_order(order)

        # Process the order according to the steps
        for step in process_steps:
            if step == OrderStatus.SUBMITTED:
                self.exec_engine.process(TestEventStubs.order_submitted(order))
            elif step == OrderStatus.ACCEPTED:
                self.exec_engine.process(TestEventStubs.order_accepted(order))
            elif step == OrderStatus.PARTIALLY_FILLED:
                self.exec_engine.process(
                    TestEventStubs.order_filled(
                        order,
                        instrument,
                        last_qty=Quantity.from_int(50_000),
                    ),
                )
            elif step == OrderStatus.FILLED:
                self.exec_engine.process(TestEventStubs.order_filled(order, instrument))
            elif step == OrderStatus.CANCELED:
                self.exec_engine.process(TestEventStubs.order_canceled(order))

        # Assert
        own_book = self.cache.own_order_book(instrument.id)

        # Check if the order is in the book as expected
        if expected_in_book:
            assert len(own_book.bids_to_dict()) > 0
            assert Decimal(price) in own_book.bids_to_dict()
            filtered_orders = self.cache.own_bid_orders(
                instrument.id,
                status={status},
            )
            assert Decimal(price) in filtered_orders
        else:
            # If we expect the order not to be in the book, check that the price level doesn't exist
            # or that it doesn't contain our order
            if Decimal(price) in own_book.bids_to_dict():
                assert len(own_book.bids_to_dict()[Decimal(price)]) == 0

    def test_own_book_combined_status_filtering(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # Create orders with different statuses
        initialized_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.00"),
        )

        submitted_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.01"),
        )

        accepted_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.02"),
        )

        partially_filled_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.03"),
        )

        # Process orders to achieve desired states
        strategy.submit_order(initialized_order)

        strategy.submit_order(submitted_order)
        self.exec_engine.process(TestEventStubs.order_submitted(submitted_order))

        strategy.submit_order(accepted_order)
        self.exec_engine.process(TestEventStubs.order_submitted(accepted_order))
        self.exec_engine.process(TestEventStubs.order_accepted(accepted_order))

        strategy.submit_order(partially_filled_order)
        self.exec_engine.process(TestEventStubs.order_submitted(partially_filled_order))
        self.exec_engine.process(TestEventStubs.order_accepted(partially_filled_order))
        self.exec_engine.process(
            TestEventStubs.order_filled(
                partially_filled_order,
                instrument,
                last_qty=Quantity.from_int(50_000),
            ),
        )

        # INITIALIZED + SUBMITTED
        early_statuses = {OrderStatus.INITIALIZED, OrderStatus.SUBMITTED}
        early_orders = self.cache.own_bid_orders(instrument.id, status=early_statuses)
        early_order_count = sum(len(orders) for orders in early_orders.values())
        assert early_order_count == 2
        assert Decimal("1.00") in early_orders
        assert Decimal("1.01") in early_orders

        # ACCEPTED + PARTIALLY_FILLED
        active_statuses = {OrderStatus.ACCEPTED, OrderStatus.PARTIALLY_FILLED}
        active_orders = self.cache.own_bid_orders(instrument.id, status=active_statuses)
        active_order_count = sum(len(orders) for orders in active_orders.values())
        assert active_order_count == 2
        assert Decimal("1.02") in active_orders
        assert Decimal("1.03") in active_orders

        # ALL orders (no filter)
        all_orders = self.cache.own_bid_orders(instrument.id)
        all_order_count = sum(len(orders) for orders in all_orders.values())
        assert all_order_count == 4
        self.cache.audit_own_order_books()

    def test_own_book_status_integrity_during_transitions(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # Create initial orders at different price levels
        prices: list[str] = ["1.00", "1.01", "1.02"]
        orders: list[Order] = []

        for price in prices:
            order = strategy.order_factory.limit(
                instrument_id=instrument.id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(100_000),
                price=Price.from_str(price),
            )
            orders.append(order)

            # Submit and process to ACCEPTED
            strategy.submit_order(order)
            self.exec_engine.process(TestEventStubs.order_submitted(order))
            self.exec_engine.process(TestEventStubs.order_accepted(order))

        # Verify initial state - all orders should be ACCEPTED
        accepted_orders = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.ACCEPTED},
        )
        assert len(accepted_orders) == 3
        for price, order in zip(prices, orders):
            assert Decimal(price) in accepted_orders
            assert order.client_order_id in [
                o.client_order_id for o in accepted_orders[Decimal(price)]
            ]

        # Test case 1: Order transitions from ACCEPTED to PARTIALLY_FILLED
        # Partially fill order 1
        self.exec_engine.process(
            TestEventStubs.order_filled(
                orders[1],
                instrument,
                last_qty=Quantity.from_int(50_000),
                position_id=PositionId("1"),
            ),
        )

        # Verify order is now PARTIALLY_FILLED and not ACCEPTED
        partially_filled_orders = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.PARTIALLY_FILLED},
        )
        assert len(partially_filled_orders) == 1
        assert Decimal(prices[1]) in partially_filled_orders

        accepted_after_partial = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.ACCEPTED},
        )
        assert len(accepted_after_partial) == 2
        assert Decimal(prices[1]) not in accepted_after_partial

        # Test case 2: Order transitions from ACCEPTED to CANCELED
        # Cancel order 2
        self.exec_engine.process(TestEventStubs.order_canceled(orders[2]))

        # Verify order is removed from book when CANCELED
        canceled_orders = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.CANCELED},
        )
        assert len(canceled_orders) == 0  # Should not appear in the book

        accepted_after_cancel = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.ACCEPTED},
        )
        assert len(accepted_after_cancel) == 1
        assert Decimal(prices[2]) not in accepted_after_cancel

        # Test case 3: Order transitions from ACCEPTED to PARTIALLY_FILLED to FILLED
        # First partial fill
        self.exec_engine.process(
            TestEventStubs.order_filled(
                orders[0],
                instrument,
                last_qty=Quantity.from_int(50_000),
                trade_id=TradeId("001"),
            ),
        )

        # Verify status is now PARTIALLY_FILLED
        partially_after_first = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.PARTIALLY_FILLED},
        )
        assert len(partially_after_first) == 2
        assert Decimal(prices[0]) in partially_after_first

        # Complete fill (failing test case)
        self.exec_engine.process(
            TestEventStubs.order_filled(
                orders[0],
                instrument,
                last_qty=Quantity.from_int(50_000),  # Remaining quantity
                trade_id=TradeId("002"),
            ),
        )

        partially_after_complete = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.PARTIALLY_FILLED},
        )

        assert len(partially_after_complete) == 1
        assert orders[0].status == OrderStatus.FILLED, "Order should be FILLED"
        assert orders[1].status == OrderStatus.PARTIALLY_FILLED, "Order should be PARTIALLY_FILLED"
        assert orders[2].status == OrderStatus.CANCELED, "Order should be CANCELED"

        # Check if order exists in own book with any status
        all_orders = self.cache.own_bid_orders(instrument.id)
        assert Decimal(prices[1]) in all_orders

        filled_orders = self.cache.own_bid_orders(instrument.id, status={OrderStatus.FILLED})
        assert len(filled_orders) == 0, "FILLED orders should not appear in the own book"

    def test_own_book_race_conditions_and_edge_cases(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # === Test Case 1: Out of order events ===
        # Create an order that receives events out of normal sequence
        out_of_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.00"),
        )

        # 1. Submit but send ACCEPTED before SUBMITTED
        strategy.submit_order(out_of_order)
        self.exec_engine.process(TestEventStubs.order_accepted(out_of_order))
        self.exec_engine.process(TestEventStubs.order_submitted(out_of_order))

        # Verify it's in the book with ACCEPTED status
        accepted_orders = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.ACCEPTED},
        )
        assert len(accepted_orders) == 1
        assert Decimal("1.00") in accepted_orders

        # 2. Send FILLED before PARTIALLY_FILLED (should handle this gracefully)
        self.exec_engine.process(
            TestEventStubs.order_filled(
                out_of_order,
                instrument,
                last_qty=Quantity.from_int(100_000),  # Full fill
                trade_id=TradeId("001"),
            ),
        )

        # Order should be fully filled and removed from book
        all_orders = self.cache.own_bid_orders(instrument.id)
        assert Decimal("1.00") not in all_orders, "Filled order should be removed from book"

        # === Test Case 2: Multiple orders at same price level ===
        same_price_orders = []
        for i in range(3):
            order = strategy.order_factory.limit(
                instrument_id=instrument.id,
                order_side=OrderSide.SELL,
                quantity=Quantity.from_int(100_000),
                price=Price.from_str("1.20"),  # Same price for all
            )
            same_price_orders.append(order)

            # Submit and process to ACCEPTED
            strategy.submit_order(order)
            self.exec_engine.process(TestEventStubs.order_submitted(order))
            self.exec_engine.process(TestEventStubs.order_accepted(order))

        # Verify all 3 orders at same price level
        asks = self.cache.own_ask_orders(
            instrument.id,
            status={OrderStatus.ACCEPTED},
        )
        assert Decimal("1.20") in asks
        assert len(asks[Decimal("1.20")]) == 3

        # Process different status changes for each order
        # Order 0: Partial fill
        self.exec_engine.process(
            TestEventStubs.order_filled(
                same_price_orders[0],
                instrument,
                last_qty=Quantity.from_int(50_000),
                trade_id=TradeId("sp1"),
            ),
        )

        # Order 1: Cancel
        self.exec_engine.process(TestEventStubs.order_canceled(same_price_orders[1]))

        # Order 2: Complete fill
        self.exec_engine.process(
            TestEventStubs.order_filled(
                same_price_orders[2],
                instrument,
                last_qty=Quantity.from_int(100_000),
                trade_id=TradeId("sp2"),
            ),
        )

        # Verify correct statuses at the price level
        asks_after = self.cache.own_ask_orders(instrument.id)
        assert Decimal("1.20") in asks_after

        # Only the partially filled order should remain
        assert len(asks_after[Decimal("1.20")]) == 1

        # === Test Case 3: Overlapping bid/ask prices ===
        # Create crossed book with buy higher than sell
        crossed_buy = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.10"),  # Higher than sell
        )

        crossed_sell = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.05"),  # Lower than buy
        )

        # Submit and process to ACCEPTED
        strategy.submit_order(crossed_buy)
        self.exec_engine.process(TestEventStubs.order_submitted(crossed_buy))
        self.exec_engine.process(TestEventStubs.order_accepted(crossed_buy))

        strategy.submit_order(crossed_sell)
        self.exec_engine.process(TestEventStubs.order_submitted(crossed_sell))
        self.exec_engine.process(TestEventStubs.order_accepted(crossed_sell))

        # Verify crossed book exists in own book (should allow this)
        all_bids = self.cache.own_bid_orders(instrument.id)
        all_asks = self.cache.own_ask_orders(instrument.id)

        assert Decimal("1.10") in all_bids
        assert Decimal("1.05") in all_asks

        # === Test Case 4: Rapid order status transitions ===
        # Create order that will undergo rapid state transitions
        rapid_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.15"),
        )

        # Submit and accept
        strategy.submit_order(rapid_order)
        self.exec_engine.process(TestEventStubs.order_submitted(rapid_order))
        self.exec_engine.process(TestEventStubs.order_accepted(rapid_order))

        # Verify in book
        transitioning_bids = self.cache.own_bid_orders(instrument.id)
        assert Decimal("1.15") in transitioning_bids

        # Rapid partial fills in sequence
        fill_sizes = [25_000, 25_000, 25_000, 25_000]  # Four fills to complete the order

        for i, size in enumerate(fill_sizes):
            self.exec_engine.process(
                TestEventStubs.order_filled(
                    rapid_order,
                    instrument,
                    last_qty=Quantity.from_int(size),
                    trade_id=TradeId(f"rapid{i}"),
                ),
            )

        # Verify order status in Python object
        assert rapid_order.status == OrderStatus.FILLED

        # Check that order is no longer in the book
        current_bids = self.cache.own_bid_orders(instrument.id)
        assert Decimal("1.15") not in current_bids

    def test_own_book_order_overfill_removes_from_book(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.00000"),
        )

        # Submit and accept the order
        strategy.submit_order(order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        # Verify order is in the own book
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book.update_count == 3
        assert len(own_book.bids_to_dict()) == 1
        assert Decimal("1.00000") in own_book.bids_to_dict()

        # Partially fill the order with 50% of quantity
        self.exec_engine.process(
            TestEventStubs.order_filled(
                order,
                instrument,
                last_qty=Quantity.from_int(50_000),
            ),
        )

        # Verify order is still in the book with updated status
        assert order.status == OrderStatus.PARTIALLY_FILLED
        own_book = self.cache.own_order_book(instrument.id)
        assert len(own_book.bids_to_dict()) == 1
        assert Decimal("1.00000") in own_book.bids_to_dict()

        # Act - overfill the order (60K more when only 50K remains)
        self.exec_engine.process(
            TestEventStubs.order_filled(
                order,
                instrument,
                last_qty=Quantity.from_int(60_000),
                trade_id=TradeId("2"),
            ),
        )

        # Assert
        own_book = self.cache.own_order_book(instrument.id)
        assert order.status == OrderStatus.FILLED
        assert (
            len(own_book.bids_to_dict()) == 0
        ), "Order should be removed from own book despite overfill"
        assert (
            self.cache.own_bid_orders(instrument.id) == {}
        ), "Own book cache should be empty after overfill"

    def test_own_book_order_denied_removes_from_book(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # Submit an order which will be denied by the RiskEngine
        order1 = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(10_000_000_000),  # <-- Size exceeds maximum for instrument
            price=Price.from_str("1.00000"),
        )

        strategy.submit_order(order1)

        # Submit a valid order
        order2 = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.00000"),
        )

        strategy.submit_order(order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))

        # Assert
        own_book = self.cache.own_order_book(instrument.id)
        assert own_book is not None
        assert order1.status == OrderStatus.DENIED
        assert order2.status == OrderStatus.SUBMITTED
        assert len(own_book.bids_to_dict()) == 1
        assert order2.client_order_id.value == own_book.bid_client_order_ids()[0].value

    def test_own_book_accepted_buffer_filtering(self) -> None:
        # Arrange
        self.exec_engine.set_manage_own_order_books(True)
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        instrument = AUDUSD_SIM

        # Create and submit orders at different times
        bid_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.00000"),
        )

        ask_order = strategy.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.10000"),
        )

        # Submit and accept first order (older)
        strategy.submit_order(bid_order)
        self.exec_engine.process(TestEventStubs.order_submitted(bid_order))

        # Set accepted time for first order
        bid_accepted_time = self.clock.timestamp_ns()
        self.exec_engine.process(
            TestEventStubs.order_accepted(bid_order, ts_event=bid_accepted_time),
        )

        # Advance time by 5 seconds
        self.clock.advance_time(5_000_000_000)  # 5 seconds in nanoseconds

        # Submit and accept second order (newer)
        strategy.submit_order(ask_order)
        self.exec_engine.process(TestEventStubs.order_submitted(ask_order))

        # Set accepted time for second order
        ask_accepted_time = self.clock.timestamp_ns()
        self.exec_engine.process(
            TestEventStubs.order_accepted(ask_order, ts_event=ask_accepted_time),
        )

        # Test without buffer - should return both orders
        bid_orders_no_buffer = self.cache.own_bid_orders(instrument.id, accepted_buffer_ns=0)
        ask_orders_no_buffer = self.cache.own_ask_orders(instrument.id, accepted_buffer_ns=0)

        assert len(bid_orders_no_buffer) == 1
        assert len(ask_orders_no_buffer) == 1
        assert Decimal("1.00000") in bid_orders_no_buffer
        assert Decimal("1.10000") in ask_orders_no_buffer

        # Test validation: accepted_buffer_ns > 0 but ts_now == 0 should raise ValueError
        with pytest.raises(ValueError, match="ts_now must be provided when accepted_buffer_ns > 0"):
            self.cache.own_bid_orders(instrument.id, accepted_buffer_ns=2_000_000_000)

        with pytest.raises(ValueError, match="ts_now must be provided when accepted_buffer_ns > 0"):
            self.cache.own_ask_orders(instrument.id, accepted_buffer_ns=2_000_000_000)

        # Test with buffer and current time (should work properly now)
        current_time = self.clock.timestamp_ns()
        bid_orders_2s_buffer = self.cache.own_bid_orders(
            instrument.id,
            accepted_buffer_ns=2_000_000_000,
            ts_now=current_time,
        )
        ask_orders_2s_buffer = self.cache.own_ask_orders(
            instrument.id,
            accepted_buffer_ns=2_000_000_000,
            ts_now=current_time,
        )

        # With proper filtering, newer ask order should be excluded
        assert len(bid_orders_2s_buffer) == 1  # Bid order is older than 2s
        assert len(ask_orders_2s_buffer) == 0  # Ask order is newer than 2s
        assert Decimal("1.00000") in bid_orders_2s_buffer

        # Test with larger buffer - should include both orders
        bid_orders_10s_buffer = self.cache.own_bid_orders(
            instrument.id,
            accepted_buffer_ns=10_000_000_000,
            ts_now=current_time,
        )
        ask_orders_10s_buffer = self.cache.own_ask_orders(
            instrument.id,
            accepted_buffer_ns=10_000_000_000,
            ts_now=current_time,
        )

        assert len(bid_orders_10s_buffer) == 0  # Both orders are newer than 10s
        assert len(ask_orders_10s_buffer) == 0

        # Advance time and test again
        self.clock.advance_time(15_000_000_000)  # 15 seconds
        current_time = self.clock.timestamp_ns()

        bid_orders_final = self.cache.own_bid_orders(
            instrument.id,
            accepted_buffer_ns=8_000_000_000,
            ts_now=current_time,
        )
        ask_orders_final = self.cache.own_ask_orders(
            instrument.id,
            accepted_buffer_ns=8_000_000_000,
            ts_now=current_time,
        )

        assert len(bid_orders_final) == 1  # Both orders are now older than 8s
        assert len(ask_orders_final) == 1
        assert Decimal("1.00000") in bid_orders_final
        assert Decimal("1.10000") in ask_orders_final

        # Test with status filtering combined with buffer
        bid_orders_status_buffer = self.cache.own_bid_orders(
            instrument.id,
            status={OrderStatus.ACCEPTED},
            accepted_buffer_ns=8_000_000_000,
            ts_now=current_time,
        )
        ask_orders_status_buffer = self.cache.own_ask_orders(
            instrument.id,
            status={OrderStatus.ACCEPTED},
            accepted_buffer_ns=8_000_000_000,
            ts_now=current_time,
        )

        assert len(bid_orders_status_buffer) == 1
        assert len(ask_orders_status_buffer) == 1
        assert Decimal("1.00000") in bid_orders_status_buffer
        assert Decimal("1.10000") in ask_orders_status_buffer
