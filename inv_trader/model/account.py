#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="account.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from datetime import datetime
from decimal import Decimal

from inv_trader.core.typing import typechecking
from inv_trader.model.events import AccountEvent


class Account:
    """
    Represents a brokerage account.
    """

    @typechecking
    def __init__(self):
        """
        Initializes a new instance of the Account class.
        """
        self._initialized = False
        self._cash_balance = Decimal('0')
        self._cash_start_day = Decimal('0')
        self._cash_activity_day = Decimal('0')
        self._margin_used_liquidation = Decimal('0')
        self._margin_used_maintenance = Decimal('0')
        self._margin_ratio = Decimal('0')
        self._margin_call_status = ""
        self._last_updated = datetime.utcnow()
        self._events = []

    def initialized(self) -> bool:
        """
        :return: A value indicating whether the account is initialized (from an account report).
        """
        return self._initialized

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
        return self._cash_start_day

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

    @typechecking
    def apply(self, event: AccountEvent):
        """
        Applies the given account event to the account.

        :param event: The account event.
        """
        self._cash_balance = event.cash_balance
        self._cash_start_day = event.cash_start_day
        self._cash_activity_day = event.cash_activity_day
        self._margin_used_liquidation = event.margin_used_liquidation
        self._margin_used_maintenance = event.margin_used_maintenance
        self._margin_ratio = event.margin_ratio
        self._margin_call_status = event.margin_call_status

        self._initialized = True
        self._events.append(event)
        self._last_updated = event.event_timestamp

    def _calculate_free_equity(self) -> Decimal:
        margin_used = self._margin_used_maintenance + self._margin_used_liquidation
        return Decimal(max(self._cash_balance - margin_used, 0))
