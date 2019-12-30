# -------------------------------------------------------------------------------------------------
# <copyright file="loaders.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from datetime import timezone
from decimal import Decimal
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.enums import Currency  # Do not remove
from nautilus_trader.model.c_enums.currency cimport Currency, currency_from_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.identifiers cimport Symbol, InstrumentId
from nautilus_trader.model.objects cimport Instrument, Quantity


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
            tick_size=Decimal('0.' + ('0' * (tick_precision - 1)) + '1'),
            round_lot_size=Quantity(1000),
            min_stop_distance_entry=0,
            min_limit_distance_entry=0,
            min_stop_distance=0,
            min_limit_distance=0,
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal(),
            rollover_interest_sell=Decimal(),
            timestamp=datetime.now(timezone.utc))
