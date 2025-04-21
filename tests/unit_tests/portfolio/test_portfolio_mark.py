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

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.config import PortfolioConfig
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


BINANCE = Venue("BINANCE")
BETFAIR = Venue("BETFAIR")

BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
BTCUSDT_PERP_BINANCE = TestInstrumentProvider.btcusdt_perp_binance()
BETTING_INSTRUMENT = TestInstrumentProvider.betting_instrument()


class TestPortfolioMarkPrices:
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

        # Create a portfolio with mark prices enabled
        config = PortfolioConfig(
            use_mark_prices=True,
            use_mark_xrates=False,
        )
        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.cache.add_instrument(BTCUSDT_BINANCE)
        self.cache.add_instrument(BTCUSDT_PERP_BINANCE)

    def test_opening_one_long_position_with_mark_prices_updates_portfolio(self):
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

        # Create a market order and simulate a fill
        order = self.order_factory.market(
            BTCUSDT_PERP_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("10.000000"),
        )
        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_PERP_BINANCE,
            strategy_id=StrategyId("S-001"),
            account_id=account_id,
            position_id=PositionId("P-123456"),
            last_px=Price.from_str("10500.00"),
        )

        # Add a mark price to the cache for the portfolio to look up
        mark_price = MarkPriceUpdate(
            instrument_id=BTCUSDT_PERP_BINANCE.id,
            value=Price.from_str("10510.00"),
            ts_event=0,
            ts_init=1,
        )
        self.cache.add_mark_price(mark_price)

        # Create a position for the filled order and update the portfolio
        position = Position(instrument=BTCUSDT_PERP_BINANCE, fill=fill)
        self.cache.add_position(position, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position))

        # Assert: the calculated portfolio values should reflect the mark price.
        # (The expected Money values mirror those from the existing test,
        # assuming the same calculation logic but using the mark price.)
        assert self.portfolio.net_exposures(BINANCE) == {USDT: Money(105100.00000000, USDT)}
        assert self.portfolio.unrealized_pnls(BINANCE) == {USDT: Money(100.00000000, USDT)}
        assert self.portfolio.realized_pnls(BINANCE) == {USDT: Money(-18.90000000, USDT)}
        assert self.portfolio.margins_maint(BINANCE) == {
            BTCUSDT_PERP_BINANCE.id: Money(2625.00000000, USDT),
        }
        assert self.portfolio.net_exposure(BTCUSDT_PERP_BINANCE.id) == Money(105100.00000000, USDT)
        assert self.portfolio.unrealized_pnl(BTCUSDT_PERP_BINANCE.id) == Money(100.00000000, USDT)
        assert self.portfolio.realized_pnl(BTCUSDT_PERP_BINANCE.id) == Money(-18.900000000, USDT)
        assert self.portfolio.net_position(order.instrument_id) == Decimal("10.00000000")
        assert self.portfolio.is_net_long(order.instrument_id)
        assert not self.portfolio.is_net_short(order.instrument_id)
        assert not self.portfolio.is_flat(order.instrument_id)
        assert not self.portfolio.is_completely_flat()
