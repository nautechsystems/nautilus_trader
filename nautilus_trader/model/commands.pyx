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

from libc.stdint cimport int64_t

from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.bracket cimport BracketOrder


cdef class TradingCommand(Command):
    """
    The abstract base class for all trading related commands.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        UUID command_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``TradingCommand`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        instrument_id : InstrumentId
            The instrument identifier for the command.
        command_id : UUID
            The commands identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the command.

        """
        super().__init__(command_id, timestamp_ns)

        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id


cdef class SubmitOrder(TradingCommand):
    """
    Represents a command to submit the given order.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_D_68.html

    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        PositionId position_id not None,
        Order order not None,
        UUID command_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``SubmitOrder`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        position_id : PositionId
            The position identifier for the command (can be NULL).
        order : Order
            The order to submit.
        command_id : UUID
            The commands identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the command.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=order.instrument_id,
            command_id=command_id,
            timestamp_ns=timestamp_ns,
        )

        self.position_id = position_id
        self.order = order

    def __repr__(self) -> str:
        cdef str position_id_str = '' if self.position_id.is_null() else f"position_id={self.position_id.value}, "
        return (f"{type(self).__name__}("
                f"{self.order.status_string_c()}, "
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.order.client_order_id.value}, "
                f"{position_id_str}"
                f"strategy_id={self.strategy_id.value}, "
                f"command_id={self.id})")


cdef class SubmitBracketOrder(TradingCommand):
    """
    Represents a command to submit a bracket order consisting of parent and child orders.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_E_69.html

    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        BracketOrder bracket_order not None,
        UUID command_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``SubmitBracketOrder`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        bracket_order : BracketOrder
            The bracket order to submit.
        command_id : UUID
            The command identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the command.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=bracket_order.instrument_id,
            command_id=command_id,
            timestamp_ns=timestamp_ns,
        )

        self.bracket_order = bracket_order

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_link_id={self.bracket_order.id.value}, "
                f"command_id={self.id})")


cdef class UpdateOrder(TradingCommand):
    """
    Represents a command to change to parameters of an existing order.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html

    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        Quantity quantity not None,
        Price price not None,
        UUID command_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``UpdateOrder`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        instrument_id : InstrumentId
            The instrument identifier for the command.
        client_order_id : VenueOrderId
            The client order identifier to update.
        venue_order_id : VenueOrderId
            The venue order identifier to update.
        quantity : Quantity
            The quantity for the order (update optional).
        price : Price
            The price for the order (update optional).
        command_id : UUID
            The command identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the command.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            timestamp_ns=timestamp_ns,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.quantity = quantity
        self.price = price

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"quantity={self.quantity.to_str()}, "
                f"price={self.price}, "
                f"command_id={self.id})")


cdef class CancelOrder(TradingCommand):
    """
    Represents a command to cancel an order.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_F_70.html

    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        VenueOrderId venue_order_id not None,
        UUID command_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``CancelOrder`` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the command.
        strategy_id : StrategyId
            The strategy identifier for the command.
        instrument_id : InstrumentId
            The instrument identifier for the command.
        client_order_id : ClientOrderId
            The client order identifier to cancel.
        venue_order_id : VenueOrderId
            The venue order identifier to cancel.
        command_id : UUID
            The command identifier.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the command.

        """
        super().__init__(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            command_id=command_id,
            timestamp_ns=timestamp_ns,
        )

        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"trader_id={self.trader_id.value}, "
                f"strategy_id={self.strategy_id.value}, "
                f"instrument_id={self.instrument_id.value}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id.value}, "
                f"command_id={self.id})")
