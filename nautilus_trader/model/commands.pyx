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
                 TraderId trader_id,
                 AccountId account_id,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the AccountInquiry class.

        :param trader_id: The trader_id.
        :param account_id: The account_id for the inquiry.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.account_id = account_id

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"trader_id={self.account_id.value}, "
                f"account_id={self.account_id.value})")


cdef class SubmitOrder(Command):
    """
    Represents a command to submit the given order.
    """

    def __init__(self,
                 TraderId trader_id,
                 AccountId account_id,
                 StrategyId strategy_id,
                 PositionId position_id,
                 Order order,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the SubmitOrder class.

        :param trader_id: The trader_id associated with the order.
        :param account_id: The account_id to submit the order to.
        :param strategy_id: The strategy_id associated with the order.
        :param position_id: The position_id associated with the order.
        :param order: The order to submit.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.account_id = account_id
        self.strategy_id = strategy_id
        self.position_id = position_id
        self.order = order

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"position_id={self.position_id.value}, "
                f"order_id={self.order.id.value})")


cdef class SubmitAtomicOrder(Command):
    """
    Represents a command to submit an atomic order consisting of parent and child orders.
    """

    def __init__(self,
                 TraderId trader_id,
                 AccountId account_id,
                 StrategyId strategy_id,
                 PositionId position_id,
                 AtomicOrder atomic_order,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the SubmitAtomicOrder class.

        :param trader_id: The trader_id associated with the order.
        :param account_id: The account_id to submit the order to.
        :param strategy_id: The strategy_id to associate with the order.
        :param position_id: The position_id to associate with the order.
        :param atomic_order: The atomic order to submit.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.account_id = account_id
        self.strategy_id = strategy_id
        self.position_id = position_id
        self.atomic_order = atomic_order

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"position_id={self.position_id.value}, "
                f"order_id={self.atomic_order.id.value})")


cdef class ModifyOrder(Command):
    """
    Represents a command to modify an order with the given modified price.
    """

    def __init__(self,
                 TraderId trader_id,
                 AccountId account_id,
                 OrderId order_id,
                 Quantity modified_quantity,
                 Price modified_price,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the ModifyOrder class.

        :param trader_id: The trader_id for the command.
        :param account_id: The account_id for the command.
        :param order_id: The order_id.
        :param modified_price: The modified quantity for the order.
        :param modified_price: The modified price for the order.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.account_id = account_id
        self.order_id = order_id
        self.modified_quantity = modified_quantity
        self.modified_price = modified_price

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"order_id={self.order_id.value}, "
                f"modified_quantity={self.modified_quantity}, "
                f"modified_price={self.modified_price})")


cdef class CancelOrder(Command):
    """
    Represents a command to cancel an order.
    """

    def __init__(self,
                 TraderId trader_id,
                 AccountId account_id,
                 OrderId order_id,
                 ValidString cancel_reason,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the CancelOrder class.

        :param trader_id: The trader_id for the command.
        :param account_id: The account_id for the command.
        :param order_id: The order_id.
        :param cancel_reason: The reason for cancellation.
        :param command_id: The command identifier.
        :param command_timestamp: The command timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.trader_id = trader_id
        self.account_id = account_id
        self.order_id = order_id
        self.cancel_reason = cancel_reason

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return (f"{self.__class__.__name__}("
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"order_id={self.order_id.value}, "
                f"cancel_reason={self.cancel_reason})")
