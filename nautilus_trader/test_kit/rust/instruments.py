# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.nautilus_pyo3.model import CryptoPerpetual
from nautilus_trader.core.nautilus_pyo3.model import InstrumentId
from nautilus_trader.core.nautilus_pyo3.model import Money
from nautilus_trader.core.nautilus_pyo3.model import Price
from nautilus_trader.core.nautilus_pyo3.model import Quantity
from nautilus_trader.core.nautilus_pyo3.model import Symbol
from nautilus_trader.test_kit.rust.types import TestTypesProviderPyo3


class TestInstrumentProviderPyo3:
    @staticmethod
    def ethusdt_perp_binance() -> CryptoPerpetual:
        return CryptoPerpetual(
            InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
            Symbol("ETHUSDT"),
            TestTypesProviderPyo3.currency_eth(),
            TestTypesProviderPyo3.currency_usdt(),
            TestTypesProviderPyo3.currency_usdt(),
            2,
            0,
            Price.from_str("0.01"),
            Quantity.from_str("0.001"),
            0.0,
            0.0,
            0.001,
            0.001,
            None,
            Quantity.from_str("10000"),
            Quantity.from_str("0.001"),
            None,
            Money(10.0, TestTypesProviderPyo3.currency_usdt()),
            Price.from_str("15000.0"),
            Price.from_str("1.0"),
        )
