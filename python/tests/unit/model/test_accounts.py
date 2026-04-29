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

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountBalance
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountState
from nautilus_trader.model import AccountType
from nautilus_trader.model import BettingAccount
from nautilus_trader.model import CashAccount
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import Currency
from nautilus_trader.model import LeveragedMarginModel
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import MarginAccount
from nautilus_trader.model import MarginBalance
from nautilus_trader.model import Money
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StandardMarginModel
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import VenueOrderId
from nautilus_trader.model import betting_account_from_account_events
from nautilus_trader.model import cash_account_from_account_events
from nautilus_trader.model import margin_account_from_account_events
from tests.providers import TestInstrumentProvider


def test_cash_account_properties_and_balances():
    usd = Currency.from_str("USD")
    balance = AccountBalance(
        total=Money.from_str("1000.00 USD"),
        locked=Money.from_str("100.00 USD"),
        free=Money.from_str("900.00 USD"),
    )
    state = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        balances=[balance],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=usd,
    )

    account = CashAccount(state, calculate_account_state=True, allow_borrowing=True)

    assert account.id == AccountId("SIM-001")
    assert account.account_type == AccountType.CASH
    assert account.base_currency == usd
    assert account.allow_borrowing is True
    assert account.calculate_account_state is True
    assert account.event_count == 1
    assert account.balance_total() == Money.from_str("1000.00 USD")
    assert account.balance_free() == Money.from_str("900.00 USD")
    assert account.balance_locked() == Money.from_str("100.00 USD")
    assert account.to_dict()["events"][0]["type"] == "AccountState"


def test_cash_account_apply_updates_balances():
    usd = Currency.from_str("USD")
    initial = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("100.00 USD"),
                free=Money.from_str("900.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=usd,
    )
    updated = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("1200.00 USD"),
                locked=Money.from_str("150.00 USD"),
                free=Money.from_str("1050.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=3,
        ts_init=4,
        base_currency=usd,
    )

    account = CashAccount(initial, calculate_account_state=True)
    account.apply(updated)

    assert account.event_count == 2
    assert account.balance_total() == Money.from_str("1200.00 USD")
    assert account.balance_free() == Money.from_str("1050.00 USD")
    assert account.balance_locked() == Money.from_str("150.00 USD")


def test_margin_account_properties_and_updates():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("100.00 USD"),
                free=Money.from_str("900.00 USD"),
            ),
        ],
        margins=[
            MarginBalance(
                initial=Money.from_str("10.00 USD"),
                maintenance=Money.from_str("5.00 USD"),
                instrument_id=instrument.id,
            ),
        ],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)
    account.set_default_leverage(Decimal(3))
    account.set_leverage(instrument.id, Decimal(5))
    account.update_initial_margin(instrument.id, Money.from_str("12.00 USD"))
    account.update_maintenance_margin(instrument.id, Money.from_str("6.00 USD"))

    assert account.id == AccountId("SIM-002")
    assert account.default_leverage == Decimal(3)
    assert account.leverage(instrument.id) == Decimal(5)
    assert account.initial_margin(instrument.id) == Money.from_str("12.00 USD")
    assert account.maintenance_margin(instrument.id) == Money.from_str("6.00 USD")
    assert account.is_unleveraged(instrument.id) is False
    assert account.to_dict()["events"][0]["account_type"] == "MARGIN"


