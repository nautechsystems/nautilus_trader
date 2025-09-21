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

import pytest

from nautilus_trader.accounting.accounts.cash import CashAccount
from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import LiquiditySide
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Position
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import cash_account_from_account_events
from nautilus_trader.test_kit.rust.accounting_pyo3 import TestAccountingProviderPyo3
from nautilus_trader.test_kit.rust.events_pyo3 import TestEventsProviderPyo3
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3
from nautilus_trader.test_kit.rust.orders_pyo3 import TestOrderProviderPyo3


AUDUSD_SIM = TestIdProviderPyo3.audusd_id()
AUD_USD = TestInstrumentProviderPyo3.default_fx_ccy("AUD/USD")
USD_JPY = TestInstrumentProviderPyo3.default_fx_ccy("USD/JPY")
USD = Currency.from_str("USD")
BTC = Currency.from_str("BTC")
ETH = Currency.from_str("ETH")
AUD = Currency.from_str("AUD")
JPY = Currency.from_str("JPY")


def test_instantiated_account_basic_properties():
    account = TestAccountingProviderPyo3.cash_account()

    assert account.id == AccountId("SIM-000")
    assert str(account) == "CashAccount(id=SIM-000, type=CASH, base=USD)"
    assert repr(account) == "CashAccount(id=SIM-000, type=CASH, base=USD)"
    assert account == account


def test_instantiate_single_asset_cash_account():
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    event = TestEventsProviderPyo3.cash_account_state_million_usd()

    assert account.base_currency == Currency.from_str("USD")
    assert account.last_event == TestEventsProviderPyo3.cash_account_state_million_usd()
    assert account.events == [event]
    assert account.event_count == 1
    assert account.balance_total() == Money(1_000_000, USD)
    assert account.balance_free() == Money(1_000_000, USD)
    assert account.balance_locked() == Money(0, USD)
    assert account.balances_total() == {USD: Money(1_000_000, USD)}
    assert account.balances_free() == {USD: Money(1_000_000, USD)}
    assert account.balances_locked() == {USD: Money(0, USD)}


def test_instantiate_multi_asset_cash_account():
    account = TestAccountingProviderPyo3.cash_account_multi()
    event = TestEventsProviderPyo3.cash_account_state_multi()

    assert account.id == AccountId("SIM-000")
    assert account.base_currency is None
    assert account.last_event == event
    assert account.event_count == 1
    assert account.events == [event]
    assert account.balance_total(BTC) == Money(10.00000000, BTC)
    assert account.balance_total(ETH) == Money(20.00000000, ETH)
    assert account.balance_free(BTC) == Money(10.00000000, BTC)
    assert account.balance_free(ETH) == Money(20.00000000, ETH)
    assert account.balance_locked(BTC) == Money(0.00000000, BTC)
    assert account.balance_locked(ETH) == Money(0.00000000, ETH)
    assert account.balances_total() == {
        BTC: Money(10.00000000, BTC),
        ETH: Money(20.00000000, ETH),
    }
    assert account.balances_free() == {
        BTC: Money(10.00000000, BTC),
        ETH: Money(20.00000000, ETH),
    }
    assert account.balances_locked() == {
        BTC: Money(0.00000000, BTC),
        ETH: Money(0.00000000, ETH),
    }


def test_apply_given_new_state_event_updates_correctly():
    account = TestAccountingProviderPyo3.cash_account_multi()
    event = TestEventsProviderPyo3.cash_account_state_multi()
    new_event = TestEventsProviderPyo3.cash_account_state_multi_changed_btc()

    account.apply(new_event)

    assert account.last_event == new_event
    assert account.events == [event, new_event]
    assert account.event_count == 2
    assert account.balance_total(BTC) == Money(9.00000000, BTC)
    assert account.balance_free(BTC) == Money(8.50000000, BTC)
    assert account.balance_locked(BTC) == Money(0.50000000, BTC)
    assert account.balance_total(ETH) == Money(20.00000000, ETH)
    assert account.balance_free(ETH) == Money(20.00000000, ETH)
    assert account.balance_locked(ETH) == Money(0.00000000, ETH)


