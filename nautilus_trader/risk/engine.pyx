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
from typing import Dict, Optional

import pandas as pd
import pydantic

from libc.stdint cimport int64_t

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.events.risk cimport TradingStateChanged
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.throttler cimport Throttler
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_status cimport OrderStatus
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.trading_state cimport TradingState
from nautilus_trader.model.c_enums.trading_state cimport TradingStateParser
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport ModifyOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.commands.trading cimport SubmitOrderList
from nautilus_trader.model.commands.trading cimport TradingCommand
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade


class RiskEngineConfig(pydantic.BaseModel):
    """
    Configuration for ``RiskEngine`` instances.

    bypass : bool
        If True then all risk checks are bypassed (will still check for duplicate IDs).
    max_order_rate : str, default=100/00:00:01
        The maximum order rate per timedelta.
    max_notional_per_order : Dict[str, str]
        The maximum notional value of an order per instrument ID.
        The value should be a valid decimal format.
    """

    bypass: bool = False
    max_order_rate: pydantic.ConstrainedStr = "100/00:00:01"
    max_notional_per_order: Dict[str, str] = {}


cdef class RiskEngine(Component):
    """
    Provides a high-performance risk engine.

    The `RiskEngine` is responsible for global strategy and portfolio risk
    within the platform. This includes both pre-trade risk checks and post-trade
    risk monitoring.

    Possible trading states:
     - ``ACTIVE`` (trading is enabled).
     - ``REDUCING`` (only new orders or updates which reduce an open position are allowed).
     - ``HALTED`` (all trading commands except cancels are denied).
    """

    def __init__(
        self,
        PortfolioFacade portfolio not None,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        Clock clock not None,
        Logger logger not None,
        config: Optional[RiskEngineConfig]=None,
    ):
        """
        Initialize a new instance of the ``RiskEngine`` class.

        Parameters
        ----------
        portfolio : PortfolioFacade
            The portfolio for the engine.
        msgbus : MessageBus
            The message bus for the engine.
        cache : CacheFacade
            The read-only cache for the engine.
        clock : Clock
            The clock for the engine.
        logger : Logger
            The logger for the engine.
        config : RiskEngineConfig, optional
            The configuration for the instance.

        Raises
        ------
        TypeError
            If config is not of type `RiskEngineConfig`.

        """
        if config is None:
            config = RiskEngineConfig()
        Condition.type(config, RiskEngineConfig, "config")
        super().__init__(
            clock=clock,
            logger=logger,
            msgbus=msgbus,
            config=config.dict(),
        )

        self._portfolio = portfolio
        self._cache = cache

        self.trading_state = TradingState.ACTIVE  # Start active by default
        self.is_bypassed = config.bypass
        self._log_state()

        # Counters
        self.command_count = 0
        self.event_count = 0

        # Throttlers
        pieces = config.max_order_rate.split("/")
        order_rate_limit = int(pieces[0])
        order_rate_interval = pd.to_timedelta(pieces[1])
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
            f"Set MAX_ORDER_RATE: "
            f"{order_rate_limit}/{str(order_rate_interval).replace('0 days ', '')}.",
            color=LogColor.BLUE,
        )

        # Risk settings
        self._max_notional_per_order = {}

        # Configure
        self._initialize_risk_checks(config)

        # Register endpoints
        self._msgbus.register(endpoint="RiskEngine.execute", handler=self.execute)

        # Required subscriptions
        self._msgbus.subscribe(topic="events.order*", handler=self._handle_event, priority=10)
        self._msgbus.subscribe(topic="events.position*", handler=self._handle_event, priority=10)

    def _initialize_risk_checks(self, config: RiskEngineConfig):
        cdef dict max_notional_config = config.max_notional_per_order
        for instrument_id, value in max_notional_config.items():
            self.set_max_notional_per_order(InstrumentId.from_str_c(instrument_id), Decimal(value))

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
        if state == self.trading_state:
            self._log.warning(
                f"No change to trading state: "
                f"already set to {TradingStateParser.to_str(self.trading_state)}.",
            )
            return

        self.trading_state = state

        cdef int64_t now = self._clock.timestamp_ns()
        cdef TradingStateChanged event = TradingStateChanged(
            trader_id=self.trader_id,
            state=self.trading_state,
            config=self._config,
            event_id=self._uuid_factory.generate(),
            ts_event=now,
            ts_init=now,
        )

        self._msgbus.publish_c(topic="events.risk", msg=event)
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
            If new_value is not ``None`` and not positive.

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
        Decimal or ``None``

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
        elif isinstance(command, SubmitOrderList):
            self._handle_submit_order_list(command)
        elif isinstance(command, ModifyOrder):
            self._handle_modify_order(command)
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

        # Check position exists
        cdef Position position
        if command.position_id is not None:
            position = self._cache.position(command.position_id)
            if position is None:
                self._deny_command(
                    command=command,
                    reason=f"Position with {repr(command.position_id)} does not exist",
                )
                return  # Denied
            if position.is_closed_c():
                self._deny_command(
                    command=command,
                    reason=f"Position with {repr(command.position_id)} already closed",
                )
                return  # Denied

        if self.is_bypassed:
            # Perform no further risk checks or throttling
            self._msgbus.send(endpoint="ExecEngine.execute", msg=command)
            return

        # Get instrument for order
        cdef Instrument instrument = self._cache.instrument(command.order.instrument_id)
        if instrument is None:
            self._deny_command(
                command=command,
                reason=f"Instrument for {command.instrument_id} not found",
            )
            return  # Denied

        ########################################################################
        # Pre-trade order checks
        ########################################################################
        if not self._check_order(instrument, command.order):
            return  # Denied

        self._execution_gateway(instrument, command, order=command.order)

    cdef void _handle_submit_order_list(self, SubmitOrderList command) except *:
        cdef Order order
        for order in command.list.orders:
            # Check IDs for duplicates
            if not self._check_order_id(order):
                self._deny_command(
                    command=command,
                    reason=f"Duplicate {repr(order.client_order_id)}")
                return  # Denied

        if self.is_bypassed:
            # Perform no further risk checks or throttling
            self._msgbus.send(endpoint="ExecEngine.execute", msg=command)
            return

        # Get instrument for orders
        cdef Instrument instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            self._deny_command(
                command=command,
                reason=f"no instrument found for {command.instrument_id}",
            )
            return  # Denied

        ########################################################################
        # Pre-trade order(s) checks
        ########################################################################
        for order in command.list.orders:
            if not self._check_order(instrument, order):
                return  # Denied

        self._execution_gateway(instrument, command, order=command.list.first)

    cdef void _handle_modify_order(self, ModifyOrder command) except *:
        ########################################################################
        # Validate command
        ########################################################################
        cdef Order order = self._cache.order(command.client_order_id)
        if order is None:
            self._deny_command(
                command=command,
                reason=f"Order with {repr(command.client_order_id)} not found",
            )
            return  # Denied
        elif order.is_completed_c():
            self._deny_command(
                command=command,
                reason=f"Order with {repr(command.client_order_id)} already completed",
            )
            return  # Denied
        elif order.is_inflight_c():
            self._deny_command(
                command=command,
                reason=f"Order with {repr(command.client_order_id)} currently in-flight",
            )
            return  # Denied

        # Get instrument for orders
        cdef Instrument instrument = self._cache.instrument(command.instrument_id)
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
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)

    cdef void _handle_cancel_order(self, CancelOrder command) except *:
        ########################################################################
        # Validate command
        ########################################################################
        cdef Order order = self._cache.order(command.client_order_id)
        if order is None:
            self._deny_command(
                command=command,
                reason=f"Order with {repr(command.client_order_id)} not found",
            )
            return  # Denied
        elif order.is_completed_c():
            self._deny_command(
                command=command,
                reason=f"Order with {repr(command.client_order_id)} already completed",
            )
            return  # Denied
        elif order.is_pending_cancel_c():
            self._deny_command(
                command=command,
                reason=f"Order with {repr(command.client_order_id)} already pending cancel",
            )
            return  # Denied

        # All checks passed: send for execution
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)

