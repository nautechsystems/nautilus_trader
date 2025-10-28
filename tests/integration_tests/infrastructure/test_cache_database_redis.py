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

import asyncio
import sys
import time
from decimal import Decimal

import msgspec
import pytest

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.cache.database import CacheDatabaseAdapter
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import DatabaseConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.serialization.serializer import MsgSpecSerializer
from nautilus_trader.test_kit.functions import eventually
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


_AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")

# Requirements:
# - A Redis service listening on the default port 6379

pytestmark = pytest.mark.skipif(
    sys.platform != "linux",
    reason="databases only supported on Linux",
)


@pytest.mark.xdist_group(name="redis_integration")
class TestCacheDatabaseAdapter:
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

        self.database = CacheDatabaseAdapter(
            trader_id=self.trader_id,
            instance_id=UUID4(),
            serializer=MsgSpecSerializer(encoding=msgspec.msgpack, timestamps_as_str=True),
            config=CacheConfig(database=DatabaseConfig()),
        )

    def teardown(self):
        # Tests will fail if Redis is not flushed on tear down
        time.sleep(0.2)
        self.database.flush()  # Comment this line out to preserve data between tests for debugging
        time.sleep(0.5)  # Ensure clean slate

    @pytest.mark.asyncio
    async def test_load_general_objects_when_nothing_in_cache_returns_empty_dict(self):
        # Arrange, Act
        result = self.database.load()

        # Assert
        assert result == {}

    @pytest.mark.asyncio
    async def test_add_general_object_adds_to_cache(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()
        key = str(bar.bar_type) + "-" + str(bar.ts_event)

        # Act
        self.database.add(key, str(bar).encode())

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load())

        # Assert
        assert self.database.load() == {key: str(bar).encode()}

    @pytest.mark.asyncio
    async def test_add_currency(self):
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

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_currency(currency.code))

        # Assert
        assert self.database.load_currency(currency.code) == currency

    @pytest.mark.asyncio
    async def test_add_account(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act
        self.database.add_account(account)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_account(account.id))

        # Assert
        assert self.database.load_account(account.id) == account

    @pytest.mark.asyncio
    async def test_add_instrument(self):
        # Arrange, Act
        self.database.add_instrument(_AUDUSD_SIM)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(_AUDUSD_SIM.id))

        # Assert
        assert self.database.load_instrument(_AUDUSD_SIM.id) == _AUDUSD_SIM

    @pytest.mark.asyncio
    async def test_add_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Assert
        assert self.database.load_order(order.client_order_id) == order

    @pytest.mark.asyncio
    async def test_add_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_instrument(_AUDUSD_SIM)
        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        position_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order,
            instrument=_AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=_AUDUSD_SIM, fill=fill)

        # Act
        self.database.add_position(position)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_position(position.id))

        # Assert
        assert self.database.load_position(position.id) == position

    @pytest.mark.asyncio
    async def test_update_account(self):
        # Arrange
        account = TestExecStubs.cash_account()
        self.database.add_account(account)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_account(account.id))

        # Act
        self.database.update_account(account)

        # Assert
        assert self.database.load_account(account.id) == account

    @pytest.mark.asyncio
    async def test_update_order_when_not_already_exists_logs(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        # Act
        self.database.update_order(order)

        # Assert
        assert True  # No exceptions raised

    @pytest.mark.asyncio
    async def test_update_order_for_open_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        order.apply(TestEventStubs.order_submitted(order))
        self.database.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))

        # Act
        self.database.update_order(order)

        # Assert
        assert self.database.load_order(order.client_order_id) == order

    @pytest.mark.asyncio
    async def test_update_order_for_closed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        order.apply(TestEventStubs.order_submitted(order))
        self.database.update_order(order)

        order.apply(TestEventStubs.order_accepted(order))
        self.database.update_order(order)

        fill = TestEventStubs.order_filled(
            order,
            instrument=_AUDUSD_SIM,
            last_px=Price.from_str("1.00001"),
        )

        order.apply(fill)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Act
        self.database.update_order(order)

        # Assert
        assert self.database.load_order(order.client_order_id) == order

    @pytest.mark.asyncio
    async def test_update_position_for_closed_position(self):
        # Arrange
        self.database.add_instrument(_AUDUSD_SIM)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(_AUDUSD_SIM.id))

        order1 = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        position_id = PositionId("P-1")
        self.database.add_order(order1)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order1.client_order_id))

        order1.apply(TestEventStubs.order_submitted(order1))
        self.database.update_order(order1)

        order1.apply(TestEventStubs.order_accepted(order1))
        self.database.update_order(order1)

        order1.apply(
            TestEventStubs.order_filled(
                order1,
                instrument=_AUDUSD_SIM,
                position_id=position_id,
                last_px=Price.from_str("1.00001"),
                trade_id=TradeId("1"),
            ),
        )
        self.database.update_order(order1)

        # Allow MPSC thread to update
        await eventually(lambda: self.database.load_order(order1.client_order_id))

        # Act
        position = Position(instrument=_AUDUSD_SIM, fill=order1.last_event)
        self.database.add_position(position)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_position(position.id))

        order2 = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order2)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order2.client_order_id))

        order2.apply(TestEventStubs.order_submitted(order2))
        self.database.update_order(order2)

        order2.apply(TestEventStubs.order_accepted(order2))
        self.database.update_order(order2)

        filled = TestEventStubs.order_filled(
            order2,
            instrument=_AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00001"),
            trade_id=TradeId("2"),
        )

        order2.apply(filled)
        self.database.update_order(order2)

        position.apply(filled)

        # Act
        self.database.update_position(position)

        # Allow MPSC thread to update
        await eventually(lambda: self.database.load_position(position.id))

        # Assert
        assert self.database.load_position(position.id) == position

    @pytest.mark.asyncio
    async def test_update_position_when_not_already_exists_logs(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        position_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order,
            instrument=_AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=_AUDUSD_SIM, fill=fill)

        # Act
        self.database.update_position(position)

        # Assert
        assert True  # No exception raised

    @pytest.mark.asyncio
    async def test_update_actor(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        self.database.update_actor(actor)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_actor(actor.id))

        result = self.database.load_actor(actor.id)

        # Assert
        assert result == {"A": 1}

    @pytest.mark.asyncio
    async def test_update_strategy(self):
        # Arrange
        strategy = MockStrategy(TestDataStubs.bartype_btcusdt_binance_100tick_last())
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        self.database.update_strategy(strategy)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_strategy(strategy.id))

        result = self.database.load_strategy(strategy.id)

        # Assert
        assert result == {"UserState": b"1"}

    @pytest.mark.asyncio
    async def test_load_currency_when_no_currencies_in_database_returns_none(self):
        # Arrange, Act
        result = self.database.load_currency("ONEINCH")

        # Assert
        assert result is None

    @pytest.mark.asyncio
    async def test_load_currency_when_currency_in_database_returns_expected(self):
        # Arrange
        aud = Currency.from_str("AUD")
        self.database.add_currency(aud)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_currency("AUD"))

        # Act
        result = self.database.load_currency("AUD")

        # Assert
        assert result == aud

    @pytest.mark.asyncio
    async def test_load_currencies_when_currencies_in_database_returns_expected(self):
        # Arrange
        aud = Currency.from_str("AUD")
        self.database.add_currency(aud)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_currencies())

        # Act
        result = self.database.load_currencies()

        # Assert
        assert result == {"AUD": aud}

    @pytest.mark.asyncio
    async def test_load_instrument_when_no_instrument_in_database_returns_none(self):
        # Arrange, Act
        result = self.database.load_instrument(_AUDUSD_SIM.id)

        # Assert
        assert result is None

    @pytest.mark.asyncio
    async def test_load_instrument_when_instrument_in_database_returns_expected(self):
        # Arrange
        self.database.add_instrument(_AUDUSD_SIM)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(_AUDUSD_SIM.id))

        # Act
        result = self.database.load_instrument(_AUDUSD_SIM.id)

        # Assert
        assert result == _AUDUSD_SIM

    @pytest.mark.asyncio
    async def test_load_instruments_when_instrument_in_database_returns_expected(self):
        # Arrange
        self.database.add_instrument(_AUDUSD_SIM)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instruments())

        # Act
        result = self.database.load_instruments()

        # Assert
        assert result == {_AUDUSD_SIM.id: _AUDUSD_SIM}

    @pytest.mark.asyncio
    async def test_load_synthetic_when_no_synthetic_instrument_in_database_returns_none(self):
        # Arrange
        synthetic = TestInstrumentProvider.synthetic_instrument()

        # Act
        result = self.database.load_synthetic(synthetic.id)

        # Assert
        assert result is None

    @pytest.mark.asyncio
    async def test_load_synthetic_when_synthetic_instrument_in_database_returns_expected(self):
        # Arrange
        synthetic = TestInstrumentProvider.synthetic_instrument()
        self.database.add_synthetic(synthetic)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_synthetic(synthetic.id))

        # Act
        result = self.database.load_synthetic(synthetic.id)

        # Assert
        assert result == synthetic

    @pytest.mark.asyncio
    async def test_load_account_when_no_account_in_database_returns_none(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act
        result = self.database.load_account(account.id)

        # Assert
        assert result is None

    @pytest.mark.asyncio
    async def test_load_account_when_account_in_database_returns_account(self):
        # Arrange
        account = TestExecStubs.cash_account()
        self.database.add_account(account)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_account(account.id))

        # Act
        result = self.database.load_account(account.id)

        # Assert
        assert result == account

    @pytest.mark.asyncio
    async def test_load_order_when_no_order_in_database_returns_none(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result is None

    @pytest.mark.asyncio
    async def test_load_order_when_market_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order

    @pytest.mark.asyncio
    async def test_load_order_with_exec_algorithm_params(self):
        # Arrange
        exec_algorithm_params = {"horizon_secs": 20, "interval_secs": 2.5}
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
            exec_algorithm_params=exec_algorithm_params,
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order
        assert result.exec_algorithm_params

    @pytest.mark.asyncio
    async def test_load_order_when_limit_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order

    @pytest.mark.asyncio
    async def test_load_order_when_transformed_to_market_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order = MarketOrder.transform_py(order, 0)

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order
        assert result.order_type == OrderType.MARKET

    @pytest.mark.asyncio
    async def test_load_order_when_transformed_to_limit_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.limit_if_touched(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            Price.from_str("1.00000"),
        )

        order = LimitOrder.transform_py(order, 0)

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order
        assert result.order_type == OrderType.LIMIT

    @pytest.mark.asyncio
    async def test_load_order_when_stop_market_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order

    @pytest.mark.asyncio
    async def test_load_order_when_stop_limit_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_limit(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.00010"),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        # Act
        result = self.database.load_order(order.client_order_id)

        # Assert
        assert result == order
        assert result.price == order.price
        assert result.trigger_price == order.trigger_price

    @pytest.mark.asyncio
    async def test_load_position_when_no_position_in_database_returns_none(self):
        # Arrange
        position_id = PositionId("P-1")

        # Act
        result = self.database.load_position(position_id)

        # Assert
        assert result is None

    @pytest.mark.asyncio
    async def test_load_position_when_no_instrument_in_database_returns_none(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        position_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order,
            instrument=_AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=_AUDUSD_SIM, fill=fill)
        self.database.add_position(position)

        # Act
        result = self.database.load_position(position.id)

        # Assert
        assert result is None

    @pytest.mark.asyncio
    async def test_load_position_when_position_in_database_returns_position(self):
        # Arrange
        self.database.add_instrument(_AUDUSD_SIM)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(_AUDUSD_SIM.id))

        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_order(order.client_order_id))

        position_id = PositionId("P-1")
        fill = TestEventStubs.order_filled(
            order,
            instrument=_AUDUSD_SIM,
            position_id=position_id,
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=_AUDUSD_SIM, fill=fill)

        self.database.add_position(position)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_position(position.id))

        # Act
        result = self.database.load_position(position_id)

        # Assert
        assert result == position
        assert position.id == position_id

    @pytest.mark.asyncio
    async def test_load_accounts_when_no_accounts_returns_empty_dict(self):
        # Arrange, Act
        result = self.database.load_accounts()

        # Assert
        assert result == {}

    @pytest.mark.asyncio
    async def test_load_accounts_cache_when_one_account_in_database(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Act
        self.database.add_account(account)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_accounts())

        # Assert
        assert self.database.load_accounts() == {account.id: account}

    @pytest.mark.asyncio
    async def test_load_orders_cache_when_no_orders(self):
        # Arrange, Act
        self.database.load_orders()

        # Assert
        assert self.database.load_orders() == {}

    @pytest.mark.asyncio
    async def test_load_orders_cache_when_one_order_in_database(self):
        # Arrange
        order = self.strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.database.add_order(order)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_orders())

        # Act
        result = self.database.load_orders()

        # Assert
        assert result == {order.client_order_id: order}

    @pytest.mark.asyncio
    async def test_load_positions_cache_when_no_positions(self):
        # Arrange, Act
        self.database.load_positions()

        # Assert
        assert self.database.load_positions() == {}

    @pytest.mark.asyncio
    async def test_load_positions_cache_when_one_position_in_database(self):
        # Arrange
        self.database.add_instrument(_AUDUSD_SIM)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_instrument(_AUDUSD_SIM.id))

        order1 = self.strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        self.database.add_order(order1)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_orders())

        position_id = PositionId("P-1")
        order1.apply(TestEventStubs.order_submitted(order1))
        order1.apply(TestEventStubs.order_accepted(order1))
        order1.apply(
            TestEventStubs.order_filled(
                order1,
                instrument=_AUDUSD_SIM,
                position_id=position_id,
                last_px=Price.from_str("1.00001"),
            ),
        )

        position = Position(instrument=_AUDUSD_SIM, fill=order1.last_event)
        self.database.add_position(position)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_positions())

        # Act
        result = self.database.load_positions()

        # Assert
        assert result == {position.id: position}

    @pytest.mark.asyncio
    async def test_delete_actor(self):
        # Arrange, Act
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.database.update_actor(actor)

        # Allow MPSC thread to insert
        await eventually(lambda: self.database.load_actor(actor.id))

        # Act
        self.database.delete_actor(actor.id)

        # Allow MPSC thread to delete
        await eventually(lambda: not self.database.load_actor(actor.id))

        result = self.database.load_actor(actor.id)

        # Assert
        assert result == {}

    @pytest.mark.asyncio
    async def test_delete_strategy(self):
        # Arrange, Act
        strategy = MockStrategy(TestDataStubs.bartype_btcusdt_binance_100tick_last())
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.database.update_strategy(strategy)

        # Allow MPSC thread to update
        await eventually(lambda: self.database.load_strategy(strategy.id))

        # Act
        self.database.delete_strategy(strategy.id)

        # Allow MPSC thread to delete
        await eventually(lambda: not self.database.load_strategy(strategy.id))

        result = self.database.load_strategy(strategy.id)

        # Assert
        assert result == {}


