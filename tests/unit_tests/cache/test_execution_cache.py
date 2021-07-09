# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACross
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
BTCUSD_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestCache:
    def setup(self):
        # Fixture Setup
        clock = TestClock()
        logger = Logger(clock)

        self.trader_id = TraderId("TESTER-000")
        self.account_id = TestStubs.account_id()

        self.strategy = TradingStrategy(order_id_tag="001")
        self.strategy.register_trader(
            TraderId("TESTER-000"),
            clock,
            logger,
        )

        self.cache = TestStubs.cache()

    def test_cache_currencies_with_no_currencies(self):
        # Arrange
        # Act
        self.cache.cache_currencies()

        # Assert
        assert True  # No exception raised

    def test_cache_instruments_with_no_instruments(self):
        # Arrange
        # Act
        self.cache.cache_instruments()

        # Assert
        assert True  # No exception raised

    def test_cache_accounts_with_no_accounts(self):
        # Arrange
        # Act
        self.cache.cache_accounts()

        # Assert
        assert True  # No exception raised

    def test_cache_orders_with_no_orders(self):
        # Arrange
        # Act
        self.cache.cache_orders()

        # Assert
        assert True  # No exception raised

    def test_cache_positions_with_no_positions(self):
        # Arrange
        # Act
        self.cache.cache_positions()

        # Assert
        assert True  # No exception raised

    def test_build_index_with_no_objects(self):
        # Arrange
        # Act
        self.cache.build_index()

        # Assert
        assert True  # No exception raised

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
        self.cache.add_currency(currency)

        # Assert
        assert Currency.from_str("1INCH") == currency

    def test_add_account(self):
        # Arrange
        initial = TestStubs.event_account_state()
        account = Account(initial)

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
        initial = TestStubs.event_account_state()
        account = Account(initial)

        self.cache.add_account(account)

        # Act
        result = self.cache.load_account(account.id)

        # Assert
        assert result == account

    def test_account_for_venue(self):
        # Arrange
        # Act
        result = self.cache.account_for_venue(Venue("SIM"))

        # Assert
        assert result is None

    def test_accounts_when_no_accounts_returns_empty_list(self):
        # Arrange
        # Act
        result = self.cache.accounts()

        # Assert
        assert result == []

    def test_get_strategy_ids_with_no_ids_returns_empty_set(self):
        # Arrange
        # Act
        result = self.cache.strategy_ids()

        # Assert
        assert result == set()

    def test_get_order_ids_with_no_ids_returns_empty_set(self):
        # Arrange
        # Act
        result = self.cache.client_order_ids()

        # Assert
        assert result == set()

    def test_get_strategy_ids_with_id_returns_correct_set(self):
        # Arrange
        self.cache.update_strategy(self.strategy)

        # Act
        result = self.cache.strategy_ids()

        # Assert
        assert result == {self.strategy.id}

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        assert not self.cache.position_exists(PositionId("P-123456"))

    def test_order_exists_when_no_order_returns_false(self):
        # Arrange
        # Act
        # Assert
        assert not self.cache.order_exists(ClientOrderId("O-123456"))

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
        # Arrange
        # Act
        # Assert
        assert self.cache.strategy_id_for_position(PositionId("P-123456")) is None

    def test_add_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")

        # Act
        self.cache.add_order(order, position_id)

        # Assert
        assert order.client_order_id in self.cache.client_order_ids()
        assert order.client_order_id in self.cache.client_order_ids(
            instrument_id=order.instrument_id
        )
        assert order.client_order_id in self.cache.client_order_ids(strategy_id=self.strategy.id)
        assert order.client_order_id not in self.cache.client_order_ids(
            strategy_id=StrategyId("S-ZX1")
        )
        assert order.client_order_id in self.cache.client_order_ids(
            instrument_id=order.instrument_id, strategy_id=self.strategy.id
        )
        assert order in self.cache.orders()
        assert self.cache.venue_order_id(order.client_order_id) == VenueOrderId.null()
        assert self.cache.client_order_id(order.venue_order_id) is None

    def test_load_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
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
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        # Act
        self.cache.add_position(position)

        # Assert
        assert self.cache.position_exists(position.id)
        assert position.id in self.cache.position_ids()
        assert position in self.cache.positions()
        assert position in self.cache.positions_open()
        assert position in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position in self.cache.positions_open(
            instrument_id=position.instrument_id, strategy_id=self.strategy.id
        )
        assert position not in self.cache.positions_closed()
        assert position not in self.cache.positions_closed(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_closed(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_closed(
            instrument_id=position.instrument_id, strategy_id=self.strategy.id
        )

    def test_load_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)
        self.cache.add_position(position)

        # Act
        result = self.cache.load_position(position.id)

        # Assert
        assert result == position

    def test_update_order_for_accepted_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        order.apply(TestStubs.event_order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))

        # Act
        self.cache.update_order(order)

        # Assert
        assert self.cache.order_exists(order.client_order_id)
        assert order.client_order_id in self.cache.client_order_ids()
        assert order in self.cache.orders()
        assert order in self.cache.orders_working()
        assert order in self.cache.orders_working(instrument_id=order.instrument_id)
        assert order in self.cache.orders_working(strategy_id=self.strategy.id)
        assert order in self.cache.orders_working(
            instrument_id=order.instrument_id, strategy_id=self.strategy.id
        )
        assert order not in self.cache.orders_completed()
        assert order not in self.cache.orders_completed(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_completed(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_completed(
            instrument_id=order.instrument_id, strategy_id=self.strategy.id
        )

        assert self.cache.orders_working_count() == 1
        assert self.cache.orders_completed_count() == 0
        assert self.cache.orders_total_count() == 1

    def test_update_order_for_completed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)
        order.apply(TestStubs.event_order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))
        self.cache.update_order(order)

        fill = TestStubs.event_order_filled(
            order, instrument=AUDUSD_SIM, last_px=Price.from_str("1.00001")
        )

        order.apply(fill)

        # Act
        self.cache.update_order(order)

        # Assert
        assert self.cache.order_exists(order.client_order_id)
        assert order.client_order_id in self.cache.client_order_ids()
        assert order in self.cache.orders()
        assert order in self.cache.orders_completed()
        assert order in self.cache.orders_completed(instrument_id=order.instrument_id)
        assert order in self.cache.orders_completed(strategy_id=self.strategy.id)
        assert order in self.cache.orders_completed(
            instrument_id=order.instrument_id, strategy_id=self.strategy.id
        )
        assert order not in self.cache.orders_working()
        assert order not in self.cache.orders_working(instrument_id=order.instrument_id)
        assert order not in self.cache.orders_working(strategy_id=self.strategy.id)
        assert order not in self.cache.orders_working(
            instrument_id=order.instrument_id, strategy_id=self.strategy.id
        )
        assert self.cache.venue_order_id(order.client_order_id) == order.venue_order_id
        assert self.cache.orders_working_count() == 0
        assert self.cache.orders_completed_count() == 1
        assert self.cache.orders_total_count() == 1

    def test_update_position_for_open_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        # Act
        self.cache.add_position(position)

        # Assert
        assert self.cache.position_exists(position.id)
        assert position.id in self.cache.position_ids()
        assert position in self.cache.positions()
        assert position in self.cache.positions_open()
        assert position in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position in self.cache.positions_open(
            instrument_id=position.instrument_id, strategy_id=self.strategy.id
        )
        assert position not in self.cache.positions_closed()
        assert position not in self.cache.positions_closed(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_closed(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_closed(
            instrument_id=position.instrument_id, strategy_id=self.strategy.id
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
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position)

        order2 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)
        order2_filled = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
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
            instrument_id=position.instrument_id, strategy_id=self.strategy.id
        )
        assert position not in self.cache.positions_open()
        assert position not in self.cache.positions_open(instrument_id=position.instrument_id)
        assert position not in self.cache.positions_open(strategy_id=self.strategy.id)
        assert position not in self.cache.positions_open(
            instrument_id=position.instrument_id, strategy_id=self.strategy.id
        )
        assert self.cache.position(position_id) == position
        assert self.cache.positions_open_count() == 0
        assert self.cache.positions_closed_count() == 1
        assert self.cache.positions_total_count() == 1

    def test_positions_queries_with_multiple_open_returns_expected_positions(self):
        # Arrange
        # -- Position 1 --------------------------------------------------------
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position1)

        # -- Position 2 --------------------------------------------------------

        order2 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)
        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=GBPUSD_SIM,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00001"),
        )

        position2 = Position(instrument=GBPUSD_SIM, fill=fill2)
        self.cache.add_position(position2)

        # Assert
        assert position1.is_open
        assert position2.is_open
        assert position1 in self.cache.positions()
        assert position2 in self.cache.positions()
        assert self.cache.positions(venue=AUDUSD_SIM.venue, instrument_id=AUDUSD_SIM.id) == [
            position1
        ]
        assert self.cache.positions(venue=GBPUSD_SIM.venue, instrument_id=GBPUSD_SIM.id) == [
            position2
        ]
        assert self.cache.positions(instrument_id=GBPUSD_SIM.id) == [position2]
        assert self.cache.positions(instrument_id=AUDUSD_SIM.id) == [position1]
        assert self.cache.positions(instrument_id=GBPUSD_SIM.id) == [position2]
        assert self.cache.positions_open(instrument_id=AUDUSD_SIM.id) == [position1]
        assert self.cache.positions_open(instrument_id=GBPUSD_SIM.id) == [position2]
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
            Quantity.from_int(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00001"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position1)

        # -- Position 2 --------------------------------------------------------

        order2 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)
        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=GBPUSD_SIM,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00001"),
        )

        position2 = Position(instrument=GBPUSD_SIM, fill=fill2)
        self.cache.add_position(position2)

        order3 = self.strategy.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        order3.apply(TestStubs.event_order_submitted(order3))
        self.cache.update_order(order3)

        order3.apply(TestStubs.event_order_accepted(order3))
        self.cache.update_order(order3)
        fill3 = TestStubs.event_order_filled(
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
        assert self.cache.positions_open(venue=BTCUSD_BINANCE.venue) == []
        assert self.cache.positions_open(venue=AUDUSD_SIM.venue) == [position1]
        assert self.cache.positions_open(instrument_id=BTCUSD_BINANCE.id) == []
        assert self.cache.positions_open(instrument_id=AUDUSD_SIM.id) == [position1]
        assert self.cache.positions_open(instrument_id=GBPUSD_SIM.id) == []
        assert self.cache.positions_closed(instrument_id=AUDUSD_SIM.id) == []
        assert self.cache.positions_closed(venue=GBPUSD_SIM.venue) == [position2]
        assert self.cache.positions_closed(instrument_id=GBPUSD_SIM.id) == [position2]

    def test_update_account(self):
        # Arrange
        event = TestStubs.event_account_state()
        account = Account(event)

        self.cache.add_account(account)

        # Act
        self.cache.update_account(account)

        # Assert
        assert True  # No exceptions raised

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
            Quantity.from_int(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
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
            Quantity.from_int(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )
        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)

        self.cache.update_order(order2)

        # Act
        self.cache.reset()

        # Assert
        assert len(self.cache.strategy_ids()) == 0
        assert self.cache.orders_total_count() == 0
        assert self.cache.positions_total_count() == 0

    def test_flush_db(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price.from_str("1.00000"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)
        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)

        # Act
        self.cache.reset()
        self.cache.flush_db()

        # Assert
        assert True  # No exception raised


class TestExecutionCacheIntegrityCheck:
    def setup(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            bypass_logging=True,  # Uncomment this to see integrity check failure messages
        )

        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        self.engine.add_instrument(self.usdjpy)
        self.engine.add_bars(
            self.usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid(),
        )
        self.engine.add_bars(
            self.usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask(),
        )

        self.engine.add_venue(
            venue=Venue("SIM"),
            venue_type=VenueType.BROKERAGE,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[],
        )

        self.cache = self.engine.get_exec_engine().cache

    def test_exec_cache_check_integrity_when_cache_cleared_fails(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.usdjpy.id,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        # Generate a lot of data
        self.engine.run(strategies=[strategy])

        # Remove data
        self.cache.clear_cache()

        # Act
        # Assert
        assert not self.cache.check_integrity()

    def test_exec_cache_check_integrity_when_index_cleared_fails(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.usdjpy.id,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        # Generate a lot of data
        self.engine.run(strategies=[strategy])

        # Clear index
        self.cache.clear_index()

        # Act
        # Assert
        assert not self.cache.check_integrity()
