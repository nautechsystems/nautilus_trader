#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="account.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.c_enums.brokerage cimport Broker
from inv_trader.c_enums.currency cimport Currency
from inv_trader.model.events cimport AccountEvent
from inv_trader.model.objects import ValidString, Money


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
        self.id = None
        self.broker = Broker.UNKNOWN
        self.account_number = None
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
        Override the default equality comparison.
        """
        return self.id == other.id

    def __ne__(self, Account other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash((self.broker, self.account_number))

    def __str__(self) -> str:
        """
        Return the str() string representation of the account.
        """
        return f"Account({str(self.broker)}-{str(self.account_number)})"

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
        self.broker = event.broker
        self.account_number = event.account_number
        self.id = event.account_id
        self.currency = event.currency
        self.initialized = True

    cpdef void apply(self, AccountEvent event):
        """
        Applies the given account event to the account.

        :param event: The account event to apply.
        """
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
        self.id = None
        self.broker = Broker.UNKNOWN
        self.account_number = None
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
