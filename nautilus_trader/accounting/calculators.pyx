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

from decimal import Decimal
from itertools import permutations

import pandas as pd

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.model.functions cimport price_type_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Currency


cdef class RolloverInterestCalculator:
    """
    Provides rollover interest rate calculations.

    If rate_data_csv_path is empty then will default to the included short-term
    interest rate data csv (data since 1956).

    Parameters
    ----------
    data : str
        The short term interest rate data.
    """

    def __init__(self, data not None: pd.DataFrame):
        self._rate_data = {
            "AUD": data.loc[data["LOCATION"] == "AUS"],
            "CAD": data.loc[data["LOCATION"] == "CAN"],
            "CHF": data.loc[data["LOCATION"] == "CHE"],
            "EUR": data.loc[data["LOCATION"] == "EA19"],
            "USD": data.loc[data["LOCATION"] == "USA"],
            "JPY": data.loc[data["LOCATION"] == "JPN"],
            "NZD": data.loc[data["LOCATION"] == "NZL"],
            "GBP": data.loc[data["LOCATION"] == "GBR"],
            "RUB": data.loc[data["LOCATION"] == "RUS"],
            "NOK": data.loc[data["LOCATION"] == "NOR"],
            "CNY": data.loc[data["LOCATION"] == "CHN"],
            "CNH": data.loc[data["LOCATION"] == "CHN"],
            "MXN": data.loc[data["LOCATION"] == "MEX"],
            "ZAR": data.loc[data["LOCATION"] == "ZAF"],
        }

    cpdef object get_rate_data(self):
        """
        Return the short-term interest rate dataframe.

        Returns
        -------
        pd.DataFrame

        """
        return self._rate_data

    cpdef object calc_overnight_rate(self, InstrumentId instrument_id, date date):
        """
        Return the rollover interest rate between the given base currency and quote currency.

        Parameters
        ----------
        instrument_id : InstrumentId
            The forex instrument ID for the calculation.
        date : date
            The date for the overnight rate.

        Returns
        -------
        Decimal

        Raises
        ------
        ValueError
            If `instrument_id.symbol` length is not in range [6, 7].

        Notes
        -----
        1% = 0.01 bp

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(date, "timestamp")
        Condition.in_range_int(len(instrument_id.symbol.value), 6, 7, "len(instrument_id)")

        cdef str symbol = instrument_id.symbol.value
        cdef str base_currency = symbol[:3]
        cdef str quote_currency = symbol[-3:]
        cdef str time_monthly = f"{date.year}-{str(date.month).zfill(2)}"
        cdef str time_quarter = f"{date.year}-Q{str(int(((date.month - 1) // 3) + 1)).zfill(2)}"

        base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_monthly]
        if base_data.empty:
            base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_quarter]

        quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_monthly]
        if quote_data.empty:
            quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_quarter]

        if base_data.empty and quote_data.empty:
            raise RuntimeError(f"cannot find rollover interest rate for {instrument_id} on {date}")  # pragma: no cover

        return Decimal(((<double>base_data['Value'] - <double>quote_data['Value']) / 365) / 100)
