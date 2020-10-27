# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class PositionSizer:
    """
    The base class for all position sizers.
    """

    def __init__(self, Instrument instrument not None):
        """
        Initialize a new instance of the PositionSizer class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for position sizing.

        """
        self._instrument = instrument

    @property
    def instrument(self):
        """
        The instrument for the position sizer.

        Returns
        -------
        Instrument

        """
        return self._instrument

    cpdef void update_instrument(self, Instrument instrument) except *:
        """
        Update the internal instrument with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the update.

        Raises
        ------
        ValueError
            If instrument symbol does not equal the currently held instrument symbol.

        """
        Condition.not_none(instrument, "instrument")
        Condition.equal(self.instrument.symbol, instrument.symbol, "instrument.symbol", "instrument.symbol")

        self._instrument = instrument

    cpdef Quantity calculate(
            self,
            Price entry,
            Price stop_loss,
            Money equity,
            Decimal risk,
            Decimal commission_rate=Decimal(),
            Decimal exchange_rate=Decimal(1),
            Decimal hard_limit=Decimal(),
            int units=1,
            int unit_batch_size=0,
    ):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cdef Decimal _calculate_risk_ticks(self, Price entry, Price stop_loss):
        return abs(entry - stop_loss) / self.instrument.tick_size

    cdef Decimal _calculate_riskable_money(
            self,
            Money equity,
            Decimal risk,
            Decimal commission_rate,
    ):
        if equity.amount <= 0:
            return Decimal()
        cdef Decimal risk_money = equity * risk
        cdef Decimal commission = risk_money * commission_rate * 2  # (round turn)

        return risk_money - commission


cdef class FixedRiskSizer(PositionSizer):
    """
    Provides position sizing calculations based on a given risk.
    """

    def __init__(self, Instrument instrument not None):
        """
        Initialize a new instance of the FixedRiskSizer class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for position sizing.

        """
        super().__init__(instrument)

    cpdef Quantity calculate(
            self,
            Price entry,
            Price stop_loss,
            Money equity,
            Decimal risk,
            Decimal commission_rate=Decimal(),
            Decimal exchange_rate=Decimal(1),
            Decimal hard_limit=Decimal(),
            int units=1,
            int unit_batch_size=0,
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
        exchange_rate : double
            The exchange rate for the instrument quote currency vs account currency.
        commission_rate : Decimal
            The commission rate (>= 0).
        hard_limit : double
            The hard limit for the total quantity (>= 0) (0 = no hard limit).
        units : int
            The number of units to batch the position into (> 0).
        unit_batch_size : int
            The unit batch size (> 0).

        Notes
        -----
        1 basis point = 0.01%.

        Raises
        ------
        ValueError
            If the risk_bp is not positive (> 0).
        ValueError
            If the xrate is not positive (> 0).
        ValueError
            If the commission_rate is negative (< 0).
        ValueError
            If the units is not positive (> 0).
        ValueError
            If the unit_batch_size is not positive (> 0).

        Returns
        -------
        Quantity

        """
        Condition.not_none(equity, "equity")
        Condition.not_none(entry, "price_entry")
        Condition.not_none(stop_loss, "price_stop_loss")
        Condition.positive(risk, "risk")
        Condition.not_negative(exchange_rate, "xrate")
        Condition.not_negative(commission_rate, "commission_rate")
        Condition.positive_int(units, "units")
        Condition.not_negative_int(unit_batch_size, "unit_batch_size")

        if exchange_rate == 0:
            return Quantity(precision=self._instrument.size_precision)

        cdef Decimal risk_points = self._calculate_risk_ticks(entry, stop_loss)
        cdef Decimal risk_money = self._calculate_riskable_money(equity, risk, commission_rate)

        if risk_points <= 0:
            # Divide by zero protection
            return Quantity(precision=self._instrument.size_precision)

        # Calculate position size
        cdef Decimal position_size = ((risk_money / exchange_rate) / risk_points) / self._instrument.tick_size

        # Limit size on hard limit
        if hard_limit > 0:
            position_size = min(position_size, Decimal(hard_limit, self._instrument.size_precision))

        # Batch into units
        cdef Decimal position_size_batched = max(Decimal(), position_size / units)

        if unit_batch_size > 0:
            # Round position size to nearest unit batch size
            position_size_batched = (position_size_batched // unit_batch_size) * unit_batch_size

        # Limit size on max trade size
        cdef Decimal final_size = min(position_size_batched, self._instrument.max_quantity)

        return Quantity(final_size, precision=self._instrument.size_precision)
