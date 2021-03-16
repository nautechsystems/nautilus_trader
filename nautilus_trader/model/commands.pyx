# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.bracket cimport BracketOrder


cdef class Routing:
    """
    Represents routing instructions.

    Depending on the broker and intermediary - the command may not be routed to
    the primary/native exchange.
    """
    def __init__(
        self,
        Venue broker=None,
        Venue intermediary=None,
        Venue exchange=None,
    ):
        """
        Initialize a new instance of the `Routing` class.

        Parameters
        ----------
        broker : Venue, optional
            The broker/dealer for routing.
        intermediary : Venue, optional
            The intermediary venue/system/dark pool for routing.
        exchange : Venue, optional
            The primary/native exchange for the instrument.

        Raises
        ------
        ValueError
            If broker, intermediary and exchange are all None.

        """
        Condition.false(
            broker is None and intermediary is None and exchange is None,
            "all routing venues were None",
        )

        self.broker = broker
        self.intermediary = intermediary
        self.exchange = exchange

    def __eq__(self, Routing other) -> bool:
        return self.broker == other.broker \
            and self.intermediary == other.intermediary \
            and self.exchange == other.exchange

    def __ne__(self, Routing other) -> bool:
        return not self == other

    def __hash__(self) -> int:
        return hash((self.broker, self.intermediary, self.exchange))

    def __str__(self) -> str:
        cdef list routing = []
        if self.broker is not None:
            routing.append(self.broker.value)
        if self.intermediary is not None:
            routing.append(self.intermediary.value)
        if self.exchange is not None:
            routing.append(self.exchange.value)
        return "->".join(routing)

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self}')"

    cpdef Venue first(self):
        """
        Return the first routing point.

        Returns
        -------
        Venue

        """
        if self.broker is not None:
            return self.broker
        if self.intermediary is not None:
            return self.intermediary
        else:
            return self.exchange

    @staticmethod
    cdef Routing from_serializable_str_c(str value):
        Condition.valid_string(value, "value")

        cdef list pieces = value.split(',', maxsplit=2)

        if len(pieces) != 3:
            raise ValueError(f"The Routing string value was malformed, was {value}")

        cdef str broker = pieces[0]
        cdef str intermediary = pieces[1]
        cdef str exchange = pieces[2]
        return Routing(
            broker=None if broker == '' else Venue(broker),
            intermediary=None if intermediary == '' else Venue(intermediary),
            exchange=None if exchange == '' else Venue(exchange),
        )

    @staticmethod
    def from_serializable_str(value: str) -> Routing:
        """
        Return `Routing` information parsed from the given string value.
        Must be correctly formatted including three commas.

        Example: "IB,,IDEALPRO" (no intermediary).

        Parameters
        ----------
        value : str
            The routing instructions string value to parse.

        Returns
        -------
        Routing

        """
        return Routing.from_serializable_str_c(value)

    cpdef str to_serializable_str(self):
        """
        Return a serializable string representation of this object.

        Returns
        -------
        str

        """
        cdef str broker = '' if self.broker is None else self.broker.value
        cdef str intermediary = '' if self.intermediary is None else self.intermediary.value
        cdef str primary_exchange = '' if self.exchange is None else self.exchange.value
        return f"{broker},{intermediary},{primary_exchange}"


cdef class TradingCommand(Command):
    """
    The abstract base class for all trading related commands.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        Routing routing not None,
        UUID command_id not None,
        datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `TradingCommand` class.

        Parameters
        ----------
        routing : Routing
            The routing instructions for the command.
        command_id : UUID
            The commands identifier.
        command_timestamp : datetime
            The commands timestamp.

        """
        super().__init__(command_id, command_timestamp)

        self.routing = routing


