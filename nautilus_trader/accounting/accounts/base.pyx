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

from nautilus_trader.accounting.error import AccountBalanceNegative

from libc.stdint cimport uint64_t

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.functions cimport account_type_to_str
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance


cdef class Account:
    """
    The base class for all trading accounts.
    """

    def __init__(self, AccountState event, bint calculate_account_state):
        Condition.not_none(event, "event")

        self.id = event.account_id
        self.type = event.account_type
        self.base_currency = event.base_currency
        self.is_cash_account = self.type == AccountType.CASH or self.type == AccountType.BETTING
        self.is_margin_account = self.type == AccountType.MARGIN
        self.calculate_account_state = calculate_account_state

        self._events: list[AccountState] = [event]  # `last_event_c()` guaranteed
        self._commissions: dict[Currency, Money] = {}
        self._balances: dict[Currency, AccountBalance] = {}
        self._balances_starting: dict[Currency, Money] = {b.currency: b.total for b in event.balances}

        self.update_balances(event.balances)

    def __eq__(self, Account other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:
        cdef str base_str = self.base_currency.code if self.base_currency is not None else None
        return (
            f"{type(self).__name__}("
            f"id={self.id.to_str()}, "
            f"type={account_type_to_str(self.type)}, "
            f"base={base_str})"
        )

# -- QUERIES --------------------------------------------------------------------------------------

    cdef AccountState last_event_c(self):
        return self._events[-1]  # Guaranteed at least one event from initialization

    cdef list events_c(self):
        return self._events.copy()

    cdef int event_count_c(self):
        return len(self._events)

    @property
    def last_event(self):
        """
        Return the accounts last state event.

        Returns
        -------
        AccountState

        """
        return self.last_event_c()

    @property
    def events(self):
        """
        Return all events received by the account.

        Returns
        -------
        list[AccountState]

        """
        return self.events_c()

    @property
    def event_count(self):
        """
        Return the count of events.

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
        return self._balances_starting.copy()

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
        cdef AccountBalance b
        return {c: b.total for c, b in self._balances.items()}

    cpdef dict balances_free(self):
        """
        Return the account balances free.

        Returns
        -------
        dict[Currency, Money]

        """
        cdef AccountBalance b
        return {c: b.free for c, b in self._balances.items()}

    cpdef dict balances_locked(self):
        """
        Return the account balances locked.

        Returns
        -------
        dict[Currency, Money]

        """
        cdef AccountBalance b
        return {c: b.locked for c, b in self._balances.items()}

    cpdef dict commissions(self):
        """
        Return the total commissions for the account.
        """
        return self._commissions.copy()

    cpdef AccountBalance balance(self, Currency currency = None):
        """
        Return the current account balance total.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If ``None`` then will use the default
            currency (if set).

        Returns
        -------
        AccountBalance or ``None``

        Raises
        ------
        ValueError
            If `currency` is ``None`` and `base_currency` is ``None``.

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.base_currency
        Condition.not_none(currency, "currency")

        return self._balances.get(currency)

    cpdef Money balance_total(self, Currency currency = None):
        """
        Return the current account balance total.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If ``None`` then will use the default
            currency (if set).

        Returns
        -------
        Money or ``None``

        Raises
        ------
        ValueError
            If `currency` is ``None`` and `base_currency` is ``None``.

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.base_currency
        Condition.not_none(currency, "currency")

        cdef AccountBalance balance = self._balances.get(currency)
        if balance is None:
            return None
        return balance.total

    cpdef Money balance_free(self, Currency currency = None):
        """
        Return the account balance free.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If ``None`` then will use the default
            currency (if set).

        Returns
        -------
        Money or ``None``

        Raises
        ------
        ValueError
            If `currency` is ``None`` and `base_currency` is ``None``.

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.base_currency
        Condition.not_none(currency, "currency")

        cdef AccountBalance balance = self._balances.get(currency)
        if balance is None:
            return None
        return balance.free

    cpdef Money balance_locked(self, Currency currency = None):
        """
        Return the account balance locked.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If ``None`` then will use the default
            currency (if set).

        Returns
        -------
        Money or ``None``

        Raises
        ------
        ValueError
            If `currency` is ``None`` and `base_currency` is ``None``.

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
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
        Money or ``None``

        """
        Condition.not_none(currency, "currency")

        return self._commissions.get(currency)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void apply(self, AccountState event):
        """
        Apply the given account event to the account.

        Parameters
        ----------
        event : AccountState
            The account event to apply.

        Raises
        ------
        ValueError
            If `event.account_type` is not equal to `self.type`.
        ValueError
            If `event.account_id` is not equal to `self.id`.
        ValueError
            If `event.base_currency` is not equal to `self.base_currency`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")
        Condition.equal(event.account_type, self.type, "event.account_type", "self.type")
        Condition.equal(event.account_id, self.id, "self.id", "event.account_id")
        Condition.equal(event.base_currency, self.base_currency, "self.base_currency", "event.base_currency")

        if self.base_currency:
            # Single-currency account
            Condition.is_true(len(event.balances) == 1, "single-currency account has multiple currency update")
            Condition.equal(event.balances[0].currency, self.base_currency, "event.balances[0].currency", "self.base_currency")

        self._events.append(event)
        self.update_balances(event.balances)

    cpdef void update_balances(self, list balances, bint allow_zero=True):
        """
        Update the account balances.

        There is no guarantee that every account currency is included in the
        given balances, therefore we only update included balances.

        Parameters
        ----------
        balances : list[AccountBalance]
            The balances for the update.
        allow_zero : bool, default True
            If zero balances are allowed (will then just clear the assets balance).

        Raises
        ------
        ValueError
            If `balances` is empty.

        """
        Condition.not_empty(balances, "balances")

        cdef AccountBalance balance
        for balance in balances:
            if not balance.total._mem.raw > 0:
                if balance.total._mem.raw < 0:
                    raise AccountBalanceNegative(balance.total.as_decimal(), balance.currency)
                if balance.total.is_zero() and not allow_zero:
                    raise AccountBalanceNegative(balance.total.as_decimal(), balance.currency)
                else:
                    # Clear asset balance
                    self._balances.pop(balance.currency, None)
            self._balances[balance.currency] = balance

    cpdef void update_commissions(self, Money commission):
        """
        Update the commissions.

        Can be negative which represents credited commission.

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
        if commission._mem.raw == 0:
            return  # Nothing to update

        cdef Currency currency = commission.currency
        total_commissions = self._commissions.get(currency, Decimal(0))
        self._commissions[currency] = Money(total_commissions + commission.as_decimal(), currency)

    cpdef void purge_account_events(self, uint64_t ts_now, uint64_t lookback_secs = 0):
        """
        Purge all account state events which are outside the lookback window.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).
        lookback_secs : uint64_t, default 0
            The purge lookback window (seconds) from when the account state event occurred.
            Only events which are outside the lookback window will be purged.
            A value of 0 means purge all account state events.

        """
        cdef uint64_t lookback_ns = nautilus_pyo3.secs_to_nanos(lookback_secs)

        cdef list[AccountState] retained_events = []

        cdef:
            AccountState event
        for event in self._events:
            if event.ts_event + lookback_ns > ts_now:
                retained_events.append(event)

        self._events = retained_events

# -- CALCULATIONS ---------------------------------------------------------------------------------

    cpdef bint is_unleveraged(self, InstrumentId instrument_id):
        """
        Return whether the given instrument is leveraged for this account (leverage == 1).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to check.

        Returns
        -------
        bool

        """
        raise NotImplementedError("method `is_unleveraged` must be implemented in the subclass")  # pragma: no cover

    cdef void _recalculate_balance(self, Currency currency):
        raise NotImplementedError("method `_recalculate_balance` must be implemented in the subclass")  # pragma: no cover

    cpdef Money calculate_commission(
        self,
        Instrument instrument,
        Quantity last_qty,
        Price last_px,
        LiquiditySide liquidity_side,
        bint use_quote_for_inverse=False,
    ):
        raise NotImplementedError("method `calculate_commission` must be implemented in the subclass")  # pragma: no cover

    cpdef list calculate_pnls(
        self,
        Instrument instrument,
        OrderFilled fill,
        Position position: Position | None = None,
    ):
        raise NotImplementedError("method `calculate_pnls` must be implemented in the subclass")  # pragma: no cover

    cpdef Money balance_impact(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        OrderSide order_side,
    ):
        raise NotImplementedError("method `balance_impact` must be implemented in the subclass")  # pragma: no cover