@pytest.mark.xdist_group(name="redis_integration")
class TestRedisCacheDatabaseIntegrity:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        config = BacktestEngineConfig(
            logging=LoggingConfig(bypass_logging=True),
            run_analysis=False,
            cache=CacheConfig(database=DatabaseConfig()),  # default redis
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
            bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv"),
            ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv"),
        )
        self.engine.add_instrument(self.usdjpy)
        self.engine.add_data(ticks)

        self.database = CacheDatabaseAdapter(
            trader_id=self.trader_id,
            instance_id=UUID4(),
            serializer=MsgSpecSerializer(encoding=msgspec.msgpack, timestamps_as_str=True),
            config=CacheConfig(database=DatabaseConfig()),
        )

    def teardown(self):
        # Tests will start failing if redis is not flushed on tear down
        self.database.flush()  # Comment this line out to preserve data between tests

    @pytest.mark.asyncio
    async def test_rerunning_backtest_with_redis_db_builds_correct_index(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=self.usdjpy.id,
            bar_type=TestDataStubs.bartype_usdjpy_1min_bid(),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
        )
        strategy = EMACross(config=config)
        self.engine.add_strategy(strategy)

        await asyncio.sleep(0.5)

        # Generate a lot of data
        self.engine.run()

        await asyncio.sleep(0.5)

        # Reset engine
        self.engine.reset()

        # Act
        self.engine.run()

        await asyncio.sleep(0.5)

        # Assert
        await eventually(lambda: self.engine.cache.check_integrity())
