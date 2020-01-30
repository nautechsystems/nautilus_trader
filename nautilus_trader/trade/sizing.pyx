# -------------------------------------------------------------------------------------------------
# <copyright file="sizing.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.common.functions cimport basis_points_as_percentage
from nautilus_trader.model.objects cimport Quantity, Price, Money, Instrument


cdef class PositionSizer:
    """
    The base class for all position sizers.
    """

    def __init__(self, Instrument instrument not None):
        """
        Initializes a new instance of the PositionSizer class.

        :param instrument: The instrument for position sizing.
        """
        self.instrument = instrument

    cpdef void update_instrument(self, Instrument instrument) except *:
        """
        Update the internal instrument with the given instrument.
        
        :param instrument: The instrument for update.
        :raises ValueError: If the instruments symbol does not equal the held instrument symbol.
        """
        Condition.not_none(instrument, 'instrument')
        Condition.equal(self.instrument.symbol, instrument.symbol, 'instrument.symbol', 'instrument.symbol')

        self.instrument = instrument

    cpdef Quantity calculate(
            self,
            Money equity,
            double risk_bp,
            Price entry,
            Price stop_loss,
            double exchange_rate=1.0,
            double commission_rate_bp=0.0,
            double hard_limit=0.0,
            int units=1,
            int unit_batch_size=0):
        """
        Return the calculated quantity for the position size.
        Note: 1 basis point = 0.01%.
        
        :param equity: The account equity.
        :param risk_bp: The risk in basis points.
        :param entry: The entry price.
        :param stop_loss: The stop loss price.
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
        
        :return Quantity.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cdef double _calculate_risk_ticks(self, double entry, double stop_loss):
        """
        Return the calculated difference in ticks between the entry and stop loss.
        
        :return int.
        """
        return abs(entry - stop_loss) / self.instrument.tick_size.as_double()

    cdef double _calculate_riskable_money(
            self,
            double equity,
            double risk_bp,
            double commission_rate_bp):
        """
        Return the calculated amount of risk money available.
        
        :return Money.
        """
        if equity <= 0.0:
            return 0.0
        cdef double risk_money = equity * basis_points_as_percentage(risk_bp)
        cdef double commission = risk_money * basis_points_as_percentage(commission_rate_bp)

        return risk_money - commission


cdef class FixedRiskSizer(PositionSizer):
    """
    Provides position sizing calculations based on a given risk.
    """

    def __init__(self, Instrument instrument not None):
        """
        Initializes a new instance of the FixedRiskSizer class.

        :param instrument: The instrument for position sizing.
        """
        super().__init__(instrument)

    cpdef Quantity calculate(
            self,
            Money equity,
            double risk_bp,
            Price entry,
            Price stop_loss,
            double exchange_rate=1.0,
            double commission_rate_bp=0.0,
            double hard_limit=0.0,
            int units=1,
            int unit_batch_size=0):
        """
        Return the calculated quantity for the position size.
        Note: 1 basis point = 0.01%.

        :param equity: The account equity.
        :param risk_bp: The risk in basis points.
        :param entry: The entry price.
        :param stop_loss: The stop loss price.
        :param exchange_rate: The exchange rate for the instrument quote currency vs account currency.
        :param commission_rate_bp: The commission rate as basis points of notional transaction value (>= 0).
        :param hard_limit: The hard limit for the total quantity (>= 0) (0 = no hard limit).
        :param units: The number of units to batch the position into (> 0).
        :param unit_batch_size: The unit batch size (>= 0) If 0 then no batching applied.
        :raises ValueError: If the risk_bp is not positive (> 0).
        :raises ValueError: If the exchange_rate is not positive (> 0).
        :raises ValueError: If the commission_rate is negative (< 0).
        :raises ValueError: If the units is not positive (> 0).
        :raises ValueError: If the unit_batch_size is not positive (> 0).

        :return Quantity.
        """
        Condition.not_none(equity, 'equity')
        Condition.not_none(entry, 'price_entry')
        Condition.not_none(stop_loss, 'price_stop_loss')
        Condition.positive(risk_bp, 'risk_bp')
        Condition.positive(exchange_rate, 'exchange_rate')
        Condition.not_negative(commission_rate_bp, 'commission_rate_bp')
        Condition.positive_int(units, 'units')
        Condition.not_negative_int(unit_batch_size, 'unit_batch_size')

        cdef double risk_points = self._calculate_risk_ticks(
            entry.as_double(),
            stop_loss.as_double())

        cdef double risk_money = self._calculate_riskable_money(
            equity.as_double(),
            risk_bp,
            commission_rate_bp)

        if risk_points <= 0.0:
            # Divide by zero protection
            return Quantity(precision=self.instrument.size_precision)

        # Calculate position size
        cdef double tick_size = self.instrument.tick_size.as_double()
        cdef double position_size = ((risk_money / exchange_rate) / risk_points) / tick_size

        # Limit size on hard limit
        if hard_limit > 0.0:
            position_size = min(position_size, hard_limit)

        # Batch into units
        cdef double position_size_batched = max(0.0, position_size / units)

        if unit_batch_size > 0:
            # Round position size to nearest unit batch size
            position_size_batched = (position_size_batched // unit_batch_size) * unit_batch_size

        # Limit size on max trade size
        cdef double final_size = min(position_size_batched, self.instrument.max_trade_size)

        return Quantity(final_size, precision=self.instrument.size_precision)
