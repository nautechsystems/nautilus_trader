#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="sizing.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from decimal import Decimal

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.objects cimport Quantity, Price, Money, Instrument


cdef class PositionSizer:
    """
    The base class for all position sizers.
    """

    def __init__(self, Instrument instrument):
        """
        Initializes a new instance of the PositionSizer class.

        :param instrument: The instrument for position sizing.
        """
        self.instrument = instrument

    cpdef void update_instrument(self, Instrument instrument):
        """
        Update the internal instrument with the given instrument.
        
        :param instrument: The instrument for update.
        :raises ValueError: If the instruments symbol does not equal the held instrument symbol.
        """
        Precondition.equal(self.instrument.symbol, instrument.symbol)

        self.instrument = instrument

    cpdef Quantity calculate(
            self,
            Money equity,
            exchange_rate,
            int risk_bp,
            Price entry,
            Price stop_loss,
            hard_limit=0,
            units=1,
            unit_batch_size=1):
        """
        Calculate the position size.

        :param equity: The account equity.
        :param exchange_rate: The exchange rate for the instrument quote currency vs account currency.
        :param risk_bp: The risk in basis points (0.01%).
        :param entry: The entry price level.
        :param stop_loss: The stop loss price level.
        :param hard_limit: The hard limit for the total quantity (>= 0) (0 = no hard limit).
        :param units: The number of units to batch the position into (> 0).
        :param unit_batch_size: The unit batch size (> 0).
        :raises ValueError: If the exchange_rate is not positive (> 0).
        :raises ValueError: If the risk_bp is not positive (> 0).
        :raises ValueError: If the units are not positive (> 0).
        :raises ValueError: If the hard limit is negative (< 0).
        :return: The calculated quantity for the position.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef Money _calculate_risk_money(self, Money equity, int risk_bp):
        """
        Calculate the amount of money based on the risk basis points.
        """
        return Money(equity.value * Decimal(round(risk_bp * 0.01, 2)))

    cdef object _calculate_risk_points(self, Price entry, Price stop_loss):
        """
        Calculate the difference in points between the entry and stop loss.
        """
        return abs(entry - stop_loss)


cdef class FixedRiskSizer(PositionSizer):
    """
    Provides position sizing calculations based on a given risk.
    """

    def __init__(self, Instrument instrument):
        """
        Initializes a new instance of the FixedRiskVolatilitySizer class.

        :param instrument: The instrument for position sizing.
        """
        super().__init__(instrument)

    cpdef Quantity calculate(
            self,
            Money equity,
            exchange_rate,
            int risk_bp,
            Price entry,
            Price stop_loss,
            hard_limit=0,
            units=1,
            unit_batch_size=1):
        """
        Calculate the position size.

        :param equity: The account equity.
        :param exchange_rate: The exchange rate for the instrument quote currency vs account currency.
        :param risk_bp: The risk in basis points (0.01%).
        :param entry: The entry price level.
        :param stop_loss: The stop loss price level.
        :param hard_limit: The hard limit for the total quantity (>= 0) (0 = no hard limit).
        :param units: The number of units to batch the position into (> 0).
        :param unit_batch_size: The unit batch size (> 0).
        :raises ValueError: If the exchange_rate is not positive (> 0).
        :raises ValueError: If the risk_bp is not positive (> 0).
        :raises ValueError: If the units are not positive (> 0).
        :raises ValueError: If the hard limit is negative (< 0).
        :return: The calculated quantity for the position.
        """
        Precondition.positive(exchange_rate, 'exchange_rate')
        Precondition.positive(risk_bp, 'risk_bp')
        Precondition.not_negative(hard_limit, 'hard_limit')
        Precondition.positive(units, 'units')
        Precondition.positive(unit_batch_size, 'unit_batch_size')

        cdef Money risk_money = self._calculate_risk_money(equity, risk_bp)
        cdef object risk_points = self._calculate_risk_points(entry, stop_loss)

        cdef object tick_value_size = self.instrument.tick_size * exchange_rate
        cdef int position_size = int(round(((risk_money / risk_points) / tick_value_size) / self.instrument.contract_size))

        # Limit size
        if hard_limit > 0:
            position_size = min(position_size, hard_limit)

        # Batch into units
        cdef int position_size_batched = int(round(position_size / units / unit_batch_size)) * unit_batch_size

        return Quantity(position_size_batched)
