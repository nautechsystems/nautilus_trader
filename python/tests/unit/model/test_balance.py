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
"""
Test balance behavior.
"""

from nautilus_trader.model import AccountBalance
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarginBalance
from nautilus_trader.model import Money


USD = Currency.from_str("USD")


def _account_balance() -> object:
    return AccountBalance(
        total=Money(1525000.00, USD),
        locked=Money(25000.00, USD),
        free=Money(1500000.00, USD),
    )


def _margin_balance() -> object:
    return MarginBalance(
        Money(1.00, USD),
        Money(1.00, USD),
        InstrumentId.from_str("AUD/USD.SIM"),
    )


def test_account_balance_equality() -> None:
    """
    Test account balance equality.
    """
    b1 = _account_balance()
    b2 = _account_balance()
    assert b1 == b2


def test_account_balance_properties() -> None:
    """
    Test account balance properties.
    """
    balance = _account_balance()

    assert balance.total == Money(1525000.00, USD)
    assert balance.locked == Money(25000.00, USD)
    assert balance.free == Money(1500000.00, USD)
    assert balance.currency == USD


def test_account_balance_display() -> None:
    """
    Test account balance display.
    """
    bal = _account_balance()
    expected = "AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)"
    assert str(bal) == expected
    assert repr(bal) == expected


def test_account_balance_to_from_dict() -> None:
    """
    Test account balance to from dict.
    """
    bal = _account_balance()
    d = bal.to_dict()
    assert bal == AccountBalance.from_dict(d)
    assert d == {
        "type": "AccountBalance",
        "free": "1500000.00",
        "locked": "25000.00",
        "total": "1525000.00",
        "currency": "USD",
    }


def test_margin_balance_equality() -> None:
    """
    Test margin balance equality.
    """
    m1 = _margin_balance()
    m2 = _margin_balance()
    assert m1 == m2


def test_margin_balance_properties() -> None:
    """
    Test margin balance properties.
    """
    balance = _margin_balance()

    assert balance.initial == Money(1.00, USD)
    assert balance.maintenance == Money(1.00, USD)
    assert balance.currency == USD
    assert balance.instrument_id == InstrumentId.from_str("AUD/USD.SIM")


def test_margin_balance_display() -> None:
    """
    Test margin balance display.
    """
    bal = _margin_balance()
    expected = "MarginBalance(initial=1.00 USD, maintenance=1.00 USD, instrument_id=AUD/USD.SIM)"
    assert str(bal) == expected


def test_margin_balance_to_from_dict() -> None:
    """
    Test margin balance to from dict.
    """
    bal = _margin_balance()
    d = bal.to_dict()
    assert bal == MarginBalance.from_dict(d)
    assert d == {
        "type": "MarginBalance",
        "initial": "1.00",
        "maintenance": "1.00",
        "instrument_id": "AUD/USD.SIM",
        "currency": "USD",
    }


def test_account_balance_hash() -> None:
    """
    Test account balance hash.
    """
    b1 = _account_balance()
    b2 = _account_balance()

    assert hash(b1) == hash(b2)


def test_account_balance_hash_differs() -> None:
    """
    Test account balance hash differs.
    """
    b1 = _account_balance()
    b2 = AccountBalance(
        total=Money(100.00, USD),
        locked=Money(0.00, USD),
        free=Money(100.00, USD),
    )

    assert hash(b1) != hash(b2)


def test_margin_balance_hash() -> None:
    """
    Test margin balance hash.
    """
    m1 = _margin_balance()
    m2 = _margin_balance()

    assert hash(m1) == hash(m2)


def test_account_balance_copy() -> None:
    """
    Test account balance copy.
    """
    bal = _account_balance()
    copy = bal.copy()

    assert copy == bal
    assert copy is not bal


def test_margin_balance_copy() -> None:
    """
    Test margin balance copy.
    """
    bal = _margin_balance()
    copy = bal.copy()

    assert copy == bal
    assert copy is not bal


def test_account_balance_not_equal_to_none() -> None:
    """
    Test account balance not equal to none.
    """
    bal = _account_balance()
    assert (bal == None) is False


def test_margin_balance_not_equal_to_none() -> None:
    """
    Test margin balance not equal to none.
    """
    bal = _margin_balance()
    assert (bal == None) is False
