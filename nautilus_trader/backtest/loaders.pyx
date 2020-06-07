# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import pandas as pd

from datetime import timezone
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.enums import Currency  # Do not remove
from nautilus_trader.model.c_enums.currency cimport Currency, currency_from_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Decimal, Quantity, Price, Instrument


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
        Condition.not_none(file_path, 'file_path')

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
        Condition.not_none(file_path, 'file_path')

        return pd.read_csv(file_path,
                           index_col='Time (UTC)',
                           parse_dates=True)


cdef class InstrumentLoader:
    """
    Provides instrument template methods for backtesting.
    """

    cpdef Instrument default_fx_ccy(self, Symbol symbol):
        """
        Return a default FX currency pair instrument from the given arguments.
        
        :param symbol: The currency pair symbol.
        :raises ValueError: If the symbol.code length is not == 6.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.equal(len(symbol.code), 6, 'len(symbol)', '6')

        cdef Currency base_currency = currency_from_string(symbol.code[:3])
        cdef Currency quote_currency = currency_from_string(symbol.code[3:])

        # Check tick precision of quote currency
        if quote_currency == Currency.JPY:
            price_precision = 3
        else:
            price_precision = 5

        return Instrument(
            symbol=symbol,
            broker_symbol=symbol.code[:3] + '/' + symbol.code[3:],
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
            rollover_interest_buy=Decimal.zero(),
            rollover_interest_sell=Decimal.zero(),
            timestamp=datetime.now(timezone.utc))
