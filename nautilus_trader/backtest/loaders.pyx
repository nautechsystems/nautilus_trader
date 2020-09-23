# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd
import pytz

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal64

from nautilus_trader.model.enums import Currency  # Do not remove

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.currency cimport currency_from_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CSVTickDataLoader:
    """
    Provides a means of loading tick data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(str file_path) -> pd.DataFrame:
        """
        Return the tick pandas.DataFrame loaded from the given csv file.

        :param file_path: The absolute path to the CSV file.
        :return: pd.DataFrame.
        """
        Condition.not_none(file_path, "file_path")

        return pd.read_csv(file_path,
                           usecols=[1, 2, 3],
                           index_col=0,
                           header=None,
                           parse_dates=True)


cdef class CSVBarDataLoader:
    """
    Provides a means of loading bar data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(str file_path) -> pd.DataFrame:
        """
        Return the bar pandas.DataFrame loaded from the given csv file.

        :param file_path: The absolute path to the CSV file.
        :return: pd.DataFrame.
        """
        Condition.not_none(file_path, "file_path")

        return pd.read_csv(file_path,
                           index_col="Time (UTC)",
                           parse_dates=True)


cdef class InstrumentLoader:
    """
    Provides instrument template methods for backtesting.
    """

    cpdef Instrument default_fx_ccy(self, Symbol symbol):
        """
        Return a default FX currency pair instrument from the given arguments.

        :param symbol: The currency pair symbol.
        :raises ValueError: If the symbol.code length is not in range [6, 7].
        """
        Condition.not_none(symbol, "symbol")
        Condition.in_range_int(len(symbol.code), 6, 7, "len(symbol)")

        cdef Currency base_currency = currency_from_string(symbol.code[:3])
        cdef Currency quote_currency = currency_from_string(symbol.code[-3:])

        # Check tick precision of quote currency
        if quote_currency == Currency.JPY:
            price_precision = 3
        else:
            price_precision = 5

        return Instrument(
            symbol=symbol,
            quote_currency=quote_currency,
            security_type=SecurityType.FOREX,
            price_precision=price_precision,
            size_precision=0,
            min_stop_distance_entry=0,
            min_limit_distance_entry=0,
            min_stop_distance=0,
            min_limit_distance=0,
            tick_size=Price(1 / (10 ** price_precision), price_precision),
            round_lot_size=Quantity(1000),
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal64(),
            rollover_interest_sell=Decimal64(),
            timestamp=datetime.now(pytz.utc)
        )
