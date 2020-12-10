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
from nautilus_trader.model.events cimport AccountState


cdef class Account:
    """
    Provides a trading account.
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
        self.account_type = self.id.account_type
        self.currency = event.currency

        self._events = [event]
        self._portfolio = None
        self._balance = event.balance
        self._order_margin = Money(0, self.currency)
        self._position_margin = Money(0, self.currency)

    def __eq__(self, Account other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, Account other) -> bool:
        return self.id.value != other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id.value})"

    cdef AccountState last_event_c(self):
        return self._events[-1]

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

    cpdef void register_portfolio(self, PortfolioFacade portfolio):
        """
        Register the given portfolio with the account.

        Parameters
        ----------
        portfolio : PortfolioFacade
            The portfolio to register

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

        """
        Condition.not_none(event, "event")
        Condition.equal(self.id, event.account_id, "id", "event.account_id")

        self._events.append(event)
        self._balance = event.balance

    cpdef void update_order_margin(self, Money margin) except *:
        """
        Update the order margin.

        Parameters
        ----------
        margin : Money
            The current order margin.

        Raises
        ----------
        ValueError
            If margin.currency is not equal to self.currency.

        """
        Condition.not_none(margin, "money")
        Condition.equal(margin.currency, self.currency, "margin.currency", "self.currency")

        self._order_margin = margin

    cpdef void update_position_margin(self, Money margin) except *:
        """
        Update the position margin.

        Parameters
        ----------
        margin : Money
            The current position margin.

        Raises
        ----------
        ValueError
            If margin.currency is not equal to self.currency.

        """
        Condition.not_none(margin, "money")
        Condition.equal(margin.currency, self.currency, "margin.currency", "self.currency")

        self._position_margin = margin

    cpdef Money balance(self):
        """
        Return the current account balance.

        Returns
        -------
        Money or None

        """
        return self._balance

    cpdef Money unrealized_pnl(self):
        """
        Return the current account unrealized PNL.

        Returns
        -------
        Money or None

        """
        if self._portfolio is None:
            return None

        return self._portfolio.unrealized_pnl_for_venue(self.id.issuer_as_venue())

    cpdef Money margin_balance(self):
        """
        Return the current account margin balance.

        Returns
        -------
        Money or None

        """
        if self._portfolio is None:
            return None

        if self._balance is None:
            return None

        cdef Money unrealized_pnl = self.unrealized_pnl()
        if unrealized_pnl is None:
            return None

        return Money(self._balance + unrealized_pnl, self.currency)

    cpdef Money margin_available(self):
        """
        Return the current account margin available.

        Returns
        -------
        Money or None

        """
        if self._portfolio is None:
            return None

        cdef Money margin_balance = self.margin_balance()
        if margin_balance is None:
            return None

        return Money(margin_balance - self._order_margin - self._position_margin, self.currency)

    cpdef Money order_margin(self):
        """
        Return the account order margin.

        Returns
        -------
        Money

        """
        return self._order_margin

    cpdef Money position_margin(self):
        """
        Return the account position margin.

        Returns
        -------
        Money

        """
        return self._position_margin
