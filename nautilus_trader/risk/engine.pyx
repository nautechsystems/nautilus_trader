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

"""
The `RiskEngine` is responsible for global strategy and portfolio risk within the platform.

Alternative implementations can be written on top of the generic engine.
"""

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.trading_state cimport TradingState
from nautilus_trader.model.c_enums.trading_state cimport TradingStateParser
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport TradingCommand
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class RiskEngine(Component):
    """
    Provides a high-performance risk engine.
    """

    def __init__(
        self,
        ExecutionEngine exec_engine not None,
        Portfolio portfolio not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``RiskEngine`` class.

        Parameters
        ----------
        exec_engine : ExecutionEngine
            The execution engine for the engine.
        portfolio : Portfolio
            The portfolio for the engine.
        cache : Cache
            The cache for the engine.
        clock : Clock
            The clock for the engine.
        logger : Logger
            The logger for the engine.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        super().__init__(clock, logger, name="RiskEngine")

        if config:
            self._log.info(f"Config: {config}.")

        self._portfolio = portfolio
        self._exec_engine = exec_engine

        self.trader_id = exec_engine.trader_id
        self.cache = cache
        self.trading_state = TradingState.ACTIVE  # Start active by default
        self._log_state()

        # Counters
        self.command_count = 0
        self.event_count = 0

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void execute(self, Command command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : Command
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, Event event) except *:
        """
        Process the given event.

        Parameters
        ----------
        event : Event
            The event to process.

        """
        Condition.not_none(event, "event")

        self._handle_event(event)

    cpdef void set_trading_state(self, TradingState state) except *:
        """
        Set the trading state for the engine.

        Parameters
        ----------
        state : TradingState
            The state to set.

        """
        self.trading_state = state
        self._log_state()

    cdef void _log_state(self) except *:
        cdef LogColor color = LogColor.BLUE
        if self.trading_state == TradingState.REDUCING:
            color = LogColor.YELLOW
        elif self.trading_state == TradingState.HALTED:
            color = LogColor.RED
        self._log.info(f"State is {TradingStateParser.to_str(self.trading_state)}.", color=color)

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *:
        pass  # Optionally override in subclass

    cpdef void _on_stop(self) except *:
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        # Do nothing else for now
        self._on_start()

    cpdef void _stop(self) except *:
        # Do nothing else for now
        self._on_stop()

    cpdef void _reset(self) except *:
        self.command_count = 0
        self.event_count = 0

    cpdef void _dispose(self) except *:
        pass
        # Nothing to dispose for now

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef void _execute_command(self, Command command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        if isinstance(command, TradingCommand):
            self._handle_trading_command(command)

    cdef void _handle_trading_command(self, TradingCommand command) except *:
        if isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, SubmitBracketOrder):
            self._handle_submit_bracket_order(command)
        elif isinstance(command, UpdateOrder):
            self._handle_update_order(command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cdef void _handle_submit_order(self, SubmitOrder command) except *:
        # Check duplicate ID
        if self.cache.order_exists(command.order.client_order_id):
            # Avoids duplicate IDs in cache / database
            self._log.error(
                f"Cannot submit order: "
                f"{repr(command.order.client_order_id)} already exists.",
            )
            return  # Denied

        # Cache order
        # *** Do not complete additional risk checks before here ***
        self.cache.add_order(command.order, command.position_id)

        # Check position exists
        if command.position_id.not_null() and not self.cache.position_exists(command.position_id):
            self._deny_order(
                command.order.client_order_id,
                f"Cannot submit order: "
                f"{repr(command.position_id)} does not exist",
            )
            return  # Denied

        # Get instrument for order
        cdef Instrument instrument = self._exec_engine.cache.instrument(command.order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot check order: "
                f"no instrument found for {command.order.instrument_id}",
            )
            self._deny_order(command.order.client_order_id, "No instrument found")
            return  # Denied

        ########################################################################
        # Validation checks
        ########################################################################
        cdef list validation_msgs = []
        self._check_order_values(instrument, command.order, validation_msgs)
        if validation_msgs:
            # Deny order
            self._deny_order(command.order.client_order_id, ",".join(validation_msgs))
            return  # Denied

        ########################################################################
        # Risk checks
        ########################################################################
        cdef list risk_msgs = self._check_order_risk(instrument, command.order)
        if self.trading_state == TradingState.HALTED:
            risk_msgs.append("TRADING_HALTED")
        elif self.trading_state == TradingState.REDUCING:
            pass  # TODO: Implement

        if risk_msgs:
            # Deny order
            self._deny_order(command.order.client_order_id, ",".join(risk_msgs))
            return  # Denied

        self._exec_engine.execute(command)

    cdef void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        cdef Order entry = command.bracket_order.entry
        cdef StopMarketOrder stop_loss = command.bracket_order.stop_loss
        cdef LimitOrder take_profit = command.bracket_order.take_profit

        # Check IDs for duplicates
        if self.cache.order_exists(entry.client_order_id):
            self._check_duplicate_ids(command.bracket_order)
            return  # Duplicate (do not add to cache)
        if self.cache.order_exists(stop_loss.client_order_id):
            self._check_duplicate_ids(command.bracket_order)
            return  # Duplicate (do not add to cache)
        if self.cache.order_exists(take_profit.client_order_id):
            self._check_duplicate_ids(command.bracket_order)
            return  # Duplicate (do not add to cache)

        # Cache all orders
        # *** Do not complete additional risk checks before here ***
        self.cache.add_order(command.bracket_order.entry, PositionId.null_c())
        self.cache.add_order(command.bracket_order.stop_loss, PositionId.null_c())
        self.cache.add_order(command.bracket_order.take_profit, PositionId.null_c())

        # Get instrument for orders
        cdef Instrument instrument = self._exec_engine.cache.instrument(command.instrument_id)
        if instrument is None:
            reason = f"no instrument found for {command.instrument_id}"
            self._log.error(f"Cannot check order: {reason}")
            self._deny_order(entry, reason)
            self._deny_order(stop_loss, reason)
            self._deny_order(take_profit, reason)
            return  # Denied

        ########################################################################
        # Validation checks
        ########################################################################
        cdef list validation_msgs = []
        self._check_order_values(instrument, entry, validation_msgs)
        self._check_order_values(instrument, stop_loss, validation_msgs)
        self._check_order_values(instrument, take_profit, validation_msgs)
        if validation_msgs:
            # Deny order
            reasons = ",".join(validation_msgs)
            self._deny_order(entry.client_order_id, reasons)
            self._deny_order(stop_loss.client_order_id, reasons)
            self._deny_order(take_profit.client_order_id, reasons)
            return  # Denied

        ########################################################################
        # Risk checks
        ########################################################################
        cdef list risk_msgs = self._check_bracket_order_risk(instrument, command.bracket_order)
        if self.trading_state == TradingState.HALTED:
            risk_msgs.append("TRADING_HALTED")
        elif self.trading_state == TradingState.REDUCING:
            pass  # TODO: Implement

        if risk_msgs:
            # Deny order
            reasons = ",".join(risk_msgs)
            self._deny_order(entry.client_order_id, reasons)
            self._deny_order(stop_loss.client_order_id, reasons)
            self._deny_order(take_profit.client_order_id, reasons)
            return  # Denied

        self._exec_engine.execute(command)

    cdef void _handle_update_order(self, UpdateOrder command) except *:
        # Validate command
        if self.cache.is_order_completed(command.client_order_id):
            self._log.warning(f"Cannot update order: "
                              f"{repr(command.client_order_id)} already completed.")
            return  # Invalid command

        # TODO(cs): Validate price, quantity
        self._exec_engine.execute(command)

    cdef void _handle_cancel_order(self, CancelOrder command) except *:
        # Validate command
        if self.cache.is_order_completed(command.client_order_id):
            self._log.warning(f"Cannot cancel order: "
                              f"{repr(command.client_order_id)} already completed.")
            return  # Invalid command

        self._exec_engine.execute(command)

# -- RISK CHECKS -----------------------------------------------------------------------------------

    cdef void _check_duplicate_ids(self, BracketOrder bracket_order):
        cdef ClientOrderId entry_id = bracket_order.entry.client_order_id
        cdef ClientOrderId stop_loss_id = bracket_order.stop_loss.client_order_id
        cdef ClientOrderId take_profit_id = None
        if bracket_order.take_profit:
            take_profit_id = bracket_order.take_profit.client_order_id

        cdef list error_msgs = []

        # Check entry ----------------------------------------------------------
        if self.cache.order_exists(entry_id):
            error_msgs.append(f"Duplicate {repr(entry_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.entry, PositionId.null_c())
            self._deny_order(
                entry_id,
                "Duplicate ClientOrderId in bracket.",
            )
        # Check stop-loss ------------------------------------------------------
        if self.cache.order_exists(stop_loss_id):
            error_msgs.append(f"Duplicate {repr(stop_loss_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.stop_loss, PositionId.null_c())
            self._deny_order(
                stop_loss_id,
                "Duplicate ClientOrderId in bracket.",
            )
        # Check take-profit ----------------------------------------------------
        if take_profit_id is not None and self.cache.order_exists(take_profit_id):
            error_msgs.append(f"Duplicate {repr(take_profit_id)}")
        else:
            # Add to cache to be able to invalidate
            self.cache.add_order(bracket_order.take_profit, PositionId.null_c())
            self._deny_order(
                take_profit_id,
                "Duplicate ClientOrderId in bracket.",
            )

        # Finally log error
        self._log.error(f"Cannot submit BracketOrder: {', '.join(error_msgs)}")

    cdef list _check_order_values(
        self, Instrument instrument,
        Order order,
        list msgs,
    ):
        ########################################################################
        # Check size
        ########################################################################
        if order.quantity.precision > instrument.size_precision:
            msgs.append(
                f"quantity {order.quantity} invalid: "
                f"precision {order.quantity.precision} > {instrument.size_precision}",
            )
        if instrument.max_quantity and order.quantity > instrument.max_quantity:
            msgs.append(
                f"quantity {order.quantity} invalid: "
                f"> maximum trade size of {instrument.max_quantity}",
            )
        if instrument.min_quantity and order.quantity < instrument.min_quantity:
            msgs.append(
                f"quantity {order.quantity} invalid: "
                f"< minimum trade size of {instrument.min_quantity}",
            )

        ########################################################################
        # Check price
        ########################################################################
        if (
            order.type == OrderType.LIMIT
            or order.type == OrderType.STOP_MARKET
            or order.type == OrderType.STOP_LIMIT
        ):
            if order.price.precision > instrument.price_precision:
                msgs.append(
                    f"price {order.price} invalid: "
                    f"precision {order.price.precision} > {instrument.price_precision}")
            if instrument.asset_type != AssetType.OPTION and order.price <= 0:
                msgs.append(f"price {order.price} invalid: not positive")

        ########################################################################
        # Check trigger
        ########################################################################
        if order.type == OrderType.STOP_LIMIT:
            if order.trigger.precision > instrument.price_precision:
                msgs.append(
                    f"trigger price {order.trigger} invalid: "
                    f"precision {order.trigger.precision} > {instrument.price_precision}")
            if instrument.asset_type != AssetType.OPTION:
                if order.trigger <= 0:
                    msgs.append(f"trigger price {order.trigger} invalid: not positive")

        # TODO(cs): Check notional limits

        return msgs

    cdef list _check_order_risk(self, Instrument instrument, Order order):
        # TODO(cs): Pre-trade risk checks
        return []

    cdef list _check_bracket_order_risk(self, Instrument instrument, BracketOrder bracket_order):
        # TODO(cs): Pre-trade risk checks
        return []

# -- EVENT GENERATION ------------------------------------------------------------------------------

    cdef void _deny_order(self, ClientOrderId client_order_id, str reason) except *:
        # Generate event
        cdef OrderDenied denied = OrderDenied(
            client_order_id=client_order_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._exec_engine.process(denied)

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1
