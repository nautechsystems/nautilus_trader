# -------------------------------------------------------------------------------------------------
# <copyright file="loaders.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd

from datetime import timezone
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.enums import Currency  # Do not remove
from nautilus_trader.model.c_enums.currency cimport Currency, currency_from_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Decimal, Instrument, Quantity


cdef class CSVTickDataLoader:
    """
    Provides a means of loading tick data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(file_path: str) -> pd.DataFrame:
        """
        Return the tick pandas.DataFrame loaded from the given csv file.

        :param file_path: The absolute path to the CSV file.
        :return: pd.DataFrame.
        """
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
    def load(file_path: str) -> pd.DataFrame:
        """
        Return the bar pandas.DataFrame loaded from the given csv file.

        :param file_path: The absolute path to the CSV file.
        :return: pd.DataFrame.
        """
        return pd.read_csv(file_path,
                           index_col='Time (UTC)',
                           parse_dates=True)


cdef class InstrumentLoader:
    """
    Provides instrument template methods for backtesting.
    """

    cpdef Instrument default_fx_ccy(self, Symbol symbol, int tick_precision):
        """
        Return a default FX currency pair instrument from the given arguments.
        
        :param symbol: The currency pair symbol.
        :param tick_precision: The currency pair tick precision.
        :raises ConditionFailed: If the symbol.code length is not == 6.
        :raises ConditionFailed: If the tick_precision is not 3 or 5.
        """
        Condition.true(len(symbol.code) == 6, 'len(symbol) == 6')
        Condition.true(tick_precision == 3 or tick_precision == 5, 'tick_precision == 3 or 5')

        cdef Currency base_currency = currency_from_string(symbol.code[:3])
        cdef Currency quote_currency = currency_from_string(symbol.code[3:])
        # Check tick precision of quote currency
        if quote_currency == Currency.USD:
            Condition.true(tick_precision == 5, 'USD tick_precision == 5')
        elif quote_currency == Currency.JPY:
            Condition.true(tick_precision == 3, 'JPY tick_precision == 3')

        return Instrument(
            symbol=symbol,
            broker_symbol=symbol.code[:3] + '/' + symbol.code[3:],
            base_currency=base_currency,
            security_type=SecurityType.FOREX,
            tick_precision=tick_precision,
            tick_size=Decimal(1 / (10 ** tick_precision), tick_precision),
            round_lot_size=Quantity(1000),
            min_stop_distance_entry=0,
            min_limit_distance_entry=0,
            min_stop_distance=0,
            min_limit_distance=0,
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal.zero(),
            rollover_interest_sell=Decimal.zero(),
            timestamp=datetime.now(timezone.utc))
