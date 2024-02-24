# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
Defines tradable asset/contract instruments with specific properties dependent on the
asset class and instrument class.
"""

from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.base import instruments_from_pyo3
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.crypto_future import CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.instruments.futures_contract import FuturesContract
from nautilus_trader.model.instruments.futures_spread import FuturesSpread
from nautilus_trader.model.instruments.options_contract import OptionsContract
from nautilus_trader.model.instruments.options_spread import OptionsSpread
from nautilus_trader.model.instruments.synthetic import SyntheticInstrument


__all__ = [
    "Instrument",
    "BettingInstrument",
    "CryptoFuture",
    "CryptoPerpetual",
    "CurrencyPair",
    "Equity",
    "FuturesContract",
    "FuturesSpread",
    "OptionsContract",
    "OptionsSpread",
    "SyntheticInstrument",
    "instruments_from_pyo3",
]