cdef class SubmitOrder(TradingCommand):
    """
    Represents a command to submit the given order.
    """

    def __init__(
        self,
        Routing routing not None,
        TraderId trader_id not None,
        AccountId account_id not None,
        StrategyId strategy_id not None,
        PositionId position_id not None,
        Order order not None,
        UUID command_id not None,
        datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `SubmitOrder` class.

        Parameters
        ----------
        routing : Routing
            The routing instructions for the command.
        trader_id : TraderId
            The trader identifier for the command.
        account_id : AccountId
            The account identifier for the order.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        position_id : PositionId
            The position identifier associated with the order.
        order : Order
            The order to submit.
        command_id : UUID
            The commands identifier.
        command_timestamp : datetime
            The commands timestamp.

        """
        super().__init__(routing, command_id, command_timestamp)

        self.trader_id = trader_id
        self.account_id = account_id
        self.strategy_id = strategy_id
        self.position_id = position_id
        self.order = order

    def __repr__(self) -> str:
        cdef str position_id_str = '' if self.position_id.is_null() else f"position_id={self.position_id.value}, "
        return (f"{type(self).__name__}("
                f"{self.order.status_string_c()}, "
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"cl_ord_id={self.order.cl_ord_id.value}, "
                f"{position_id_str}"
                f"strategy_id={self.strategy_id.value}, "
                f"command_id={self.id})")


cdef class SubmitBracketOrder(TradingCommand):
    """
    Represents a command to submit a bracket order consisting of parent and child orders.
    """

    def __init__(
        self,
        Routing routing not None,
        TraderId trader_id not None,
        AccountId account_id not None,
        StrategyId strategy_id not None,
        BracketOrder bracket_order not None,
        UUID command_id not None,
        datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `SubmitBracketOrder` class.

        Parameters
        ----------
        routing : Routing
            The routing instructions for the command.
        trader_id : TraderId
            The trader identifier for the command.
        account_id : AccountId
            The account identifier for the command.
        strategy_id : StrategyId
            The strategy identifier to associate with the order.
        bracket_order : BracketOrder
            The bracket order to submit.
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(routing, command_id, command_timestamp)

        self.trader_id = trader_id
        self.account_id = account_id
        self.strategy_id = strategy_id
        self.bracket_order = bracket_order

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"routing={self.routing}, "
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"entry_cl_ord_id={self.bracket_order.entry.cl_ord_id.value}, "
                f"command_id={self.id})")


cdef class AmendOrder(TradingCommand):
    """
    Represents a command to change to parameters of an existing order.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/4.4/msgType_G_71.html

    """

    def __init__(
        self,
        Routing routing not None,
        TraderId trader_id not None,
        AccountId account_id not None,
        ClientOrderId cl_ord_id not None,
        Quantity quantity not None,
        Price price not None,
        UUID command_id not None,
        datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `AmendOrder` class.

        Parameters
        ----------
        routing : Routing
            The routing instructions for the command.
        trader_id : TraderId
            The trader identifier for the command.
        account_id : AccountId
            The account identifier for the command.
        cl_ord_id : OrderId
            The client order identifier.
        quantity : Quantity
            The quantity for the order (amending optional).
        price : Price
            The price for the order (amending optional).
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(routing, command_id, command_timestamp)

        self.trader_id = trader_id
        self.account_id = account_id
        self.cl_ord_id = cl_ord_id
        self.quantity = quantity
        self.price = price

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"routing={self.routing}, "
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"cl_ord_id={self.cl_ord_id.value}, "
                f"quantity={self.quantity.to_str()}, "
                f"price={self.price}, "
                f"command_id={self.id})")


cdef class CancelOrder(TradingCommand):
    """
    Represents a command to cancel an order.
    """

    def __init__(
        self,
        Routing routing not None,
        TraderId trader_id not None,
        AccountId account_id not None,
        ClientOrderId cl_ord_id not None,
        OrderId order_id not None,
        UUID command_id not None,
        datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `CancelOrder` class.

        Parameters
        ----------
        routing : Routing
            The routing instructions for the command.
        trader_id : TraderId
            The trader identifier for the command.
        account_id : AccountId
            The account identifier for the command.
        cl_ord_id : ClientOrderId
            The client order identifier to cancel.
        order_id : OrderId
            The order identifier to cancel.
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(routing, command_id, command_timestamp)

        self.trader_id = trader_id
        self.account_id = account_id
        self.cl_ord_id = cl_ord_id
        self.order_id = order_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"routing={self.routing}, "
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"cl_ord_id={self.cl_ord_id.value}, "
                f"order_id={self.order_id.value}, "
                f"command_id={self.id})")
