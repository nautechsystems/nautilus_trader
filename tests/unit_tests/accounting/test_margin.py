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

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
ADABTC_BINANCE = TestInstrumentProvider.adabtc_binance()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestMarginAccount:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_instantiated_accounts_basic_properties(self):
        # Arrange, Act
        account = TestExecStubs.margin_account()

        # Assert
        assert account.id == AccountId("SIM-000")
        assert str(account) == "MarginAccount(id=SIM-000, type=MARGIN, base=USD)"
        assert repr(account) == "MarginAccount(id=SIM-000, type=MARGIN, base=USD)"
        assert isinstance(hash(account), int)
        assert account == account
        assert account == account
        assert account.default_leverage == Decimal(1)

    def test_set_default_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act
        account.set_default_leverage(Decimal(100))

        # Assert
        assert account.default_leverage == Decimal(100)
        assert account.leverages() == {}

    def test_set_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act
        account.set_leverage(AUDUSD_SIM.id, Decimal(100))

        # Assert
        assert account.leverage(AUDUSD_SIM.id) == Decimal(100)
        assert account.leverages() == {AUDUSD_SIM.id: Decimal(100)}

    def test_is_unleveraged_with_leverage_returns_false(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act
        account.set_leverage(AUDUSD_SIM.id, Decimal(100))

        # Assert
        assert not account.is_unleveraged(AUDUSD_SIM.id)

    def test_is_unleveraged_with_no_leverage_returns_true(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act
        account.set_leverage(AUDUSD_SIM.id, Decimal(1))

        # Assert
        assert account.is_unleveraged(AUDUSD_SIM.id)

    def test_is_unleveraged_with_default_leverage_of_1_returns_true(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act, Assert
        assert account.is_unleveraged(AUDUSD_SIM.id)

    def test_update_margin_init(self):
        # Arrange
        account = TestExecStubs.margin_account()
        margin = Money(1_000.00, USD)

        # Act
        account.update_margin_init(AUDUSD_SIM.id, margin)

        # Assert
        assert account.margin_init(AUDUSD_SIM.id) == margin
        assert account.margins_init() == {AUDUSD_SIM.id: margin}

    def test_update_margin_maint(self):
        # Arrange
        account = TestExecStubs.margin_account()
        margin = Money(1_000.00, USD)

        # Act
        account.update_margin_maint(AUDUSD_SIM.id, margin)

        # Assert
        assert account.margin_maint(AUDUSD_SIM.id) == margin
        assert account.margins_maint() == {AUDUSD_SIM.id: margin}

    def test_calculate_margin_init_with_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        account.set_leverage(instrument.id, Decimal(50))

        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("0.80000"),
        )

        # Assert
        assert result == Money(48.00, USD)

    def test_calculate_margin_init_with_default_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        account.set_default_leverage(Decimal(10))

        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("0.80000"),
        )

        # Assert
        assert result == Money(240.00, USD)

    @pytest.mark.parametrize(
        ("use_quote_for_inverse", "expected"),
        [
            [False, Money(0.08700494, BTC)],
            [True, Money(1000.00, USD)],
        ],
    )
    def test_calculate_margin_init_with_no_leverage_for_inverse(
        self,
        use_quote_for_inverse,
        expected,
    ):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        result = account.calculate_margin_init(
            instrument=instrument,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11493.60"),
            use_quote_for_inverse=use_quote_for_inverse,
        )

        # Assert
        assert result == expected

    def test_calculate_margin_maint_with_no_leverage(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = account.calculate_margin_maint(
            instrument=instrument,
            side=PositionSide.LONG,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("11493.60"),
        )

        # Assert
        assert result == Money(0.03045173, BTC)

    def test_calculate_margin_maint_with_leverage_fx_instrument(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        account.set_default_leverage(Decimal(50))

        # Act
        result = account.calculate_margin_maint(
            instrument=instrument,
            side=PositionSide.LONG,
            quantity=Quantity.from_int(1_000_000),
            price=Price.from_str("1.00000"),
        )

        # Assert
        assert result == Money(600.00, USD)

    def test_calculate_margin_maint_with_leverage_inverse_instrument(self):
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()
        account.set_default_leverage(Decimal(10))

        # Act
        result = account.calculate_margin_maint(
            instrument=instrument,
            side=PositionSide.LONG,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("100000.00"),
        )

        # Assert
        assert result == Money(0.00035000, BTC)

    def test_calculate_pnls_with_no_position_returns_empty_list(self):
        # Arrange
        account = TestExecStubs.margin_account()

        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        # Act
        result = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill,
            position=None,  # No position
        )

        # Assert
        assert result == []

    def test_calculate_pnls_with_flat_position_returns_empty_list(self):
        # Arrange
        account = TestExecStubs.margin_account()

        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("1.0"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("51000.00"),
        )

        position.apply(fill2)  # Close the position

        # Act
        result = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        # Assert
        assert result == []
        assert position.is_closed

    def test_calculate_pnls_with_same_side_fill_returns_empty_list(self):
        # Arrange
        account = TestExecStubs.margin_account()

        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        # Add another BUY order (same side as position entry)
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("0.5"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("51000.00"),
        )

        # Act
        result = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        # Assert
        assert result == []

    def test_calculate_pnls_with_reducing_fill_calculates_pnl(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Open a LONG position
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("2.0"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        # Partially close the position (SELL 1.0 of 2.0)
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("1.0"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("52000.00"),  # $2000 profit per BTC
        )

        # Act
        account_pnl = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        expected_position_pnl = position.calculate_pnl(
            avg_px_open=position.avg_px_open,
            avg_px_close=fill2.last_px.as_double(),
            quantity=fill2.last_qty,
        )

        # Assert
        assert len(account_pnl) == 1
        expected_currency = BTCUSDT_BINANCE.get_cost_currency()
        assert account_pnl[0] == Money(2000.00, expected_currency)
        assert account_pnl[0] == expected_position_pnl

    def test_calculate_pnls_with_fill_larger_than_position_limits_correctly(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Open a LONG position of 1.0 BTC
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.0"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        # Try to sell MORE than the position size (2.0 vs 1.0)
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("2.0"),  # Larger than position!
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("52000.00"),
        )

        # Act
        account_pnl = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        position_pnl = position.calculate_pnl(
            avg_px_open=position.avg_px_open,
            avg_px_close=fill2.last_px.as_double(),
            quantity=Quantity.from_str("1.0"),
        )

        # Assert
        assert len(account_pnl) == 1
        expected_currency = BTCUSDT_BINANCE.get_cost_currency()
        assert account_pnl[0] == Money(2000.00, expected_currency)
        assert account_pnl[0] == position_pnl

    def test_calculate_pnls_with_short_position_reducing_fill(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Open a SHORT position
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("1.0"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        # Cover part of the short position (BUY to reduce SHORT)
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("0.5"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("48000.00"),  # $2000 profit per BTC (short position)
        )

        # Act
        account_pnl = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        expected_position_pnl = position.calculate_pnl(
            avg_px_open=position.avg_px_open,
            avg_px_close=fill2.last_px.as_double(),
            quantity=fill2.last_qty,
        )

        # Assert
        assert len(account_pnl) == 1
        expected_currency = BTCUSDT_BINANCE.get_cost_currency()
        assert account_pnl[0] == Money(1000.00, expected_currency)
        assert account_pnl[0] == expected_position_pnl

    def test_calculate_pnls_multiple_partial_reductions(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Open a LONG position of 3.0 BTC
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("3.0"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        # First partial close: sell 1.0 BTC
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("1.0"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("52000.00"),
        )

        position.apply(fill2)  # Update position after first fill

        account_pnl1 = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        expected_position_pnl1 = position.calculate_pnl(
            avg_px_open=position.avg_px_open,
            avg_px_close=fill2.last_px.as_double(),
            quantity=fill2.last_qty,
        )

        # Second partial close: sell 1.5 BTC
        order3 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("1.5"),
        )

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("53000.00"),
        )

        account_pnl2 = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill3,
            position=position,
        )

        # Act
        expected_position_pnl2 = position.calculate_pnl(
            avg_px_open=position.avg_px_open,
            avg_px_close=fill3.last_px.as_double(),
            quantity=fill3.last_qty,
        )

        # Assert
        assert len(account_pnl1) == 1
        expected_currency = BTCUSDT_BINANCE.get_cost_currency()
        assert account_pnl1[0] == Money(2000.00, expected_currency)

        assert len(account_pnl2) == 1
        assert account_pnl2[0] == Money(4500.00, expected_currency)

        assert account_pnl1[0] == expected_position_pnl1
        assert account_pnl2[0] == expected_position_pnl2

    def test_calculate_pnls_consistency_with_position_calculate_pnl(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Open a LONG position
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("2.0"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        # Reduce position
        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("1.0"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("52000.00"),
        )

        # Act - Calculate using both methods
        account_pnl = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        position_pnl = position.calculate_pnl(
            avg_px_open=position.avg_px_open,
            avg_px_close=fill2.last_px.as_double(),
            quantity=Quantity.from_str("1.0"),
        )

        # Assert
        assert len(account_pnl) == 1
        assert account_pnl[0] == position_pnl

    def test_calculate_pnls_github_issue_2657_reproduction(self):
        """
        Reproduce the exact scenario from GitHub issue #2657.

        https://github.com/nautechsystems/nautilus_trader/discussions/2657

        """
        # Arrange
        account = TestExecStubs.margin_account()

        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("0.001"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-GITHUB-2657"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50000.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("0.002"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-GITHUB-2657"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("50075.00"),
        )

        # Act
        account_pnl = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        expected_position_pnl = position.calculate_pnl(
            avg_px_open=position.avg_px_open,
            avg_px_close=fill2.last_px.as_double(),
            quantity=Quantity.from_str("0.001"),
        )

        # Assert
        assert len(account_pnl) == 1
        expected_currency = BTCUSDT_BINANCE.get_cost_currency()
        expected_amount = 75.0 * 0.001
        assert account_pnl[0] == Money(expected_amount, expected_currency)
        assert account_pnl[0] == expected_position_pnl
        assert account_pnl[0].as_double() == expected_amount

    def test_balance_impact_buy_order(self):
        # Arrange
        account = TestExecStubs.margin_account()
        account.set_default_leverage(Decimal(10))  # 10x leverage

        instrument = BTCUSDT_BINANCE
        quantity = Quantity.from_str("1.0")
        price = Price.from_str("50000.00")

        # Act
        impact = account.balance_impact(instrument, quantity, price, OrderSide.BUY)

        # Assert
        # With 10x leverage, should be -5000.00 USDT for 1.0 BTC at $50,000
        expected = Money(-5000.00, USDT)
        assert impact == expected

    def test_balance_impact_sell_order(self):
        # Arrange
        account = TestExecStubs.margin_account()
        account.set_default_leverage(Decimal(5))  # 5x leverage

        instrument = BTCUSDT_BINANCE
        quantity = Quantity.from_str("0.5")
        price = Price.from_str("60000.00")

        # Act
        impact = account.balance_impact(instrument, quantity, price, OrderSide.SELL)

        # Assert
        # With 5x leverage, should be +6000.00 USDT for 0.5 BTC at $60,000
        expected = Money(6000.00, USDT)
        assert impact == expected
