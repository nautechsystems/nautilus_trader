# -------------------------------------------------------------------------------------------------
# <copyright file="account.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport ValidString
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.events cimport AccountEvent
from nautilus_trader.model.objects cimport Money


cdef class Account:
    """
    Represents a brokerage account.
    """

    def __init__(self, Currency currency=Currency.USD):
        """
        Initializes a new instance of the Account class.
        """
        self._events = []

        self.initialized = False
        self.id = AccountId('UNKNOWN', '000000000')
        self.broker = self.id.broker
        self.number = self.id.number
        self.currency = currency
        self.cash_balance = Money.zero()
        self.cash_start_day = Money.zero()
        self.cash_activity_day = Money.zero()
        self.margin_used_liquidation = Money.zero()
        self.margin_used_maintenance = Money.zero()
        self.margin_ratio = Money.zero()
        self.margin_call_status = ValidString('NONE')
        self.free_equity = Money.zero()
        self.last_updated = None
        self.event_count = 0
        self.last_event = None

    def __eq__(self, Account other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.
        """
        return self.id == other.id

    def __ne__(self, Account other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash((self.broker, self.number))

    def __str__(self) -> str:
        """
        Return the str() string representation of the account.
        """
        return f"Account({str(self.broker)}-{str(self.number)})"

    def __repr__(self) -> str:
        """
        Return the repr() string representation of the account.
        """
        return f"<{str(self)} object at {id(self)}>"

    cpdef list get_events(self):
        """
        :return: List[Event]. 
        """
        return self._events.copy()

    cpdef void initialize(self, AccountEvent event):
        """
        Initialize the account with the given event.
        
        :param event: The event to initialize with.
        """
        self.id = event.account_id
        self.broker = self.id.broker
        self.number = self.id.number
        self.currency = event.currency
        self.initialized = True

    cpdef void apply(self, AccountEvent event):
        """
        Applies the given account event to the account.

        :param event: The account event to apply.
        """
        if self.initialized:
            Condition.equal(self.id, event.account_id)

        self._events.append(event)
        self.event_count += 1
        self.last_event = event

        if not self.initialized:
            self.initialize(event)

        self.cash_balance = event.cash_balance
        self.cash_start_day = event.cash_start_day
        self.cash_activity_day = event.cash_activity_day
        self.margin_used_liquidation = event.margin_used_liquidation
        self.margin_used_maintenance = event.margin_used_maintenance
        self.margin_ratio = event.margin_ratio
        self.margin_call_status = event.margin_call_status
        self.free_equity = Money(max((self.cash_balance.value - (self.margin_used_maintenance.value + self.margin_used_liquidation.value)), 0))

        self.last_updated = event.timestamp

    cpdef void reset(self):
        """
        Reset the account by returning all stateful internal values to their initial value.
        """
        self._events = []

        self.initialized = False
        self.id = AccountId('UNKNOWN', '000000000')
        self.broker = self.id.broker
        self.number = self.id.number
        self.currency = Currency.UNKNOWN
        self.cash_balance = Money.zero()
        self.cash_start_day = Money.zero()
        self.cash_activity_day = Money.zero()
        self.margin_used_liquidation = Money.zero()
        self.margin_used_maintenance = Money.zero()
        self.margin_ratio = Money.zero()
        self.margin_call_status = ValidString('NONE')
        self.free_equity = Money.zero()
        self.last_updated = None
        self.event_count = 0
        self.last_event = None
