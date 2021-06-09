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
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance


cdef class Account:
    """
    The base class for all trading accounts.

    Represents Cash account type.
    """

    def __init__(self, AccountState event):
        """
        Initialize a new instance of the ``Account`` class.

        Parameters
        ----------
        event : AccountState
            The initial account state event.

        """
        Condition.not_none(event, "event")

        self.id = event.account_id
        self.type = event.account_type
        self.base_currency = event.base_currency

        cdef dict initial_margins = event.info.get("initial_margins", {})
        cdef dict maint_margins = event.info.get("maint_margins", {})

        self._starting_balances = {b.currency: b.total for b in event.balances}
        self._events = [event]                    # type: list[AccountState]
        self._balances = {}                       # type: dict[Currency, AccountBalance]
        self._commissions = {}                    # type: dict[Currency, Money]
        self._initial_margins = initial_margins   # type: dict[Currency, Money]
        self._maint_margins = maint_margins       # type: dict[Currency, Money]
        self._portfolio = None  # Initialized when registered with portfolio

        self._update_balances(event.balances)

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
        Condition.equal(event.account_id, self.id, "self.id", "event.account_id")
        Condition.equal(event.base_currency, self.base_currency, "self.base_currency", "event.base_currency")

        if self.base_currency:
            # Single-currency account
            Condition.true(len(event.balances) == 1, "single-currency account has multiple currency update")
            Condition.equal(event.balances[0].currency, self.base_currency, "event.balances[0].currency", "self.base_currency")

        self._events.append(event)
        self._update_balances(event.balances)

    cpdef void update_initial_margin(self, Money margin) except *:
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

        self._initial_margins[margin.currency] = margin

    cpdef void update_maint_margin(self, Money margin) except *:
        """
        Update the maintenance margin.

        Parameters
        ----------
        margin : Money
            The current maintenance margin for the currency.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(margin, "money")

        self._maint_margins[margin.currency] = margin\

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

    cpdef Money unrealized_pnl(self, Currency currency=None):
        """
        Return the current account unrealized PnL.

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
        ValueError
            If portfolio is not registered.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.base_currency
        Condition.not_none(currency, "currency")
        Condition.not_none(self._portfolio, "self._portfolio")

        # TODO: Assumption that issuer == venue
        cdef dict unrealized_pnls = self._portfolio.unrealized_pnls(Venue(self.id.issuer))
        if unrealized_pnls is None:
            return None

        return unrealized_pnls.get(currency, Money(0, currency))

    cpdef Money equity(self, Currency currency=None):
        """
        Return the account equity (`balance + unrealized_pnl`).

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

        balance: Decimal = self._balances.get(currency)
        if balance is None:
            return None

        cdef Money unrealized_pnl = self.unrealized_pnl(currency)
        if unrealized_pnl is None:
            return None

        return Money(balance.free + unrealized_pnl, currency)

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

# -- QUERIES-MARGIN --------------------------------------------------------------------------------

    cpdef dict initial_margins(self):
        """
        Return the initial margins for the account.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._initial_margins.copy()

    cpdef dict maint_margins(self):
        """
        Return the maintenance margins for the account.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._maint_margins.copy()

    cpdef Money initial_margin(self, Currency currency=None):
        """
        Return the current initial margin.

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

        return self._initial_margins.get(currency)

    cpdef Money maint_margin(self, Currency currency=None):
        """
        Return the current maintenance margin.

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

        return self._maint_margins.get(currency)

    cpdef Money margin_available(self, Currency currency=None):
        """
        Return the current margin available.

        (`equity - initial_margin - maint_margin`).

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

        cdef Money equity = self.equity(currency)
        if equity is None:
            return None

        initial_margin: Decimal = self._initial_margins.get(currency, Decimal())
        maint_margin: Decimal = self._maint_margins.get(currency, Decimal())

        return Money(equity - initial_margin - maint_margin, currency)

# -- CALCULATIONS ----------------------------------------------------------------------------------

    cpdef list calculate_pnls(
        self,
        Instrument instrument,
        Position position,
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
        if self.type == AccountType.CASH:
            return self._calculate_pnls_cash_account(instrument, fill)
        elif self.type == AccountType.MARGIN:
            return [self._calculate_pnl_margin_account(instrument, position, fill)]

    cdef list _calculate_pnls_cash_account(
        self,
        Instrument instrument,
        OrderFilled fill,
    ):
        # Assumption that a cash account never deals
        # with inverse or quanto instruments.
        cdef list pnls = []

        cdef Currency quote_currency = instrument.quote_currency
        cdef Currency base_currency = instrument.get_base_currency()

        if fill.order_side == OrderSide.BUY:
            if base_currency:
                pnls.append(Money(fill.last_qty, base_currency))
            pnls.append(Money(-(fill.last_qty * (1 / fill.last_px)), quote_currency))
        else:  # OrderSide.SELL
            if base_currency:
                pnls.append(Money(-fill.last_qty, base_currency))
            pnls.append(Money(fill.last_qty * (1 / fill.last_px), quote_currency))

        return pnls

    cdef Money _calculate_pnl_margin_account(
        self,
        Instrument instrument,
        Position position,
        OrderFilled fill,
    ):
        if position and position.entry != fill.order_side:
            # Calculate positional PnL
            return position.calculate_pnl(
                avg_px_open=position.avg_px_open,
                avg_px_close=fill.last_px,
                quantity=fill.last_qty,
            )
        else:
            return Money(0, instrument.get_cost_currency())

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef void _update_balances(self, list balances) except *:
        # Update the balances. Note that there is no guarantee that every
        # account currency is included in the event, which is why we don't just
        # assign a dict.
        cdef AccountBalance balance
        for balance in balances:
            self._balances[balance.currency] = balance
