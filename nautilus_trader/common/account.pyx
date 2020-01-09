# -------------------------------------------------------------------------------------------------
# <copyright file="account.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.model.objects cimport Decimal, Money


cdef class Account:
    """
    Represents a brokerage account.
    """

    def __init__(self, AccountStateEvent event):
        """
        Initializes a new instance of the Account class.

        :param: event: The initial account state event.
        """
        self._events = [event]
        self.event_count = 1
        self.last_event = event

        self.id = event.account_id
        self.broker = self.id.broker
        self.account_number = self.id.account_number
        self.account_type = self.id.account_type
        self.currency = event.currency
        self.cash_balance = event.cash_balance
        self.cash_start_day = event.cash_start_day
        self.cash_activity_day = event.cash_activity_day
        self.margin_used_liquidation = event.margin_used_liquidation
        self.margin_used_maintenance = event.margin_used_maintenance
        self.margin_ratio = event.margin_ratio
        self.margin_call_status = event.margin_call_status
        self.free_equity = self._calculate_free_equity()

        self.last_updated = event.timestamp

    def __eq__(self, Account other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.id == other.id

    def __ne__(self, Account other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return a hash representation of this object.

        :return int.
        """
        return hash(self.id.value)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"Account({self.id.value})"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"

    cpdef list get_events(self):
        """
        :return List[Event]. 
        """
        return self._events.copy()

    cpdef void apply(self, AccountStateEvent event):
        """
        Applies the given account event to the account.

        :param event: The account event to apply.
        """
        Condition.equals(self.id, event.account_id, 'id', 'event.account_id')

        self._events.append(event)
        self.event_count += 1
        self.last_event = event

        self.cash_balance = event.cash_balance
        self.cash_start_day = event.cash_start_day
        self.cash_activity_day = event.cash_activity_day
        self.margin_used_liquidation = event.margin_used_liquidation
        self.margin_used_maintenance = event.margin_used_maintenance
        self.margin_ratio = event.margin_ratio
        self.margin_call_status = event.margin_call_status
        self.free_equity = self._calculate_free_equity()

        self.last_updated = event.timestamp

    cdef Money _calculate_free_equity(self):
        return Money(max((self.cash_balance.value - (self.margin_used_maintenance.value + self.margin_used_liquidation.value)), 0))


cdef class NullAccount(Account):
    """
    Represents a null un-initialized brokerage account.
    """

    def __init__(self, datetime time_now):
        """
        Initializes a new instance of the NullAccount class.

        :param: time_now: The creation datetime.
        """
        cdef AccountStateEvent event = AccountStateEvent(
            AccountId('NULL', '000', AccountType.SIMULATED),
            Currency.USD,
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Decimal.zero(),
            ValidString('N'),
            GUID(uuid.uuid4()),
            time_now)
        super().__init__(event)
