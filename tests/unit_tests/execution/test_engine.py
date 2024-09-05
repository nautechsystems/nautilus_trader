# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import InvalidConfiguration
from nautilus_trader.config import StrategyConfig
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
from nautilus_trader.model.enums import PositionSide
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
    def setup(self):
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

        config = ExecEngineConfig(debug=True)
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
            None,
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            UUID4(),
            self.clock.timestamp_ns(),
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