def test_cash_account_from_account_events():
    state = AccountState(
        account_id=AccountId("SIM-003"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("100.00 USD"),
                free=Money.from_str("900.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=Currency.from_str("USD"),
    )

    account = cash_account_from_account_events(
        [state.to_dict()],
        calculate_account_state=True,
        allow_borrowing=True,
    )

    assert account.id == AccountId("SIM-003")
    assert account.balance_free() == Money.from_str("900.00 USD")
    assert account.allow_borrowing is True


def test_margin_account_from_account_events():
    instrument = TestInstrumentProvider.audusd_sim()
    state = AccountState(
        account_id=AccountId("SIM-004"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("100.00 USD"),
                free=Money.from_str("900.00 USD"),
            ),
        ],
        margins=[
            MarginBalance(
                initial=Money.from_str("10.00 USD"),
                maintenance=Money.from_str("5.00 USD"),
                instrument_id=instrument.id,
            ),
        ],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=Currency.from_str("USD"),
    )

    account = margin_account_from_account_events(
        [state.to_dict()],
        calculate_account_state=True,
    )

    assert account.id == AccountId("SIM-004")
    assert account.initial_margin(instrument.id) == Money.from_str("10.00 USD")
    assert account.maintenance_margin(instrument.id) == Money.from_str("5.00 USD")


def test_margin_model_exports():
    assert type(StandardMarginModel()).__name__ == "StandardMarginModel"
    assert type(LeveragedMarginModel()).__name__ == "LeveragedMarginModel"


def test_betting_account_properties():
    state = AccountState(
        account_id=AccountId("SIM-005"),
        account_type=AccountType.BETTING,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("125.00 USD"),
                free=Money.from_str("875.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=Currency.from_str("USD"),
    )

    account = BettingAccount(state, calculate_account_state=True)

    assert account.id == AccountId("SIM-005")
    assert account.account_type == AccountType.BETTING
    assert account.balance_locked() == Money.from_str("125.00 USD")


def test_betting_account_from_account_events():
    state = AccountState(
        account_id=AccountId("SIM-006"),
        account_type=AccountType.BETTING,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("125.00 USD"),
                free=Money.from_str("875.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=Currency.from_str("USD"),
    )

    account = betting_account_from_account_events(
        [state.to_dict()],
        calculate_account_state=True,
    )

    assert account.id == AccountId("SIM-006")
    assert account.balance_free() == Money.from_str("875.00 USD")


def test_cash_account_multi_currency_balances():
    usd = Currency.from_str("USD")
    btc = Currency.from_str("BTC")
    state = AccountState(
        account_id=AccountId("BINANCE-001"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("10000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("10000.00 USD"),
            ),
            AccountBalance(
                total=Money.from_str("1.50000000 BTC"),
                locked=Money.from_str("0.00000000 BTC"),
                free=Money.from_str("1.50000000 BTC"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    account = CashAccount(state, calculate_account_state=True)

    assert account.base_currency is None
    assert account.balance_total(usd) == Money.from_str("10000.00 USD")
    assert account.balance_total(btc) == Money.from_str("1.50000000 BTC")
    assert account.balance_free(usd) == Money.from_str("10000.00 USD")
    assert account.balance_free(btc) == Money.from_str("1.50000000 BTC")
    assert len(account.balances_total()) == 2
    assert len(account.balances_free()) == 2
    assert len(account.balances_locked()) == 2


def test_cash_account_calculate_balance_locked_buy():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = CashAccount(state, calculate_account_state=True)

    locked = account.calculate_balance_locked(
        instrument=instrument,
        side=OrderSide.BUY,
        quantity=Quantity.from_int(10_000),
        price=Price.from_str("0.80000"),
    )

    assert isinstance(locked, Money)


def test_cash_account_calculate_balance_locked_sell():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = CashAccount(state, calculate_account_state=True)

    locked = account.calculate_balance_locked(
        instrument=instrument,
        side=OrderSide.SELL,
        quantity=Quantity.from_int(10_000),
        price=Price.from_str("0.80000"),
    )

    assert isinstance(locked, Money)


def test_cash_account_calculate_commission():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = CashAccount(state, calculate_account_state=True)

    commission = account.calculate_commission(
        instrument=instrument,
        last_qty=Quantity.from_int(10_000),
        last_px=Price.from_str("0.80000"),
        liquidity_side=LiquiditySide.TAKER,
    )

    assert isinstance(commission, Money)


def test_cash_account_calculate_pnls():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = CashAccount(state, calculate_account_state=True)

    fill = OrderFilled(
        trader_id=TraderId("TESTER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-001"),
        venue_order_id=VenueOrderId("V-001"),
        account_id=AccountId("SIM-001"),
        trade_id=TradeId("T-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(10_000),
        last_px=Price.from_str("0.80000"),
        currency=Currency.from_str("USD"),
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        reconciliation=False,
    )

    pnls = account.calculate_pnls(instrument=instrument, fill=fill)

    assert isinstance(pnls, list)
    assert all(isinstance(m, Money) for m in pnls)


def test_cash_account_last_event_and_events():
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("1000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = CashAccount(state, calculate_account_state=True)

    assert account.last_event is not None
    assert isinstance(account.events, list)
    assert len(account.events) == 1


def test_margin_account_leverage_operations():
    instrument = TestInstrumentProvider.audusd_sim()
    instrument2 = TestInstrumentProvider.usdjpy_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)
    account.set_default_leverage(Decimal(10))
    account.set_leverage(instrument.id, Decimal(20))

    assert account.default_leverage == Decimal(10)
    assert account.leverage(instrument.id) == Decimal(20)
    assert account.leverage(instrument2.id) == Decimal(10)
    assert account.is_unleveraged(instrument.id) is False
    assert isinstance(account.leverages(), dict)


def test_margin_account_initial_margins():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)
    account.update_initial_margin(instrument.id, Money.from_str("500.00 USD"))
    account.update_maintenance_margin(instrument.id, Money.from_str("250.00 USD"))

    assert account.initial_margin(instrument.id) == Money.from_str("500.00 USD")
    assert account.maintenance_margin(instrument.id) == Money.from_str("250.00 USD")
    assert isinstance(account.initial_margins(), dict)
    assert isinstance(account.maintenance_margins(), dict)


def test_margin_account_calculate_initial_margin():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)
    account.set_default_leverage(Decimal(10))

    margin = account.calculate_initial_margin(
        instrument=instrument,
        quantity=Quantity.from_int(10_000),
        price=Price.from_str("0.80000"),
    )

    assert isinstance(margin, Money)


def test_margin_account_calculate_maintenance_margin():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)
    account.set_default_leverage(Decimal(10))

    margin = account.calculate_maintenance_margin(
        instrument=instrument,
        quantity=Quantity.from_int(10_000),
        price=Price.from_str("0.80000"),
    )

    assert isinstance(margin, Money)


def test_margin_account_is_unleveraged_default():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)

    assert account.is_unleveraged(instrument.id) is True


def test_margin_account_full_account_api():
    """
    MarginAccount must expose the full Account trait surface in pyo3 (parity with
    CashAccount and BettingAccount).

    Each newly exposed method below was missing
    before this patch; assert exact values rather than just ``isinstance`` so a
    regression that returns the wrong field (e.g. ``balance_free`` from
    ``balance_total``) would fail.

    """
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("100.00 USD"),
                free=Money.from_str("900.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)

    assert account.account_type == AccountType.MARGIN
    assert account.base_currency == usd
    assert account.calculate_account_state is True
    assert account.is_cash_account() is False
    assert account.is_margin_account() is True

    assert account.balance_total() == Money.from_str("1000.00 USD")
    assert account.balance_total(usd) == Money.from_str("1000.00 USD")
    assert account.balance_free() == Money.from_str("900.00 USD")
    assert account.balance_locked() == Money.from_str("100.00 USD")
    assert account.balances_total() == {usd: Money.from_str("1000.00 USD")}
    assert account.balances_free() == {usd: Money.from_str("900.00 USD")}
    assert account.balances_locked() == {usd: Money.from_str("100.00 USD")}

    expected_balance = AccountBalance(
        total=Money.from_str("1000.00 USD"),
        locked=Money.from_str("100.00 USD"),
        free=Money.from_str("900.00 USD"),
    )
    assert account.balance(usd) == expected_balance
    assert account.balances() == {usd: expected_balance}

    assert account.starting_balances() == {usd: Money.from_str("1000.00 USD")}
    assert account.currencies() == [usd]

    assert account.event_count == 1
    assert account.last_event == state
    assert account.events == [state]


def test_margin_account_apply_updates_balances():
    """
    Mirrors ``test_cash_account_apply_updates_balances`` to confirm the newly exposed
    ``apply`` method on ``MarginAccount`` updates state and bumps ``event_count``.
    """
    usd = Currency.from_str("USD")
    initial = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("100.00 USD"),
                free=Money.from_str("900.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1,
        ts_init=2,
        base_currency=usd,
    )
    updated = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("1200.00 USD"),
                locked=Money.from_str("150.00 USD"),
                free=Money.from_str("1050.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=3,
        ts_init=4,
        base_currency=usd,
    )

    account = MarginAccount(initial, calculate_account_state=True)
    account.apply(updated)

    assert account.event_count == 2
    assert account.balance_total() == Money.from_str("1200.00 USD")
    assert account.balance_free() == Money.from_str("1050.00 USD")
    assert account.balance_locked() == Money.from_str("150.00 USD")


def test_margin_account_calculate_balance_locked_buy():
    """
    Ensures ``calculate_balance_locked`` is callable via the newly exposed pyo3 method
    on ``MarginAccount``.
    """
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)
    account.set_default_leverage(Decimal(10))

    locked = account.calculate_balance_locked(
        instrument=instrument,
        side=OrderSide.BUY,
        quantity=Quantity.from_int(10_000),
        price=Price.from_str("0.80000"),
    )

    assert isinstance(locked, Money)
    assert locked.currency == usd


def test_margin_account_calculate_commission():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)

    commission = account.calculate_commission(
        instrument=instrument,
        last_qty=Quantity.from_int(10_000),
        last_px=Price.from_str("0.80000"),
        liquidity_side=LiquiditySide.TAKER,
    )

    assert isinstance(commission, Money)


def test_margin_account_calculate_pnls():
    instrument = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-002"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("100000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("100000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=usd,
    )

    account = MarginAccount(state, calculate_account_state=True)

    fill = OrderFilled(
        trader_id=TraderId("TESTER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-001"),
        venue_order_id=VenueOrderId("V-001"),
        account_id=AccountId("SIM-002"),
        trade_id=TradeId("T-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(10_000),
        last_px=Price.from_str("0.80000"),
        currency=usd,
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        reconciliation=False,
    )

    pnls = account.calculate_pnls(instrument=instrument, fill=fill)

    assert isinstance(pnls, list)
    assert all(isinstance(m, Money) for m in pnls)


def _account_for_purge(account_type: AccountType):
    """
    Build an account of the given type for ``purge_account_events`` parametrization.
    """
    usd = Currency.from_str("USD")
    state = AccountState(
        account_id=AccountId("SIM-007"),
        account_type=account_type,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("1000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=1_000_000_000,
        ts_init=1_000_000_000,
        base_currency=usd,
    )

    if account_type == AccountType.CASH:
        return CashAccount(state, calculate_account_state=True), state
    if account_type == AccountType.MARGIN:
        return MarginAccount(state, calculate_account_state=True), state
    if account_type == AccountType.BETTING:
        return BettingAccount(state, calculate_account_state=True), state
    raise ValueError(account_type)


def test_account_purge_account_events_retains_at_least_latest():
    """
    ``purge_account_events`` is documented to always retain at least the latest
    event (see ``BaseAccount::base_purge_account_events``), so a zero-lookback
    purge with a single starting event still leaves ``event_count == 1``.
    Exercised across all three account types since the method was newly added on
    each.
    """
    for account_type in (AccountType.CASH, AccountType.MARGIN, AccountType.BETTING):
        account, _state = _account_for_purge(account_type)
        ts_now = 2_000_000_000  # one second after ts_event in helper
        account.purge_account_events(ts_now=ts_now, lookback_secs=0)
        assert account.event_count == 1, (
            f"{account_type}: latest event must be retained even with lookback=0"
        )


def test_account_purge_account_events_drops_outdated_events():
    """
    With multiple events present, a zero-lookback purge keeps only the most recent one
    (the retain-latest guarantee).
    """
    usd = Currency.from_str("USD")
    balances = [
        AccountBalance(
            total=Money.from_str("1000.00 USD"),
            locked=Money.from_str("0.00 USD"),
            free=Money.from_str("1000.00 USD"),
        ),
    ]
    base_ts = 1_000_000_000

    for account_type in (AccountType.CASH, AccountType.MARGIN, AccountType.BETTING):
        account, _state = _account_for_purge(account_type)
        # Apply a second, newer event so the account holds two.
        newer = AccountState(
            account_id=AccountId("SIM-007"),
            account_type=account_type,
            balances=balances,
            margins=[],
            is_reported=True,
            event_id=UUID4(),
            ts_event=base_ts + 1,
            ts_init=base_ts + 1,
            base_currency=usd,
        )
        account.apply(newer)
        assert account.event_count == 2

        ts_now = base_ts + 1_000_000_000  # 1 second past the newer event
        account.purge_account_events(ts_now=ts_now, lookback_secs=0)
        assert account.event_count == 1, (
            f"{account_type}: only latest event should remain after purge"
        )


def test_account_purge_account_events_retains_recent_events():
    """
    With a large ``lookback_secs`` window, no events are purged.
    """
    for account_type in (AccountType.CASH, AccountType.MARGIN, AccountType.BETTING):
        account, _state = _account_for_purge(account_type)
        ts_now = 2_000_000_000
        account.purge_account_events(ts_now=ts_now, lookback_secs=10_000)
        assert account.event_count == 1, (
            f"{account_type}: event should be retained inside lookback window"
        )


def test_account_is_cash_vs_margin_helpers():
    """
    ``is_cash_account`` / ``is_margin_account`` were newly exposed on all three classes.

    CashAccount and BettingAccount both classify as cash accounts (the
    Rust trait impl on BettingAccount returns ``is_cash_account = true``) so the
    classification is binary cash-vs-margin, not three-way.

    """
    usd = Currency.from_str("USD")

    def _state_for(account_type: AccountType) -> AccountState:
        return AccountState(
            account_id=AccountId("SIM-008"),
            account_type=account_type,
            balances=[
                AccountBalance(
                    total=Money.from_str("1000.00 USD"),
                    locked=Money.from_str("0.00 USD"),
                    free=Money.from_str("1000.00 USD"),
                ),
            ],
            margins=[],
            is_reported=True,
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
            base_currency=usd,
        )

    cash = CashAccount(_state_for(AccountType.CASH), calculate_account_state=True)
    margin = MarginAccount(_state_for(AccountType.MARGIN), calculate_account_state=True)
    betting = BettingAccount(_state_for(AccountType.BETTING), calculate_account_state=True)

    assert cash.is_cash_account() is True
    assert cash.is_margin_account() is False
    assert margin.is_cash_account() is False
    assert margin.is_margin_account() is True
    assert betting.is_cash_account() is True
    assert betting.is_margin_account() is False
