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

import pytest

from nautilus_trader.accounting.accounts.margin import MarginAccount
from nautilus_trader.accounting.manager import AccountsManager
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
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


def _fresh_margin_account() -> MarginAccount:
    """
    Build a margin account with no pre-populated margin balances.
    """
    event = AccountState(
        account_id=TestIdStubs.account_id(),
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
    return MarginAccount(event)


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

    def test_instantiate_multi_asset_margin_account_with_empty_balances(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=None,
            reported=True,
            balances=[],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account = MarginAccount(event)

        # Assert
        assert account.base_currency is None
        assert account.last_event == event
        assert account.events == [event]
        assert account.event_count == 1
        assert account.currencies() == []
        assert account.balances_total() == {}

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

    def test_apply_replaces_margin_balances_from_account_state(self):
        # Arrange
        account = TestExecStubs.margin_account()
        event = AccountState(
            account_id=account.id,
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
            margins=[
                MarginBalance(
                    Money(12_500, USD),
                    Money(25_000, USD),
                    USDJPY_SIM.id,
                ),
            ],
            info={},
            event_id=UUID4(),
            ts_event=1,
            ts_init=1,
        )

        # Act
        account.apply(event)

        # Assert
        assert account.margin_init(USDJPY_SIM.id) == Money(12_500, USD)
        assert account.margin_maint(USDJPY_SIM.id) == Money(25_000, USD)
        assert account.margin_init(AUDUSD_SIM.id) is None
        assert account.margin_maint(AUDUSD_SIM.id) is None

    def test_apply_empty_balance_update_to_multi_asset_margin_account_is_noop(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=None,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[
                MarginBalance(
                    Money(12_500, USD),
                    Money(25_000, USD),
                    USDJPY_SIM.id,
                ),
                MarginBalance(
                    Money(5_000, USD),
                    Money(10_000, USD),
                    None,
                ),
            ],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )
        account = MarginAccount(event)
        new_event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=None,
            reported=True,
            balances=[],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=1,
            ts_init=1,
        )

        # Act
        account.apply(new_event)

        # Assert
        assert account.last_event == new_event
        assert account.event_count == 2
        assert account.balance_total(USD) == Money(1_000_000, USD)
        assert account.margin_init(USDJPY_SIM.id) == Money(12_500, USD)
        assert account.margin_maint(USDJPY_SIM.id) == Money(25_000, USD)
        assert account.account_margins_init()[USD] == Money(5_000, USD)
        assert account.account_margins_maint()[USD] == Money(10_000, USD)

    def test_apply_routes_account_wide_margin_by_currency(self):
        # Arrange
        account = TestExecStubs.margin_account()
        event = AccountState(
            account_id=account.id,
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
            margins=[
                MarginBalance(
                    Money(12_500, USD),
                    Money(25_000, USD),
                    None,  # account-wide entry keyed by currency
                ),
            ],
            info={},
            event_id=UUID4(),
            ts_event=1,
            ts_init=1,
        )

        # Act
        account.apply(event)

        # Assert
        assert account.margins() == {}
        assert USD in account.account_margins()
        assert account.margin_init_for_currency(USD) == Money(12_500, USD)
        assert account.margin_maint_for_currency(USD) == Money(25_000, USD)
        assert account.account_margins_init()[USD] == Money(12_500, USD)
        assert account.account_margins_maint()[USD] == Money(25_000, USD)
        # Strict per-instrument lookup must not pick up account-wide entries.
        assert account.margin_init(USDJPY_SIM.id) is None
        assert account.margin_maint(USDJPY_SIM.id) is None

    def test_apply_routes_mixed_per_instrument_and_account_wide(self):
        # Arrange
        account = TestExecStubs.margin_account()
        event = AccountState(
            account_id=account.id,
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
            margins=[
                MarginBalance(Money(100, USD), Money(50, USD), USDJPY_SIM.id),
                MarginBalance(Money(200, USD), Money(150, USD), None),
            ],
            info={},
            event_id=UUID4(),
            ts_event=1,
            ts_init=1,
        )

        # Act
        account.apply(event)

        # Assert
        assert account.margin_init(USDJPY_SIM.id) == Money(100, USD)
        assert account.margin_init_for_currency(USD) == Money(200, USD)
        assert account.total_margin_init(USD) == Money(300, USD)
        assert account.total_margin_maint(USD) == Money(200, USD)

    def test_update_margin_routes_account_wide_entry(self):
        # Arrange
        account = TestExecStubs.margin_account()
        per_instrument_before = dict(account.margins())
        account_wide = MarginBalance(Money(1_500, USD), Money(750, USD), None)

        # Act
        account.update_margin(account_wide)

        # Assert
        assert account.margin_for_currency(USD) == account_wide
        assert account.margin_init_for_currency(USD) == Money(1_500, USD)
        assert account.margin_maint_for_currency(USD) == Money(750, USD)
        # Account-wide entry must not leak into per-instrument storage.
        assert account.margins() == per_instrument_before

    def test_total_margin_sums_per_instrument_and_account_wide(self):
        # Arrange
        account = _fresh_margin_account()

        # Act
        account.update_margin(MarginBalance(Money(80, USD), Money(20, USD), USDJPY_SIM.id))
        account.update_margin(MarginBalance(Money(200, USD), Money(100, USD), None))

        # Assert
        assert account.total_margin_init(USD) == Money(280, USD)
        assert account.total_margin_maint(USD) == Money(120, USD)

    def test_total_margin_ignores_mismatched_currency(self):
        # Arrange
        account = _fresh_margin_account()

        # Act
        account.update_margin(MarginBalance(Money(100, USD), Money(50, USD), None))
        account.update_margin(MarginBalance(Money("1.5", BTC), Money("0.5", BTC), None))

        # Assert
        assert account.total_margin_init(USD) == Money(100, USD)
        assert account.total_margin_init(BTC) == Money("1.5", BTC)
        assert account.total_margin_maint(USD) == Money(50, USD)
        assert account.total_margin_maint(BTC) == Money("0.5", BTC)

    def test_clear_account_margin_removes_entry_and_recalculates(self):
        # Arrange
        account = _fresh_margin_account()
        account.update_margin(MarginBalance(Money(500, USD), Money(250, USD), None))
        assert account.margin_for_currency(USD) is not None

        # Act
        account.clear_account_margin(USD)

        # Assert
        assert account.margin_for_currency(USD) is None
        assert account.margin_init_for_currency(USD) is None
        assert account.margin_maint_for_currency(USD) is None
        assert account.total_margin_init(USD) == Money(0, USD)

    def test_margin_for_currency_returns_none_when_absent(self):
        # Arrange
        account = TestExecStubs.margin_account()

        # Act / Assert
        assert account.margin_for_currency(USD) is None
        assert account.margin_init_for_currency(USD) is None
        assert account.margin_maint_for_currency(USD) is None

    def test_accounts_manager_generate_account_state_includes_account_margins(self):
        # Arrange
        clock = TestClock()
        logger = Logger("TestAccountsManager")
        cache = Cache()
        account = _fresh_margin_account()
        account.update_margin(MarginBalance(Money(150, USD), Money(75, USD), USDJPY_SIM.id))
        account.update_margin(MarginBalance(Money(500, USD), Money(250, USD), None))
        cache.add_account(account)

        manager = AccountsManager(cache=cache, clock=clock, logger=logger)

        # Act
        state = manager.generate_account_state(account, ts_event=0)

        # Assert: regenerated event must retain both per-instrument and account-wide entries.
        assert len(state.margins) == 2
        per_instrument = [m for m in state.margins if m.instrument_id is not None]
        account_wide = [m for m in state.margins if m.instrument_id is None]
        assert len(per_instrument) == 1
        assert per_instrument[0].initial == Money(150, USD)
        assert len(account_wide) == 1
        assert account_wide[0].initial == Money(500, USD)
        assert account_wide[0].currency == USD

    @pytest.mark.parametrize(
        (
            "per_instrument_initial",
            "per_instrument_maint",
            "account_wide_initial",
            "account_wide_maint",
            "expected_locked",
        ),
        [
            # Only per-instrument: locked = initial + maintenance
            (100, 50, 0, 0, 150),
            # Only account-wide: locked = initial + maintenance
            (0, 0, 200, 150, 350),
            # Both: locked sums across buckets
            (100, 50, 200, 150, 500),
        ],
        ids=["per_instrument_only", "account_wide_only", "mixed"],
    )
    def test_recalculate_balance_sums_per_instrument_and_account_wide(
        self,
        per_instrument_initial,
        per_instrument_maint,
        account_wide_initial,
        account_wide_maint,
        expected_locked,
    ):
        # Arrange
        account = _fresh_margin_account()
        if per_instrument_initial or per_instrument_maint:
            account.update_margin(
                MarginBalance(
                    Money(per_instrument_initial, USD),
                    Money(per_instrument_maint, USD),
                    USDJPY_SIM.id,
                ),
            )
        if account_wide_initial or account_wide_maint:
            account.update_margin(
                MarginBalance(
                    Money(account_wide_initial, USD),
                    Money(account_wide_maint, USD),
                    None,
                ),
            )

        # Assert: balance.locked reflects the combined margin reservation.
        assert account.balance_locked(USD) == Money(expected_locked, USD)
        assert account.balance_total(USD) == Money(1_000_000, USD)
        assert account.balance_free(USD) == Money(1_000_000 - expected_locked, USD)

    def test_accounts_manager_generate_account_state_survives_round_trip(self):
        # Arrange — regenerating and re-applying must not drop account-wide margins.
        clock = TestClock()
        logger = Logger("TestAccountsManager")
        cache = Cache()
        account = _fresh_margin_account()
        account.update_margin(MarginBalance(Money(500, USD), Money(250, USD), None))
        cache.add_account(account)

        manager = AccountsManager(cache=cache, clock=clock, logger=logger)

        # Act
        regenerated = manager.generate_account_state(account, ts_event=1)
        account.apply(regenerated)

        # Assert
        assert account.margin_init_for_currency(USD) == Money(500, USD)
        assert account.margin_maint_for_currency(USD) == Money(250, USD)

    def test_recalculate_balance_uses_raw_and_clamps(self):
        # Arrange
        raw_total = 5_000_000_000_000  # large raw value to guard against float drift
        total_money = Money.from_raw(raw_total, USD)
        event = AccountState(
            account_id=AccountId("RAW-MARGIN"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    total_money,
                    Money.from_raw(0, USD),
                    Money.from_raw(raw_total, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = MarginAccount(event)
        instrument = AUDUSD_SIM

        # Act/Assert: non-clamp path (margin == total)
        account.update_margin_init(instrument.id, Money.from_raw(raw_total, USD))
        balance = account.balance(USD)
        assert balance.total.raw - balance.locked.raw == balance.free.raw
        assert balance.locked.raw == raw_total
        assert balance.free.raw == 0

        # Act/Assert: clamp path (margin > total)
        account.update_margin_init(instrument.id, Money.from_raw(raw_total + 12345, USD))
        balance = account.balance(USD)
        assert balance.total.raw - balance.locked.raw == balance.free.raw
        assert balance.locked.raw == raw_total
        assert balance.free.raw == 0

    def test_recalculate_balance_uses_raw_and_clamps_with_maintenance_margin(self):
        # Arrange
        raw_total = 5_000_000_000_000  # large raw value to guard against float drift
        total_money = Money.from_raw(raw_total, USD)
        event = AccountState(
            account_id=AccountId("RAW-MARGIN-MAINT"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    total_money,
                    Money.from_raw(0, USD),
                    Money.from_raw(raw_total, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = MarginAccount(event)
        instrument = AUDUSD_SIM

        # Act/Assert: non-clamp path (maintenance == total)
        account.update_margin_maint(instrument.id, Money.from_raw(raw_total, USD))
        balance = account.balance(USD)
        assert balance.total.raw - balance.locked.raw == balance.free.raw
        assert balance.locked.raw == raw_total
        assert balance.free.raw == 0

        # Act/Assert: clamp path (maintenance > total)
        account.update_margin_maint(instrument.id, Money.from_raw(raw_total + 12345, USD))
        balance = account.balance(USD)
        assert balance.total.raw - balance.locked.raw == balance.free.raw
        assert balance.locked.raw == raw_total
        assert balance.free.raw == 0

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

    def test_margin_account_calculate_initial_margin(self):
        """
        Test that MarginAccount correctly calculates initial margin requirements.
        """
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = AUDUSD_SIM

        quantity = Quantity.from_int(100_000)
        price = Price.from_str("1.00000")

        # Act
        initial_margin = account.calculate_margin_init(
            instrument=instrument,
            quantity=quantity,
            price=price,
            use_quote_for_inverse=False,
        )

        # Assert
        # With default leverage, margin should be a fraction of notional
        notional = quantity.as_decimal() * price.as_decimal()
        assert initial_margin > Money(0, USD)
        assert initial_margin < Money(notional, USD)

    def test_margin_account_calculate_maintenance_margin(self):
        """
        Test that MarginAccount correctly calculates maintenance margin requirements.
        """
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = AUDUSD_SIM

        quantity = Quantity.from_int(100_000)
        price = Price.from_str("1.00000")

        # Act
        maint_margin = account.calculate_margin_maint(
            instrument=instrument,
            side=OrderSide.BUY,
            quantity=quantity,
            price=price,
            use_quote_for_inverse=False,
        )

        # Assert
        # Maintenance margin should be less than initial margin
        initial_margin = account.calculate_margin_init(
            instrument=instrument,
            quantity=quantity,
            price=price,
            use_quote_for_inverse=False,
        )
        assert maint_margin > Money(0, USD)
        assert maint_margin <= initial_margin

    def test_margin_account_calculate_balance_locked_with_leverage(self):
        """
        Test that MarginAccount leverage affects margin calculations.
        """
        # Arrange
        account = TestExecStubs.margin_account()
        account.set_default_leverage(Decimal(10))  # 10x leverage
        instrument = AUDUSD_SIM

        # Act - Calculate initial margin with leverage
        margin = account.calculate_margin_init(
            instrument=instrument,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("1.00000"),
            use_quote_for_inverse=False,
        )

        # Assert
        # With 10x leverage, margin should be ~10,000 USD for 100,000 notional
        assert margin > Money(0, USD)
        assert margin < Money(20_000.00, USD)  # Should be around 10,000

    def test_margin_account_calculate_commission_on_trade(self):
        """
        Test that MarginAccount correctly calculates commission on trades.
        """
        # Arrange
        account = TestExecStubs.margin_account()
        instrument = AUDUSD_SIM

        quantity = Quantity.from_int(100_000)
        price = Price.from_str("1.00000")

        # Act
        commission = account.calculate_commission(
            instrument=instrument,
            last_qty=quantity,
            last_px=price,
            liquidity_side=LiquiditySide.TAKER,
            use_quote_for_inverse=False,
        )

        # Assert
        assert commission is not None
        assert commission.currency == USD
        assert commission > Money(0, USD)
        # Commission should be small percentage of notional
        assert commission < Money(1000.00, USD)  # Less than 1% of 100k

    def test_margin_account_update_commissions(self):
        """
        Test that MarginAccount.update_commissions tracks commissions.
        """
        # Arrange
        account = TestExecStubs.margin_account()

        # Act - Update commissions
        account.update_commissions(Money(10.00, USD))
        account.update_commissions(Money(5.00, USD))

        # Assert
        commission = account.commission(USD)
        assert commission == Money(15.00, USD)  # Should accumulate
