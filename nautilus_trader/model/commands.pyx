# -------------------------------------------------------------------------------------------------
# <copyright file="commands.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport ValidString, GUID
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.identifiers cimport TraderId, StrategyId, PositionId, AccountId
from nautilus_trader.model.order cimport Order, AtomicOrder


cdef class AccountInquiry(Command):
    """
    Represents a request for account status.
    """

    def __init__(self,
                 AccountId account_id,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the AccountInquiry class.

        :param account_id: The account identifier for the inquiry.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.account_id = account_id


cdef class SubmitOrder(Command):
    """
    Represents a command to submit the given order.
    """

    def __init__(self,
                 TraderId trader_id,
                 StrategyId strategy_id,
                 PositionId position_id,
                 AccountId account_id,
                 Order order,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the SubmitOrder class.

        :param trader_id: The trader identifier associated with the order.
        :param strategy_id: The strategy identifier associated with the order.
        :param position_id: The position identifier associated with the order.
        :param account_id: The account identifier to submit the order to.
        :param order: The order to submit.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.position_id = position_id
        self.account_id = account_id
        self.order = order

    def __str__(self) -> str:
        """
        :return: The str() string representation of the command.
        """
        return f"{self.__class__.__name__}({self.order})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the command.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class SubmitAtomicOrder(Command):
    """
    Represents a command to submit an atomic order consisting of parent and child orders.
    """

    def __init__(self,
                 TraderId trader_id,
                 StrategyId strategy_id,
                 PositionId position_id,
                 AccountId account_id,
                 AtomicOrder atomic_order,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the SubmitAtomicOrder class.

        :param trader_id: The trader identifier associated with the order.
        :param strategy_id: The strategy identifier to associate with the order.
        :param position_id: The position identifier.
        :param account_id: The account identifier to submit the order to.
        :param atomic_order: The atomic order to submit.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.position_id = position_id
        self.account_id = account_id
        self.atomic_order = atomic_order

    def __str__(self) -> str:
        """
        :return: The str() string representation of the command.
        """
        return f"{self.__class__.__name__}({self.atomic_order})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the command.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class ModifyOrder(Command):
    """
    Represents a command to modify an order with the given modified price.
    """

    def __init__(self,
                 TraderId trader_id,
                 StrategyId strategy_id,
                 AccountId account_id,
                 OrderId order_id,
                 Price modified_price,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the ModifyOrder class.

        :param trader_id: The trader identifier associated with the order.
        :param strategy_id: The strategy identifier associated with the order.
        :param account_id: The account identifier to submit the order to.
        :param order_id: The order identifier.
        :param modified_price: The modified price for the order.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.account_id = account_id
        self.order_id = order_id
        self.modified_price = modified_price


cdef class CancelOrder(Command):
    """
    Represents a command to cancel an order.
    """

    def __init__(self,
                 TraderId trader_id,
                 StrategyId strategy_id,
                 AccountId account_id,
                 OrderId order_id,
                 ValidString cancel_reason,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the CancelOrder class.

        :param trader_id: The trader identifier associated with the order.
        :param strategy_id: The strategy identifier associated with the order.
        :param account_id: The account identifier to submit the order to.
        :param order_id: The order identifier.
        :param cancel_reason: The reason for cancellation.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.account_id = account_id
        self.order_id = order_id
        self.cancel_reason = cancel_reason
