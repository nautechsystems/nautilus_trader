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
