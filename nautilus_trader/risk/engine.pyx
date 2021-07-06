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

from decimal import Decimal

from cpython.datetime cimport timedelta

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.throttler cimport Throttler
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.trading_state cimport TradingState
from nautilus_trader.model.c_enums.trading_state cimport TradingStateParser
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport TradingCommand
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class RiskEngine(Component):
    """
    Provides a high-performance risk engine.

    The `RiskEngine` is responsible for global strategy and portfolio risk
    within the platform. This includes both pre-trade risk checks and post-trade
    risk monitoring.

    Configuration
    -------------
    The following configuration options are possible.

    - bypass: If True then all risk checks are bypassed (will still check for duplicate IDs).
    - max_order_rate: tuple(int, timedelta). Default=(10, timedelta(seconds=1)).
    - max_notional_per_order: { str: Decimal }. Default = {}.

    TradingStates
    -------------
    - ACTIVE (trading is enabled).
    - REDUCING (only new orders or updates which reduce an open position are allowed).
    - HALTED (all trading commands except cancels are denied).

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
        super().__init__(
            clock=clock,
            logger=logger,
            name="RiskEngine",
        )

        self._portfolio = portfolio
        self._exec_engine = exec_engine

        self.trader_id = exec_engine.trader_id
        self.cache = cache
        self.trading_state = TradingState.ACTIVE  # Start active by default
        self.is_bypassed = config.get("bypass", False)
        self._log_state()

        # Counters
        self.command_count = 0
        self.event_count = 0

        # Throttlers
        config_max_order_rate = config.get("max_order_rate")
        if config_max_order_rate is None:
            order_rate_limit = 100
            order_rate_interval = timedelta(seconds=1)
        else:
            order_rate_limit = config_max_order_rate[0]
            order_rate_interval = config_max_order_rate[1]

        self._order_throttler = Throttler(
            name="ORDER_RATE",
            limit=order_rate_limit,
            interval=order_rate_interval,
            output_send=self._send_command,
            output_drop=self._deny_new_order,
            clock=clock,
            logger=logger,
        )

        self._log.info(
            f"Set MAX_ORDER_RATE: {order_rate_limit} / {order_rate_interval}.",
            color=LogColor.BLUE,
        )

        # Risk settings
        self._max_notional_per_order = {}

        # Configure
        self._initialize_risk_checks(config)

    cdef void _initialize_risk_checks(self, dict config) except *:
        cdef dict max_notional_config = config.get("max_notional_per_order", {})
        for instrument_id, value in max_notional_config.items():
            self.set_max_notional_per_order(InstrumentId.from_str_c(instrument_id), value)

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
        self._log.info(
            f"TradingState is {TradingStateParser.to_str(self.trading_state)}.",
            color=color,
        )

        if self.is_bypassed:
            self._log.info(
                "PRE-TRADE RISK CHECKS BYPASSED. This is not advisable for live trading.",
                color=LogColor.RED,
            )

    cpdef void set_max_notional_per_order(self, InstrumentId instrument_id, new_value) except *:
        """
        Set the maximum notional value per order for the given instrument ID.

        Passing a new_value of ``None`` will disable the pre-trade risk max
        notional check.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the max notional.
        new_value : integer, float, string or Decimal
            The max notional value to set.

        Raises
        ------
        decimal.InvalidOperation
            If new_value not a valid input for decimal.Decimal.
        ValueError
            If new_value is not None and not positive.

        """
        if new_value is not None:
            new_value = Decimal(new_value)
            Condition.type(new_value, Decimal, "new_value")
            Condition.positive(new_value, "new_value")

        old_value: Decimal = self._max_notional_per_order.get(instrument_id)
        self._max_notional_per_order[instrument_id] = new_value

        cdef str new_value_str = f"{new_value:,}" if new_value is not None else str(None)
        self._log.info(
            f"Set MAX_NOTIONAL_PER_ORDER: {instrument_id} {new_value_str}.",
            color=LogColor.BLUE,
        )

# -- RISK SETTINGS ---------------------------------------------------------------------------------

    cpdef tuple max_order_rate(self):
        """
        Return the current maximum order rate limit setting.

        Returns
        -------
        (int, timedelta)
            The limit per timedelta interval.

        """
        return (
            self._order_throttler.limit,
            self._order_throttler.interval,
        )

    cpdef dict max_notionals_per_order(self):
        """
        Return the current maximum notionals per order settings.

        Returns
        -------
        dict[InstrumentId, Decimal]

        """
        return self._max_notional_per_order.copy()

    cpdef object max_notional_per_order(self, InstrumentId instrument_id):
        """
        Return the current maximum notional per order for the given instrument ID.

        Returns
        -------
        Decimal or None

        """
        return self._max_notional_per_order.get(instrument_id)

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
        # Check IDs for duplicate
        if not self._check_order_id(command.order):
            self._deny_command(
                command=command,
                reason=f"Duplicate {repr(command.order.client_order_id)}")
            return  # Denied

        # Cache order
        self.cache.add_order(command.order, command.position_id)

        # Check position exists
        if command.position_id.not_null() and not self.cache.position_exists(command.position_id):
            self._deny_command(
                command=command,
                reason=f"{repr(command.position_id)} does not exist",
            )
            return  # Denied

        if self.is_bypassed:
            # Perform no further risk checks or throttling
            self._exec_engine.execute(command)
            return

        # Get instrument for order
        cdef Instrument instrument = self._exec_engine.cache.instrument(command.order.instrument_id)
        if instrument is None:
            self._deny_command(
                command=command,
                reason=f"no instrument found for {command.instrument_id}",
            )
            return  # Denied

        ########################################################################
        # Pre-trade order checks
        ########################################################################
        if not self._check_order(instrument, command.order):
            return  # Denied

        self._execution_gateway(instrument, command, order=command.order)

    cdef void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        cdef Order entry = command.bracket_order.entry
        cdef StopMarketOrder stop_loss = command.bracket_order.stop_loss
        cdef LimitOrder take_profit = command.bracket_order.take_profit

        # Check IDs for duplicates
        if not self._check_order_id(entry):
            self._deny_command(
                command=command,
                reason=f"Duplicate {repr(entry.client_order_id)}")
            return  # Denied
        if not self._check_order_id(stop_loss):
            self._deny_command(
                command=command,
                reason=f"Duplicate {repr(stop_loss.client_order_id)}")
            return  # Denied
        if not self._check_order_id(take_profit):
            self._deny_command(
                command=command,
                reason=f"Duplicate {repr(take_profit.client_order_id)}")
            return  # Denied

        # Cache all orders
        self.cache.add_order(entry, PositionId.null_c())
        self.cache.add_order(stop_loss, PositionId.null_c())
        self.cache.add_order(take_profit, PositionId.null_c())

        if self.is_bypassed:
            # Perform no further risk checks or throttling
            self._exec_engine.execute(command)
            return

        # Get instrument for orders
        cdef Instrument instrument = self._exec_engine.cache.instrument(command.instrument_id)
        if instrument is None:
            self._deny_command(
                command=command,
                reason=f"no instrument found for {command.instrument_id}",
            )
            return  # Denied

        ########################################################################
        # Pre-trade order(s) checks
        ########################################################################
        if not self._check_order(instrument, entry):
            return  # Denied
        if not self._check_order(instrument, stop_loss):
            return  # Denied
        if not self._check_order(instrument, take_profit):
            return  # Denied

        self._execution_gateway(instrument, command, order=entry)

    cdef void _handle_update_order(self, UpdateOrder command) except *:
        ########################################################################
        # Validate command
        ########################################################################
        if self.cache.is_order_completed(command.client_order_id):
            self._deny_command(
                command=command,
                reason=f"{repr(command.client_order_id)} already completed",
            )
            return  # Denied

        # Get instrument for orders
        cdef Instrument instrument = self._exec_engine.cache.instrument(command.instrument_id)
        if instrument is None:
            self._deny_command(
                command=command,
                reason=f"no instrument found for {command.instrument_id}",
            )
            return  # Denied

        cdef str risk_msg = None

        # Check price
        risk_msg = self._check_price(instrument, command.price)
        if risk_msg:
            self._deny_command(command=command, reason=risk_msg)
            return  # Denied

        # Check trigger
        risk_msg = self._check_price(instrument, command.trigger)
        if risk_msg:
            self._deny_command(command=command, reason=risk_msg)
            return  # Denied

        # Check quantity
        risk_msg = self._check_quantity(instrument, command.quantity)
        if risk_msg:
            self._deny_command(command=command, reason=risk_msg)
            return  # Denied

        # Get order relating to update
        cdef Order order = self.cache.order(command.client_order_id)
        if order is None:
            self._deny_command(
                command=command,
                reason=f"{command.client_order_id} not found in cache",
            )
            return  # Denied

        # Check TradingState
        if self.trading_state == TradingState.HALTED:
            self._deny_command(
                command=command,
                reason="TradingState is HALTED",
            )
            return  # Denied
        elif self.trading_state == TradingState.REDUCING:
            if command.quantity and command.quantity > order.quantity:
                if order.is_buy_c() and self._portfolio.is_net_long(instrument.id):
                    self._deny_command(
                        command=command,
                        reason="TradingState is REDUCING and update will increase exposure",
                    )
                    return  # Denied
                elif order.is_sell_c() and self._portfolio.is_net_short(instrument.id):
                    self._deny_command(
                        command=command,
                        reason="TradingState is REDUCING and update will increase exposure",
                    )
                    return  # Denied

        # All checks passed: send for execution
        self._exec_engine.execute(command)

    cdef void _handle_cancel_order(self, CancelOrder command) except *:
        ########################################################################
        # Validate command
        ########################################################################
        if self.cache.is_order_completed(command.client_order_id):
            self._deny_command(
                command=command,
                reason=f"{repr(command.client_order_id)} already completed",
            )
            return  # Denied

        # All checks passed: send for execution
        self._exec_engine.execute(command)

# -- PRE-TRADE CHECKS ------------------------------------------------------------------------------

    cdef bint _check_order_id(self, Order order) except *:
        if order is None or not self.cache.order_exists(order.client_order_id):
            return True  # Check passed
        else:
            return False  # Check failed (duplicate ID)

    cdef bint _check_order(self, Instrument instrument, Order order) except *:
        ########################################################################
        # Validation checks
        ########################################################################
        if not self._check_order_price(instrument, order):
            return False  # Denied
        if not self._check_order_quantity(instrument, order):
            return False  # Denied

        ########################################################################
        # Risk checks
        ########################################################################
        if not self._check_order_risk(instrument, order):
            return False  # Denied

        return True  # Check passed

    cdef bint _check_order_quantity(self, Instrument instrument, Order order) except *:
        cdef str risk_msg = self._check_quantity(instrument, order.quantity)
        if risk_msg:
            self._deny_order(order=order, reason=risk_msg)
            return False  # Denied

        return True  # Passed

    cdef bint _check_order_price(self, Instrument instrument, Order order) except *:
        ########################################################################
        # Check price
        ########################################################################
        cdef str risk_msg = None
        if (
            order.type == OrderType.LIMIT
            or order.type == OrderType.STOP_MARKET
            or order.type == OrderType.STOP_LIMIT
        ):
            risk_msg = self._check_price(instrument, order.price)
            if risk_msg:
                self._deny_order(order=order, reason=risk_msg)
                return False  # Denied

        ########################################################################
        # Check trigger
        ########################################################################
        if order.type == OrderType.STOP_LIMIT:
            risk_msg = self._check_price(instrument, order.trigger)
            if risk_msg:
                self._deny_order(order=order, reason=f"trigger {risk_msg}")
                return False  # Denied

        return True  # Passed

    cdef bint _check_order_risk(self, Instrument instrument, Order order) except *:
        max_notional = self._max_notional_per_order.get(order.instrument_id)
        if max_notional is None:
            return True  # No check

        if order.type == OrderType.MARKET:
            # Determine entry price
            last = self.cache.quote_tick(instrument.id)
            if last is None:
                self._deny_order(
                    order=order,
                    reason=f"No market to check MAX_NOTIONAL_PER_ORDER",
                )
                return False  # Denied
            if order.side == OrderSide.BUY:
                price = last.ask
            else:  # order.side == OrderSide.SELL
                price = last.bid
        else:
            price = order.price

        notional: Decimal = instrument.notional_value(order.quantity, price).as_decimal()
        if notional > max_notional:
            self._deny_order(
                order=order,
                reason=f"Exceeds MAX_NOTIONAL_PER_ORDER of {max_notional:,} @ {notional:,}",
            )
            return False  # Denied

        # TODO(cs): Additional pre-trade risk checks
        return True  # Passed

    cdef str _check_price(self, Instrument instrument, Price price):
        if price is None:
            # Nothing to check
            return None
        if price.precision > instrument.price_precision:
            # Check failed
            return f"price {price} invalid (precision {price.precision} > {instrument.price_precision})"
        if instrument.asset_type != AssetType.OPTION:
            if price.as_decimal() <= 0:
                # Check failed
                return f"price {price} invalid (not positive)"

    cdef str _check_quantity(self, Instrument instrument, Quantity quantity):
        if quantity is None:
            # Nothing to check
            return None
        if quantity.precision > instrument.size_precision:
            # Check failed
            return f"quantity {quantity.to_str()} invalid (precision {quantity.precision} > {instrument.size_precision})"
        if instrument.max_quantity and quantity > instrument.max_quantity:
            # Check failed
            return f"quantity {quantity.to_str()} invalid (> maximum trade size of {instrument.max_quantity})"
        if instrument.min_quantity and quantity < instrument.min_quantity:
            # Check failed
            return f"quantity {quantity.to_str()} invalid (< minimum trade size of {instrument.min_quantity})"

# -- DENIALS ---------------------------------------------------------------------------------------

    cdef void _deny_command(self, TradingCommand command, str reason) except *:
        if isinstance(command, SubmitOrder):
            self._deny_order(command.order, reason=reason)
        elif isinstance(command, SubmitBracketOrder):
            self._deny_bracket_order(command.bracket_order, reason=reason)
        elif isinstance(command, UpdateOrder):
            self._log.error(f"UpdateOrder DENIED: {reason}.")
        elif isinstance(command, CancelOrder):
            self._log.error(f"CancelOrder DENIED: {reason}.")

    cpdef _deny_new_order(self, TradingCommand command):
        if isinstance(command, SubmitOrder):
            self._deny_order(command.order, reason="Exceeded MAX_ORDER_RATE")
        elif isinstance(command, SubmitBracketOrder):
            self._deny_bracket_order(command.bracket_order, reason="Exceeded MAX_ORDER_RATE")

    cdef void _deny_order(self, Order order, str reason) except *:
        self._log.error(f"SubmitOrder DENIED: {reason}.")

        if order is None:
            # Nothing to deny
            return

        if order.state_c() != OrderState.INITIALIZED:
            # Already denied or duplicated (INITIALIZED -> DENIED only valid state transition)
            return

        if not self.cache.order_exists(order.client_order_id):
            self.cache.add_order(order, PositionId.null_c())

        # Generate event
        cdef OrderDenied denied = OrderDenied(
            client_order_id=order.client_order_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._exec_engine.process(denied)

    cdef void _deny_bracket_order(self, BracketOrder bracket_order, str reason) except *:
        self._deny_order(order=bracket_order.entry, reason=reason)
        self._deny_order(order=bracket_order.stop_loss, reason=reason)
        self._deny_order(order=bracket_order.take_profit, reason=reason)

# -- EGRESS ----------------------------------------------------------------------------------------

    cdef void _execution_gateway(self, Instrument instrument, TradingCommand command, Order order):
        # Check TradingState
        if self.trading_state == TradingState.HALTED:
            self._deny_bracket_order(
                bracket_order=command.bracket_order,
                reason="TradingState.HALTED",
            )
            return  # Denied
        elif self.trading_state == TradingState.REDUCING:
            if order.is_buy_c() and self._portfolio.is_net_long(instrument.id):
                self._deny_command(
                    command=command,
                    reason=f"BUY when TradingState.REDUCING and LONG {instrument.id}",
                )
                return  # Denied
            elif order.is_sell_c() and self._portfolio.is_net_short(instrument.id):
                self._deny_command(
                    command=command,
                    reason=f"SELL when TradingState.REDUCING and SHORT {instrument.id}",
                )
                return  # Denied

        # All checks passed: send to ORDER_RATE throttler
        self._order_throttler.send(command)

    cpdef _send_command(self, TradingCommand command):
        self._exec_engine.execute(command)

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1
