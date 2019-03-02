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
            int risk_bp,
            Price entry_price,
            Price stop_loss_price,
            exchange_rate=Decimal(1),
            commission_rate=Decimal(15),
            int leverage=1,
            int hard_limit=0,
            int units=1,
            int unit_batch_size=1):
        """
        Return the calculated quantity for the position size.

        :param equity: The account equity.
        :param risk_bp: The risk in basis points (1 basis point = 0.01%).
        :param entry_price: The entry price.
        :param stop_loss_price: The stop loss price.
        :param exchange_rate: The exchange rate for the instrument quote currency vs account currency.
        :param commission_rate: The commission rate per million notional (>= 0).
        :param leverage: The broker leverage for the instrument (> 0).
        :param hard_limit: The hard limit for the total quantity (>= 0) (0 = no hard limit).
        :param units: The number of units to batch the position into (> 0).
        :param unit_batch_size: The unit batch size (> 0).
        :raises ValueError: If the risk_bp is not positive (> 0).
        :raises ValueError: If the exchange_rate is not positive (> 0).
        :raises ValueError: If the commission_rate is negative (< 0).
        :raises ValueError: If the leverage is not positive (> 0).
        :raises ValueError: If the units is not positive (> 0).
        :raises ValueError: If the unit_batch_size is not positive (> 0).
        :return: Quantity.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef Money _calculate_risk_money(
            self,
            Money equity,
            int risk_bp,
            int leverage,
            commission_rate):
        """
        Calculate the amount of risk money available.
        """
        cdef Money risk_money = Money(equity.value * Decimal(round(risk_bp * 0.01, 2)))
        cdef Money commission = Money(((risk_money.value * leverage) / 1000000) * commission_rate)

        return risk_money - commission

    cdef int _calculate_risk_points(self, Price entry, Price stop_loss):
        """
        Calculate the difference in points between the entry and stop loss.
        """
        return int(abs(entry - stop_loss) / self.instrument.tick_size)


cdef class FixedRiskSizer(PositionSizer):
    """
    Provides position sizing calculations based on a given risk.
    """

    def __init__(self, Instrument instrument):
        """
        Initializes a new instance of the FixedRiskSizer class.

        :param instrument: The instrument for position sizing.
        """
        super().__init__(instrument)

    cpdef Quantity calculate(
            self,
            Money equity,
            int risk_bp,
            Price entry_price,
            Price stop_loss_price,
            exchange_rate=Decimal(1),
            commission_rate=Decimal(15),
            int leverage=1,
            int hard_limit=0,
            int units=1,
            int unit_batch_size=1):
        """
        Return the calculated quantity for the position size.

        :param equity: The account equity.
        :param risk_bp: The risk in basis points (1 basis point = 0.01%).
        :param entry_price: The entry price.
        :param stop_loss_price: The stop loss price.
        :param exchange_rate: The exchange rate for the instrument quote currency vs account currency.
        :param commission_rate: The commission rate per million notional (>= 0).
        :param leverage: The broker leverage for the instrument (> 0).
        :param hard_limit: The hard limit for the total quantity (>= 0) (0 = no hard limit).
        :param units: The number of units to batch the position into (> 0).
        :param unit_batch_size: The unit batch size (> 0).
        :raises ValueError: If the risk_bp is not positive (> 0).
        :raises ValueError: If the exchange_rate is not positive (> 0).
        :raises ValueError: If the commission_rate is negative (< 0).
        :raises ValueError: If the leverage is not positive (> 0).
        :raises ValueError: If the units is not positive (> 0).
        :raises ValueError: If the unit_batch_size is not positive (> 0).
        :return: Quantity.
        """
        Precondition.positive(risk_bp, 'risk_bp')
        Precondition.positive(exchange_rate, 'exchange_rate')
        Precondition.not_negative(commission_rate, 'commission_rate')
        Precondition.positive(leverage, 'leverage')
        Precondition.positive(units, 'units')
        Precondition.positive(unit_batch_size, 'unit_batch_size')

        cdef Money risk_money = self._calculate_risk_money(equity, risk_bp, leverage, commission_rate)
        cdef int risk_points = self._calculate_risk_points(entry_price, stop_loss_price)
        cdef long position_size = long(round(((risk_money.value / risk_points) / (self.instrument.tick_size * exchange_rate)) / self.instrument.contract_size.value))

        # Limit size
        if hard_limit > 0:
            position_size = min(position_size, hard_limit)

        # Batch into units
        cdef long position_size_batched = long(round(position_size / units / unit_batch_size) * unit_batch_size)

        return Quantity(min(position_size_batched, self.instrument.max_trade_size.value))
