# -------------------------------------------------------------------------------------------------
# <copyright file="sizing.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport basis_points_as_percentage
from nautilus_trader.model.objects cimport Quantity, Price, Money, Instrument


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
        Condition.equal(self.instrument.symbol, instrument.symbol)

        self.instrument = instrument

    cpdef Quantity calculate(
            self,
            Money equity,
            float risk_bp,
            Price price_entry,
            Price price_stop_loss,
            float exchange_rate=1.0,
            float commission_rate_bp=0.20,
            int hard_limit=0,
            int units=1,
            int unit_batch_size=1):
        """
        Return the calculated quantity for the position size.
        
        Note: 1 basis point = 0.01%.
        :param equity: The account equity.
        :param risk_bp: The risk in basis points.
        :param price_entry: The entry price.
        :param price_stop_loss: The stop loss price.
        :param exchange_rate: The exchange rate for the instrument quote currency vs account currency.
        :param commission_rate_bp: The commission rate as basis points of notional transaction value (>= 0).
        :param hard_limit: The hard limit for the total quantity (>= 0) (0 = no hard limit).
        :param units: The number of units to batch the position into (> 0).
        :param unit_batch_size: The unit batch size (> 0).
        :raises ValueError: If the risk_bp is not positive (> 0).
        :raises ValueError: If the exchange_rate is not positive (> 0).
        :raises ValueError: If the commission_rate is negative (< 0).
        :raises ValueError: If the units is not positive (> 0).
        :raises ValueError: If the unit_batch_size is not positive (> 0).
        :return: Quantity.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef int _calculate_risk_ticks(self, Price entry, Price stop_loss):
        """
        Return the calculated difference in ticks between the entry and stop loss.
        
        :return int.
        """
        return int(abs(entry - stop_loss) / self.instrument.tick_size)

    cdef Money _calculate_riskable_money(
            self,
            Money equity,
            float risk_bp,
            float commission_rate_bp,
            float exchange_rate):
        """
        Return the calculated amount of risk money available.
        
        :return Money.
        """
        if equity.value <= 0:
            return Money.zero()
        cdef Money risk_money = Money(equity.value * Decimal(basis_points_as_percentage(risk_bp)))
        cdef Money commission = Money(risk_money.value * Decimal(basis_points_as_percentage(commission_rate_bp)))

        return risk_money - commission


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
            float risk_bp,
            Price price_entry,
            Price price_stop_loss,
            float exchange_rate=1.0,
            float commission_rate_bp=0.20,
            int hard_limit=0,
            int units=1,
            int unit_batch_size=1):
        """
        Return the calculated quantity for the position size.

        Note: 1 basis point = 0.01%.
        :param equity: The account equity.
        :param risk_bp: The risk in basis points.
        :param price_entry: The entry price.
        :param price_stop_loss: The stop loss price.
        :param exchange_rate: The exchange rate for the instrument quote currency vs account currency.
        :param commission_rate_bp: The commission rate as basis points of notional transaction value (>= 0).
        :param hard_limit: The hard limit for the total quantity (>= 0) (0 = no hard limit).
        :param units: The number of units to batch the position into (> 0).
        :param unit_batch_size: The unit batch size (> 0).
        :raises ValueError: If the risk_bp is not positive (> 0).
        :raises ValueError: If the exchange_rate is not positive (> 0).
        :raises ValueError: If the commission_rate is negative (< 0).
        :raises ValueError: If the units is not positive (> 0).
        :raises ValueError: If the unit_batch_size is not positive (> 0).
        :return: Quantity.
        """
        Condition.positive(risk_bp, 'risk_bp')
        Condition.positive(exchange_rate, 'exchange_rate')
        Condition.not_negative(commission_rate_bp, 'commission_rate_bp')
        Condition.positive(units, 'units')
        Condition.positive(unit_batch_size, 'unit_batch_size')

        cdef int risk_points = self._calculate_risk_ticks(price_entry, price_stop_loss)
        cdef Money risk_money = self._calculate_riskable_money(equity, risk_bp, commission_rate_bp, exchange_rate)

        cdef long position_size = long(long((((risk_money.value / Decimal(exchange_rate)) / risk_points) / self.instrument.tick_size)))

        # Limit size
        if hard_limit > 0:
            position_size = min(position_size, hard_limit)

        # Batch into units
        cdef long position_size_batched = long(long(position_size / units / unit_batch_size) * unit_batch_size)

        return Quantity(min(position_size_batched, self.instrument.max_trade_size.value))
