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
from nautilus_trader.model.c_enums.account_type cimport AccountTypeParser
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance


cdef class Account:
    """
    The base class for all trading accounts.
    """

    def __init__(self, AccountState event):
        """
        Initialize a new instance of the ``Account`` class.

        Parameters
        ----------
        event : AccountState
            The initial account state event.

        Raises
        ------
        ValueError
            If account_type is not equal to event.account_type.

        """
        Condition.not_none(event, "event")

        self.id = event.account_id
        self.type = event.account_type
        self.base_currency = event.base_currency
        self.calculate_account_state = False

        self._starting_balances = {b.currency: b.total for b in event.balances}
        self._events = [event]  # type: list[AccountState]
        self._balances = {}     # type: dict[Currency, AccountBalance]
        self._commissions = {}  # type: dict[Currency, Money]

        self._update_balances(event.balances)

    def __eq__(self, Account other) -> bool:
        return self.id.value == other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __repr__(self) -> str:
        cdef str base_str = self.base_currency.code if self.base_currency is not None else None
        return (f"{type(self).__name__}("
                f"id={self.id.value}, "
                f"type={AccountTypeParser.to_str(self.type)}, "
                f"base={base_str})")

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef void _update_balances(self, list balances) except *:
        # Update the balances. Note that there is no guarantee that every
        # account currency is included in the event, which is why we don't just
        # assign a dict.
        cdef AccountBalance balance
        for balance in balances:
            self._balances[balance.currency] = balance

# -- QUERIES ---------------------------------------------------------------------------------------

    cdef AccountState last_event_c(self):
        return self._events[-1]  # Always at least one event

    cdef list events_c(self):
        return self._events.copy()

    cdef int event_count_c(self):
        return len(self._events)

    @property
    def last_event(self):
        """
        The accounts last state event.

        Returns
        -------
        AccountState

        """
        return self.last_event_c()

    @property
    def events(self):
        """
        All events received by the account.

        Returns
        -------
        list[AccountState]

        """
        return self.events_c()

    @property
    def event_count(self):
        """
        The count of events.

        Returns
        -------
        int

        """
        return self.event_count_c()

    cpdef list currencies(self):
        """
        Return the account currencies.

        Returns
        -------
        list[Currency]

        """
        return list(self._balances.keys())

    cpdef dict starting_balances(self):
        """
        Return the account starting balances.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._starting_balances.copy()

    cpdef dict balances(self):
        """
        Return the account balances totals.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._balances.copy()

    cpdef dict balances_total(self):
        """
        Return the account balances totals.

        Returns
        -------
        dict[Currency, Money]

        """
        return {c: b.total for c, b in self._balances.items()}

    cpdef dict balances_free(self):
        """
        Return the account balances free.

        Returns
        -------
        dict[Currency, Money]

        """
        return {c: b.free for c, b in self._balances.items()}

    cpdef dict balances_locked(self):
        """
        Return the account balances locked.

        Returns
        -------
        dict[Currency, Money]

        """
        return {c: b.locked for c, b in self._balances.items()}

    cpdef dict commissions(self):
        """
        Return the total commissions for the account.
        """
        return self._commissions.copy()

    cpdef AccountBalance balance(self, Currency currency=None):
        """
        Return the current account balance total.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        AccountBalance or None

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

        return self._balances.get(currency)

    cpdef Money balance_total(self, Currency currency=None):
        """
        Return the current account balance total.

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

        cdef AccountBalance balance = self._balances.get(currency)
        if balance is None:
            return None
        return balance.total

    cpdef Money balance_free(self, Currency currency=None):
        """
        Return the account balance free.

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

        cdef AccountBalance balance = self._balances.get(currency)
        if balance is None:
            return None
        return balance.free

    cpdef Money balance_locked(self, Currency currency=None):
        """
        Return the account balance locked.

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

        cdef AccountBalance balance = self._balances.get(currency)
        if balance is None:
            return None
        return balance.locked

    cpdef Money commission(self, Currency currency):
        """
        Return the total commissions for the given currency.

        Parameters
        ----------
        currency : Currency
            The currency for the commission.

        Returns
        -------
        Money or None

        """
        return self._commissions.get(currency)

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void set_calculate_account_state(self, bint value) except *:
        """
        Set the `calculate_account_state` flag.

        Parameters
        ----------
        value : bool
            The value to set.

        """
        self.calculate_account_state = value

    cpdef void apply(self, AccountState event) except *:
        """
        Apply the given account event to the account.

        Parameters
        ----------
        event : AccountState
            The account event to apply.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")
        Condition.equal(event.account_id, self.id, "self.id", "event.account_id")
        Condition.equal(event.base_currency, self.base_currency, "self.base_currency", "event.base_currency")

        if self.base_currency:
            # Single-currency account
            Condition.true(len(event.balances) == 1, "single-currency account has multiple currency update")
            Condition.equal(event.balances[0].currency, self.base_currency, "event.balances[0].currency", "self.base_currency")

        self._events.append(event)
        self._update_balances(event.balances)

    cpdef void update_commissions(self, Money commission) except *:
        """
        Update the commissions.

        Parameters
        ----------
        commission : Money
            The commission to update with.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(commission, "commission")

        # Increment total commissions
        if commission.as_decimal() == 0:
            return  # Nothing to update

        cdef Currency currency = commission.currency
        total_commissions: Decimal = self._commissions.get(currency, Decimal())
        self._commissions[currency] = Money(total_commissions + commission, currency)

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
            raise RuntimeError("Cannot apply initial margin when no current balance")

        cdef AccountBalance new_balance = AccountBalance(
            currency,
            current_balance.total,
            margin_initial,
            Money(current_balance.total.as_decimal() - margin_initial.as_decimal(), currency),
        )

        self._balances[currency] = new_balance

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
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money calculate_commission(
        self,
        Instrument instrument,
        Quantity last_qty,
        last_px: Decimal,
        LiquiditySide liquidity_side,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the commission generated from a transaction with the given
        parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        last_qty : Quantity
            The transaction quantity.
        last_px : Decimal or Price
            The transaction price.
        liquidity_side : LiquiditySide
            The liquidity side for the transaction.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If liquidity_side is NONE.

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(last_qty, "last_qty")
        Condition.type(last_px, (Decimal, Price), "last_px")
        Condition.not_equal(liquidity_side, LiquiditySide.NONE, "liquidity_side", "NONE")

        notional: Decimal = instrument.notional_value(
            quantity=last_qty,
            price=last_px,
            inverse_as_quote=inverse_as_quote,
        ).as_decimal()

        if liquidity_side == LiquiditySide.MAKER:
            commission: Decimal = notional * instrument.maker_fee
        elif liquidity_side == LiquiditySide.TAKER:
            commission: Decimal = notional * instrument.taker_fee
        else:
            raise ValueError(
                f"invalid LiquiditySide, was {LiquiditySideParser.to_str(liquidity_side)}"
            )

        if instrument.is_inverse and not inverse_as_quote:
            return Money(commission, instrument.base_currency)
        else:
            return Money(commission, instrument.quote_currency)

    cpdef list calculate_pnls(
        self,
        Instrument instrument,
        Position position,  # Can be None
        OrderFilled fill,
    ):
        """
        Return the calculated PnLs.

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
        raise NotImplementedError("method must be implemented in the subclass")
