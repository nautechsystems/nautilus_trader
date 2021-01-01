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
from nautilus_trader.model.events cimport AccountState


cdef class Account:
    """
    Provides a trading account.

    Represents Cash, Margin or Futures account types.
    """

    def __init__(self, AccountState event):
        """
        Initialize a new instance of the `Account` class.

        Parameters
        ----------
        event : AccountState
            The initial account state event.

        """
        Condition.not_none(event, "event")

        self.id = event.account_id

        default_currency_str = event.info.get("default_currency")
        if default_currency_str:
            self.default_currency = Currency.from_str_c(default_currency_str)
        else:
            self.default_currency = None

        self._events = [event]
        self._starting_balances = {b.currency: b for b in event.balances}
        self._balances = {}                                        # type: dict[Currency, Money]
        self._balances_free = {}                                   # type: dict[Currency, Money]
        self._balances_locked = {}                                 # type: dict[Currency, Money]
        self._init_margins = event.info.get("init_margins", {})    # type: dict[Currency, Money]
        self._maint_margins = event.info.get("maint_margins", {})  # type: dict[Currency, Money]
        self._portfolio = None  # Initialized when registered with portfolio

        self._update_balances(
            event.balances,
            event.balances_free,
            event.balances_locked,
        )

    def __eq__(self, Account other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, Account other) -> bool:
        return self.id.value != other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id.value})"

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

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void register_portfolio(self, PortfolioFacade portfolio):
        """
        Register the given portfolio with the account.

        Parameters
        ----------
        portfolio : PortfolioFacade
            The portfolio to register.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(portfolio, "portfolio")

        self._portfolio = portfolio

    cpdef void apply(self, AccountState event) except *:
        """
        Applies the given account event to the account.

        Parameters
        ----------
        event : AccountState
            The account event to apply.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")
        Condition.equal(self.id, event.account_id, "id", "event.account_id")

        self._events.append(event)
        self._update_balances(
            event.balances,
            event.balances_free,
            event.balances_locked,
        )

    cpdef void update_init_margin(self, Money margin) except *:
        """
        Update the initial margin.

        Parameters
        ----------
        margin : Money
            The current initial margin for the currency.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(margin, "money")

        self._init_margins[margin.currency] = margin

    cpdef void update_maint_margin(self, Money margin) except *:
        """
        Update the maintenance margin.

        Parameters
        ----------
        margin : Decimal
            The current maintenance margin for the currency.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(margin, "money")

        self._maint_margins[margin.currency] = margin

# -- QUERIES-CASH ----------------------------------------------------------------------------------

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
        Return the account balances.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._balances.copy()

    cpdef dict balances_free(self):
        """
        Return the account balances free.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._balances_free.copy()

    cpdef dict balances_locked(self):
        """
        Return the account balances locked.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._balances_locked.copy()

    cpdef Money balance(self, Currency currency=None):
        """
        Return the current account balance.

        For multi-asset accounts, specify the currency for the query.

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
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        return self._balances.get(currency)

    cpdef Money balance_free(self, Currency currency=None):
        """
        Return the account balance free.

        For multi-asset accounts, specify the currency for the query.

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
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        return self._balances_free.get(currency)

    cpdef Money balance_locked(self, Currency currency=None):
        """
        Return the account balance locked.

        For multi-asset accounts, specify the currency for the query.

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
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        return self._balances_locked.get(currency)

    cpdef Money unrealized_pnl(self, Currency currency=None):
        """
        Return the current account unrealized P&L.

        For multi-asset accounts, specify the currency for the query.

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
            If currency is None and default_currency is None.
        ValueError
            If portfolio is not registered.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")
        Condition.not_none(self._portfolio, "self._portfolio")

        cdef dict unrealized_pnls = self._portfolio.unrealized_pnls(self.id.issuer_as_venue())
        if unrealized_pnls is None:
            return None

        return unrealized_pnls.get(currency, Money(0, currency))

    cpdef Money equity(self, Currency currency=None):
        """
        Return the account equity.

        For multi-asset accounts, specify the currency for the query.

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
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        balance: Decimal = self._balances.get(currency)
        if balance is None:
            return None

        cdef Money unrealized_pnl = self.unrealized_pnl(currency)
        if unrealized_pnl is None:
            return None

        return Money(balance + unrealized_pnl, currency)

# -- QUERIES-MARGIN --------------------------------------------------------------------------------

    cpdef dict init_margins(self):
        """
        Return the initial margins for the account.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._init_margins.copy()

    cpdef dict maint_margins(self):
        """
        Return the maintenance margins for the account.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._maint_margins.copy()

    cpdef Money init_margin(self, Currency currency=None):
        """
        Return the current initial margin.

        For multi-asset accounts, specify the currency for the query.

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
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        return self._init_margins.get(currency)

    cpdef Money maint_margin(self, Currency currency=None):
        """
        Return the current maintenance margin.

        For multi-asset accounts, specify the currency for the query.

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
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        return self._maint_margins.get(currency)

    cpdef Money free_margin(self, Currency currency=None):
        """
        Return the current free margin.

        For multi-asset accounts, specify the currency for the query.

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
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        cdef Money equity = self.equity(currency)
        if equity is None:
            return None

        init_margin: Decimal = self._init_margins.get(currency, Decimal())
        maint_margin: Decimal = self._maint_margins.get(currency, Decimal())

        return Money(equity - init_margin - maint_margin, currency)

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _update_balances(
        self,
        list balances,
        list balances_free,
        list balances_locked,
    ) except *:
        # Update the balances. Note that there is no guarantee that every
        # account currency is included in the event, which is my we don't just
        # assign a dict.
        cdef Money balance
        for balance in balances:
            self._balances[balance.currency] = balance

        for balance in balances_free:
            self._balances_free[balance.currency] = balance

        for balance in balances_locked:
            self._balances_locked[balance.currency] = balance
