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
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.position cimport Position


cdef class MarginAccount(Account):
    """
    Provides a margin account.
    """

    def __init__(self, AccountState event):
        """
        Initialize a new instance of the ``MarginAccount`` class.

        Parameters
        ----------
        event : AccountState
            The initial account state event.

        Raises
        ------
        ValueError
            If account_type is not equal to AccountType.MARGIN.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.account_type, AccountType.MARGIN, "event.account_type", "account_type")

        super().__init__(event)

        cdef dict margins_initial = event.info.get("margins_initial", {})
        cdef dict margins_maint = event.info.get("margins_maint", {})

        self._leverages = {}                     # type: dict[InstrumentId, Decimal]
        self._margins_maint = margins_maint      # type: dict[Currency, Money]
        self._margins_initial = margins_initial  # type: dict[Currency, Money]

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef dict leverages(self):
        """
        Return the account leverages.

        Returns
        -------
        dict[InstrumentId, Decimal]

        """
        return self._leverages.copy()

    cpdef dict margins_initial(self):
        """
        Return the initial (order) margins for the account.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._margins_initial.copy()

    cpdef dict margins_maint(self):
        """
        Return the maintenance (position) margins for the account.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._margins_maint.copy()

    cpdef object leverage(self, InstrumentId instrument_id):
        """
        Return the leverage for the given instrument (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the leverage.

        Returns
        -------
        Decimal or None

        """
        return self._leverages.get(instrument_id)

    cpdef Money margin_initial(self, Currency currency=None):
        """
        Return the current initial (order) margin.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and base_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.base_currency
        Condition.not_none(currency, "currency")

        return self._margins_initial.get(currency)

    cpdef Money margin_maint(self, Currency currency=None):
        """
        Return the current maintenance (position) margin.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and base_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.base_currency
        Condition.not_none(currency, "currency")

        return self._margins_maint.get(currency)

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void set_leverage(self, InstrumentId instrument_id, leverage: Decimal) except *:
        """
        Set the leverage for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the leverage.
        leverage : Decimal
            The leverage value

        Returns
        -------
        TypeError
            If leverage is not of type Decimal.
        ValueError
            If leverage is not >= 1.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.type(leverage, Decimal, "leverage")
        Condition.true(leverage >= 1, "leverage was not >= 1")

        self._leverages[instrument_id] = leverage

    cpdef void update_margin_initial(self, Money margin_initial) except *:
        """
        Update the initial (order) margin.

        Parameters
        ----------
        margin_initial : Money
            The current initial (order) margin for the currency.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(margin_initial, "margin_initial")

        cdef Currency currency = margin_initial.currency
        cdef AccountBalance current_balance = self._balances.get(currency)
        if current_balance is None:
            raise RuntimeError("Cannot update initial margin when no current balance")

        cdef Money margin_maint = self._margins_maint.get(currency, Money(0, currency))
        cdef Money margin_total = Money(margin_initial.as_decimal() + margin_maint.as_decimal(), currency)
        cdef AccountBalance new_balance = AccountBalance(
            currency,
            current_balance.total,
            margin_total,
            Money(current_balance.total.as_decimal() - margin_total, currency),
        )

        self._margins_initial[currency] = margin_initial
        self._balances[currency] = new_balance

    cpdef void update_margin_maint(self, Money margin_maint) except *:
        """
        Update the maintenance (position) margin.

        Parameters
        ----------
        margin_maint : Money
            The current maintenance (position) margin for the currency.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(margin_maint, "margin_maint")

        cdef Currency currency = margin_maint.currency
        cdef AccountBalance current_balance = self._balances.get(currency)
        if current_balance is None:
            raise RuntimeError("Cannot update maintenance margin when no current balance")

        cdef Money margin_initial = self._margins_initial.get(currency, Money(0, currency))
        cdef Money margin_total = Money(margin_initial.as_decimal() + margin_maint.as_decimal(), currency)
        cdef AccountBalance new_balance = AccountBalance(
            currency,
            current_balance.total,
            margin_total,
            Money(current_balance.total.as_decimal() - margin_total, currency),
        )

        self._margins_maint[currency] = margin_maint
        self._balances[currency] = new_balance

# -- CALCULATIONS ----------------------------------------------------------------------------------

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

        if position and position.entry != fill.side:
            # Calculate positional PnL
            return [position.calculate_pnl(
                avg_px_open=position.avg_px_open,
                avg_px_close=fill.last_px,
                quantity=fill.last_qty,
            )]
        else:
            return [Money(0, instrument.get_cost_currency())]

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

        leverage: Decimal = self._leverages.get(instrument.id)
        if leverage is None:
            leverage = Decimal(1)
            self._leverages[instrument.id] = leverage

        adjusted_notional: Decimal = notional / leverage

        margin: Decimal = adjusted_notional * instrument.margin_initial
        margin += (adjusted_notional * instrument.taker_fee * 2)

        if instrument.is_inverse and not inverse_as_quote:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price last,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the maintenance (position) margin from the given parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        side : PositionSide
            The currency position side.
        quantity : Quantity
            The currency position quantity.
        last : Price
            The position instruments last price.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")
        Condition.not_none(last, "last")

        notional: Decimal = instrument.notional_value(
            quantity=quantity,
            price=last.as_decimal(),
            inverse_as_quote=inverse_as_quote
        ).as_decimal()

        leverage: Decimal = self._leverages.get(instrument.id)
        if leverage is None:
            leverage = Decimal(1)
            self._leverages[instrument.id] = leverage

        adjusted_notional: Decimal = notional / leverage

        margin: Decimal = adjusted_notional * instrument.margin_maint
        margin += adjusted_notional * instrument.taker_fee

        if instrument.is_inverse and not inverse_as_quote:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)
