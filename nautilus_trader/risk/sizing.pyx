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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class PositionSizer:
    """
    The base class for all position sizers.

    Parameters
    ----------
    instrument : Instrument
        The instrument for position sizing.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, Instrument instrument not None):
        self.instrument = instrument

    cpdef void update_instrument(self, Instrument instrument):
        """
        Update the internal instrument with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the update.

        Raises
        ------
        ValueError
            If `instrument` does not equal the currently held instrument.

        """
        Condition.not_none(instrument, "instrument")
        Condition.equal(self.instrument.id, instrument.id, "instrument.id", "instrument.id")

        self.instrument = instrument

    cpdef Quantity calculate(
        self,
        Price entry,
        Price stop_loss,
        Money equity,
        risk: Decimal,
        commission_rate: Decimal = Decimal(0),
        exchange_rate: Decimal = Decimal(1),
        hard_limit: Decimal | None = None,
        unit_batch_size: Decimal = Decimal(1),
        int units=1,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `calculate` must be implemented in the subclass")  # pragma: no cover

    cdef object _calculate_risk_ticks(self, Price entry, Price stop_loss):
        return abs(entry - stop_loss) / self.instrument.price_increment

    cdef object _calculate_riskable_money(
            self,
            equity: Decimal,
            risk: Decimal,
            commission_rate: Decimal,
    ):
        if equity <= 0:
            return Decimal(0)
        risk_money: Decimal = equity * risk
        commission: Decimal = risk_money * commission_rate * 2  # (round turn)

        return risk_money - commission


cdef class FixedRiskSizer(PositionSizer):
    """
    Provides position sizing calculations based on a given risk.

    Parameters
    ----------
    instrument : Instrument
        The instrument for position sizing.
    """

    def __init__(self, Instrument instrument not None):
        super().__init__(instrument)

    cpdef Quantity calculate(
        self,
        Price entry,
        Price stop_loss,
        Money equity,
        risk: Decimal,
        commission_rate: Decimal = Decimal(0),
        exchange_rate: Decimal = Decimal(1),
        hard_limit: Decimal | None = None,
        unit_batch_size: Decimal=Decimal(1),
        int units=1,
    ):
        """
        Calculate the position size quantity.

        Parameters
        ----------
        entry : Price
            The entry price.
        stop_loss : Price
            The stop loss price.
        equity : Money
            The account equity.
        risk : Decimal
            The risk percentage.
        exchange_rate : Decimal
            The exchange rate for the instrument quote currency vs account currency.
        commission_rate : Decimal
            The commission rate (>= 0).
        hard_limit : Decimal, optional
            The hard limit for the total quantity (>= 0).
        unit_batch_size : Decimal
            The unit batch size (> 0).
        units : int
            The number of units to batch the position into (> 0).

        Raises
        ------
        ValueError
            If `risk_bp` is not positive (> 0).
        ValueError
            If `xrate` is not positive (> 0).
        ValueError
            If `commission_rate` is negative (< 0).
        ValueError
            If `hard_limit` is not ``None`` and is not positive (> 0).
        ValueError
            If `unit_batch_size` is not positive (> 0).
        ValueError
            If `units` is not positive (> 0).

        Returns
        -------
        Quantity

        """
        Condition.not_none(equity, "equity")
        Condition.not_none(entry, "price_entry")
        Condition.not_none(stop_loss, "price_stop_loss")
        Condition.type(risk, Decimal, "risk")
        Condition.positive(risk, "risk")
        Condition.type(exchange_rate, Decimal, "exchange_rate")
        Condition.not_negative(exchange_rate, "xrate")
        Condition.type(commission_rate, Decimal, "commission_rate")
        Condition.not_negative(commission_rate, "commission_rate")
        if hard_limit is not None:
            Condition.positive(hard_limit, "hard_limit")
        Condition.type(unit_batch_size, Decimal, "unit_batch_size")
        Condition.not_negative(unit_batch_size, "unit_batch_size")
        Condition.positive_int(units, "units")

        if exchange_rate == 0:
            return self.instrument.make_qty(0)

        risk_points: Decimal = self._calculate_risk_ticks(entry, stop_loss)
        risk_money: Decimal = self._calculate_riskable_money(equity.as_decimal(), risk, commission_rate)

        if risk_points <= 0:
            # Divide by zero protection
            return self.instrument.make_qty(0)

        # Calculate position size
        position_size: Decimal = ((risk_money / exchange_rate) / risk_points) / self.instrument.price_increment

        # Limit size on hard limit
        if hard_limit is not None:
            position_size = min(position_size, hard_limit)

        # Batch into units
        position_size_batched: Decimal = max(Decimal(0), position_size / units)

        if unit_batch_size > 0:
            # Round position size to nearest unit batch size
            position_size_batched = (position_size_batched // unit_batch_size) * unit_batch_size

        # Limit size on max trade size
        final_size: Decimal = min(position_size_batched, self.instrument.max_quantity)

        return Quantity(final_size, precision=self.instrument.size_precision)
