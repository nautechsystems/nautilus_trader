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
        Initialize a new instance of the Account class.

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
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : Account
            The other account to equate.

        Returns
        -------
        bool

        """
        Condition.not_none(other, "other")

        return self.id == other.id

    def __ne__(self, Account other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the
        given object.

        Parameters
        ----------
        other : Account
            The other account to equate.

        Returns
        -------
        bool

        """
        Condition.not_none(other, "other")

        return self.id != other.id

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self.id.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"Account({self.id.value})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the
        objects location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"

    cpdef void register_portfolio(self, PortfolioReadOnly portfolio):
        """
        Register the given portfolio with the account.

        Parameters
        ----------
        portfolio : PortfolioReadOnly
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
        if not self._portfolio:
            return None

        return self._portfolio.unrealized_pnl_for_venue(self.id.issuer_as_venue())

    cpdef Money margin_balance(self):
        """
        Return the current account margin balance.

        Returns
        -------
        Money or None

        """
        if not self._portfolio:
            return None

        if not self._balance:
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
        if not self._portfolio:
            return None

        cdef Money margin_balance = self.margin_balance()
        if not margin_balance:
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

    cpdef AccountState last_event(self):
        """
        Return the accounts last state event.

        Returns
        -------
        AccountState

        """
        return self._events[-1]

    cpdef list events(self):
        """
        Return all events received by the account.

        Returns
        -------
        List[AccountState]

        """
        return self._events.copy()

    cpdef int event_count(self) except *:
        """
        Return the count of events.

        Returns
        -------
        int

        """
        return len(self._events)