# -- PRE-TRADE CHECKS ------------------------------------------------------------------------------

    cdef bint _check_order_id(self, Order order) except *:
        if order is None or not self._cache.order_exists(order.client_order_id):
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
            last = self._cache.quote_tick(instrument.id)
            if last is None:
                self._deny_order(
                    order=order,
                    reason="No market to check MAX_NOTIONAL_PER_ORDER",
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
        elif isinstance(command, SubmitOrderList):
            self._deny_order_list(command.list, reason=reason)
        elif isinstance(command, ModifyOrder):
            self._log.error(f"ModifyOrder DENIED: {reason}.")
        elif isinstance(command, CancelOrder):
            self._log.error(f"CancelOrder DENIED: {reason}.")

    cpdef _deny_new_order(self, TradingCommand command):
        if isinstance(command, SubmitOrder):
            self._deny_order(command.order, reason="Exceeded MAX_ORDER_RATE")
        elif isinstance(command, SubmitOrderList):
            self._deny_order_list(command.list, reason="Exceeded MAX_ORDER_RATE")

    cdef void _deny_order(self, Order order, str reason) except *:
        self._log.error(f"SubmitOrder DENIED: {reason}.")

        if order is None:
            # Nothing to deny
            return

        if order.status_c() != OrderStatus.INITIALIZED:
            # Already denied or duplicated (INITIALIZED -> DENIED only valid state transition)
            return

        if not self._cache.order_exists(order.client_order_id):
            self._cache.add_order(order, position_id=None)

        # Generate event
        cdef OrderDenied denied = OrderDenied(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="ExecEngine.process", msg=denied)

    cdef void _deny_order_list(self, OrderList order_list, str reason) except *:
        cdef Order order
        for order in order_list.orders:
            self._deny_order(order=order, reason=reason)

# -- EGRESS ----------------------------------------------------------------------------------------

    cdef void _execution_gateway(self, Instrument instrument, TradingCommand command, Order order):
        # Check TradingState
        if self.trading_state == TradingState.HALTED:
            self._deny_order_list(
                order_list=command.list,
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
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_event(self, Event event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1
