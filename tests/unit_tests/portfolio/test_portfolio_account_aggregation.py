# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
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
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestPortfolioAccountAggregation:
    def setup_method(self):
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

        # Register instruments
        self.cache.add_instrument(BTCUSDT_BINANCE)

        # Setup accounts
        AccountFactory.register_calculated_account("BINANCE")

        self.account_id_1 = AccountId("BINANCE-001")
        self.account_id_2 = AccountId("BINANCE-002")

        state_1 = AccountState(
            account_id=self.account_id_1,
            account_type=AccountType.CASH,
            base_currency=None,
            reported=True,
            balances=[
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

        state_2 = AccountState(
            account_id=self.account_id_2,
            account_type=AccountType.CASH,
            base_currency=None,
            reported=True,
            balances=[
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

        self.portfolio.update_account(state_1)
        self.portfolio.update_account(state_2)

    def test_net_position_sums_across_multiple_accounts(self):
        # Arrange - Open positions in both accounts
        # Account 1: Long 1.0 BTC
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_1,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("50000.00"),
        )
        position1 = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        self.cache.add_position(position1, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position1))

        # Account 2: Short 0.4 BTC (Hedging mode allows this even if it's the same instrument)
        # Note: In hedging mode, positions are separate.
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("0.4"),
        )
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_2,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("50000.00"),
        )
        position2 = Position(instrument=BTCUSDT_BINANCE, fill=fill2)
        self.cache.add_position(position2, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position2))

        # Initialize positions
        self.portfolio.initialize_positions()

        # Act
        # Aggregate net position
        total_net = self.portfolio.net_position(BTCUSDT_BINANCE.id)

        # Individual net positions
        net_1 = self.portfolio.net_position(BTCUSDT_BINANCE.id, account_id=self.account_id_1)
        net_2 = self.portfolio.net_position(BTCUSDT_BINANCE.id, account_id=self.account_id_2)

        # Assert
        assert net_1 == Decimal("1.0")
        assert net_2 == Decimal("-0.4")
        assert total_net == Decimal("0.6")

    def test_net_exposure_sums_across_multiple_accounts(self):
        # Arrange - Open positions in both accounts
        # Account 1: Long 1.0 BTC @ 50,000
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_1,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("50000.00"),
        )
        position1 = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        self.cache.add_position(position1, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position1))

        # Account 2: Long 0.5 BTC @ 50,000
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("0.5"),
        )
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_2,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("50000.00"),
        )
        position2 = Position(instrument=BTCUSDT_BINANCE, fill=fill2)
        self.cache.add_position(position2, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position2))

        # Initialize positions
        self.portfolio.initialize_positions()

        # Act
        # Aggregate net exposure with explicit price
        current_price = Price.from_str("50000.00")

        # Exposure = Quantity * Price * Multiplier (1.0 for BTCUSDT)
        # Account 1: 1.0 * 50000 = 50000 USDT
        # Account 2: 0.5 * 50000 = 25000 USDT
        # Total: 75000 USDT
        total_exposure = self.portfolio.net_exposure(BTCUSDT_BINANCE.id, price=current_price)

        # Individual exposures
        exposure_1 = self.portfolio.net_exposure(
            BTCUSDT_BINANCE.id,
            price=current_price,
            account_id=self.account_id_1,
        )
        exposure_2 = self.portfolio.net_exposure(
            BTCUSDT_BINANCE.id,
            price=current_price,
            account_id=self.account_id_2,
        )

        # Assert
        assert exposure_1 == Money(50000.00, USDT)
        assert exposure_2 == Money(25000.00, USDT)
        assert total_exposure == Money(75000.00, USDT)

    def test_unrealized_pnl_with_explicit_price_sums_across_multiple_accounts(self):
        # Arrange - Open positions in both accounts
        # Account 1: Long 1.0 BTC @ 50,000
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_1,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("50000.00"),
        )
        position1 = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        self.cache.add_position(position1, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position1))

        # Account 2: Short 0.5 BTC @ 50,000
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("0.5"),
        )
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_2,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("50000.00"),
        )
        position2 = Position(instrument=BTCUSDT_BINANCE, fill=fill2)
        self.cache.add_position(position2, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position2))

        # Initialize positions
        self.portfolio.initialize_positions()

        # Act
        # Calculate PnL with explicit price of 51,000
        # Account 1 (Long 1.0): (51000 - 50000) * 1.0 = +1000 USDT
        # Account 2 (Short 0.5): (50000 - 51000) * 0.5 = -500 USDT
        # Total: +500 USDT
        current_price = Price.from_str("51000.00")

        total_pnl = self.portfolio.unrealized_pnl(BTCUSDT_BINANCE.id, price=current_price)
        pnl_1 = self.portfolio.unrealized_pnl(
            BTCUSDT_BINANCE.id,
            price=current_price,
            account_id=self.account_id_1,
        )
        pnl_2 = self.portfolio.unrealized_pnl(
            BTCUSDT_BINANCE.id,
            price=current_price,
            account_id=self.account_id_2,
        )

        # Assert
        assert pnl_1 == Money(1000.00, USDT)
        assert pnl_2 == Money(-500.00, USDT)
        assert total_pnl == Money(500.00, USDT)

    def test_is_net_long_short_flat_across_accounts(self):
        # Arrange - Open positions
        # Account 1: Long 1.0 BTC
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_1,
            position_id=PositionId("P-1"),
            last_px=Price.from_str("50000.00"),
        )
        position1 = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        self.cache.add_position(position1, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position1))

        # Account 2: Short 1.0 BTC
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("1.0"),
        )
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_2,
            position_id=PositionId("P-2"),
            last_px=Price.from_str("50000.00"),
        )
        position2 = Position(instrument=BTCUSDT_BINANCE, fill=fill2)
        self.cache.add_position(position2, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position2))

        # Initialize positions
        self.portfolio.initialize_positions()

        # Act & Assert

        # 1. Total is Flat (1.0 - 1.0 = 0)
        assert self.portfolio.is_flat(BTCUSDT_BINANCE.id) is True
        assert self.portfolio.is_net_long(BTCUSDT_BINANCE.id) is False
        assert self.portfolio.is_net_short(BTCUSDT_BINANCE.id) is False

        # 2. Account 1 is Long
        assert self.portfolio.is_net_long(BTCUSDT_BINANCE.id, account_id=self.account_id_1) is True
        assert self.portfolio.is_flat(BTCUSDT_BINANCE.id, account_id=self.account_id_1) is False

        # 3. Account 2 is Short
        assert self.portfolio.is_net_short(BTCUSDT_BINANCE.id, account_id=self.account_id_2) is True
        assert self.portfolio.is_flat(BTCUSDT_BINANCE.id, account_id=self.account_id_2) is False

        # Add more to Account 1 to make total Net Long
        order3 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("0.1"),
        )
        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=self.account_id_1,
            position_id=PositionId("P-3"),
            last_px=Price.from_str("50000.00"),
        )
        position3 = Position(instrument=BTCUSDT_BINANCE, fill=fill3)
        self.cache.add_position(position3, OmsType.HEDGING)
        self.portfolio.update_position(TestEventStubs.position_opened(position3))
        self.portfolio.initialize_positions()  # Update internal cache

        # Now Total is Net Long (1.1 - 1.0 = 0.1)
        assert self.portfolio.is_net_long(BTCUSDT_BINANCE.id) is True
        assert self.portfolio.is_flat(BTCUSDT_BINANCE.id) is False

    def test_net_exposure_returns_none_for_different_base_currencies(self):
        # Arrange - Create accounts with different base currencies
        account_id_usd = AccountId("BINANCE-USD")
        account_id_eur = AccountId("BINANCE-EUR")

        state_usd = AccountState(
            account_id=account_id_usd,
            account_type=AccountType.CASH,
            base_currency=USD,
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

        state_eur = AccountState(
            account_id=account_id_eur,
            account_type=AccountType.CASH,
            base_currency=EUR,
            reported=True,
            balances=[
                AccountBalance(
                    Money(100000.00, EUR),
                    Money(0.00, EUR),
                    Money(100000.00, EUR),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(state_usd)
        self.portfolio.update_account(state_eur)

        # Create positions in both accounts
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )
        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=account_id_usd,
            position_id=PositionId("P-USD"),
            last_px=Price.from_str("50000.00"),
        )
        position1 = Position(instrument=BTCUSDT_BINANCE, fill=fill1)
        self.cache.add_position(position1, OmsType.HEDGING)

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )
        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            strategy_id=StrategyId("S-1"),
            account_id=account_id_eur,
            position_id=PositionId("P-EUR"),
            last_px=Price.from_str("50000.00"),
        )
        position2 = Position(instrument=BTCUSDT_BINANCE, fill=fill2)
        self.cache.add_position(position2, OmsType.HEDGING)

        # Act - Aggregate exposure across accounts with different base currencies
        result = self.portfolio.net_exposure(BTCUSDT_BINANCE.id)

        # Assert - Should return None due to mismatched base currencies
        assert result is None
