# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


cdef class CashAccount:
    """
    Provides a cash account.
    """

    def __init__(self, AccountState event):
        """
        Initialize a new instance of the ``CashAccount`` class.

        Parameters
        ----------
        event : AccountState
            The initial account state event.

        Raises
        ------
        ValueError
            If account_type is not equal to AccountType.CASH.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.account_type, AccountType.CASH, "event.account_type", "account_type")

        super().__init__(event)

# -- CALCULATIONS ----------------------------------------------------------------------------------

    cpdef Money calculate_margin_initial(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the initial (order) margin from the given parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")

        notional: Decimal = instrument.notional_value(
            quantity=quantity,
            price=price.as_decimal(),
            inverse_as_quote=inverse_as_quote,
        ).as_decimal()

        margin: Decimal = notional
        margin += (notional * instrument.taker_fee * 2)

        if instrument.is_inverse and not inverse_as_quote:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)

    cpdef list calculate_pnls(
        self,
        Instrument instrument,
        Position position,  # Can be None
        OrderFilled fill,
    ):
        """
        Return the calculated immediate PnL.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        position : Position, optional
            The position for the calculation (can be None).
        fill : OrderFilled
            The fill for the calculation.

        Returns
        -------
        list[Money] or None

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(fill, "fill")

        cdef list pnls = []

        cdef Currency quote_currency = instrument.quote_currency
        cdef Currency base_currency = instrument.get_base_currency()

        fill_qty: Decimal = fill.last_qty.as_decimal()
        fill_px: Decimal = fill.last_px.as_decimal()

        if fill.side == OrderSide.BUY:
            if base_currency:
                pnls.append(Money(fill_qty, base_currency))
            pnls.append(Money(-(fill_px * fill_qty), quote_currency))
        else:  # OrderSide.SELL
            if base_currency:
                pnls.append(Money(-fill_qty, base_currency))
            pnls.append(Money(fill_px * fill_qty, quote_currency))

        return pnls
