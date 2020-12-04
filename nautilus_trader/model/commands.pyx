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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport Order


cdef class VenueCommand(Command):
    """
    The base class for all commands relating to a specific venue.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
            self,
            Venue venue not None,
            UUID command_id not None,
            datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `VenueCommand` class.

        Parameters
        ----------
        venue : Venue
            The venue the command relates to.
        command_id : UUID
            The commands identifier.
        command_timestamp : datetime
            The commands timestamp.

        """
        super().__init__(command_id, command_timestamp)

        self.venue = venue


cdef class SubmitOrder(VenueCommand):
    """
    Represents a command to submit the given order.
    """

    def __init__(
            self,
            Venue venue not None,
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
        venue : Venue
            The venue for the command.
        trader_id : TraderId
            The trader identifier for the command.
        account_id : AccountId
            The account identifier for the order.
        strategy_id : StrategyId
            The strategy identifier for the order.
        position_id : PositionId
            The position identifier for the order.
        order : Order
            The order to submit.
        command_id : UUID
            The commands identifier.
        command_timestamp : datetime
            The commands timestamp.

        Raises
        ------
        ValueError
            If venue is not equal to order.symbol.venue.

        """
        Condition.equal(venue, order.symbol.venue, "venue", "order.symbol.venue")
        super().__init__(venue, command_id, command_timestamp)

        self.trader_id = trader_id
        self.account_id = account_id
        self.strategy_id = strategy_id
        self.position_id = position_id
        self.order = order

    def __repr__(self) -> str:
        cdef str position_id_str = "NULL" if self.position_id.is_null() else self.position_id.value
        return (f"{type(self).__name__}("
                f"venue={self.venue.value}, "
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"cl_ord_id={self.order.cl_ord_id.value}, "
                f"position_id={position_id_str}, "
                f"strategy_id={self.strategy_id.value}")


cdef class SubmitBracketOrder(VenueCommand):
    """
    Represents a command to submit a bracket order consisting of parent and child orders.
    """

    def __init__(
            self,
            Venue venue not None,
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
        venue : Venue
            The venue for the command.
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

        Raises
        ------
        ValueError
            If venue is not equal to order.symbol.venue.

        """
        Condition.equal(venue, bracket_order.entry.symbol.venue, "venue", "bracket_order.entry.symbol.venue")
        super().__init__(venue, command_id, command_timestamp)

        self.trader_id = trader_id
        self.account_id = account_id
        self.strategy_id = strategy_id
        self.bracket_order = bracket_order

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"venue={self.venue.value}, "
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"id={self.bracket_order.id.value})")


cdef class ModifyOrder(VenueCommand):
    """
    Represents a command to modify an order with the given modified price.
    """

    def __init__(
            self,
            Venue venue not None,
            TraderId trader_id not None,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            Quantity quantity not None,
            Price price not None,
            UUID command_id not None,
            datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `ModifyOrder` class.

        Parameters
        ----------
        venue : Venue
            The venue for the command.
        trader_id : TraderId
            The trader identifier for the command.
        account_id : AccountId
            The account identifier for the command.
        cl_ord_id : OrderId
            The client order identifier.
        quantity : Quantity
            The quantity for the order (modifying optional).
        price : Price
            The price for the order (modifying optional).
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(venue, command_id, command_timestamp)

        self.trader_id = trader_id
        self.account_id = account_id
        self.cl_ord_id = cl_ord_id
        self.quantity = quantity
        self.price = price

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"venue={self.venue.value}, "
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"cl_ord_id={self.cl_ord_id.value}, "
                f"quantity={self.quantity.to_str()}, "
                f"price={self.price})")


cdef class CancelOrder(VenueCommand):
    """
    Represents a command to cancel an order.
    """

    def __init__(
            self,
            Venue venue not None,
            TraderId trader_id not None,
            AccountId account_id not None,
            ClientOrderId cl_ord_id not None,
            UUID command_id not None,
            datetime command_timestamp not None,
    ):
        """
        Initialize a new instance of the `CancelOrder` class.

        Parameters
        ----------
        venue : Venue
            The venue for the command.
        trader_id : TraderId
            The trader identifier for the command.
        account_id : AccountId
            The account identifier for the command.
        cl_ord_id : OrderId
            The client order identifier.
        command_id : UUID
            The command identifier.
        command_timestamp : datetime
            The command timestamp.

        """
        super().__init__(venue, command_id, command_timestamp)

        self.trader_id = trader_id
        self.account_id = account_id
        self.cl_ord_id = cl_ord_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"venue={self.venue.value}, "
                f"trader_id={self.trader_id.value}, "
                f"account_id={self.account_id.value}, "
                f"cl_ord_id={self.cl_ord_id.value})")
