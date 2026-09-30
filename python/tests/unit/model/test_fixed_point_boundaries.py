# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Test fixed-point Python boundaries.
"""

import subprocess
import sys


def test_fixed_point_boundaries_do_not_abort_subprocess() -> None:
    """
    Test fixed-point boundary errors do not abort a subprocess.
    """
    code = (
        "from nautilus_trader.model import Currency, CurrencyType, Money, Price, Quantity\n"
        "currency = Currency('TST18', 18, 0, 'Test 18dp', CurrencyType.CRYPTO)\n"
        "money = Money.zero(currency)\n"
        "assert money.raw == 0\n"
        "assert money.currency == currency\n"
        "calls = (\n"
        "    lambda: Price.from_mantissa_exponent(9223372036854775807, 100, 0),\n"
        "    lambda: Quantity.from_mantissa_exponent(18446744073709551615, 100, 0),\n"
        ")\n"
        "for call in calls:\n"
        "    try:\n"
        "        call()\n"
        "    except ValueError:\n"
        "        pass\n"
        "    else:\n"
        "        raise AssertionError('expected ValueError')\n"
        "print('fixed-point boundaries passed')\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "fixed-point boundaries passed"
    assert result.stderr == ""


def test_float_precision_boundaries_do_not_abort_subprocess() -> None:
    """
    Test float precision boundary errors do not abort a subprocess.
    """
    code = (
        "from nautilus_trader.indicators import ExponentialMovingAverage\n"
        "from nautilus_trader.model import AccountBalance, Bar, BarType, BookOrder, Currency\n"
        "from nautilus_trader.model import CurrencyType, Money, OrderSide, Price, Quantity\n"
        "currency = Currency('TST18', 18, 0, 'Test 18dp', CurrencyType.CRYPTO)\n"
        "price = Price.from_str('1.500000000000000000')\n"
        "quantity = Quantity.from_str('2.000000000000000000')\n"
        "money = Money.from_raw(1, currency)\n"
        "balance = AccountBalance(total=money, locked=Money.zero(currency), free=money)\n"
        "assert balance.to_dict()['total'] == '0.000000000000000001'\n"
        "bar_type = BarType.from_str('AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL')\n"
        "bar = Bar(bar_type, price, price, price, price, quantity, 0, 0)\n"
        "calls = (\n"
        "    lambda: float(price),\n"
        "    lambda: quantity.as_double(),\n"
        "    lambda: float(money),\n"
        "    lambda: BookOrder(OrderSide.BUY, price, quantity, 1).exposure(),\n"
        "    lambda: ExponentialMovingAverage(10).handle_bar(bar),\n"
        ")\n"
        "for call in calls:\n"
        "    try:\n"
        "        call()\n"
        "    except ValueError:\n"
        "        pass\n"
        "    else:\n"
        "        raise AssertionError('expected ValueError')\n"
        "print('float precision boundaries passed')\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "float precision boundaries passed"
    assert result.stderr == ""
