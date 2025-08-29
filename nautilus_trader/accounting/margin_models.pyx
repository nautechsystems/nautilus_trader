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
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class MarginModel:
    """
    Abstract base class for margin calculation models.

    Different venues and instrument types may have varying approaches to
    calculating margin requirements. This abstraction allows for flexible
    margin calculation strategies.
    """

    cpdef Money calculate_margin_init(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        leverage: Decimal,
        bint use_quote_for_inverse=False,
    ):
        """
        Calculate the initial (order) margin requirement.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.
        leverage : Decimal
            The account leverage for this instrument.
        use_quote_for_inverse : bool, default False
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money
            The initial margin requirement.
        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price price,
        leverage: Decimal,
        bint use_quote_for_inverse=False,
    ):
        """
        Calculate the maintenance (position) margin requirement.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        side : PositionSide
            The position side.
        quantity : Quantity
            The position quantity.
        price : Price
            The current price.
        leverage : Decimal
            The account leverage for this instrument.
        use_quote_for_inverse : bool, default False
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money
            The maintenance margin requirement.
        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover


cdef class StandardMarginModel(MarginModel):
    """
    Standard margin model that uses fixed percentages without leverage division.

    This model matches traditional broker behavior (e.g., Interactive Brokers)
    where margin requirements are fixed percentages of notional value regardless
    of account leverage. Leverage affects buying power but not margin requirements.

    Formula:
    - Initial Margin = notional_value * instrument.margin_init
    - Maintenance Margin = notional_value * instrument.margin_maint
    """

    cpdef Money calculate_margin_init(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        leverage: Decimal,
        bint use_quote_for_inverse=False,
    ):
        """
        Calculate initial margin using fixed percentage of notional value.
        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")

        notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            use_quote_for_inverse=use_quote_for_inverse,
        ).as_decimal()

        margin = notional * instrument.margin_init

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price price,
        leverage: Decimal,
        bint use_quote_for_inverse=False,
    ):
        """
        Calculate maintenance margin using fixed percentage of notional value.
        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")

        notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            use_quote_for_inverse=use_quote_for_inverse,
        ).as_decimal()

        margin = notional * instrument.margin_maint

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)


cdef class LeveragedMarginModel(MarginModel):
    """
    Leveraged margin model that divides margin requirements by leverage.

    This model represents the current Nautilus behavior and may be appropriate
    for certain crypto exchanges or specific trading scenarios where leverage
    directly reduces margin requirements.

    Formula:
    - Initial Margin = (notional_value / leverage) * instrument.margin_init
    - Maintenance Margin = (notional_value / leverage) * instrument.margin_maint
    """

    cpdef Money calculate_margin_init(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        leverage: Decimal,
        bint use_quote_for_inverse=False,
    ):
        """
        Calculate initial margin with leverage division.
        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")
        Condition.positive(leverage, "leverage")

        notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            use_quote_for_inverse=use_quote_for_inverse,
        ).as_decimal()

        # Apply leverage division (current Nautilus behavior)
        adjusted_notional = notional / leverage
        margin = adjusted_notional * instrument.margin_init

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price price,
        leverage: Decimal,
        bint use_quote_for_inverse=False,
    ):
        """
        Calculate maintenance margin with leverage division.
        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")
        Condition.positive(leverage, "leverage")

        notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            use_quote_for_inverse=use_quote_for_inverse,
        ).as_decimal()

        # Apply leverage division (current Nautilus behavior)
        adjusted_notional = notional / leverage
        margin = adjusted_notional * instrument.margin_maint

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)