def test_calculate_balance_locked_buy():
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    result = account.calculate_balance_locked(
        instrument=AUD_USD,
        side=OrderSide.BUY,
        quantity=Quantity.from_int(1_000_000),
        price=Price.from_str("0.80"),
    )
    assert result == Money(800_000.00, USD)  # Notional


def test_calculate_balance_locked_sell():
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    result = account.calculate_balance_locked(
        instrument=AUD_USD,
        side=OrderSide.SELL,
        quantity=Quantity.from_int(1_000_000),
        price=Price.from_str("0.80"),
    )
    assert result == Money(1_000_000.00, AUD)  # Notional


def test_calculate_balance_locked_sell_no_base_currency():
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    result = account.calculate_balance_locked(
        instrument=TestInstrumentProviderPyo3.aapl_equity(),
        side=OrderSide.SELL,
        quantity=Quantity.from_int(100),
        price=Price.from_str("1500.00"),
    )
    assert result == Money(100.00, USD)  # Notional


def test_calculate_pnls_for_single_currency_cash_account():
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    order = TestOrderProviderPyo3.market_order(
        instrument_id=AUD_USD.id,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(1_000_000),
    )
    fill = TestEventsProviderPyo3.order_filled(
        order=order,
        instrument=AUD_USD,
        position_id=TestIdProviderPyo3.position_id(),
        strategy_id=TestIdProviderPyo3.strategy_id(),
        last_px=Price.from_str("0.80"),
    )
    position = Position(AUD_USD, fill)
    result = account.calculate_pnls(
        instrument=AUD_USD,
        fill=fill,
        position=position,
    )
    assert result == [Money(-800000.00, USD)]


def test_calculate_commission_when_given_liquidity_side_none_raises_value_error():
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    instrument = TestInstrumentProviderPyo3.xbtusd_bitmex()
    with pytest.raises(ValueError):
        account.calculate_commission(
            instrument=instrument,
            last_qty=Quantity.from_int(100_000),
            last_px=Price.from_str("11450.50"),
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
        )


@pytest.mark.parametrize(
    ("use_quote_for_inverse", "expected"),
    [
        [False, Money(-0.00218331, BTC)],  # Negative commission = credit
        [True, Money(-25.00, USD)],  # Negative commission = credit
    ],
)
def test_calculate_commission_for_inverse_maker_crypto(use_quote_for_inverse, expected):
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    instrument = TestInstrumentProviderPyo3.xbtusd_bitmex()

    result = account.calculate_commission(
        instrument=instrument,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("11450.50"),
        liquidity_side=LiquiditySide.MAKER,
        use_quote_for_inverse=use_quote_for_inverse,
    )

    assert result == expected


def test_calculate_commission_for_taker_fx():
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    instrument = AUD_USD

    result = account.calculate_commission(
        instrument=instrument,
        last_qty=Quantity.from_int(1_500_000),
        last_px=Price.from_str("0.80050"),
        liquidity_side=LiquiditySide.TAKER,
    )

    assert result == Money(24.02, USD)


def test_calculate_commission_crypto_taker():
    account = TestAccountingProviderPyo3.cash_account_million_usd()
    instrument = TestInstrumentProviderPyo3.xbtusd_bitmex()

    result = account.calculate_commission(
        instrument=instrument,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("11450.50"),
        liquidity_side=LiquiditySide.TAKER,
    )

    assert result == Money(0.00654993, BTC)


def test_calculate_commission_fx_taker():
    account = TestAccountingProviderPyo3.cash_account_million_usd()

    # Act
    result = account.calculate_commission(
        instrument=USD_JPY,
        last_qty=Quantity.from_int(2_200_000),
        last_px=Price.from_str("120.310"),
        liquidity_side=LiquiditySide.TAKER,
    )

    # Assert
    assert result == Money(5294, JPY)


def test_pyo3_cython_conversion():
    account_pyo3 = TestAccountingProviderPyo3.cash_account_million_usd()
    account_cython = CashAccount.from_dict(account_pyo3.to_dict())
    account_cython_dict = CashAccount.to_dict(account_cython)
    account_pyo3_back = cash_account_from_account_events(
        events=account_cython_dict["events"],
        calculate_account_state=account_cython_dict["calculate_account_state"],
        allow_borrowing=account_cython_dict.get("allow_borrowing", False),
    )
    assert account_pyo3 == account_pyo3_back
