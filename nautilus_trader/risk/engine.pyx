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
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class RiskEngine(Component):
    """
    Provides a high-performance risk engine.

    The `RiskEngine` is responsible for global strategy and portfolio risk
    within the platform. Alternative implementations can be written on top of
    the generic engine.

    Configuration
    -------------
    The following options are possible in the configuration dictionary.

    - bypass: If True then all risk checks are bypassed (will still check for duplicate IDs).
    - max_orders_per_second: int. Default=10.
    - max_notional_per_order: { str: Decimal }. Default = {}.

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

        self._portfolio = portfolio
        self._exec_engine = exec_engine

        self.trader_id = exec_engine.trader_id
        self.cache = cache
        self.trading_state = TradingState.ACTIVE  # Start active by default
        self.is_bypassed = config.get("bypass", False)
        self._log_state()

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
            output=self._send_command,
            clock=clock,
            logger=logger,
        )

        self._log.info(
            f"Set MAX_ORDER_RATE: {order_rate_limit} / {order_rate_interval}.",
            color=LogColor.BLUE,
        )

        # Risk settings
        self._max_notional_per_order = {}

        # Counters
        self.command_count = 0
        self.event_count = 0

        ########################################################################
        # Configure pre-trade risk checks
        ########################################################################
        max_notional_config = config.get("max_notional_per_order", {})
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

# -- RISK SETTINGS ---------------------------------------------------------------------------------

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
                order=command.order,
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
            self._deny_order(
                order=command.order,
                reason=f"no instrument found for {command.instrument_id}",
            )
            return  # Denied

        ########################################################################
        # Validation checks
        ########################################################################
        if not self._check_order_values(instrument, command.order):
            return  # Denied

        ########################################################################
        # Risk checks
        ########################################################################
        if not self._check_order_risk(instrument, command.order):
            return  # Denied

        if self.trading_state == TradingState.HALTED:
            self._deny_order(order=command.order, reason="TRADING_HALTED")
            return  # Denied
        elif self.trading_state == TradingState.REDUCING:
            if command.order.is_buy_c():
                if self._portfolio.is_net_long(instrument.id):
                    self._deny_order(
                        order=command.order,
                        reason=f"BUY when TradingState.REDUCING and LONG {instrument.id}",
                    )
            elif command.order.is_sell_c():
                if self._portfolio.is_net_short(instrument.id):
                    self._deny_order(
                        order=command.order,
                        reason=f"SELL when TradingState.REDUCING and SHORT {instrument.id}",
                    )

        # All checks passed
        self._order_throttler.send(command)

    cdef void _handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        cdef Order entry = command.bracket_order.entry
        cdef StopMarketOrder stop_loss = command.bracket_order.stop_loss
        cdef LimitOrder take_profit = command.bracket_order.take_profit

        # Check IDs for duplicates
        if not self._check_duplicate_id(entry):
            self._deny_bracket_order(
                command.bracket_order,
                f"Duplicate {repr(entry.client_order_id)}")
            return  # Denied
        if not self._check_duplicate_id(stop_loss):
            self._deny_bracket_order(
                command.bracket_order,
                f"Duplicate {repr(stop_loss.client_order_id)}")
            return  # Denied
        if not self._check_duplicate_id(take_profit):
            self._deny_bracket_order(
                command.bracket_order,
                f"Duplicate {repr(take_profit.client_order_id)}")
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
            self._deny_bracket_order(
                bracket_order=command.bracket_order,
                reason=f"no instrument found for {command.instrument_id}",
            )
            return  # Denied

        ########################################################################
        # Validation checks
        ########################################################################
        if not self._check_order_values(instrument, entry):
            return  # Denied
        if not self._check_order_values(instrument, stop_loss):
            return  # Denied
        if not self._check_order_values(instrument, take_profit):
            return  # Denied

        ########################################################################
        # Risk checks
        ########################################################################
        if not self._check_order_risk(instrument, command.bracket_order.entry):
            return  # Denied

        if self.trading_state == TradingState.HALTED:
            self._deny_bracket_order(
                bracket_order=command.bracket_order,
                reason="TradingState.HALTED",
            )
            return  # Denied
        elif self.trading_state == TradingState.REDUCING:
            if entry.is_buy_c():
                if self._portfolio.is_net_long(instrument.id):
                    self._deny_bracket_order(
                        bracket_order=command.bracket_order,
                        reason=f"BUY when TradingState.REDUCING and LONG {instrument.id}",
                    )
            elif entry.is_sell_c():
                if self._portfolio.is_net_short(instrument.id):
                    self._deny_bracket_order(
                        bracket_order=command.bracket_order,
                        reason=f"SELL when TradingState.REDUCING and SHORT {instrument.id}",
                    )

        # All checks passed
        self._order_throttler.send(command)

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

# -- PRE-TRADE CHECKS ------------------------------------------------------------------------------

    cdef bint _check_duplicate_id(self, Order order):
        if order is None or not self.cache.order_exists(order.client_order_id):
            # Check passed
            return True
        else:
            # Check failed (duplicate ID
            return False

    cdef bint _check_order_values(self, Instrument instrument, Order order):
        ########################################################################
        # Check size
        ########################################################################
        if order.quantity.precision > instrument.size_precision:
            self._deny_order(
                order=order,
                reason=f"quantity {order.quantity} invalid: "
                       f"precision {order.quantity.precision} > {instrument.size_precision}",
            )
            return False  # Denied
        if instrument.max_quantity and order.quantity > instrument.max_quantity:
            self._deny_order(
                order=order,
                reason=f"quantity {order.quantity} invalid: "
                       f"> maximum trade size of {instrument.max_quantity}",
            )
            return False  # Denied
        if instrument.min_quantity and order.quantity < instrument.min_quantity:
            self._deny_order(
                order=order,
                reason=f"quantity {order.quantity} invalid: "
                       f"< minimum trade size of {instrument.min_quantity}",
            )
            return False  # Denied

        ########################################################################
        # Check price
        ########################################################################
        if (
            order.type == OrderType.LIMIT
            or order.type == OrderType.STOP_MARKET
            or order.type == OrderType.STOP_LIMIT
        ):
            if order.price.precision > instrument.price_precision:
                self._deny_order(
                    order=order,
                    reason=f"price {order.price} invalid: "
                           f"precision {order.price.precision} > {instrument.price_precision}",
                )
                return False  # Denied
            if instrument.asset_type != AssetType.OPTION and order.price <= 0:
                self._deny_order(
                    order=order,
                    reason=f"price {order.price} invalid: not positive",
                )
                return False  # Denied

        ########################################################################
        # Check trigger
        ########################################################################
        if order.type == OrderType.STOP_LIMIT:
            if order.trigger.precision > instrument.price_precision:
                self._deny_order(
                    order=order,
                    reason=f"trigger price {order.trigger} invalid: "
                           f"precision {order.trigger.precision} > {instrument.price_precision}",
                )
                return False  # Denied
            if instrument.asset_type != AssetType.OPTION:
                if order.trigger <= 0:
                    self._deny_order(
                        order=order,
                        reason=f"trigger price {order.trigger} invalid: not positive",
                    )
                    return False  # Denied

        # Passed
        return True

    cdef bint _check_order_risk(self, Instrument instrument, Order order):
        max_notional = self._max_notional_per_order.get(order.instrument_id)
        if max_notional is not None:
            if order.type == OrderType.MARKET:
                # Determine entry price
                last = self.cache.quote_tick(instrument.id)
                if order.side == OrderSide.BUY:
                    entry_px = last.ask
                else:  # order.side == OrderSide.SELL
                    entry_px = last.bid

                notional: Decimal = order.quantity * last.ask * instrument.multiplier
                if notional > max_notional:
                    self._deny_order(
                        order=order,
                        reason=f"Exceeds MAX_NOTIONAL_PER_ORDER @ {max_notional}",
                    )
                    return False  # Denied

        # Passed
        return True

# -- EVENT GENERATION ------------------------------------------------------------------------------

    cdef void _deny_order(self, Order order, str reason) except *:
        if order is None:
            return  # Nothing to deny

        if order.state_c() != OrderState.INITIALIZED:
            return  # Already denied or duplicated

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

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1

# -- EGRESS ----------------------------------------------------------------------------------------

    cpdef _send_command(self, TradingCommand command):
        self._exec_engine.execute(command)
