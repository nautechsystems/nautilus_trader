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

import random

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_MILLISECOND
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.functions cimport liquidity_side_to_str
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class FeeModel:
    """
    Provides an abstract fee model for trades.
    """

    cpdef Money get_commission(
        self,
        Order order,
        Quantity fill_qty,
        Price fill_px,
        Instrument instrument,
    ):
        """
        Return the commission for a trade.

        Parameters
        ----------
        order : Order
            The order to calculate the commission for.
        fill_qty : Quantity
            The fill quantity of the order.
        fill_px : Price
            The fill price of the order.
        instrument : Instrument
            The instrument for the order.

        Returns
        -------
        Money

        """
        raise NotImplementedError("Method 'get_commission' must be implemented in a subclass.")


cdef class MakerTakerFeeModel(FeeModel):
    """
    Provide a fee model for trades based on a maker/taker fee schedule
    and notional value of the trade.

    Parameters
    ----------
    config : MakerTakerFeeModelConfig, optional
        The configuration for the fee model.
    """

    def __init__(self, config = None) -> None:
        # No configuration needed for this model
        pass

    cpdef Money get_commission(
        self,
        Order order,
        Quantity fill_qty,
        Price fill_px,
        Instrument instrument,
    ):
        cdef double notional = instrument.notional_value(
            quantity=fill_qty,
            price=fill_px,
            use_quote_for_inverse=False,
        ).as_f64_c()

        cdef double commission_f64
        if order.liquidity_side == LiquiditySide.MAKER:
            commission_f64 = notional * float(instrument.maker_fee)
        elif order.liquidity_side == LiquiditySide.TAKER:
            commission_f64 = notional * float(instrument.taker_fee)
        else:
            raise ValueError(
                f"invalid `LiquiditySide`, was {liquidity_side_to_str(order.liquidity_side)}"
            )

        cdef Money commission
        if instrument.is_inverse:  # Not using quote for inverse (see above):
            commission = Money(commission_f64, instrument.base_currency)
        else:
            commission = Money(commission_f64, instrument.quote_currency)

        return commission


cdef class FixedFeeModel(FeeModel):
    """
    Provides a fixed fee model for trades.

    Parameters
    ----------
    commission : Money, optional
        The fixed commission amount for trades.
    charge_commission_once : bool, default True
        Whether to charge the commission once per order or per fill.
    config : FixedFeeModelConfig, optional
        The configuration for the model.

    Raises
    ------
    ValueError
        If both ``commission`` **and** ``config`` are provided, **or** if both are ``None`` (exactly one must be supplied).
    ValueError
        If `commission` is not a positive amount.
    """

    def __init__(
        self,
        Money commission = None,
        bint charge_commission_once: bool = True,
        config = None,
    ) -> None:
        Condition.is_true((commission is None) ^ (config is None), "Provide exactly one of `commission` or `config`")

        if config is not None:
            # Initialize from config
            commission = Money.from_str(config.commission)
            charge_commission_once = config.charge_commission_once

        Condition.positive(commission, "commission")

        self._commission = commission
        self._zero_commission = Money(0, commission.currency)
        self._charge_commission_once = charge_commission_once

    cpdef Money get_commission(
        self,
        Order order,
        Quantity fill_qty,
        Price fill_px,
        Instrument instrument,
    ):
        if not self._charge_commission_once or order.filled_qty == 0:
            return self._commission
        else:
            return self._zero_commission


cdef class PerContractFeeModel(FeeModel):
    """
    Provides a fee model which charges a commission per contract traded.

    Parameters
    ----------
    commission : Money, optional
        The commission amount per contract.
    config : PerContractFeeModelConfig, optional
        The configuration for the model.

    Raises
    ------
    ValueError
        If both ``commission`` **and** ``config`` are provided, **or** if both are ``None`` (exactly one must be supplied).
    ValueError
        If `commission` is negative (< 0).
    """

    def __init__(
        self,
        Money commission = None,
        config = None,
    ) -> None:
        Condition.is_true((commission is None) ^ (config is None), "Provide exactly one of `commission` or `config`")

        if config is not None:
            # Initialize from config
            commission = Money.from_str(config.commission)

        Condition.not_negative(commission, "commission")

        self._commission = commission

    cpdef Money get_commission(
        self,
        Order order,
        Quantity fill_qty,
        Price fill_px,
        Instrument instrument,
    ):
        return Money(self._commission * fill_qty, self._commission.currency)
