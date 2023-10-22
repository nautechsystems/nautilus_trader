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

from datetime import datetime
from typing import Optional

import pandas as pd
import pytz

from nautilus_trader.core.nautilus_pyo3 import CryptoFuture
from nautilus_trader.core.nautilus_pyo3 import CryptoPerpetual
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.test_kit.rust.types import TestTypesProviderPyo3


class TestInstrumentProviderPyo3:
    @staticmethod
    def ethusdt_perp_binance() -> CryptoPerpetual:
        return CryptoPerpetual(  # type: ignore
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

    @staticmethod
    def btcusdt_future_binance(expiry: Optional[pd.Timestamp] = None) -> CryptoFuture:
        if expiry is None:
            expiry = pd.Timestamp(datetime(2022, 3, 25), tz=pytz.UTC)
            nanos_expiry = int(expiry.timestamp() * 1e9)
        instrument_id_str = f"BTCUSDT_{expiry.strftime('%y%m%d')}.BINANCE"
        return CryptoFuture(  # type: ignore
            InstrumentId.from_str(instrument_id_str),
            Symbol("BTCUSDT"),
            TestTypesProviderPyo3.currency_btc(),
            TestTypesProviderPyo3.currency_usdt(),
            TestTypesProviderPyo3.currency_usdt(),
            nanos_expiry,
            2,
            6,
            Price.from_str("0.01"),
            Quantity.from_str("0.000001"),
            0.0,
            0.0,
            0.001,
            0.001,
            None,
            Quantity.from_str("9000"),
            Quantity.from_str("0.00001"),
            None,
            Money(10.0, TestTypesProviderPyo3.currency_usdt()),
            Price.from_str("1000000.0"),
            Price.from_str("0.01"),
        )
