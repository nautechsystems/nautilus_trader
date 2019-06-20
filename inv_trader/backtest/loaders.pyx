#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="loaders.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from datetime import timezone
from decimal import Decimal

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.venue cimport Venue
from inv_trader.enums.security_type cimport SecurityType
from inv_trader.model.enums import Currency
from inv_trader.model.objects cimport Symbol, Instrument, Quantity
from inv_trader.model.identifiers cimport InstrumentId


cdef class InstrumentLoader:
    """
    Provides instrument template methods for backtesting.
    """

    cpdef Instrument default_fx_ccy(
            self,
            str symbol_code,
            Venue venue,
            int tick_precision):
        """
        Return a default FX currency pair instrument from the given arguments.
        
        :param symbol_code: The symbol code (must be length 6).
        :param venue: The symbol venue.
        :param tick_precision: The tick precision for the currency pair.
        :raises ValueError: If the symbol_code length is not == 6.
        :raises ValueError: If the tick_precision is not 3 or 5.
        """
        Precondition.true(len(symbol_code) == 6, 'len(symbol) == 6')
        Precondition.true(tick_precision == 3 or tick_precision == 5, 'tick_precision == 3 or 5')

        cdef Symbol symbol = Symbol(symbol_code, venue)
        return Instrument(
            instrument_id=InstrumentId(str(symbol)),
            symbol=symbol,
            broker_symbol=symbol.code[:3] + '/' + symbol.code[3:],
            quote_currency=Currency[symbol.code[3:]],
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
