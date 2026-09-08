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
Re-export the shipped test kit providers for the test suite.
"""

from datetime import UTC
from datetime import datetime

from nautilus_trader.model import AssetClass
from nautilus_trader.model import Currency
from nautilus_trader.model import Equity
from nautilus_trader.model import FuturesContract
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.testkit.providers import TEST_DATA_DIR
from nautilus_trader.testkit.providers import TestDataProvider
from nautilus_trader.testkit.providers import TestInstrumentProvider as InstrumentProvider


__all__ = [
    "TEST_DATA_DIR",
    "TestDataProvider",
    "TestInstrumentProvider",
]


class TestInstrumentProvider(InstrumentProvider):
    """
    Supply instruments for catalog tests.
    """

    @staticmethod
    def aapl_equity() -> Equity:
        """
        Return an AAPL equity instrument.
        """
        return Equity(
            instrument_id=InstrumentId.from_str("AAPL.XNAS"),
            raw_symbol=Symbol("AAPL"),
            isin="US0378331005",
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            lot_size=Quantity.from_int(100),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def futures_contract_es() -> FuturesContract:
        """
        Return an ES futures contract.
        """
        activation_ns = int(datetime(2021, 9, 17, tzinfo=UTC).timestamp() * 1_000_000_000)
        expiration_ns = int(datetime(2021, 12, 17, tzinfo=UTC).timestamp() * 1_000_000_000)
        return FuturesContract(
            instrument_id=InstrumentId.from_str("ESZ21.GLBX"),
            raw_symbol=Symbol("ESZ21"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            underlying="ES",
            activation_ns=activation_ns,
            expiration_ns=expiration_ns,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
