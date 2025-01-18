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

from nautilus_trader.core.nautilus_pyo3 import AccountBalance
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import MarginBalance
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


class TestTypesProviderPyo3:
    @staticmethod
    def account_balance(
        total: Money = Money.from_str("1525000 USD"),
        locked: Money = Money.from_str("25000 USD"),
        free: Money = Money.from_str("1500000 USD"),
    ) -> AccountBalance:
        return AccountBalance(total, locked, free)

    @staticmethod
    def margin_balance(
        initial: Money = Money(1, Currency.from_str("USD")),
        maintenance: Money = Money(1, Currency.from_str("USD")),
        instrument_id: InstrumentId = TestIdProviderPyo3.audusd_id(),
    ) -> MarginBalance:
        return MarginBalance(initial, maintenance, instrument_id)
