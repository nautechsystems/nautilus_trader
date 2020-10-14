# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import unittest

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.uuid import TestUUIDFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Decimal
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

FXCM = Venue("FXCM")
BINANCE = Venue("BINANCE")
BITMEX = Venue("BITMEX")

AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(Symbol("AUD/USD", Venue("FXCM")))
GBPUSD_FXCM = InstrumentLoader.default_fx_ccy(Symbol("GBP/USD", Venue("FXCM")))
USDJPY_FXCM = InstrumentLoader.default_fx_ccy(Symbol("USD/JPY", Venue("FXCM")))
BTCUSDT_BINANCE = InstrumentLoader.btcusdt_binance()
BTCUSD_BITMEX = InstrumentLoader.btcusd_bitmex(leverage=Decimal("10.0"))
ETHUSD_BITMEX = InstrumentLoader.ethusd_bitmex(leverage=Decimal("10.0"))


class PortfolioTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        uuid_factor = TestUUIDFactory()
        logger = TestLogger(self.clock)
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

        state = AccountState(
            AccountId.from_string("BINANCE-1513111-SIMULATED"),
            BTC,
            Money(10., BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH
        )

        self.account = Account(state)
        self.portfolio = Portfolio(self.clock, uuid_factor, logger)
        self.portfolio.register_account(self.account)
        self.portfolio.update_instrument(AUDUSD_FXCM)
        self.portfolio.update_instrument(GBPUSD_FXCM)
        self.portfolio.update_instrument(BTCUSDT_BINANCE)
        self.portfolio.update_instrument(BTCUSD_BITMEX)
        self.portfolio.update_instrument(ETHUSD_BITMEX)

    def test_account_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.account(FXCM))

    def test_account_when_account_returns_the_account_facade(self):
        # Arrange
        # Act
        result = self.portfolio.account(BINANCE)

        # Assert
        self.assertEqual(self.account, result)

    def test_unrealized_pnl_for_symbol_when_no_instrument_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.unrealized_pnl_for_symbol(USDJPY_FXCM.symbol))

    def test_unrealized_pnl_for_venue_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.unrealized_pnl_for_venue(FXCM))

    def test_order_margin_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.order_margin(FXCM))

    def test_position_margin_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.position_margin(FXCM))

    def test_open_value_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.open_value(FXCM))

    def test_opening_one_position_updates_portfolio(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(10),
        )

        fill = TestStubs.event_order_filled(
            order=order,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
            base_currency=BTC,
            quote_currency=USD,
        )

        last = QuoteTick(
            BTCUSDT_BINANCE.symbol,
            Price("10500.05"),
            Price("10501.51"),
            Quantity("2.54"),
            Quantity("0.91"),
            UNIX_EPOCH,
        )

        self.portfolio.update_tick(last)

        position = Position(fill)

        # Act
        self.portfolio.update_position(TestStubs.event_position_opened(position))

        # Assert
        self.assertEqual(Money(10., BTC), self.portfolio.open_value(BINANCE))
        self.assertEqual(Money(0, BTC), self.portfolio.position_margin(BINANCE))
        self.assertEqual(Money(0.00004762, BTC), self.portfolio.unrealized_pnl_for_venue(BINANCE))
        self.assertEqual(Money(0.00004762, BTC), self.portfolio.unrealized_pnl_for_symbol(BTCUSDT_BINANCE.symbol))

    def test_opening_one_position_when_account_in_different_base(self):
        # Arrange
        state = AccountState(
            AccountId.from_string("BITMEX-01234-SIMULATED"),
            BTC,
            Money(10., BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH,
        )

        account = Account(state)

        self.portfolio.register_account(account)

        last_ethusd = QuoteTick(
            ETHUSD_BITMEX.symbol,
            Price("376.05"),
            Price("377.10"),
            Quantity("16"),
            Quantity("25"),
            UNIX_EPOCH,
        )

        last_btcusd = QuoteTick(
            BTCUSD_BITMEX.symbol,
            Price("10500.05"),
            Price("10501.51"),
            Quantity("2.54"),
            Quantity("0.91"),
            UNIX_EPOCH,
        )

        self.portfolio.update_tick(last_ethusd)
        self.portfolio.update_tick(last_btcusd)

        order = self.order_factory.market(
            ETHUSD_BITMEX.symbol,
            OrderSide.BUY,
            Quantity(100),
        )

        fill = TestStubs.event_order_filled(
            order=order,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("376.05"),
            base_currency=ETH,
            quote_currency=USD,
        )

        position = Position(fill)

        # Act
        self.portfolio.update_position(TestStubs.event_position_opened(position))

        # Assert
        self.assertEqual(Money(3.58141152, BTC), self.portfolio.open_value(BITMEX))
        self.assertEqual(Money(0.00519305, BTC), self.portfolio.position_margin(BITMEX))
        self.assertEqual(Money(0, BTC), self.portfolio.unrealized_pnl_for_venue(BITMEX))
        self.assertEqual(Money(0, BTC), self.portfolio.unrealized_pnl_for_symbol(BTCUSD_BITMEX.symbol))

    def test_unrealized_pnl_when_insufficient_data_for_xrate_returns_none(self):
        # Arrange
        state = AccountState(
            AccountId.from_string("BITMEX-01234-SIMULATED"),
            BTC,
            Money(10., BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH,
        )

        account = Account(state)

        self.portfolio.register_account(account)
        order = self.order_factory.market(
            ETHUSD_BITMEX.symbol,
            OrderSide.BUY,
            Quantity(100),
        )

        fill = TestStubs.event_order_filled(
            order=order,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("376.05"),
            base_currency=ETH,
            quote_currency=USD,
        )

        position = Position(fill)

        self.portfolio.update_position(TestStubs.event_position_opened(position))

        # Act
        result = self.portfolio.unrealized_pnl_for_venue(BITMEX)

        # # Assert
        self.assertIsNone(result)

    def test_open_value_when_insufficient_data_for_xrate_returns_none(self):
        # Arrange
        state = AccountState(
            AccountId.from_string("BITMEX-01234-SIMULATED"),
            BTC,
            Money(10., BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH,
        )

        account = Account(state)

        self.portfolio.register_account(account)
        order = self.order_factory.market(
            ETHUSD_BITMEX.symbol,
            OrderSide.BUY,
            Quantity(100),
        )

        fill = TestStubs.event_order_filled(
            order=order,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("376.05"),
            base_currency=ETH,
            quote_currency=USD,
        )

        last_ethusd = QuoteTick(
            ETHUSD_BITMEX.symbol,
            Price("376.05"),
            Price("377.10"),
            Quantity("16"),
            Quantity("25"),
            UNIX_EPOCH,
        )

        position = Position(fill)

        self.portfolio.update_position(TestStubs.event_position_opened(position))
        self.portfolio.update_tick(last_ethusd)

        # Act
        result = self.portfolio.open_value(BITMEX)

        # Assert
        self.assertEqual(None, result)

    def test_opening_several_positions_updates_portfolio(self):
        # Arrange
        state = AccountState(
            AccountId.from_string("FXCM-01234-SIMULATED"),
            USD,
            Money(1000000, USD),
            Money(0., USD),
            Money(0., USD),
            uuid4(),
            UNIX_EPOCH,
        )

        account = Account(state)

        self.portfolio.register_account(account)

        last_audusd = QuoteTick(
            AUDUSD_FXCM.symbol,
            Price("0.80501"),
            Price("0.80505"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        last_gbpusd = QuoteTick(
            GBPUSD_FXCM.symbol,
            Price("1.30315"),
            Price("1.30317"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        self.portfolio.update_tick(last_audusd)
        self.portfolio.update_tick(last_gbpusd)

        order1 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            GBPUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order1_filled = TestStubs.event_order_filled(order1, PositionId("P-1"), StrategyId("S", "1"), Price("1.00000"))
        order2_filled = TestStubs.event_order_filled(order2, PositionId("P-2"), StrategyId("S", "1"), Price("1.00000"))

        position1 = Position(order1_filled)
        position2 = Position(order2_filled)
        position_opened1 = TestStubs.event_position_opened(position1)
        position_opened2 = TestStubs.event_position_opened(position2)

        # Act
        self.portfolio.update_position(position_opened1)
        self.portfolio.update_position(position_opened2)

        # Assert
        self.assertEqual(Money(23808.10, USD), self.portfolio.unrealized_pnl_for_venue(FXCM))
        self.assertEqual(Money(210816.00, USD), self.portfolio.open_value(FXCM))
        self.assertEqual(Money(0., BTC), self.portfolio.unrealized_pnl_for_venue(BINANCE))
        self.assertEqual(Money(0., BTC), self.portfolio.open_value(BINANCE))

    def test_modifying_position_updates_portfolio(self):
        # Arrange
        state = AccountState(
            AccountId.from_string("FXCM-01234-SIMULATED"),
            USD,
            Money(1000000, USD),
            Money(0., USD),
            Money(0., USD),
            uuid4(),
            UNIX_EPOCH,
        )

        account = Account(state)

        self.portfolio.register_account(account)

        last_audusd = QuoteTick(
            AUDUSD_FXCM.symbol,
            Price("0.80501"),
            Price("0.80505"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        self.portfolio.update_tick(last_audusd)

        order1 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order1_filled = TestStubs.event_order_filled(order1, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
        position = Position(order1_filled)

        self.portfolio.update_position(TestStubs.event_position_opened(position))

        order2 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(50000),
        )

        order2_filled = TestStubs.event_order_filled(order2, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
        position.apply(order2_filled)

        # Act
        self.portfolio.update_position(TestStubs.event_position_modified(position))

        # Assert
        self.assertEqual(Money(-7848.44, USD), self.portfolio.unrealized_pnl_for_venue(FXCM))
        self.assertEqual(Money(40250.50, USD), self.portfolio.open_value(FXCM))
        self.assertEqual(Money(0., BTC), self.portfolio.unrealized_pnl_for_venue(BINANCE))
        self.assertEqual(Money(0., BTC), self.portfolio.open_value(BINANCE))

    def test_closing_position_updates_portfolio(self):
        # Arrange
        state = AccountState(
            AccountId.from_string("FXCM-01234-SIMULATED"),
            USD,
            Money(1000000, USD),
            Money(0., USD),
            Money(0., USD),
            uuid4(),
            UNIX_EPOCH,
        )

        account = Account(state)

        self.portfolio.register_account(account)

        order1 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order1_filled = TestStubs.event_order_filled(order1, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
        position = Position(order1_filled)

        self.portfolio.update_position(TestStubs.event_position_opened(position))

        order2 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        order2_filled = TestStubs.event_order_filled(order2, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00010"))
        position.apply(order2_filled)

        # Act
        self.portfolio.update_position(TestStubs.event_position_closed(position))

        # Assert
        self.assertEqual(Money(0, USD), self.portfolio.unrealized_pnl_for_venue(FXCM))
        self.assertEqual(Money(0, USD), self.portfolio.open_value(FXCM))
        self.assertEqual(Money(0, BTC), self.portfolio.unrealized_pnl_for_venue(BINANCE))
        self.assertEqual(Money(0, BTC), self.portfolio.open_value(BINANCE))

    def test_several_positions_with_different_symbols_updates_portfolio(self):
        # Arrange
        state = AccountState(
            AccountId.from_string("FXCM-01234-SIMULATED"),
            USD,
            Money(1000000, USD),
            Money(0., USD),
            Money(0., USD),
            uuid4(),
            UNIX_EPOCH,
        )

        account = Account(state)

        self.portfolio.register_account(account)

        order1 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order3 = self.order_factory.market(
            GBPUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order4 = self.order_factory.market(
            GBPUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        order1_filled = TestStubs.event_order_filled(order1, PositionId("P-1"), StrategyId("S", "1"), Price("1.00000"))
        order2_filled = TestStubs.event_order_filled(order2, PositionId("P-2"), StrategyId("S", "1"), Price("1.00000"))
        order3_filled = TestStubs.event_order_filled(order3, PositionId("P-3"), StrategyId("S", "1"), Price("1.00000"))
        order4_filled = TestStubs.event_order_filled(order4, PositionId("P-3"), StrategyId("S", "1"), Price("1.00100"))

        position1 = Position(order1_filled)
        position2 = Position(order2_filled)
        position3 = Position(order3_filled)

        last_audusd = QuoteTick(
            AUDUSD_FXCM.symbol,
            Price("0.80501"),
            Price("0.80505"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        last_gbpusd = QuoteTick(
            GBPUSD_FXCM.symbol,
            Price("1.30315"),
            Price("1.30317"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        self.portfolio.update_tick(last_audusd)
        self.portfolio.update_tick(last_gbpusd)

        # Act
        self.portfolio.update_position(TestStubs.event_position_opened(position1))
        self.portfolio.update_position(TestStubs.event_position_opened(position2))
        self.portfolio.update_position(TestStubs.event_position_opened(position3))

        position3.apply(order4_filled)
        self.portfolio.update_position(TestStubs.event_position_closed(position3))

        # Assert
        self.assertEqual(Money(-31393.78, USD), self.portfolio.unrealized_pnl_for_venue(FXCM))
        self.assertEqual(Money(161002.00, USD), self.portfolio.open_value(FXCM))
        self.assertEqual(Money(323.61, USD), self.portfolio.account(FXCM).position_margin())
        self.assertEqual(Money(0, BTC), self.portfolio.unrealized_pnl_for_venue(BINANCE))
        self.assertEqual(Money(0, BTC), self.portfolio.open_value(BINANCE))
