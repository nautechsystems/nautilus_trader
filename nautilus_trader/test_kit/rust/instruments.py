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

import pandas as pd
import pytz

from nautilus_trader.core.nautilus_pyo3 import AssetClass
from nautilus_trader.core.nautilus_pyo3 import CryptoFuture
from nautilus_trader.core.nautilus_pyo3 import CryptoPerpetual
from nautilus_trader.core.nautilus_pyo3 import CurrencyPair
from nautilus_trader.core.nautilus_pyo3 import Equity
from nautilus_trader.core.nautilus_pyo3 import FuturesContract
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OptionKind
from nautilus_trader.core.nautilus_pyo3 import OptionsContract
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
    def btcusdt_future_binance(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> CryptoFuture:
        if activation is None:
            activation = pd.Timestamp(2021, 12, 25, tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp(2022, 3, 25, tz=pytz.utc)

        instrument_id_str = f"BTCUSDT_{expiration.strftime('%y%m%d')}.BINANCE"
        return CryptoFuture(  # type: ignore
            InstrumentId.from_str(instrument_id_str),
            Symbol("BTCUSDT"),
            TestTypesProviderPyo3.currency_btc(),
            TestTypesProviderPyo3.currency_usdt(),
            TestTypesProviderPyo3.currency_usdt(),
            activation.value,
            expiration.value,
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

    @staticmethod
    def btcusdt_binance() -> CurrencyPair:
        return CurrencyPair(  # type: ignore
            InstrumentId.from_str("BTCUSDT.BINANCE"),
            Symbol("BTCUSDT"),
            TestTypesProviderPyo3.currency_btc(),
            TestTypesProviderPyo3.currency_usdt(),
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
            Price.from_str("1000000"),
            Price.from_str("0.01"),
        )

    @staticmethod
    def appl_option(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> OptionsContract:
        if activation is None:
            activation = pd.Timestamp(datetime(2021, 9, 17), tz=pytz.UTC)
        if expiration is None:
            expiration = pd.Timestamp(datetime(2021, 12, 17), tz=pytz.UTC)
        return OptionsContract(  # type: ignore
            InstrumentId.from_str("AAPL211217C00150000.OPRA"),
            Symbol("AAPL211217C00150000"),
            AssetClass.EQUITY,
            "AAPL",
            OptionKind.CALL,
            activation.value,
            expiration.value,
            Price.from_str("149.0"),
            TestTypesProviderPyo3.currency_usdt(),
            2,
            Price.from_str("0.01"),
            0.0,
            0.0,
            0.001,
            0.001,
            Quantity.from_str("1.0"),
        )

    @staticmethod
    def appl_equity() -> Equity:
        return Equity(  # type: ignore
            InstrumentId.from_str("AAPL.NASDAQ"),
            Symbol("AAPL"),
            "US0378331005",
            TestTypesProviderPyo3.currency_usd(),
            2,
            Price.from_str("0.01"),
            Quantity.from_str("1"),
            0.0,
            0.0,
            0.001,
            0.001,
            Quantity.from_str("1.0"),
            None,
            None,
            None,
            None,
        )

    @staticmethod
    def futures_contract_es(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> FuturesContract:
        if activation is None:
            activation = pd.Timestamp(2021, 9, 17, tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp(2021, 12, 17, tz=pytz.utc)
        return FuturesContract(  # type: ignore
            InstrumentId.from_str("ESZ21.CME"),
            Symbol("ESZ21"),
            AssetClass.INDEX,
            "ES",
            activation.value,
            expiration.value,
            TestTypesProviderPyo3.currency_usd(),
            2,
            Price.from_str("0.01"),
            0.0,
            0.0,
            0.001,
            0.001,
            Quantity.from_str("1.0"),
            Quantity.from_str("1.0"),
        )
