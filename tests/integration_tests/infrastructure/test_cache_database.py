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

import sys
from decimal import Decimal

import pytest
import redis

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import CacheDatabaseConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.infrastructure.cache import RedisCacheDatabase
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.serialization.msgpack.serializer import MsgPackSerializer
from nautilus_trader.test_kit.mocks.actors import MockActor
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")

# Requirements:
# - A Redis instance listening on the default port 6379

pytestmark = pytest.mark.skipif(
    sys.platform == "win32",
    reason="not longer testing with Memurai database",
)


class TestRedisCacheDatabase:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
            bypass=True,
        )

        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()

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
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.database = RedisCacheDatabase(
            trader_id=self.trader_id,
            logger=self.logger,
            serializer=MsgPackSerializer(timestamps_as_str=True),
        )

        self.test_redis = redis.Redis(host="localhost", port=6379, db=0)

    def teardown(self):
        # Tests will start failing if redis is not flushed on tear down
        self.test_redis.flushall()  # Comment this line out to preserve data between tests

    def test_load_general_objects_when_nothing_in_cache_returns_empty_dict(self):
        # Arrange, Act
        result = self.database.load()

        # Assert
        assert result == {}

    def test_add_general_object_adds_to_cache(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()
        key = str(bar.bar_type) + "-" + str(bar.ts_event)

        # Act
        self.database.add(key, str(bar).encode())

        # Assert
        assert self.database.load() == {key: str(bar).encode()}

    def test_add_currency(self):
        # Arrange
        currency = Currency(
            code="1INCH",
            precision=8,
            iso4217=0,
            name="1INCH",
            currency_type=CurrencyType.CRYPTO,
        )

        # Act
        self.database.add_currency(currency)

        # Assert
        assert self.database.load_currency(currency.code) == currency

    def test_add_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act
        self.database.add_account(account)

        # Assert
        assert self.database.load_account(account.id) == account

    def test_add_instrument(self):
        # Arrange, Act
        self.database.add_instrument(AUDUSD_SIM)

        # Assert
        assert self.database.load_instrument(AUDUSD_SIM.id) == AUDUSD_SIM

    def test_add_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        self.database.add_order(order)

        # Assert
        assert self.database.load_order(order.client_order_id) == order

    def test_add_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_instrument(AUDUSD_SIM)
        self.database.add_order(order)

        position_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        self.database.add_position(position)

        # Assert
        assert self.database.load_position(position.id) == position

    def test_add_submit_order_command(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.database.add_submit_order_command(command)

        # Act
        result = self.database.load_submit_order_command(order.client_order_id)

        # Assert
        assert result == command

    def test_update_account(self):
        # Arrange
        account = TestExecStubs.cash_account()
        self.database.add_account(account)

        # Act
        self.database.update_account(account)

        # Assert
        assert self.database.load_account(account.id) == account

    def test_update_order_when_not_already_exists_logs(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        # Act
        self.database.update_order(order)

        # Assert
        assert True  # No exceptions raised

    def test_update_order_for_open_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order)

        order.apply(TestEventStubs.order_submitted(order))
        self.database.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))

        # Act
        self.database.update_order(order)

        # Assert
        assert self.database.load_order(order.client_order_id) == order

    def test_update_order_for_closed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        order.apply(TestEventStubs.order_submitted(order))
        self.database.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))
        self.database.update_order(order)

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            last_px=Price.from_str("1.00001"),
        )

        order.apply(fill)

        # Act
        self.database.update_order(order)

        # Assert
        assert self.database.load_order(order.client_order_id) == order

    def test_update_position_for_closed_position(self):
        # Arrange
        self.database.add_instrument(AUDUSD_SIM)

        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.database.add_order(order1)

        order1.apply(TestEventStubs.order_submitted(order1))
        self.database.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.database.update_order(order1)

        order1.apply(
            TestEventStubs.order_filled(
                order1,
                instrument=AUDUSD_SIM,
                position_id=position_id,
                last_px=Price.from_str("1.00001"),
            ),
        )
        self.database.update_order(order1)

        # Act
        position = Position(instrument=AUDUSD_SIM, fill=order1.last_event)
        self.database.add_position(position)

        order2 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order2)

        order2.apply(TestEventStubs.order_submitted(order2))
        self.database.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2))
        self.database.update_order(order2)

        filled = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00001"),
        )

        order2.apply(filled)
        self.database.update_order(order2)

        position.apply(filled)

        # Act
        self.database.update_position(position)

        # Assert
        assert self.database.load_position(position.id) == position

    def test_update_position_when_not_already_exists_logs(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        position_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        self.database.update_position(position)

        # Assert
        assert True  # No exception raised

    def test_update_actor(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        self.database.update_actor(actor)
        result = self.database.load_actor(actor.id)

        # Assert
        assert result == {"A": b"1"}

    def test_update_strategy(self):
        # Arrange
        strategy = MockStrategy(TestDataStubs.bartype_btcusdt_binance_100tick_last())
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        self.database.update_strategy(strategy)
        result = self.database.load_strategy(strategy.id)

        # Assert
        assert result == {"UserState": b"1"}

    def test_load_currency_when_no_currencies_in_database_returns_none(self):
        # Arrange, Act
        result = self.database.load_currency("ONEINCH")

        # Assert
        assert result is None

    def test_load_currency_when_currency_in_database_returns_expected(self):
        # Arrange
        aud = Currency.from_str("AUD")
        self.database.add_currency(aud)

        # Act
        result = self.database.load_currency("AUD")

        # Assert
        assert result == aud

    def test_load_currencies_when_currencies_in_database_returns_expected(self):
        # Arrange
        aud = Currency.from_str("AUD")
        self.database.add_currency(aud)

        # Act
        result = self.database.load_currencies()

        # Assert
        assert result == {"AUD": aud}

    def test_load_instrument_when_no_instrument_in_database_returns_none(self):
        # Arrange, Act
        result = self.database.load_instrument(AUDUSD_SIM.id)

        # Assert
        assert result is None

    def test_load_instrument_when_instrument_in_database_returns_expected(self):
        # Arrange
        self.database.add_instrument(AUDUSD_SIM)

        # Act
        result = self.database.load_instrument(AUDUSD_SIM.id)

        # Assert
        assert result == AUDUSD_SIM

    def test_load_instruments_when_instrument_in_database_returns_expected(self):
        # Arrange
        self.database.add_instrument(AUDUSD_SIM)

        # Act
        result = self.database.load_instruments()

        # Assert
        assert result == {AUDUSD_SIM.id: AUDUSD_SIM}

    def test_load_account_when_no_account_in_database_returns_none(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act
        result = self.database.load_account(account.id)

        # Assert
        assert result is None

    def test_load_account_when_account_in_database_returns_account(self):
        # Arrange
        account = TestExecStubs.cash_account()
        self.database.add_account(account)

        # Act
        result = self.database.load_account(account.id)

        # Assert
        assert result == account

    def test_load_order_when_no_order_in_database_returns_none(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result is None

    def test_load_order_when_market_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order

    def test_load_order_when_limit_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order

    def test_load_order_when_transformed_to_market_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order = MarketOrder.transform_py(order, 0)

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order
        assert result.order_type == OrderType.MARKET

    def test_load_order_when_transformed_to_limit_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.limit_if_touched(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
        )

        order = LimitOrder.transform_py(order, 0)

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order
        assert result.order_type == OrderType.LIMIT

    def test_load_order_when_stop_market_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order

    def test_load_order_when_stop_limit_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.00010"),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order
        assert result.price == order.price
        assert result.trigger_price == order.trigger_price

    def test_load_position_when_no_position_in_database_returns_none(self):
        # Arrange
        position_id = PositionId("P-1")

        # Act
        result = self.database.load_position(position_id)

        # Assert
        assert result is None

    def test_load_position_when_instrument_in_database_returns_none(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        position_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)
        self.database.add_position(position)

        # Act
        result = self.database.load_position(position.id)

        # Assert
        assert result is None

    def test_load_position_when_position_in_database_returns_position(self):
        # Arrange
        self.database.add_instrument(AUDUSD_SIM)

        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        position_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        self.database.add_position(position)

        # Act
        result = self.database.load_position(position_id)

        # Assert
        assert result == position

    def test_load_accounts_when_no_accounts_returns_empty_dict(self):
        # Arrange, Act
        result = self.database.load_accounts()

        # Assert
        assert result == {}

    def test_load_accounts_cache_when_one_account_in_database(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act
        self.database.add_account(account)

        # Assert
        assert self.database.load_accounts() == {account.id: account}

    def test_load_orders_cache_when_no_orders(self):
        # Arrange, Act
        self.database.load_orders()

        # Assert
        assert self.database.load_orders() == {}

    def test_load_orders_cache_when_one_order_in_database(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_orders()

        # Assert
        assert result == {order.client_order_id: order}

    def test_load_positions_cache_when_no_positions(self):
        # Arrange, Act
        self.database.load_positions()

        # Assert
        assert self.database.load_positions() == {}

    def test_load_positions_cache_when_one_position_in_database(self):
        # Arrange
        self.database.add_instrument(AUDUSD_SIM)

        order1 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order1)

        position_id = PositionId("P-1")
        order1.apply(TestEventStubs.order_submitted(order1))
        order1.apply(TestEventStubs.order_accepted(order1))
        order1.apply(
            TestEventStubs.order_filled(
                order1,
                instrument=AUDUSD_SIM,
                position_id=position_id,
                last_px=Price.from_str("1.00001"),
            ),
        )

        position = Position(instrument=AUDUSD_SIM, fill=order1.last_event)
        self.database.add_position(position)

        # Act
        result = self.database.load_positions()

        # Assert
        assert result == {position.id: position}

    def test_delete_actor(self):
        # Arrange, Act
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.database.update_actor(actor)

        # Act
        self.database.delete_actor(actor.id)
        result = self.database.load_actor(actor.id)

        # Assert
        assert result == {}

    def test_delete_strategy(self):
        # Arrange, Act
        strategy = MockStrategy(TestDataStubs.bartype_btcusdt_binance_100tick_last())
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.database.update_strategy(strategy)

        # Act
        self.database.delete_strategy(self.strategy.id)
        result = self.database.load_strategy(self.strategy.id)

        # Assert
        assert result == {}

    def test_load_submit_order_command_when_not_in_database(self):
        # Arrange, Act
        result = self.cache.load_submit_order_command(ClientOrderId("O-123456789"))

        # Assert
        assert result is None

    def test_load_submit_order_list_command_when_not_in_database(self):
        # Arrange, Act
        result = self.cache.load_submit_order_list_command(OrderListId("1"))

        # Assert
        assert result is None

    def test_load_submit_order_command(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.database.add_submit_order_command(command)
        self.cache.add_submit_order_command(command)

        # Act
        result = self.cache.load_submit_order_command(order.client_order_id)

        # Assert
        assert result == command

    def test_load_submit_order_list_command(self):
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

        bracket = order_factory.bracket(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00100"),
            entry_exec_algorithm_id=ExecAlgorithmId("VWAP"),
            entry_exec_algorithm_params={"max_percentage": 100.0, "start": 0, "end": 1},
        )

        command = SubmitOrderList(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            order_list=bracket,
            position_id=PositionId("P-001"),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.database.add_submit_order_list_command(command)
        self.cache.add_submit_order_list_command(command)

        # Act
        result = self.cache.load_submit_order_list_command(bracket.id)

        # Assert
        assert result == command

    def test_flush(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order1)

        position1_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill)
        self.database.update_order(order1)
        self.database.add_position(position1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order2)

        order2.apply(TestEventStubs.order_submitted(order2))
        order2.apply(TestEventStubs.order_accepted(order2))

        self.database.update_order(order2)

        # Act
        self.database.flush()

        # Assert
        assert self.database.load_order(order1.client_order_id) is None
        assert self.database.load_order(order2.client_order_id) is None
        assert self.database.load_position(position1.id) is None


class TestRedisCacheDatabaseIntegrity:
    def setup(self):
        # Fixture Setup
        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
            cache_database=CacheDatabaseConfig(),  # default redis
        )

        self.engine = BacktestEngine(config=config)
        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[],
        )

        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        wrangler = QuoteTickDataWrangler(self.usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv"),
            ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv"),
        )
        self.engine.add_instrument(self.usdjpy)
        self.engine.add_data(ticks)

        self.test_redis = redis.Redis(host="localhost", port=6379, db=0)

    def teardown(self):
        # Tests will start failing if redis is not flushed on tear down
        self.test_redis.flushall()  # Comment this line out to preserve data between tests

    def test_rerunning_backtest_with_redis_db_builds_correct_index(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.usdjpy.id),
            bar_type=str(TestDataStubs.bartype_usdjpy_1min_bid()),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        # Generate a lot of data
        self.engine.run()

        # Reset engine
        self.engine.reset()

        # Act
        self.engine.run()

        # Assert
        assert self.engine.cache.check_integrity()
