#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="account.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from datetime import datetime
from decimal import Decimal

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.enums import Broker, CurrencyCode
from inv_trader.model.events import AccountEvent
from inv_trader.model.identifiers import AccountId
from inv_trader.model.objects import Money


cdef class Account:
    """
    Represents a brokerage account.
    """
    cdef bint _initialized
    cdef object _id
    cdef object _broker
    cdef object _account_number
    cdef object _currency
    cdef object _cash_balance
    cdef object _cash_start_day
    cdef object _cash_activity_day
    cdef object _margin_used_liquidation
    cdef object _margin_used_maintenance
    cdef object _margin_ratio
    cdef str _margin_call_status
    cdef object _last_updated
    cdef list _events

    def __init__(self):
        """
        Initializes a new instance of the Account class.
        """
        self._initialized = False
        self._id = None
        self._broker = None
        self._account_number = None
        self._currency = None
        self._cash_balance = Money.zero()
        self._cash_start_day = Money.zero()
        self._cash_activity_day = Money.zero()
        self._margin_used_liquidation = Money.zero()
        self._margin_used_maintenance = Money.zero()
        self._margin_ratio = Money.zero()
        self._margin_call_status = ""
        self._last_updated = datetime.utcnow()
        self._events = []

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self._id == other._id
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash((self._broker, self._account_number))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the account.
        """
        return f"Account({str(self._broker)}-{str(self._account_number)})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the account.
        """
        return f"<{str(self)} object at {id(self)}>"

    @property
    def initialized(self) -> bool:
        """
        :return: A value indicating whether the account is initialized (from an account event).
        """
        return self._initialized

    @property
    def broker(self) -> Broker or None:
        """
        :return: The brokerage the account belongs to (returns None if not initialized).
        """
        return self._broker

    @property
    def number(self) -> str or None:
        """
        :return: The account number of the account (returns None if not initialized).
        """
        return self._account_number

    @property
    def id(self) -> AccountId or None:
        """
        :return: The account identifier (returns None if not initialized).
        """
        return self._id

    @property
    def currency(self) -> CurrencyCode or None:
        """
        :return: The account currency (returns None if not initialized).
        """
        return self._currency

    @property
    def free_equity(self) -> Decimal:
        """
        :return: The accounts free equity after used margin.
        """
        return self._calculate_free_equity()

    @property
    def cash_balance(self) -> Decimal:
        """
        :return: The accounts cash balance.
        """
        return self._cash_balance

    @property
    def cash_start_day(self) -> Decimal:
        """
        :return: The accounts cash balance at the start of the trading day.
        """
        return self._cash_start_day

    @property
    def cash_activity_day(self) -> Decimal:
        """
        :return: The account activity for the day.
        """
        return self._cash_activity_day

    @property
    def margin_used_liquidation(self) -> Decimal:
        """
        :return: The accounts liquidation margin used.
        """
        return self._margin_used_liquidation

    @property
    def margin_used_maintenance(self) -> Decimal:
        """
        :return: The accounts maintenance margin used.
        """
        return self._margin_used_maintenance

    @property
    def margin_ratio(self) -> Decimal:
        """
        :return: The accounts margin ratio.
        """
        return self._margin_ratio

    @property
    def margin_call_status(self) -> str:
        """
        :return: The accounts margin call status.
        """
        return self._margin_call_status

    @property
    def last_updated(self) -> datetime:
        """
        :return: The time the account was last updated.
        """
        return self._last_updated

    cpdef void apply(self, event: AccountEvent):
        """
        Applies the given account event to the account.

        :param event: The account event.
        """
        Precondition.type(event, AccountEvent, 'event')

        if not self._initialized:
            self._broker = event.broker
            self._account_number = event.account_number
            self._id = event.account_id
            self._currency = event.currency
            self._initialized = True

        self._cash_balance = event.cash_balance
        self._cash_start_day = event.cash_start_day
        self._cash_activity_day = event.cash_activity_day
        self._margin_used_liquidation = event.margin_used_liquidation
        self._margin_used_maintenance = event.margin_used_maintenance
        self._margin_ratio = event.margin_ratio
        self._margin_call_status = event.margin_call_status

        self._events.append(event)
        self._last_updated = event.timestamp

    cdef object _calculate_free_equity(self):
        """
        Calculate the free equity for this account.
        
        :return: The free equity (Decimal).
        """
        margin_used = self._margin_used_maintenance + self._margin_used_liquidation
        return Decimal(max(self._cash_balance - margin_used, 0))
