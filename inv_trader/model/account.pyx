#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="account.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

import datetime as dt

from cpython.datetime cimport datetime
from decimal import Decimal

from inv_trader.model.events cimport AccountEvent
from inv_trader.model.objects import Money


cdef class Account:
    """
    Represents a brokerage account.
    """
    cdef readonly bint initialized
    cdef readonly object id
    cdef readonly object broker
    cdef readonly object account_number
    cdef readonly object currency
    cdef readonly object cash_balance
    cdef readonly object cash_start_day
    cdef readonly object cash_activity_day
    cdef readonly object margin_used_liquidation
    cdef readonly object margin_used_maintenance
    cdef readonly object margin_ratio
    cdef readonly str margin_call_status
    cdef readonly datetime last_updated
    cdef readonly list events

    def __init__(self):
        """
        Initializes a new instance of the Account class.
        """
        self.initialized = False
        self.id = None
        self.broker = None
        self.account_number = None
        self.currency = None
        self.cash_balance = Money.zero()
        self.cash_start_day = Money.zero()
        self.cash_activity_day = Money.zero()
        self.margin_used_liquidation = Money.zero()
        self.margin_used_maintenance = Money.zero()
        self.margin_ratio = Money.zero()
        self.margin_call_status = ""
        self.last_updated = dt.datetime.utcnow()
        self.events = []

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
        :return: The str() string representation of the account.
        """
        return f"Account({str(self.broker)}-{str(self.account_number)})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the account.
        """
        return f"<{str(self)} object at {id(self)}>"

    @property
    def free_equity(self) -> Decimal:
        """
        :return: The accounts free equity after used margin.
        """
        return self._calculate_free_equity()

    cpdef void apply(self, AccountEvent event):
        """
        Applies the given account event to the account.

        :param event: The account event.
        """
        if not self.initialized:
            self.broker = event.broker
            self.account_number = event.account_number
            self.id = event.account_id
            self.currency = event.currency
            self.initialized = True

        self.cash_balance = event.cash_balance
        self.cash_start_day = event.cash_start_day
        self.cash_activity_day = event.cash_activity_day
        self.margin_used_liquidation = event.margin_used_liquidation
        self.margin_used_maintenance = event.margin_used_maintenance
        self.margin_ratio = event.margin_ratio
        self.margin_call_status = event.margin_call_status

        self.events.append(event)
        self.last_updated = event.timestamp

    cdef object _calculate_free_equity(self):
        """
        Calculate the free equity for this account.
        
        :return: The free equity (Decimal).
        """
        margin_used = self.margin_used_maintenance + self.margin_used_liquidation
        return Decimal(max(self.cash_balance - margin_used, 0))
