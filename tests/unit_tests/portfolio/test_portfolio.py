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

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.config import PortfolioConfig
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
BINANCE = Venue("BINANCE")
BITMEX = Venue("BITMEX")
BETFAIR = Venue("BETFAIR")

AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
BTCUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
ETHUSD_BITMEX = TestInstrumentProvider.ethusd_bitmex()
BETTING_INSTRUMENT = TestInstrumentProvider.betting_instrument()


class TestPortfolio:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=PortfolioConfig(debug=True),
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Prepare components
        self.cache.add_instrument(AUDUSD_SIM)
        self.cache.add_instrument(GBPUSD_SIM)
        self.cache.add_instrument(BTCUSDT_BINANCE)
        self.cache.add_instrument(BTCUSD_BITMEX)
        self.cache.add_instrument(ETHUSD_BITMEX)

    def test_account_when_no_account_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.account(SIM) is None

    def test_account_when_account_returns_the_account_facade(self):
        # Arrange
        state = AccountState(
            account_id=AccountId("BINANCE-1513111"),
            account_type=AccountType.CASH,
            base_currency=None,
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        # Act
        result = self.portfolio.account(BINANCE)

        # Assert
        assert result.id.get_issuer() == "BINANCE"
        assert result.id.get_id() == "1513111"

    def test_balances_locked_when_no_account_for_venue_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.balances_locked(SIM) is None

    def test_margins_init_when_no_account_for_venue_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.margins_init(SIM) is None

    def test_margins_maint_when_no_account_for_venue_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.margins_maint(SIM) is None

    def test_unrealized_pnl_for_instrument_when_no_instrument_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.unrealized_pnl(USDJPY_SIM.id) is None

    def test_unrealized_pnls_for_venue_when_no_account_returns_empty_dict(self):
        # Arrange, Act, Assert
        assert self.portfolio.unrealized_pnls(SIM) == {}

    def test_realized_pnl_for_instrument_when_no_instrument_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.realized_pnl(USDJPY_SIM.id) is None

    def test_realized_pnl_for_venue_when_no_account_returns_empty_dict(self):
        # Arrange, Act, Assert
        assert self.portfolio.realized_pnls(SIM) == {}

    def test_total_pnl_for_instrument_when_no_instrument_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.total_pnl(USDJPY_SIM.id) is None

    def test_total_pnls_for_venue_when_no_account_returns_empty_dict(self):
        # Arrange, Act, Assert
        assert self.portfolio.total_pnls(SIM) == {}

    def test_net_position_when_no_positions_returns_zero(self):
        # Arrange, Act, Assert
        assert self.portfolio.net_position(AUDUSD_SIM.id) == Decimal(0)

    def test_net_exposures_when_no_positions_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.net_exposures(SIM) is None

    def test_is_net_long_when_no_positions_returns_false(self):
        # Arrange, Act, Assert
        assert self.portfolio.is_net_long(AUDUSD_SIM.id) is False

    def test_is_net_short_when_no_positions_returns_false(self):
        # Arrange, Act, Assert
        assert self.portfolio.is_net_short(AUDUSD_SIM.id) is False

    def test_is_flat_when_no_positions_returns_true(self):
        # Arrange, Act, Assert
        assert self.portfolio.is_flat(AUDUSD_SIM.id) is True

    def test_is_completely_flat_when_no_positions_returns_true(self):
        # Arrange, Act, Assert
        assert self.portfolio.is_flat(AUDUSD_SIM.id) is True

    def test_open_value_when_no_account_returns_none(self):
        # Arrange, Act, Assert
        assert self.portfolio.net_exposures(SIM) is None

    def test_update_tick(self):
        # Arrange
        tick = TestDataStubs.quote_tick()

        # Act
        self.portfolio.update_quote_tick(tick)

        # Assert
        assert self.portfolio.unrealized_pnl(GBPUSD_SIM.id) is None

    def test_exceed_free_balance_single_currency_raises_account_balance_negative_exception(self):
        # Arrange
        AccountFactory.register_calculated_account("SIM")

        account_id = AccountId("SIM-000")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=USD,  # Single-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(100000.00, USD),
                    Money(0.00, USD),
                    Money(100000.00, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        # Create order
        order = self.order_factory.market(  # <-- order value 150_000 USDT
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_str("1000000.0"),
        )

        self.cache.add_order(order, position_id=None)

        self.exec_engine.process(TestEventStubs.order_submitted(order, account_id=account_id))

        # Act, Assert: push account to negative balance (wouldn't normally be allowed by risk engine)
        with pytest.raises(ValueError):
            fill = TestEventStubs.order_filled(
                order,
                instrument=AUDUSD_SIM,
                account_id=account_id,
            )
            self.exec_engine.process(fill)

    def test_exceed_free_balance_multi_currency_raises_account_balance_negative_exception(self):
        # Arrange
        AccountFactory.register_calculated_account("BINANCE")

        account_id = AccountId("BINANCE-000")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(100_000.00000000, USDT),
                    Money(0.00000000, USDT),
                    Money(100_000.00000000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        account = self.portfolio.account(BINANCE)

        # Create order
        order = self.order_factory.market(  # <-- order value 150_000 USDT
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("3.0"),
        )

        self.cache.add_order(order, position_id=None)

        self.exec_engine.process(TestEventStubs.order_submitted(order, account_id=account_id))

        # Act, Assert: push account to negative balance (wouldn't normally be allowed by risk engine)
        # TODO: The below is the old test prior to validating balance updates
        #  in the account manager. Leaving here pending accounts refactoring
        # with pytest.raises(ValueError):
        #     fill = TestEventStubs.order_filled(
        #         order,
        #         instrument=BTCUSDT_BINANCE,
        #         account_id=account_id,
        #         last_px=Price.from_str("100_000"),
        #     )
        #     self.exec_engine.process(fill)
        assert account.balances_total()[BTC] == Money(10.00000000, BTC)
        assert account.balances_total()[USDT] == Money(100_000.00000000, USDT)

    def test_update_orders_open_cash_account(self):
        # Arrange
        AccountFactory.register_calculated_account("BINANCE")

        account_id = AccountId("BINANCE-000")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(100000.00000000, USDT),
                    Money(0.00000000, USDT),
                    Money(100000.00000000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        # Create open order
        order = self.order_factory.limit(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
            Price.from_str("50000.00"),
        )

        self.cache.add_order(order, position_id=None)

        # Act: push order state to ACCEPTED
        self.exec_engine.process(TestEventStubs.order_submitted(order, account_id=account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order, account_id=account_id))

        # Assert
        assert self.portfolio.balances_locked(BINANCE)[USDT].as_decimal() == 50100

    def test_update_orders_open_margin_account(self):
        # Arrange
        AccountFactory.register_calculated_account("BINANCE")

        account_id = AccountId("BINANCE-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
                AccountBalance(
                    Money(100000.00000000, USDT),
                    Money(0.00000000, USDT),
                    Money(100000.00000000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        # Create two open orders
        order1 = self.order_factory.stop_market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("10.5"),
            Price.from_str("25000.00"),
        )

        order2 = self.order_factory.stop_market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("10.5"),
            Price.from_str("25000.00"),
        )

        self.cache.add_order(order1, position_id=None)
        self.cache.add_order(order2, position_id=None)

        # Push states to ACCEPTED
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)
        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        filled1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("25000.00"),
        )
        self.exec_engine.process(filled1)

        # Update the last quote
        last = QuoteTick(
            instrument_id=BTCUSDT_BINANCE.id,
            bid_price=Price.from_str("25001.00"),
            ask_price=Price.from_str("25002.00"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.portfolio.update_quote_tick(last)
        self.portfolio.initialize_orders()

        # Assert
        assert self.portfolio.margins_init(BINANCE) == {BTCUSDT_BINANCE.id: Money("0E-8", USDT)}

    def test_order_accept_updates_margin_init(self):
        # Arrange
        AccountFactory.register_calculated_account("BINANCE")

        account_id = AccountId("BINANCE-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
                AccountBalance(
                    Money(100000.00000000, USDT),
                    Money(0.00000000, USDT),
                    Money(100000.00000000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        # Create a limit order
        order1 = self.order_factory.limit(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("100"),
            Price.from_str("0.5"),
        )

        self.cache.add_order(order1, position_id=None)

        # Push states to ACCEPTED
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)
        order1.apply(TestEventStubs.order_accepted(order1, venue_order_id=VenueOrderId("1")))
        self.cache.update_order(order1)

        # Act
        self.portfolio.initialize_orders()

        # Assert
        assert self.portfolio.margins_init(BINANCE)[BTCUSDT_BINANCE.id] == Money(0.1, USDT)

    def test_update_positions(self):
        # Arrange
        AccountFactory.register_calculated_account("BINANCE")

        account_id = AccountId("BINANCE-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        # Create a closed position
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("10.500000"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("10.500000"),
        )

        self.cache.add_order(order1, position_id=None)
        self.cache.add_order(order2, position_id=None)

        # Push states to ACCEPTED
        order1.apply(TestEventStubs.order_submitted(order1))
        self.cache.update_order(order1)
        order1.apply(TestEventStubs.order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("25000.00"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("25000.00"),
        )

        position1 = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        position1.apply(fill2)

        order3 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("10.000000"),
        )

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("25000.00"),
        )

        position2 = Position(instrument=BTCUSDT_BINANCE, fill=fill3)

        # Update the last quote
        last = QuoteTick(
            instrument_id=BTCUSDT_BINANCE.id,
            bid_price=Price.from_str("25001.00"),
            ask_price=Price.from_str("25002.00"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.cache.add_position(position1, OmsType.HEDGING)
        self.cache.add_position(position2, OmsType.HEDGING)
        self.portfolio.initialize_positions()
        self.portfolio.update_quote_tick(last)

        # Assert
        assert self.portfolio.is_net_long(BTCUSDT_BINANCE.id)

    def test_opening_one_long_position_updates_portfolio(self):
        # Arrange
        AccountFactory.register_calculated_account("BINANCE")

        account_id = AccountId("BINANCE-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
                AccountBalance(
                    Money(100000.00000000, USDT),
                    Money(0.00000000, USDT),
                    Money(100000.00000000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("10.000000"),
        )

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("10500.00"),
        )

        last = QuoteTick(
            instrument_id=BTCUSDT_BINANCE.id,
            bid_price=Price.from_str("10510.00"),
            ask_price=Price.from_str("10511.00"),
            bid_size=Quantity.from_str("1.000000"),
            ask_size=Quantity.from_str("1.000000"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_quote_tick(last)
        self.portfolio.update_quote_tick(last)

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position))

        # Assert
        assert self.portfolio.net_exposures(BINANCE) == {USDT: Money(105100.00000000, USDT)}
        assert self.portfolio.unrealized_pnls(BINANCE) == {USDT: Money(100.00000000, USDT)}
        assert self.portfolio.realized_pnls(BINANCE) == {USDT: Money(-105.00000000, USDT)}
        assert self.portfolio.margins_maint(BINANCE) == {
            BTCUSDT_BINANCE.id: Money(105.00000000, USDT),
        }
        assert self.portfolio.net_exposure(BTCUSDT_BINANCE.id) == Money(105100.00000000, USDT)
        assert self.portfolio.unrealized_pnl(BTCUSDT_BINANCE.id) == Money(100.00000000, USDT)
        assert self.portfolio.realized_pnl(BTCUSDT_BINANCE.id) == Money(-105.00000000, USDT)
        assert self.portfolio.net_position(order.instrument_id) == Decimal("10.00000000")
        assert self.portfolio.is_net_long(order.instrument_id)
        assert not self.portfolio.is_net_short(order.instrument_id)
        assert not self.portfolio.is_flat(order.instrument_id)
        assert not self.portfolio.is_completely_flat()

    def test_opening_one_long_position_updates_portfolio_with_bar(self):
        # Arrange
        AccountFactory.register_calculated_account("BINANCE")

        account_id = AccountId("BINANCE-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
                AccountBalance(
                    Money(100000.00000000, USDT),
                    Money(0.00000000, USDT),
                    Money(100000.00000000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("10.000000"),
        )

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("10500.00"),
        )

        last = Bar(
            bar_type=BarType.from_str(f"{BTCUSDT_BINANCE.id}-1-MINUTE-LAST-EXTERNAL"),
            open=Price.from_str("10510.00"),
            high=Price.from_str("10510.00"),
            low=Price.from_str("10510.00"),
            close=Price.from_str("10510.00"),
            volume=Quantity.from_str("1.000000"),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_bar(last)

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position))

        # Assert
        assert self.portfolio.net_exposures(BINANCE) == {USDT: Money(105100.00000000, USDT)}
        assert self.portfolio.unrealized_pnls(BINANCE) == {USDT: Money(100.00000000, USDT)}
        assert self.portfolio.realized_pnls(BINANCE) == {USDT: Money(-105.00000000, USDT)}
        assert self.portfolio.margins_maint(BINANCE) == {
            BTCUSDT_BINANCE.id: Money(105.00000000, USDT),
        }
        assert self.portfolio.net_exposure(BTCUSDT_BINANCE.id) == Money(105100.00000000, USDT)
        assert self.portfolio.unrealized_pnl(BTCUSDT_BINANCE.id) == Money(100.00000000, USDT)
        assert self.portfolio.realized_pnl(BTCUSDT_BINANCE.id) == Money(-105.00000000, USDT)
        assert self.portfolio.net_position(order.instrument_id) == Decimal("10.00000000")
        assert self.portfolio.is_net_long(order.instrument_id)
        assert not self.portfolio.is_net_short(order.instrument_id)
        assert not self.portfolio.is_flat(order.instrument_id)
        assert not self.portfolio.is_completely_flat()

    def test_opening_one_short_position_updates_portfolio(self):
        # Arrange
        AccountFactory.register_calculated_account("BINANCE")

        account_id = AccountId("BINANCE-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
                AccountBalance(
                    Money(100000.00000000, USDT),
                    Money(0.00000000, USDT),
                    Money(100000.00000000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("0.515"),
        )

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("15000.00"),
        )

        last = QuoteTick(
            instrument_id=BTCUSDT_BINANCE.id,
            bid_price=Price.from_str("15510.15"),
            ask_price=Price.from_str("15510.25"),
            bid_size=Quantity.from_str("12.62"),
            ask_size=Quantity.from_str("3.10"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_quote_tick(last)
        self.portfolio.update_quote_tick(last)

        position = Position(instrument=BTCUSDT_BINANCE, fill=fill)

        # Act
        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position))

        # Assert
        assert self.portfolio.net_exposures(BINANCE) == {USDT: Money(7987.77875000, USDT)}
        assert self.portfolio.unrealized_pnls(BINANCE) == {USDT: Money(-262.77875000, USDT)}
        assert self.portfolio.realized_pnls(BINANCE) == {USDT: Money(-7.72500000, USDT)}
        assert self.portfolio.margins_maint(BINANCE) == {
            BTCUSDT_BINANCE.id: Money(7.72500000, USDT),
        }
        assert self.portfolio.net_exposure(BTCUSDT_BINANCE.id) == Money(7987.77875000, USDT)
        assert self.portfolio.unrealized_pnl(BTCUSDT_BINANCE.id) == Money(-262.77875000, USDT)
        assert self.portfolio.net_position(order.instrument_id) == Decimal("-0.515000")
        assert not self.portfolio.is_net_long(order.instrument_id)
        assert self.portfolio.is_net_short(order.instrument_id)
        assert not self.portfolio.is_flat(order.instrument_id)
        assert not self.portfolio.is_completely_flat()

    def test_opening_positions_with_multi_asset_account(self):
        # Arrange
        AccountFactory.register_calculated_account("BITMEX")

        account_id = AccountId("BITMEX-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        last_ethusd = QuoteTick(
            instrument_id=ETHUSD_BITMEX.id,
            bid_price=Price.from_str("376.05"),
            ask_price=Price.from_str("377.10"),
            bid_size=Quantity.from_str("16"),
            ask_size=Quantity.from_str("25"),
            ts_event=0,
            ts_init=0,
        )

        last_btcusd = QuoteTick(
            instrument_id=BTCUSD_BITMEX.id,
            bid_price=Price.from_str("10500.05"),
            ask_price=Price.from_str("10501.51"),
            bid_size=Quantity.from_str("2.54"),
            ask_size=Quantity.from_str("0.91"),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_quote_tick(last_ethusd)
        self.cache.add_quote_tick(last_btcusd)
        self.portfolio.update_quote_tick(last_ethusd)
        self.portfolio.update_quote_tick(last_btcusd)

        order = self.order_factory.market(
            ETHUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(10_000),
        )

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=ETHUSD_BITMEX,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("376.05"),
        )

        position = Position(instrument=ETHUSD_BITMEX, fill=fill)

        # Act
        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position))

        # Assert
        assert self.portfolio.net_exposures(BITMEX) == {ETH: Money(26.59220848, ETH)}
        assert self.portfolio.margins_maint(BITMEX) == {ETHUSD_BITMEX.id: Money(0.20608962, ETH)}
        assert self.portfolio.net_exposure(ETHUSD_BITMEX.id) == Money(26.59220848, ETH)
        assert self.portfolio.unrealized_pnl(ETHUSD_BITMEX.id) == Money(0.00000000, ETH)

    def test_unrealized_pnl_when_insufficient_data_for_xrate_returns_none(self):
        # Arrange
        AccountFactory.register_calculated_account("BITMEX")

        state = AccountState(
            account_id=AccountId("BITMEX-01234"),
            account_type=AccountType.MARGIN,
            base_currency=BTC,
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        order = self.order_factory.market(
            ETHUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(100),
        )

        self.cache.add_order(order, position_id=None)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=ETHUSD_BITMEX,
            strategy_id=StrategyId("S-1"),
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("376.05"),
        )

        self.exec_engine.process(fill)

        position = Position(instrument=ETHUSD_BITMEX, fill=fill)

        self.portfolio.update_position(TestEventStubs.position_opened(position))

        # Act
        result = self.portfolio.unrealized_pnls(BITMEX)

        # # Assert
        assert result == {}

    def test_total_pnl_for_instrument_when_both_pnls_exist(self):
        # Arrange
        AccountFactory.register_calculated_account("SIM")

        account_id = AccountId("SIM-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        # Create and fill an order
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("1.00000"),
        )

        # Add market data
        last = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("1.00010"),
            ask_price=Price.from_str("1.00011"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_quote_tick(last)

        position = Position(instrument=AUDUSD_SIM, fill=fill)
        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position))

        # Act
        result = self.portfolio.total_pnl(AUDUSD_SIM.id)

        # Assert
        # The realized PnL should be -2 USD (commission)
        assert result == Money(-2, USD)

    def test_total_pnl_for_instrument_when_only_realized_exists(self):
        # Arrange
        AccountFactory.register_calculated_account("SIM")

        account_id = AccountId("SIM-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        # Create and fill orders to open and close position
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("1.00010"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_closed(position))

        # Act
        result = self.portfolio.total_pnl(AUDUSD_SIM.id)

        # Assert
        # Should just return realized PnL since position is closed
        assert result == Money(6, USD)  # 10 USD profit - 4 USD commission

    def test_net_exposures_when_insufficient_data_for_xrate_returns_none(self):
        # Arrange
        AccountFactory.register_calculated_account("BITMEX")

        account_id = AccountId("BITMEX-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=BTC,
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        order = self.order_factory.market(
            ETHUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(100),
        )

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=ETHUSD_BITMEX,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("376.05"),
        )

        last_ethusd = QuoteTick(
            instrument_id=ETHUSD_BITMEX.id,
            bid_price=Price.from_str("376.05"),
            ask_price=Price.from_str("377.10"),
            bid_size=Quantity.from_str("16"),
            ask_size=Quantity.from_str("25"),
            ts_event=0,
            ts_init=0,
        )

        last_xbtusd = QuoteTick(
            instrument_id=BTCUSD_BITMEX.id,
            bid_price=Price.from_str("50000.00"),
            ask_price=Price.from_str("50000.00"),
            bid_size=Quantity.from_str("1"),
            ask_size=Quantity.from_str("1"),
            ts_event=0,
            ts_init=0,
        )

        position = Position(instrument=ETHUSD_BITMEX, fill=fill)

        self.portfolio.update_position(TestEventStubs.position_opened(position))
        self.cache.add_position(position, OmsType.HEDGING)
        self.cache.add_quote_tick(last_ethusd)
        self.cache.add_quote_tick(last_xbtusd)
        self.portfolio.update_quote_tick(last_ethusd)
        self.portfolio.update_quote_tick(last_xbtusd)

        # Act
        result = self.portfolio.net_exposures(BITMEX)

        # Assert
        assert result == {BTC: Money(0.00200000, BTC)}

    def test_opening_several_positions_updates_portfolio(self):
        # Arrange
        AccountFactory.register_calculated_account("SIM")

        account_id = AccountId("SIM-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        last_audusd = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.80501"),
            ask_price=Price.from_str("0.80505"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        last_gbpusd = QuoteTick(
            instrument_id=GBPUSD_SIM.id,
            bid_price=Price.from_str("1.30315"),
            ask_price=Price.from_str("1.30317"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_quote_tick(last_audusd)
        self.cache.add_quote_tick(last_gbpusd)
        self.portfolio.update_quote_tick(last_audusd)
        self.portfolio.update_quote_tick(last_gbpusd)

        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.cache.add_order(order1, position_id=None)
        self.cache.add_order(order2, position_id=None)

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=GBPUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00000"),
        )

        self.cache.update_order(order1)
        self.cache.update_order(order2)

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        position2 = Position(instrument=GBPUSD_SIM, fill=fill2)
        position_opened1 = TestEventStubs.position_opened(position1)
        position_opened2 = TestEventStubs.position_opened(position2)

        # Act
        self.cache.add_position(position1, OmsType.HEDGING)
        self.cache.add_position(position2, OmsType.HEDGING)
        self.portfolio.update_position(position_opened1)
        self.portfolio.update_position(position_opened2)

        # Assert
        assert self.portfolio.net_exposures(SIM) == {USD: Money(210816.00, USD)}
        assert self.portfolio.unrealized_pnls(SIM) == {USD: Money(10816.00, USD)}
        assert self.portfolio.realized_pnls(SIM) == {USD: Money(-4.00, USD)}
        assert self.portfolio.margins_maint(SIM) == {
            AUDUSD_SIM.id: Money(3002.00, USD),
            GBPUSD_SIM.id: Money(3002.00, USD),
        }
        assert self.portfolio.net_exposure(AUDUSD_SIM.id) == Money(80501.00, USD)
        assert self.portfolio.net_exposure(GBPUSD_SIM.id) == Money(130315.00, USD)
        assert self.portfolio.unrealized_pnl(AUDUSD_SIM.id) == Money(-19499.00, USD)
        assert self.portfolio.unrealized_pnl(GBPUSD_SIM.id) == Money(30315.00, USD)
        assert self.portfolio.realized_pnl(AUDUSD_SIM.id) == Money(-2.00, USD)
        assert self.portfolio.realized_pnl(GBPUSD_SIM.id) == Money(-2.00, USD)
        assert self.portfolio.total_pnl(AUDUSD_SIM.id) == Money(-19501.00, USD)
        assert self.portfolio.total_pnl(GBPUSD_SIM.id) == Money(30313.00, USD)
        assert self.portfolio.total_pnls(SIM) == {USD: Money(10812.00, USD)}
        assert self.portfolio.net_position(AUDUSD_SIM.id) == Decimal(100000)
        assert self.portfolio.net_position(GBPUSD_SIM.id) == Decimal(100000)
        assert self.portfolio.is_net_long(AUDUSD_SIM.id)
        assert not self.portfolio.is_net_short(AUDUSD_SIM.id)
        assert not self.portfolio.is_flat(AUDUSD_SIM.id)
        assert not self.portfolio.is_completely_flat()

    def test_modifying_position_updates_portfolio(self):
        # Arrange
        AccountFactory.register_calculated_account("SIM")

        account_id = AccountId("SIM-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        last_audusd = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.80501"),
            ask_price=Price.from_str("0.80505"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_quote_tick(last_audusd)
        self.portfolio.update_quote_tick(last_audusd)

        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position))

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(50_000),
        )

        order2_filled = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("1.00000"),
        )

        position.apply(order2_filled)

        # Act
        self.portfolio.update_position(TestEventStubs.position_changed(position))

        # Assert
        assert self.portfolio.net_exposures(SIM) == {USD: Money(40250.50, USD)}
        assert self.portfolio.unrealized_pnls(SIM) == {USD: Money(-9749.50, USD)}
        assert self.portfolio.realized_pnls(SIM) == {USD: Money(-3.00, USD)}
        assert self.portfolio.total_pnls(SIM) == {USD: Money(-9752.50, USD)}
        assert self.portfolio.margins_maint(SIM) == {AUDUSD_SIM.id: Money(1501.00, USD)}
        assert self.portfolio.net_exposure(AUDUSD_SIM.id) == Money(40250.50, USD)
        assert self.portfolio.realized_pnl(AUDUSD_SIM.id) == Money(-3.00, USD)
        assert self.portfolio.unrealized_pnl(AUDUSD_SIM.id) == Money(-9749.50, USD)
        assert self.portfolio.total_pnl(AUDUSD_SIM.id) == Money(-9752.50, USD)
        assert self.portfolio.net_position(AUDUSD_SIM.id) == Decimal(50000)
        assert self.portfolio.is_net_long(AUDUSD_SIM.id)
        assert not self.portfolio.is_net_short(AUDUSD_SIM.id)
        assert not self.portfolio.is_flat(AUDUSD_SIM.id)
        assert not self.portfolio.is_completely_flat()
        assert self.portfolio.unrealized_pnls(BINANCE) == {}
        assert self.portfolio.realized_pnls(BINANCE) == {}
        assert self.portfolio.total_pnls(BINANCE) == {}
        assert self.portfolio.net_exposures(BINANCE) is None

    def test_closing_position_updates_portfolio(self):
        # Arrange
        AccountFactory.register_calculated_account("SIM")

        account_id = AccountId("SIM-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position))

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        order2_filled = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("1.00010"),
        )

        position.apply(order2_filled)
        self.cache.update_position(position)

        # Act
        self.portfolio.update_position(TestEventStubs.position_closed(position))

        # Assert
        assert self.portfolio.net_exposures(SIM) == {}
        assert self.portfolio.unrealized_pnls(SIM) == {}
        assert self.portfolio.realized_pnls(SIM) == {USD: Money(6, USD)}
        assert self.portfolio.total_pnl(AUDUSD_SIM.id) == Money(6, USD)
        assert self.portfolio.margins_maint(SIM) == {}
        assert self.portfolio.net_exposure(AUDUSD_SIM.id) == Money(0, USD)
        assert self.portfolio.unrealized_pnl(AUDUSD_SIM.id) == Money(0, USD)
        assert self.portfolio.realized_pnl(AUDUSD_SIM.id) == Money(6, USD)
        assert self.portfolio.total_pnls(SIM) == {USD: Money(6, USD)}
        assert self.portfolio.net_position(AUDUSD_SIM.id) == Decimal(0)
        assert not self.portfolio.is_net_long(AUDUSD_SIM.id)
        assert not self.portfolio.is_net_short(AUDUSD_SIM.id)
        assert self.portfolio.is_flat(AUDUSD_SIM.id)
        assert self.portfolio.is_completely_flat()

    def test_several_positions_with_different_instruments_updates_portfolio(self):
        # Arrange
        account_id = AccountId("SIM-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state)

        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order3 = self.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order4 = self.order_factory.market(
            GBPUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("1.00000"),
        )

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=GBPUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-3"),
            last_px=Price.from_str("1.00000"),
        )

        fill4 = TestEventStubs.order_filled(
            order4,
            instrument=GBPUSD_SIM,
            strategy_id=StrategyId("S-1"),
            account_id=account_id,
            position_id=PositionId("P-3"),
            last_px=Price.from_str("1.00100"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        position2 = Position(instrument=AUDUSD_SIM, fill=fill2)
        position3 = Position(instrument=GBPUSD_SIM, fill=fill3)

        last_audusd = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.80501"),
            ask_price=Price.from_str("0.80505"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        last_gbpusd = QuoteTick(
            instrument_id=GBPUSD_SIM.id,
            bid_price=Price.from_str("1.30315"),
            ask_price=Price.from_str("1.30317"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_quote_tick(last_audusd)
        self.cache.add_quote_tick(last_gbpusd)
        self.portfolio.update_quote_tick(last_audusd)
        self.portfolio.update_quote_tick(last_gbpusd)

        self.cache.add_position(position1, OmsType.HEDGING)
        self.cache.add_position(position2, OmsType.HEDGING)
        self.cache.add_position(position3, OmsType.HEDGING)

        # Act
        self.portfolio.update_position(TestEventStubs.position_opened(position1))
        self.portfolio.update_position(TestEventStubs.position_opened(position2))
        self.portfolio.update_position(TestEventStubs.position_opened(position3))

        position3.apply(fill4)
        self.cache.update_position(position3)
        self.portfolio.update_position(TestEventStubs.position_closed(position3))

        # Assert
        assert self.portfolio.unrealized_pnls(SIM) == {USD: Money(-38998.00, USD)}
        assert self.portfolio.realized_pnls(SIM) == {USD: Money(92.00, USD)}
        assert self.portfolio.total_pnls(SIM) == {USD: Money(-38906.00, USD)}
        assert self.portfolio.net_exposures(SIM) == {USD: Money(161002.00, USD)}
        assert self.portfolio.net_exposure(AUDUSD_SIM.id) == Money(161002.00, USD)
        assert self.portfolio.unrealized_pnl(AUDUSD_SIM.id) == Money(-38998.00, USD)
        assert self.portfolio.realized_pnl(AUDUSD_SIM.id) == Money(-4.00, USD)
        assert self.portfolio.total_pnl(AUDUSD_SIM.id) == Money(-39002.00, USD)
        assert self.portfolio.unrealized_pnl(GBPUSD_SIM.id) == Money(0, USD)
        assert self.portfolio.total_pnl(GBPUSD_SIM.id) == Money(96.00, USD)
        assert self.portfolio.net_position(AUDUSD_SIM.id) == Decimal(200000)
        assert self.portfolio.net_position(GBPUSD_SIM.id) == Decimal(0)
        assert self.portfolio.is_net_long(AUDUSD_SIM.id)
        assert self.portfolio.is_flat(GBPUSD_SIM.id)
        assert not self.portfolio.is_completely_flat()

    def test_opening_betting_position_updates_portfolio(self):
        # Arrange
        AccountFactory.register_calculated_account("BETFAIR")
        account_id = AccountId("BETFAIR-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, GBP),
                    Money(0, GBP),
                    Money(1_000_000, GBP),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Add betting instrument to cache
        self.cache.add_instrument(BETTING_INSTRUMENT)
        self.portfolio.update_account(state)

        # Create and fill a BACK bet order
        order = self.order_factory.limit(
            BETTING_INSTRUMENT.id,
            OrderSide.BUY,  # BACK bet
            Quantity.from_str("100.0"),  # Stake
            price=Price.from_str("2.0"),  # Betting odds
        )

        self.cache.add_order(order, position_id=None)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=BETTING_INSTRUMENT,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("2.0"),
        )

        self.exec_engine.process(fill)

        # Add market data
        last = QuoteTick(
            instrument_id=BETTING_INSTRUMENT.id,
            bid_price=Price.from_str("2.10"),  # Odds have moved higher
            ask_price=Price.from_str("2.12"),
            bid_size=Quantity.from_str("1000.0"),
            ask_size=Quantity.from_str("1000.0"),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_quote_tick(last)

        # Act
        position = Position(instrument=BETTING_INSTRUMENT, fill=fill)
        self.cache.add_position(position, OmsType.NETTING)
        self.portfolio.initialize_orders()  # Add this
        self.portfolio.initialize_positions()  # Add this
        self.portfolio.update_position(TestEventStubs.position_opened(position))
        self.portfolio.update_quote_tick(last)

        # Assert
        assert self.portfolio.net_exposures(BETFAIR) == {GBP: Money(-200.00, GBP)}  # Stake * odds
        assert self.portfolio.unrealized_pnls(BETFAIR) == {GBP: Money(4.76, GBP)}
        assert self.portfolio.realized_pnls(BETFAIR) == {GBP: Money(0.00, GBP)}  # Commission
        assert self.portfolio.net_exposure(BETTING_INSTRUMENT.id) == Money(-200.00, GBP)
        assert self.portfolio.unrealized_pnl(BETTING_INSTRUMENT.id) == Money(4.76, GBP)
        assert self.portfolio.realized_pnl(BETTING_INSTRUMENT.id) == Money(0.00, GBP)
        assert self.portfolio.total_pnl(BETTING_INSTRUMENT.id) == Money(4.76, GBP)
        assert self.portfolio.total_pnls(BETFAIR) == {GBP: Money(4.76, GBP)}
        assert self.portfolio.net_position(BETTING_INSTRUMENT.id) == Decimal("100.0")
        assert self.portfolio.is_net_long(BETTING_INSTRUMENT.id)
        assert not self.portfolio.is_net_short(BETTING_INSTRUMENT.id)
        assert not self.portfolio.is_flat(BETTING_INSTRUMENT.id)
        assert not self.portfolio.is_completely_flat()

    def test_closing_betting_position_updates_portfolio(self):
        # Arrange
        AccountFactory.register_calculated_account("BETFAIR")
        account_id = AccountId("BETFAIR-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, GBP),
                    Money(0, GBP),
                    Money(1_000_000, GBP),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Add betting instrument to cache
        self.cache.add_instrument(BETTING_INSTRUMENT)
        self.portfolio.update_account(state)

        # Create and fill a BACK bet order
        order1 = self.order_factory.limit(
            BETTING_INSTRUMENT.id,
            OrderSide.BUY,  # BACK bet
            Quantity.from_str("100.0"),  # Stake
            price=Price.from_str("2.0"),  # Betting odds
        )

        self.cache.add_order(order1, position_id=None)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BETTING_INSTRUMENT,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("2.0"),
        )
        self.exec_engine.process(fill1)

        # LAY bet to close position at better odds (profit)
        order2 = self.order_factory.limit(
            BETTING_INSTRUMENT.id,
            OrderSide.SELL,  # LAY bet
            Quantity.from_str("100.0"),  # Same stake
            price=Price.from_str("1.8"),  # Better odds for LAY
        )

        self.cache.add_order(order2, position_id=None)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(TestEventStubs.order_accepted(order2))

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BETTING_INSTRUMENT,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("1.8"),
        )
        self.exec_engine.process(fill2)

        last = QuoteTick(
            instrument_id=BETTING_INSTRUMENT.id,
            bid_price=Price.from_str("1.80"),  # Match the closing price
            ask_price=Price.from_str("1.81"),
            bid_size=Quantity.from_str("1000.0"),
            ask_size=Quantity.from_str("1000.0"),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_quote_tick(last)

        # Create and update position
        position = Position(instrument=BETTING_INSTRUMENT, fill=fill1)
        position.apply(fill2)  # Apply the closing fill
        self.cache.add_position(position, OmsType.NETTING)

        # Initialize and update
        self.portfolio.initialize_orders()
        self.portfolio.initialize_positions()
        self.portfolio.update_position(TestEventStubs.position_closed(position))
        self.portfolio.update_quote_tick(last)

        # Assert
        assert self.portfolio.net_exposures(BETFAIR) == {}
        assert self.portfolio.unrealized_pnls(BETFAIR) == {GBP: Money(0.00, GBP)}
        assert self.portfolio.realized_pnls(BETFAIR) == {GBP: Money(-10.00, GBP)}
        assert self.portfolio.net_exposure(BETTING_INSTRUMENT.id) == Money(-20.00, GBP)
        assert self.portfolio.unrealized_pnl(BETTING_INSTRUMENT.id) == Money(0.00, GBP)
        assert self.portfolio.realized_pnl(BETTING_INSTRUMENT.id) == Money(-10.00, GBP)
        assert self.portfolio.total_pnl(BETTING_INSTRUMENT.id) == Money(-10.00, GBP)
        assert self.portfolio.total_pnls(BETFAIR) == {GBP: Money(-10.00, GBP)}
        assert self.portfolio.net_position(BETTING_INSTRUMENT.id) == Decimal("0.0")
        assert not self.portfolio.is_net_long(BETTING_INSTRUMENT.id)
        assert not self.portfolio.is_net_short(BETTING_INSTRUMENT.id)
        assert self.portfolio.is_flat(BETTING_INSTRUMENT.id)
        assert self.portfolio.is_completely_flat()

    @pytest.mark.parametrize(
        (
            "back_price",
            "lay_price",
            "back_size",
            "lay_size",
            "mark_price",
            "expected_exposure",
            "expected_realized",
            "expected_unrealized",
        ),
        [
            [2.0, 3.0, 100_000, 10_000, 3.0, -170_000.0, 0.0, 33_333.33],
            [2.0, 3.0, 10_000, 100_000, 3.0, 280_000.0, 0.0, 3_333.33],
            [2.0, 3.0, 50_000, 10_000, 3.0, -70_000.0, 0.0, 16_666.67],
            [6.0, 2.0, 10_000, 50_000, 4.0, 40_000.0, 0.0, -30_000.0],
            [3.0, 2.0, 10_000, 100_000, 3.0, 170_000, 0.0, -33_333.33],
        ],
    )
    def test_betting_position_hedging_back_then_lay_open(
        self,
        back_price: float,
        lay_price: float,
        back_size: int,
        lay_size: int,
        mark_price: float,
        expected_exposure: float,
        expected_realized: float,
        expected_unrealized: float,
    ) -> None:
        # Arrange
        self.portfolio.set_specific_venue(Venue("BETFAIR"))
        self.portfolio.set_use_mark_prices(True)
        self.portfolio.set_use_mark_xrates(True)

        AccountFactory.register_calculated_account("BETFAIR")
        account_id = AccountId("BETFAIR-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    total=Money(1_000_000, GBP),
                    locked=Money(0, GBP),
                    free=Money(1_000_000, GBP),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        instrument = TestInstrumentProvider.betting_instrument()
        self.cache.add_instrument(instrument)
        self.portfolio.update_account(state)

        mark = MarkPriceUpdate(
            instrument_id=instrument.id,
            value=Price(mark_price, GBP.precision),
            ts_event=0,
            ts_init=1,
        )
        self.cache.add_mark_price(mark)

        # Step 1: Place and fill a BACK bet (BUY)
        order1 = self.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,  # BACK bet
            quantity=Quantity.from_int(back_size),  # Stake
            price=Price(back_price, GBP.precision),  # Odds
        )
        position_id1 = PositionId("1")
        self.cache.add_order(order1, position_id=position_id1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1, account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order1, account_id))

        fill1 = TestEventStubs.order_filled(
            order=order1,
            instrument=instrument,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=position_id1,
            last_px=Price(back_price, GBP.precision),
        )
        self.exec_engine.process(fill1)

        # Step 2: Place and fill a LAY bet (SELL)
        order2 = self.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,  # LAY bet
            quantity=Quantity.from_int(lay_size),
            price=Price(lay_price, GBP.precision),
        )
        position_id2 = PositionId("2")
        self.cache.add_order(order2, position_id=position_id2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2, account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order2, account_id))

        fill2 = TestEventStubs.order_filled(
            order=order2,
            instrument=instrument,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=position_id2,
            last_px=Price(lay_price, GBP.precision),
        )
        self.exec_engine.process(fill2)

        # Assert
        assert self.portfolio.net_exposure(instrument.id, mark.value) == Money(
            expected_exposure,
            GBP,
        )
        assert self.portfolio.realized_pnl(instrument.id) == Money(expected_realized, GBP)
        assert self.portfolio.unrealized_pnl(instrument.id, mark.value) == Money(
            expected_unrealized,
            GBP,
        )

    @pytest.mark.parametrize(
        (
            "back_price",
            "lay_price",
            "back_size",
            "lay_size",
            "mark_price",
            "expected_exposure",
            "expected_realized",
            "expected_unrealized",
        ),
        [
            [3.0, 2.0, 10_000, 100_000, 3.0, 170_000, 0, -33_333.33],
            [2.0, 3.0, 100_000, 10_000, 3.0, -170_000, 0, 33_333.33],
        ],
    )
    def test_betting_position_hedging_lay_then_back_open(
        self,
        back_price: float,
        lay_price: float,
        back_size: int,
        lay_size: int,
        mark_price: float,
        expected_exposure: float,
        expected_realized: float,
        expected_unrealized: float,
    ) -> None:
        # Arrange
        self.portfolio.set_specific_venue(Venue("BETFAIR"))
        self.portfolio.set_use_mark_prices(True)
        self.portfolio.set_use_mark_xrates(True)

        AccountFactory.register_calculated_account("BETFAIR")
        account_id = AccountId("BETFAIR-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    total=Money(1_000_000, GBP),
                    locked=Money(0, GBP),
                    free=Money(1_000_000, GBP),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        instrument = TestInstrumentProvider.betting_instrument()
        self.cache.add_instrument(instrument)
        self.portfolio.update_account(state)

        mark = MarkPriceUpdate(
            instrument_id=instrument.id,
            value=Price(mark_price, GBP.precision),
            ts_event=0,
            ts_init=1,
        )
        self.cache.add_mark_price(mark)

        # Step 1: Place and fill a LAY bet (SELL)
        order1 = self.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL,  # LAY bet
            quantity=Quantity.from_int(lay_size),
            price=Price(lay_price, GBP.precision),
        )
        position_id1 = PositionId("1")
        self.cache.add_order(order1, position_id=position_id1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1, account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order1, account_id))

        fill1 = TestEventStubs.order_filled(
            order=order1,
            instrument=instrument,
            account_id=account_id,
            position_id=position_id1,
            last_px=Price(lay_price, GBP.precision),
        )
        self.exec_engine.process(fill1)

        # Step 2: Place and fill a BACK bet (BUY)
        order2 = self.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,  # BACK bet
            quantity=Quantity.from_int(back_size),  # Stake
            price=Price(back_price, GBP.precision),  # Odds
        )
        position_id2 = PositionId("2")
        self.cache.add_order(order2, position_id=position_id2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2, account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order2, account_id))

        fill2 = TestEventStubs.order_filled(
            order=order2,
            instrument=instrument,
            account_id=account_id,
            position_id=position_id2,
            last_px=Price(back_price, GBP.precision),
        )
        self.exec_engine.process(fill2)

        # Assert
        assert self.portfolio.net_exposure(instrument.id, mark.value) == Money(
            expected_exposure,
            GBP,
        )
        assert self.portfolio.realized_pnl(instrument.id) == Money(expected_realized, GBP)
        assert self.portfolio.unrealized_pnl(instrument.id, mark.value) == Money(
            expected_unrealized,
            GBP,
        )

    @pytest.mark.parametrize(
        (
            "side",
            "open_price",
            "close_price",
            "size",
            "mark_price",
            "expected_exposure",
            "expected_realized",
            "expected_unrealized",
        ),
        [
            [OrderSide.BUY, 3.0, 3.0, 100_000, 3.0, 0.0, 0.0, 0.0],
            [OrderSide.BUY, 3.0, 2.0, 100_000, 1.0, -100_000.0, 0.0, -100_000.0],
            [OrderSide.SELL, 3.0, 3.0, 100_000, 3.0, 0.0, 0.0, 0.0],
            [OrderSide.SELL, 3.0, 2.0, 100_000, 1.0, 100_000.0, 0.0, 100_000.0],
        ],
    )
    def test_betting_position_hedging_close(
        self,
        side: OrderSide,
        open_price: float,
        close_price: float,
        size: int,
        mark_price: float,
        expected_exposure: float,
        expected_realized: float,
        expected_unrealized: float,
    ) -> None:
        # Arrange
        self.portfolio.set_specific_venue(Venue("BETFAIR"))
        self.portfolio.set_use_mark_prices(True)
        self.portfolio.set_use_mark_xrates(True)

        AccountFactory.register_calculated_account("BETFAIR")
        account_id = AccountId("BETFAIR-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    total=Money(1_000_000, GBP),
                    locked=Money(0, GBP),
                    free=Money(1_000_000, GBP),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        instrument = TestInstrumentProvider.betting_instrument()
        self.cache.add_instrument(instrument)
        self.portfolio.update_account(state)

        mark = MarkPriceUpdate(
            instrument_id=instrument.id,
            value=Price(mark_price, GBP.precision),
            ts_event=0,
            ts_init=1,
        )
        self.cache.add_mark_price(mark)

        # Step 1: Place opening bet
        order1 = self.order_factory.limit(
            instrument_id=instrument.id,
            order_side=side,
            quantity=Quantity.from_int(size),
            price=Price(open_price, GBP.precision),
        )
        position_id1 = PositionId("1")
        self.cache.add_order(order1, position_id=position_id1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1, account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order1, account_id))

        fill1 = TestEventStubs.order_filled(
            order=order1,
            instrument=instrument,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=position_id1,
            last_px=Price(open_price, GBP.precision),
        )
        self.exec_engine.process(fill1)

        # Step 2: Place closing bet
        order2 = self.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL if side == OrderSide.BUY else OrderSide.BUY,
            quantity=Quantity.from_int(size),  # Stake
            price=Price(close_price, GBP.precision),
        )
        position_id2 = PositionId("2")
        self.cache.add_order(order2, position_id=position_id2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2, account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order2, account_id))

        fill2 = TestEventStubs.order_filled(
            order=order2,
            instrument=instrument,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=position_id2,
            last_px=Price(close_price, GBP.precision),
        )
        self.exec_engine.process(fill2)

        # Assert
        assert self.portfolio.net_exposure(instrument.id, mark.value) == Money(
            expected_exposure,
            GBP,
        )
        assert self.portfolio.realized_pnl(instrument.id) == Money(expected_realized, GBP)
        assert self.portfolio.unrealized_pnl(instrument.id, mark.value) == Money(
            expected_unrealized,
            GBP,
        )

    @pytest.mark.parametrize(
        (
            "side",
            "open_price",
            "close_price",
            "size",
            "mark_price",
            "expected_exposure",
            "expected_realized",
            "expected_unrealized",
        ),
        [
            [OrderSide.BUY, 3.0, 3.0, 100_000, 3.0, 0.0, 0.0, 0.0],
            [OrderSide.BUY, 3.0, 2.0, 100_000, 1.0, 0.0, -33_333.33, 0.0],
            [OrderSide.SELL, 3.0, 3.0, 100_000, 3.0, 0.0, 0.0, 0.0],
            [OrderSide.SELL, 3.0, 2.0, 100_000, 1.0, 0.0, 33_333.33, 0.0],
        ],
    )
    def test_betting_position_netting_close(
        self,
        side: OrderSide,
        open_price: float,
        close_price: float,
        size: int,
        mark_price: float,
        expected_exposure: float,
        expected_realized: float,
        expected_unrealized: float,
    ) -> None:
        # Arrange
        self.portfolio.set_specific_venue(Venue("BETFAIR"))
        self.portfolio.set_use_mark_prices(True)
        self.portfolio.set_use_mark_xrates(True)

        AccountFactory.register_calculated_account("BETFAIR")
        account_id = AccountId("BETFAIR-01234")
        state = AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=GBP,
            reported=True,
            balances=[
                AccountBalance(
                    total=Money(1_000_000, GBP),
                    locked=Money(0, GBP),
                    free=Money(1_000_000, GBP),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        instrument = TestInstrumentProvider.betting_instrument()
        self.cache.add_instrument(instrument)
        self.portfolio.update_account(state)

        mark = MarkPriceUpdate(
            instrument_id=instrument.id,
            value=Price(mark_price, GBP.precision),
            ts_event=0,
            ts_init=1,
        )
        self.cache.add_mark_price(mark)

        # Step 1: Place opening bet
        order1 = self.order_factory.limit(
            instrument_id=instrument.id,
            order_side=side,
            quantity=Quantity.from_int(size),
            price=Price(open_price, GBP.precision),
        )
        self.cache.add_order(order1, position_id=None)
        self.exec_engine.process(TestEventStubs.order_submitted(order1, account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order1, account_id))

        fill1 = TestEventStubs.order_filled(
            order=order1,
            instrument=instrument,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=None,
            last_px=Price(open_price, GBP.precision),
        )
        self.exec_engine.process(fill1)

        # Step 2: Place closing bet
        order2 = self.order_factory.limit(
            instrument_id=instrument.id,
            order_side=OrderSide.SELL if side == OrderSide.BUY else OrderSide.BUY,
            quantity=Quantity.from_int(size),  # Stake
            price=Price(close_price, GBP.precision),
        )
        self.cache.add_order(order2, position_id=None)
        self.exec_engine.process(TestEventStubs.order_submitted(order2, account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order2, account_id))

        fill2 = TestEventStubs.order_filled(
            order=order2,
            instrument=instrument,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=None,
            last_px=Price(close_price, GBP.precision),
        )
        self.exec_engine.process(fill2)

        # Assert
        assert self.portfolio.net_exposure(instrument.id, mark.value) == Money(
            expected_exposure,
            GBP,
        )
        assert self.portfolio.realized_pnl(instrument.id) == Money(expected_realized, GBP)
        assert self.portfolio.unrealized_pnl(instrument.id, mark.value) == Money(
            expected_unrealized,
            GBP,
        )
