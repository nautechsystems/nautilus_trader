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
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.tick cimport QuoteTick


cdef class Account:
    """
    Represents a trading account.
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

        self._events = [event]
        self._portfolio = None  # Initialized when registered with portfolio

        self.id = event.account_id
        self.account_type = self.id.account_type
        self.currency = event.currency
        self.balance = Money(0, self.currency)
        self.margin_balance = Money(0, self.currency)
        self.margin_available = Money(0, self.currency)
        self.order_margin = Money(0, self.currency)
        self.position_margin = Money(0, self.currency)

        # Update account
        self.apply(event)

    def __eq__(self, Account other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        Condition.not_none(other, "other")

        return self.id == other.id

    def __ne__(self, Account other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

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
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"

    cpdef void register_portfolio(self, Portfolio portfolio) except *:
        """
        Register the given portfolio with the account.

        Parameters
        ----------
        portfolio : Portfolio
            The portfolio to register.

        Raises
        ----------
        ValueError
            If a portfolio is already registered.

        """
        Condition.not_none(portfolio, "portfolio")
        Condition.none(self._portfolio, "self._portfolio")  # No portfolio should be registered

        self._portfolio = portfolio

    cpdef int event_count(self):
        """
        Return the count of events.

        Returns
        -------
        int

        """
        return len(self._events)

    cpdef list get_events(self):
        """
        Return the events received by the account.

        Returns
        -------
        List[AccountState]

        """
        return self._events.copy()

    cpdef AccountState last_event(self):
        """
        Return the last event.

        Returns
        -------
        AccountState

        """
        return self._events[-1]

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

        self.balance = event.balance
        self.margin_balance = event.margin_balance
        self.margin_available = event.margin_available
        self.order_margin = Money(0, self.currency)
        self.position_margin = Money(0, self.currency)
