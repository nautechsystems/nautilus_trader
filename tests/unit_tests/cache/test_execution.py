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

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.config import LoggingConfig
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.actors import MockActor
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
BTCUSD_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestCache:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = Cache(
            database=None,
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

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    def test_cache_general_with_no_objects(self):
        # Arrange, Act
        self.cache.cache_general()

        # Assert
        assert True  # No exception raised

    def test_cache_currencies_with_no_currencies(self):
        # Arrange, Act
        self.cache.cache_currencies()

        # Assert
        assert True  # No exception raised

    def test_cache_instruments_with_no_instruments(self):
        # Arrange, Act
        self.cache.cache_instruments()

        # Assert
        assert True  # No exception raised

    def test_cache_accounts_with_no_accounts(self):
        # Arrange, Act
        self.cache.cache_accounts()

        # Assert
        assert True  # No exception raised

    def test_cache_orders_with_no_orders(self):
        # Arrange, Act
        self.cache.cache_orders()

        # Assert
        assert True  # No exception raised

    def test_cache_order_lists_with_no_orders(self):
        # Arrange, Act
        self.cache.cache_order_lists()

        # Assert
        assert True  # No exception raised

    def test_orders_for_position_when_no_position_returns_empty_list(self):
        # Arrange, Act
        result = self.cache.orders_for_position(PositionId("1"))

        # Assert
        assert result == []

    def test_cache_positions_with_no_positions(self):
        # Arrange, Act
        self.cache.cache_positions()

        # Assert
        assert True  # No exception raised

    def test_build_index_with_no_objects(self):
        # Arrange, Act
        self.cache.build_index()

        # Assert
        assert True  # No exception raised

    def test_add_currency(self):
        # Arrange
        currency = Currency(
            code="1INCH",
            precision=8,
            iso4217=0,
            name="1inch Network",
            currency_type=CurrencyType.CRYPTO,
        )

        # Act
        self.cache.add_currency(currency)

        # Assert
        assert Currency.from_str("1INCH") == currency

    def test_add_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act
        self.cache.add_account(account)

        # Assert
        assert self.cache.load_account(account.id) == account

    def test_load_instrument(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        # Act
        result = self.cache.load_instrument(AUDUSD_SIM.id)

        # Assert
        assert result == AUDUSD_SIM

    def test_load_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        self.cache.add_account(account)

        # Act
        result = self.cache.load_account(account.id)

        # Assert
        assert result == account

    def test_account_for_venue(self):
        # Arrange, Act
        result = self.cache.account_for_venue(Venue("SIM"))

        # Assert
        assert result is None

    def test_accounts_when_no_accounts_returns_empty_list(self):
        # Arrange, Act
        result = self.cache.accounts()

        # Assert
        assert result == []

    def test_get_actor_ids_with_no_ids_returns_empty_set(self):
        # Arrange, Act
        result = self.cache.actor_ids()

        # Assert
        assert result == set()

    def test_get_strategy_ids_with_no_ids_returns_empty_set(self):
        # Arrange, Act
        result = self.cache.strategy_ids()

        # Assert
        assert result == set()

    def test_get_exec_algorithm_ids_with_no_ids_returns_empty_set(self):
        # Arrange, Act
        result = self.cache.exec_algorithm_ids()

        # Assert
        assert result == set()

    def test_get_order_ids_with_no_ids_returns_empty_set(self):
        # Arrange, Act
        result = self.cache.client_order_ids()

        # Assert
        assert result == set()

    def test_get_actor_ids_with_id_returns_correct_set(self):
        # Arrange
        actor = MockActor()
        self.cache.update_actor(actor)

        # Act
        result = self.cache.actor_ids()

        # Assert
        assert result == {actor.id}

    def test_get_strategy_ids_with_id_returns_correct_set(self):
        # Arrange
        self.cache.update_strategy(self.strategy)

        # Act
        result = self.cache.strategy_ids()

        # Assert
        assert result == {self.strategy.id}

    def test_orders_for_exec_spawn_when_not_found(self):
        # Arrange, Act, Assert
        assert self.cache.orders_for_exec_spawn(ClientOrderId("O-UNKNOWN")) == []

    def test_orders_for_exec_algorithm_when_not_found(self):
        # Arrange, Act, Assert
        assert self.cache.orders_for_exec_algorithm(ExecAlgorithmId("UNKNOWN")) == []

    def test_exec_spawn_total_quantity_when_not_found(self):
        # Arrange, Act, Assert
        assert self.cache.exec_spawn_total_quantity(ClientOrderId("O-UNKNOWN")) is None

    def test_exec_spawn_total_filled_qty_when_not_found(self):
        # Arrange, Act, Assert
        assert self.cache.exec_spawn_total_filled_qty(ClientOrderId("O-UNKNOWN")) is None

    def test_exec_spawn_total_leaves_qty_when_not_found(self):
        # Arrange, Act, Assert
        assert self.cache.exec_spawn_total_leaves_qty(ClientOrderId("O-UNKNOWN")) is None

    def test_position_for_order_when_no_position_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.position_for_order(ClientOrderId("O-123456")) is None

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.position_exists(PositionId("P-123456"))

    def test_order_exists_when_no_order_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.order_exists(ClientOrderId("O-123456"))

    def test_order_list_exists_when_no_order_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.order_list_exists(OrderListId("OL-123456"))

    def test_order_list_ids_when_no_order_lists_returns_empty_set(self):
        # Arrange, Act
        result = self.cache.order_list_ids()

        # Assert
        assert result == set()

    def test_order_lists_when_no_order_lists_returns_empty_list(self):
        # Arrange, Act
        result = self.cache.order_lists()

        # Assert
        assert result == []

    def test_position_when_no_position_returns_none(self):
        # Arrange
        position_id = PositionId("P-123456")

        # Act
        result = self.cache.position(position_id)

        # Assert
        assert result is None

    def test_order_when_no_order_returns_none(self):
        # Arrange
        order_id = ClientOrderId("O-201908080101-000-001")

        # Act
        result = self.cache.order(order_id)

        # Assert
        assert result is None

    def test_strategy_id_for_position_when_no_strategy_registered_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.strategy_id_for_position(PositionId("P-123456")) is None

    def test_add_market_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            exec_algorithm_id=ExecAlgorithmId("SizeStagger"),
        )

        position_id = PositionId("P-1")

        # Act
        self.cache.add_order(order, position_id)

        # Assert
        assert order.client_order_id in self.cache.client_order_ids()
        assert order.client_order_id in self.cache.client_order_ids(
            instrument_id=order.instrument_id,
        )
        assert order.client_order_id in self.cache.client_order_ids(strategy_id=self.strategy.id)
        assert order.client_order_id not in self.cache.client_order_ids(
            strategy_id=StrategyId("S-ZX1"),
        )
        assert order.client_order_id in self.cache.client_order_ids(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order in self.cache.orders()
        assert order in self.cache.orders(side=OrderSide.BUY)
        assert order not in self.cache.orders(side=OrderSide.SELL)
        assert order not in self.cache.orders_inflight()
        assert order not in self.cache.orders_emulated()
        assert not self.cache.is_order_inflight(order.client_order_id)
        assert not self.cache.is_order_emulated(order.client_order_id)
        assert not self.cache.is_order_pending_cancel_local(order.client_order_id)
        assert self.cache.venue_order_id(order.client_order_id) is None
        assert order in self.cache.orders_for_exec_spawn(order.client_order_id)
        assert order in self.cache.orders_for_exec_algorithm(order.exec_algorithm_id)
        assert order in self.cache.orders_for_exec_algorithm(
            order.exec_algorithm_id,
            venue=order.venue,
            instrument_id=order.instrument_id,
            strategy_id=order.strategy_id,
            side=OrderSide.BUY,
        )
        assert order not in self.cache.orders_for_exec_algorithm(
            order.exec_algorithm_id,
            side=OrderSide.SELL,
        )
        assert order not in self.cache.orders_for_exec_algorithm(ExecAlgorithmId("UnknownAlgo"))

    def test_add_emulated_limit_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            emulation_trigger=TriggerType.BID_ASK,
            exec_algorithm_id=ExecAlgorithmId("ICE-SCALPER"),
        )

        position_id = PositionId("P-1")

        # Act
        self.cache.add_order(order, position_id)

        # Assert
        assert order.client_order_id in self.cache.client_order_ids_emulated()
        assert order in self.cache.orders_emulated()
        assert self.cache.is_order_emulated(order.client_order_id)
        assert self.cache.orders_emulated_count() == 1
        assert order.strategy_id in self.cache.strategy_ids()
        assert order.exec_algorithm_id in self.cache.exec_algorithm_ids()

    def test_load_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        # Act
        result = self.cache.load_order(order.client_order_id)

        # Assert
        assert result == order

    def test_add_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        self.cache.add_position(position, OmsType.HEDGING)

        # Assert
        assert self.cache.position_exists(position.id)
        assert position.id in self.cache.position_ids()
        assert position in self.cache.positions()
        assert position in self.cache.positions_open()
        assert position in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position in self.cache.positions_open(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert position not in self.cache.positions_closed()
        assert position not in self.cache.positions_closed(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_closed(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_closed(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert self.cache.position_for_order(order.client_order_id) == position
        assert self.cache.orders_for_position(position.id) == [order]

    def test_snapshot_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        self.cache.snapshot_position(position)
        self.cache.snapshot_position(position)
        snapshots = self.cache.position_snapshots(position.id)

        # Assert
        assert len(snapshots) == 2
        assert snapshots[0].id.value.startswith(position.id.value)
        snapshot_dict = snapshots[0].to_dict()
        del snapshot_dict["position_id"]
        position_dict = position.to_dict()
        del position_dict["position_id"]
        assert snapshot_dict == position_dict

    def test_snapshot_multiple_netted_positions(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )
        order2 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )
        order3 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )
        order4 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        self.cache.add_order(order2, position_id)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
            trade_id=TradeId("1"),
        )
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.10000"),
            trade_id=TradeId("2"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        position1.apply(fill2)
        self.cache.snapshot_position(position1)

        # Create new position (NETTING)
        self.cache.add_order(order3, position_id)
        self.cache.add_order(order4, position_id)

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.10000"),
            trade_id=TradeId("3"),
        )
        fill4 = TestEventStubs.order_filled(
            order4,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.30000"),
            trade_id=TradeId("4"),
        )

        # Act
        position2 = Position(instrument=AUDUSD_SIM, fill=fill3)
        position2.apply(fill4)
        self.cache.snapshot_position(position2)

        # Assert
        snapshots = self.cache.position_snapshots(position_id)
        assert len(snapshots) == 2
        assert position1.is_closed
        assert position2.is_closed
        assert position1.realized_return == pytest.approx(0.1)
        assert position2.realized_return == pytest.approx(0.1818181818)
        assert position1.realized_pnl == Money(9995.80, USD)
        assert position2.realized_pnl == Money(19995.20, USD)

    def test_load_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)
        self.cache.add_position(position, OmsType.HEDGING)

        # Act
        result = self.cache.load_position(position.id)

        # Assert
        assert result == position

    def test_update_order_for_submitted_order(self):
        # Arrange
        exec_algorithm_id = ExecAlgorithmId("AutoRetry")
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            exec_algorithm_id=exec_algorithm_id,
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        order.apply(TestEventStubs.order_submitted(order))

        # Act
        self.cache.update_order(order)

        # Assert
        assert self.cache.order_exists(order.client_order_id)
        assert order.client_order_id in self.cache.client_order_ids()
        assert order in self.cache.orders_for_exec_spawn(order.client_order_id)
        assert order in self.cache.orders_for_exec_algorithm(exec_algorithm_id)
        assert order in self.cache.orders()
        assert order in self.cache.orders_inflight()
        assert order in self.cache.orders_inflight(instrument_id=order.instrument_id)
        assert order in self.cache.orders_inflight(strategy_id=self.strategy.id)
        assert order in self.cache.orders_inflight(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_open()
        assert order not in self.cache.orders_open(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_open(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_open(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_closed()
        assert order not in self.cache.orders_closed(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_closed(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_closed(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )

        assert self.cache.orders_open_count() == 0
        assert self.cache.orders_closed_count() == 0
        assert self.cache.orders_emulated_count() == 0
        assert self.cache.orders_emulated_count(side=OrderSide.BUY) == 0
        assert self.cache.orders_emulated_count(side=OrderSide.SELL) == 0
        assert self.cache.orders_inflight_count() == 1
        assert self.cache.orders_inflight_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_inflight_count(side=OrderSide.SELL) == 0
        assert self.cache.orders_total_count() == 1
        assert self.cache.orders_total_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_total_count(side=OrderSide.SELL) == 0

    def test_update_order_for_accepted_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        order.apply(TestEventStubs.order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))

        # Act
        self.cache.update_order(order)

        # Assert
        assert self.cache.order_exists(order.client_order_id)
        assert order.client_order_id in self.cache.client_order_ids()
        assert order in self.cache.orders()
        assert order in self.cache.orders_open()
        assert order in self.cache.orders_open(instrument_id=order.instrument_id)
        assert order in self.cache.orders_open(strategy_id=self.strategy.id)
        assert order in self.cache.orders_open(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_closed()
        assert order not in self.cache.orders_closed(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_closed(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_closed(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_inflight()
        assert order not in self.cache.orders_inflight()
        assert order not in self.cache.orders_inflight(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_inflight(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_inflight(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )

        assert self.cache.orders_open_count() == 1
        assert self.cache.orders_open_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_open_count(side=OrderSide.SELL) == 0
        assert self.cache.orders_closed_count() == 0
        assert self.cache.orders_inflight_count() == 0
        assert self.cache.orders_total_count() == 1
        assert self.cache.orders_total_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_total_count(side=OrderSide.SELL) == 0

    def test_update_order_for_closed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)
        order.apply(TestEventStubs.order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))
        self.cache.update_order(order)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            last_px=Price.from_str("1.00001"),
        )

        order.apply(fill)

        # Act
        self.cache.update_order(order)

        # Assert
        assert self.cache.order_exists(order.client_order_id)
        assert order.client_order_id in self.cache.client_order_ids()
        assert order in self.cache.orders()
        assert order not in self.cache.orders_open()
        assert order not in self.cache.orders_open(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_open(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_open(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order in self.cache.orders_closed()
        assert order in self.cache.orders_closed(instrument_id=order.instrument_id)
        assert order in self.cache.orders_closed(strategy_id=self.strategy.id)
        assert order in self.cache.orders_closed(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert order not in self.cache.orders_inflight()
        assert order not in self.cache.orders_inflight(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_inflight(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_inflight(
            instrument_id=order.instrument_id,
            strategy_id=self.strategy.id,
        )

        assert self.cache.venue_order_id(order.client_order_id) == order.venue_order_id
        assert self.cache.orders_open_count() == 0
        assert self.cache.orders_closed_count() == 1
        assert self.cache.orders_closed_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_closed_count(side=OrderSide.SELL) == 0
        assert self.cache.orders_emulated_count() == 0
        assert self.cache.orders_inflight_count() == 0
        assert self.cache.orders_total_count() == 1
        assert self.cache.orders_total_count(side=OrderSide.BUY) == 1
        assert self.cache.orders_total_count(side=OrderSide.SELL) == 0

    def test_update_position_for_open_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        # Act
        self.cache.add_position(position, OmsType.HEDGING)

        # Assert
        assert self.cache.position_exists(position.id)
        assert position.id in self.cache.position_ids()
        assert position in self.cache.positions()
        assert position in self.cache.positions_open()
        assert position in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position in self.cache.positions_open(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert position not in self.cache.positions_closed()
        assert position not in self.cache.positions_closed(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_closed(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_closed(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert self.cache.position(position_id) == position
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0
        assert self.cache.positions_total_count() == 1

    def test_update_position_for_closed_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position, OmsType.HEDGING)

        order2 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )
        self.cache.add_order(order2, position_id)

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")))
        self.cache.update_order(order2)
        order2_filled = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00001"),
        )

        position.apply(order2_filled)

        # Act
        self.cache.update_position(position)

        # Assert
        assert self.cache.position_exists(position.id)
        assert position.id in self.cache.position_ids()
        assert position in self.cache.positions()
        assert position in self.cache.positions_closed()
        assert position in self.cache.positions_closed(instrument_id=position.instrument_id)
        assert position in self.cache.positions_closed(strategy_id=self.strategy.id)
        assert position in self.cache.positions_closed(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert position not in self.cache.positions_open()
        assert position not in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_open(
            instrument_id=position.instrument_id,
            strategy_id=self.strategy.id,
        )
        assert self.cache.position(position_id) == position
        assert self.cache.positions_open_count() == 0
        assert self.cache.positions_closed_count() == 1
        assert self.cache.positions_total_count() == 1
        assert self.cache.position_for_order(order1.client_order_id) == position
        assert self.cache.position_for_order(order2.client_order_id) == position
        assert order1 in self.cache.orders_for_position(position.id)
        assert order2 in self.cache.orders_for_position(position.id)

    def test_positions_queries_with_multiple_open_returns_expected_positions(self):
        # Arrange
        # -- Position 1 --------------------------------------------------------
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position1, OmsType.HEDGING)

        # -- Position 2 --------------------------------------------------------

        order2 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")))
        self.cache.update_order(order2)
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=GBPUSD_SIM,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00001"),
        )

        position2 = Position(instrument=GBPUSD_SIM, fill=fill2)
        self.cache.add_position(position2, OmsType.HEDGING)

        # Assert
        assert position1.is_open
        assert position2.is_open
        assert position1 in self.cache.positions()
        assert position2 in self.cache.positions()
        assert self.cache.positions(
            venue=AUDUSD_SIM.venue,
            instrument_id=AUDUSD_SIM.id,
        ) == [position1]
        assert self.cache.positions(
            venue=GBPUSD_SIM.venue,
            instrument_id=GBPUSD_SIM.id,
        ) == [position2]
        assert self.cache.positions(instrument_id=GBPUSD_SIM.id, side=PositionSide.LONG) == [
            position2,
        ]
        assert self.cache.positions(instrument_id=AUDUSD_SIM.id, side=PositionSide.LONG) == [
            position1,
        ]
        assert self.cache.positions(instrument_id=GBPUSD_SIM.id, side=PositionSide.LONG) == [
            position2,
        ]
        assert self.cache.positions_open(instrument_id=AUDUSD_SIM.id, side=PositionSide.LONG) == [
            position1,
        ]
        assert self.cache.positions_open(instrument_id=GBPUSD_SIM.id, side=PositionSide.LONG) == [
            position2,
        ]
        assert position1 in self.cache.positions_open()
        assert position2 in self.cache.positions_open()
        assert position1 not in self.cache.positions_closed()
        assert position2 not in self.cache.positions_closed()

    def test_positions_queries_with_one_closed_returns_expected_positions(self):
        # Arrange
        # -- Position 1 --------------------------------------------------------
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position1, OmsType.HEDGING)

        # -- Position 2 --------------------------------------------------------

        order2 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")))
        self.cache.update_order(order2)
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=GBPUSD_SIM,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00001"),
        )

        position2 = Position(instrument=GBPUSD_SIM, fill=fill2)
        self.cache.add_position(position2, OmsType.HEDGING)

        order3 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        order3.apply(TestEventStubs.order_submitted(order3))
        self.cache.update_order(order3)

        order3.apply(TestEventStubs.order_accepted(order3, venue_order_id=VenueOrderId("3")))
        self.cache.update_order(order3)
        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=GBPUSD_SIM,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00001"),
        )

        position2.apply(fill3)
        self.cache.update_position(position2)

        # Assert
        assert position1.is_open
        assert position2.is_closed
        assert position1 in self.cache.positions()
        assert position1 in self.cache.positions(instrument_id=AUDUSD_SIM.id)
        assert position2 in self.cache.positions()
        assert position2 in self.cache.positions(instrument_id=GBPUSD_SIM.id)
        assert self.cache.positions_open(venue=BTCUSD_BINANCE.venue, side=PositionSide.LONG) == []
        assert self.cache.positions_open(venue=AUDUSD_SIM.venue, side=PositionSide.LONG) == [
            position1,
        ]
        assert (
            self.cache.positions_open(instrument_id=BTCUSD_BINANCE.id, side=PositionSide.LONG) == []
        )
        assert self.cache.positions_open(instrument_id=AUDUSD_SIM.id, side=PositionSide.LONG) == [
            position1,
        ]
        assert self.cache.positions_open(instrument_id=GBPUSD_SIM.id, side=PositionSide.LONG) == []
        assert self.cache.positions_closed(instrument_id=AUDUSD_SIM.id) == []
        assert self.cache.positions_closed(venue=GBPUSD_SIM.venue) == [position2]
        assert self.cache.positions_closed(instrument_id=GBPUSD_SIM.id) == [position2]

    def test_update_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        self.cache.add_account(account)

        # Act
        self.cache.update_account(account)

        # Assert
        assert True  # No exceptions raised

    def test_delete_actor(self):
        # Arrange
        actor = MockActor()
        self.cache.update_actor(actor)

        # Act
        self.cache.delete_actor(actor)

        # Assert
        assert actor.id not in self.cache.actor_ids()

    def test_delete_strategy(self):
        # Arrange
        self.cache.update_strategy(self.strategy)

        # Act
        self.cache.delete_strategy(self.strategy)

        # Assert
        assert self.strategy.id not in self.cache.strategy_ids()

    def test_check_residuals(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1, OmsType.HEDGING)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")))
        self.cache.update_order(order2)

        # Act
        self.cache.check_residuals()

        # Assert
        assert True  # No exception raised

    def test_reset(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )
        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1, OmsType.HEDGING)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)

        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")))
        self.cache.update_order(order2)

        self.cache.update_order(order2)

        # Act
        self.cache.reset()

        # Assert
        assert len(self.cache.actor_ids()) == 0
        assert len(self.cache.strategy_ids()) == 0
        assert len(self.cache.exec_algorithm_ids()) == 0
        assert self.cache.orders_total_count() == 0
        assert self.cache.positions_total_count() == 0

    def test_flush_db(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1, OmsType.HEDGING)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)
        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")))
        self.cache.update_order(order2)

        # Act
        self.cache.reset()
        self.cache.flush_db()

        # Assert
        assert True  # No exception raised

    def test_purge_closed_orders_when_empty_does_nothing(self):
        # Arrange, Act, Assert
        self.cache.purge_closed_orders(ts_now=0)

    def test_purge_closed_positions_when_empty_does_nothing(self):
        # Arrange, Act, Assert
        self.cache.purge_closed_positions(ts_now=0)

    def test_purge_order_when_empty_does_nothing(self):
        # Arrange, Act, Assert
        self.cache.purge_order(client_order_id=ClientOrderId("O-123456789"))

    def test_purge_position_when_empty_does_nothing(self):
        # Arrange, Act, Assert
        self.cache.purge_position(position_id=PositionId("P-1"))

    def test_purge_closed_orders(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )
        order1.apply(fill1)
        self.cache.update_order(order1)

        position = Position(AUDUSD_SIM, fill1)
        self.cache.add_position(position, OmsType.NETTING)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)
        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")))
        self.cache.update_order(order2)

        # Act
        self.cache.purge_closed_orders(ts_now=0)

        # Assert
        assert not self.cache.order_exists(order1.client_order_id)
        assert self.cache.order(order1.client_order_id) is None
        assert self.cache.venue_order_id(order1.client_order_id) is None
        assert order1 not in self.cache.orders_open()
        assert order1 not in self.cache.orders_closed()
        assert self.cache.orders_total_count() == 1
        assert self.cache.orders_closed_count() == 0
        assert len(position.events) == 0  # <-- Events for order were purged

    def test_purge_closed_positions(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )
        order1.apply(fill1)
        self.cache.update_order(order1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)
        order2.apply(TestEventStubs.order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")))
        self.cache.update_order(order2)

        # Act
        self.cache.purge_closed_positions(ts_now=0)

        # Assert
        assert not self.cache.position_exists(position1_id)
        assert self.cache.position(position1_id) is None
        assert position1_id not in self.cache.positions_open()
        assert position1_id not in self.cache.positions_closed()
        assert self.cache.positions_total_count() == 0
        assert self.cache.positions_closed_count() == 0


class TestExecutionCacheIntegrityCheck:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
        )
        self.engine = BacktestEngine(config=config)

        # Set up venue
        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[],
        )

        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        # Set up data
        wrangler = QuoteTickDataWrangler(self.usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv"),
            ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv"),
        )
        self.engine.add_instrument(self.usdjpy)
        self.engine.add_data(ticks)

    def test_exec_cache_check_integrity_when_index_cleared_fails(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=self.usdjpy.id,
            bar_type=BarType.from_str("USD/JPY.SIM-15-MINUTE-BID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Generate a lot of data
        self.engine.run()

        # Clear index
        self.engine.cache.clear_index()

        # Act, Assert
        assert not self.engine.cache.check_integrity()
