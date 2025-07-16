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
"""
Margin calculation models for different venue types and trading scenarios.

This module provides flexible margin calculation strategies that can be used with
different brokers and exchanges that have varying margin requirements.

"""

from abc import ABC
from abc import abstractmethod
from decimal import Decimal

from nautilus_trader.core.rust.model import PositionSide
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class MarginModel(ABC):
    """
    Abstract base class for margin calculation models.

    Different venues and instrument types may have varying approaches to calculating
    margin requirements. This abstraction allows for flexible margin calculation
    strategies.

    """

    @abstractmethod
    def calculate_margin_init(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: bool = False,
    ) -> Money:
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

    @abstractmethod
    def calculate_margin_maint(
        self,
        instrument: Instrument,
        side: PositionSide,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: bool = False,
    ) -> Money:
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


class StandardMarginModel(MarginModel):
    """
    Standard margin model that uses fixed percentages without leverage division.

    This model matches traditional broker behavior (e.g., Interactive Brokers)
    where margin requirements are fixed percentages of notional value regardless
    of account leverage. Leverage affects buying power but not margin requirements.

    Formula:
    - Initial Margin = notional_value * instrument.margin_init
    - Maintenance Margin = notional_value * instrument.margin_maint

    """

    def calculate_margin_init(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: bool = False,
    ) -> Money:
        """
        Calculate initial margin using fixed percentage of notional value.
        """
        notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            use_quote_for_inverse=use_quote_for_inverse,
        )

        margin_amount = notional.as_decimal() * instrument.margin_init

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(margin_amount, instrument.base_currency)
        else:
            return Money(margin_amount, instrument.quote_currency)

    def calculate_margin_maint(
        self,
        instrument: Instrument,
        side: PositionSide,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: bool = False,
    ) -> Money:
        """
        Calculate maintenance margin using fixed percentage of notional value.
        """
        notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            use_quote_for_inverse=use_quote_for_inverse,
        )

        margin_amount = notional.as_decimal() * instrument.margin_maint

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(margin_amount, instrument.base_currency)
        else:
            return Money(margin_amount, instrument.quote_currency)


class LeveragedMarginModel(MarginModel):
    """
    Leveraged margin model that divides margin requirements by leverage.

    This model represents the current Nautilus behavior and may be appropriate
    for certain crypto exchanges or specific trading scenarios where leverage
    directly reduces margin requirements.

    Formula:
    - Initial Margin = (notional_value / leverage) * instrument.margin_init
    - Maintenance Margin = (notional_value / leverage) * instrument.margin_maint

    """

    def calculate_margin_init(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: bool = False,
    ) -> Money:
        """
        Calculate initial margin with leverage division.
        """
        notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            use_quote_for_inverse=use_quote_for_inverse,
        )

        # Apply leverage division (current Nautilus behavior)
        adjusted_notional = notional.as_decimal() / leverage
        margin_amount = adjusted_notional * instrument.margin_init

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(margin_amount, instrument.base_currency)
        else:
            return Money(margin_amount, instrument.quote_currency)

    def calculate_margin_maint(
        self,
        instrument: Instrument,
        side: PositionSide,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: bool = False,
    ) -> Money:
        """
        Calculate maintenance margin with leverage division.
        """
        notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            use_quote_for_inverse=use_quote_for_inverse,
        )

        # Apply leverage division (current Nautilus behavior)
        adjusted_notional = notional.as_decimal() / leverage
        margin_amount = adjusted_notional * instrument.margin_maint

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(margin_amount, instrument.base_currency)
        else:
            return Money(margin_amount, instrument.quote_currency)
