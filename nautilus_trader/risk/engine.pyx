# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd

from nautilus_trader.risk.config import RiskEngineConfig

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport EVT
from nautilus_trader.common.component cimport RECV
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport Throttler
from nautilus_trader.common.messages cimport TradingStateChanged
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport InstrumentClass
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport TradingState
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.functions cimport order_type_to_str
from nautilus_trader.model.functions cimport trading_state_to_str
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.currency_pair cimport CurrencyPair
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.position cimport Position
from nautilus_trader.portfolio.base cimport PortfolioFacade


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

    Parameters
    ----------
    portfolio : PortfolioFacade
        The portfolio for the engine.
    msgbus : MessageBus
        The message bus for the engine.
    cache : Cache
        The cache for the engine.
    clock : Clock
        The clock for the engine.
    config : RiskEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `RiskEngineConfig`.
    """

    def __init__(
        self,
        PortfolioFacade portfolio not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        config: RiskEngineConfig | None = None,
    ):
        if config is None:
            config = RiskEngineConfig()
        Condition.type(config, RiskEngineConfig, "config")
        super().__init__(
            clock=clock,
            component_id=ComponentId("RiskEngine"),
            msgbus=msgbus,
            config=config,
        )

        self._portfolio = portfolio
        self._cache = cache

        # Configuration
        self.trading_state = TradingState.ACTIVE  # Start active by default
        self.is_bypassed = config.bypass
        self.debug = config.debug
        self._log_state()

        # Counters
        self.command_count = 0
        self.event_count = 0

        # Throttlers
        pieces = config.max_order_submit_rate.split("/")
        order_submit_rate_limit = int(pieces[0])
        order_submit_rate_interval = pd.to_timedelta(pieces[1])
        self._order_submit_throttler = Throttler(
            name="ORDER_SUBMIT_THROTTLER",
            limit=order_submit_rate_limit,
            interval=order_submit_rate_interval,
            output_send=self._send_to_execution,
            output_drop=self._deny_new_order,
            clock=clock,
        )

        self._log.info(
            f"Set MAX_ORDER_SUBMIT_RATE: "
            f"{order_submit_rate_limit}/{str(order_submit_rate_interval).replace('0 days ', '')}",
            color=LogColor.BLUE,
        )

        pieces = config.max_order_modify_rate.split("/")
        order_modify_rate_limit = int(pieces[0])
        order_modify_rate_interval = pd.to_timedelta(pieces[1])
        self._order_modify_throttler = Throttler(
            name="ORDER_MODIFY_THROTTLER",
            limit=order_modify_rate_limit,
            interval=order_modify_rate_interval,
            output_send=self._send_to_execution,
            output_drop=self._deny_modify_order,
            clock=clock,
        )

        self._log.info(
            f"Set MAX_ORDER_MODIFY_RATE: "
            f"{order_modify_rate_limit}/{str(order_modify_rate_interval).replace('0 days ', '')}",
            color=LogColor.BLUE,
        )

        # Risk settings
        self._max_notional_per_order: dict[InstrumentId, Decimal] = {}

        # Configure
        self._initialize_risk_checks(config)

        # Register endpoints
        self._msgbus.register(endpoint="RiskEngine.execute", handler=self.execute)
        self._msgbus.register(endpoint="RiskEngine.process", handler=self.process)

        # Required subscriptions
        self._msgbus.subscribe(topic="events.order.*", handler=self._handle_event, priority=10)
        self._msgbus.subscribe(topic="events.position.*", handler=self._handle_event, priority=10)

    def _initialize_risk_checks(self, config: RiskEngineConfig):
        cdef dict max_notional_config = config.max_notional_per_order
        for instrument_id, value in max_notional_config.items():
            self.set_max_notional_per_order(InstrumentId.from_str_c(instrument_id), Decimal(value))

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void execute(self, Command command):
        """
        Execute the given command.

        Parameters
        ----------
        command : Command
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, Event event):
        """
        Process the given event.

        Parameters
        ----------
        event : Event
            The event to process.

        """
        Condition.not_none(event, "event")

        self._handle_event(event)

    cpdef void set_trading_state(self, TradingState state):
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
                f"already set to {trading_state_to_str(self.trading_state)}",
            )
            return

        self.trading_state = state

        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef TradingStateChanged event = TradingStateChanged(
            trader_id=self.trader_id,
            state=self.trading_state,
            config=self._config,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

        self._msgbus.publish_c(topic="events.risk", msg=event)
        self._log_state()

    cpdef void _log_state(self):
        cdef LogColor color = LogColor.BLUE
        if self.trading_state == TradingState.REDUCING:
            color = LogColor.YELLOW
        elif self.trading_state == TradingState.HALTED:
            color = LogColor.RED
        self._log.info(
            f"TradingState is {trading_state_to_str(self.trading_state)}",
            color=color,
        )

        if self.is_bypassed:
            self._log.info(
                "PRE-TRADE RISK CHECKS BYPASSED. This is not advisable for live trading",
                color=LogColor.RED,
            )

    cpdef void set_max_notional_per_order(self, InstrumentId instrument_id, new_value):
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
            If `new_value` not a valid input for `decimal.Decimal`.
        ValueError
            If `new_value` is not ``None`` and not positive.

        """
        if new_value is not None:
            new_value = Decimal(new_value)
            Condition.type(new_value, Decimal, "new_value")
            Condition.positive(new_value, "new_value")

        old_value: Decimal = self._max_notional_per_order.get(instrument_id)
        self._max_notional_per_order[instrument_id] = new_value

        cdef str new_value_str = f"{new_value:,}" if new_value is not None else str(None)
        self._log.info(
            f"Set MAX_NOTIONAL_PER_ORDER: {instrument_id} {new_value_str}",
            color=LogColor.BLUE,
        )

# -- RISK SETTINGS --------------------------------------------------------------------------------

    cpdef tuple max_order_submit_rate(self):
        """
        Return the current maximum order submit rate limit setting.

        Returns
        -------
        (int, timedelta)
            The limit per timedelta interval.

        """
        return (
            self._order_submit_throttler.limit,
            self._order_submit_throttler.interval,
        )

    cpdef tuple max_order_modify_rate(self):
        """
        Return the current maximum order modify rate limit setting.

        Returns
        -------
        (int, timedelta)
            The limit per timedelta interval.

        """
        return (
            self._order_modify_throttler.limit,
            self._order_modify_throttler.interval,
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

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self):
        pass  # Optionally override in subclass

    cpdef void _on_stop(self):
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _start(self):
        # Do nothing else for now
        self._on_start()

    cpdef void _stop(self):
        # Do nothing else for now
        self._on_stop()

    cpdef void _reset(self):
        self.command_count = 0
        self.event_count = 0
        self._order_submit_throttler.reset()
        self._order_modify_throttler.reset()

    cpdef void _dispose(self):
        pass
        # Nothing to dispose for now

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void _execute_command(self, Command command):
        if self.debug:
            self._log.debug(f"{RECV}{CMD} {command}", LogColor.MAGENTA)
        self.command_count += 1

        if isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, SubmitOrderList):
            self._handle_submit_order_list(command)
        elif isinstance(command, ModifyOrder):
            self._handle_modify_order(command)
        else:
            self._log.error(f"Cannot handle command: {command}")

    cpdef void _handle_submit_order(self, SubmitOrder command):
        if self.is_bypassed:
            # Perform no further risk checks or throttling
            self._send_to_execution(command)
            return

        cdef Order order = command.order

        # Check reduce only
        cdef Position position
        if command.position_id is not None:
            if order.is_reduce_only:
                position = self._cache.position(command.position_id)
                if position is None or not order.would_reduce_only(position.side, position.quantity):
                    self._deny_command(
                        command=command,
                        reason=f"Reduce only order would increase position {command.position_id!r}",
                    )
                    return  # Denied

        # Get instrument for order
        cdef Instrument instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._deny_command(
                command=command,
                reason=f"Instrument for {order.instrument_id} not found",
            )
            return  # Denied

        ########################################################################
        # PRE-TRADE ORDER(S) CHECKS
        ########################################################################
        if not self._check_order(instrument, order):
            return  # Denied

        if not self._check_orders_risk(instrument, [order]):
            return # Denied

        self._execution_gateway(instrument, command)

    cpdef void _handle_submit_order_list(self, SubmitOrderList command):
        if self.is_bypassed:
            # Perform no further risk checks or throttling
            self._send_to_execution(command)
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
        # PRE-TRADE ORDER(S) CHECKS
        ########################################################################
        for order in command.order_list.orders:
            if not self._check_order(instrument, order):
                return  # Denied

        if not self._check_orders_risk(instrument, command.order_list.orders):
            # Deny all orders in list
            self._deny_order_list(command.order_list, "OrderList {command.order_list.id.to_str()} DENIED")
            return # Denied

        self._execution_gateway(instrument, command)

    cpdef void _handle_modify_order(self, ModifyOrder command):
        ########################################################################
        # VALIDATE COMMAND
        ########################################################################
        cdef Order order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(
                f"ModifyOrder DENIED: Order with {command.client_order_id!r} not found",
            )
            return  # Denied
        elif order.is_closed_c():
            self._reject_modify_order(
                order=order,
                reason=f"Order with {command.client_order_id!r} already closed",
            )
            return  # Denied
        elif order.is_pending_cancel_c():
            self._reject_modify_order(
                order=order,
                reason=f"Order with {command.client_order_id!r} already pending cancel",
            )
            return  # Denied

        # Get instrument for orders
        cdef Instrument instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            self._reject_modify_order(
                order=order,
                reason=f"no instrument found for {command.instrument_id}",
            )
            return  # Denied

        cdef str risk_msg = None

        # Check price
        risk_msg = self._check_price(instrument, command.price)
        if risk_msg:
            self._reject_modify_order(order=order, reason=risk_msg)
            return  # Denied

        # Check trigger
        risk_msg = self._check_price(instrument, command.trigger_price)
        if risk_msg:
            self._reject_modify_order(order=order, reason=risk_msg)
            return  # Denied

        # Check quantity
        risk_msg = self._check_quantity(instrument, command.quantity)
        if risk_msg:
            self._reject_modify_order(order=order, reason=risk_msg)
            return  # Denied

        # Check TradingState
        if self.trading_state == TradingState.HALTED:
            self._reject_modify_order(
                order=order,
                reason="TradingState is HALTED",
            )
            return  # Denied
        elif self.trading_state == TradingState.REDUCING:
            if command.quantity and command.quantity > order.quantity:
                if order.is_buy_c() and self._portfolio.is_net_long(instrument.id):
                    self._reject_modify_order(
                        order=order,
                        reason="TradingState is REDUCING and update will increase exposure",
                    )
                    return  # Denied
                elif order.is_sell_c() and self._portfolio.is_net_short(instrument.id):
                    self._reject_modify_order(
                        order=order,
                        reason="TradingState is REDUCING and update will increase exposure",
                    )
                    return  # Denied

        self._order_modify_throttler.send(command)

# -- PRE-TRADE CHECKS -----------------------------------------------------------------------------

    cpdef bint _check_order(self, Instrument instrument, Order order):
        ########################################################################
        # VALIDATION CHECKS
        ########################################################################
        if not self._check_order_price(instrument, order):
            return False  # Denied
        if not self._check_order_quantity(instrument, order):
            return False  # Denied

        return True  # Check passed

    cpdef bint _check_order_price(self, Instrument instrument, Order order):
        ########################################################################
        # CHECK PRICE
        ########################################################################
        cdef str risk_msg = None
        if order.has_price_c():
            risk_msg = self._check_price(instrument, order.price)
            if risk_msg:
                self._deny_order(order=order, reason=risk_msg)
                return False  # Denied

        ########################################################################
        # CHECK TRIGGER
        ########################################################################
        if order.has_trigger_price_c():
            risk_msg = self._check_price(instrument, order.trigger_price)
            if risk_msg:
                self._deny_order(order=order, reason=f"trigger {risk_msg}")
                return False  # Denied

        return True  # Passed

    cpdef bint _check_order_quantity(self, Instrument instrument, Order order):
        cdef str risk_msg = self._check_quantity(instrument, order.quantity)
        if risk_msg:
            self._deny_order(order=order, reason=risk_msg)
            return False  # Denied

        return True  # Passed

    cpdef bint _check_orders_risk(self, Instrument instrument, list orders):
        ########################################################################
        # RISK CHECKS
        ########################################################################
        cdef QuoteTick last_quote = None
        cdef TradeTick last_trade = None
        cdef Price last_px = None
        cdef Money free

        # Determine max notional
        cdef Money max_notional = None
        max_notional_setting: Decimal | None = self._max_notional_per_order.get(instrument.id)
        if max_notional_setting:
            # TODO: Improve efficiency of this
            max_notional = Money(float(max_notional_setting), instrument.quote_currency)

        # Get account for risk checks
        cdef Account account = self._cache.account_for_venue(instrument.id.venue)
        if account is None:
            self._log.debug(f"Cannot find account for venue {instrument.id.venue}")
            return True  # TODO: Temporary early return until handling routing/multiple venues

        if account.is_margin_account:
            return True  # TODO: Determine risk controls for margin

        free = account.balance_free(instrument.quote_currency)
        if self.debug:
            self._log.debug(f"Free: {free!r}", LogColor.MAGENTA)

        cdef:
            Order order
            Money notional
            Money cum_notional_buy = None
            Money cum_notional_sell = None
            Money order_balance_impact = None
            Money cash_value = None
            Currency base_currency = None
            double xrate
        for order in orders:
            if order.order_type == OrderType.MARKET or order.order_type == OrderType.MARKET_TO_LIMIT:
                if last_px is None:
                    # Determine entry price
                    last_quote = self._cache.quote_tick(instrument.id)
                    if last_quote is not None:
                        if order.side == OrderSide.BUY:
                            last_px = last_quote.ask_price
                        elif order.side == OrderSide.SELL:
                            last_px = last_quote.bid_price
                        else:  # pragma: no cover (design-time error)
                            raise RuntimeError(f"invalid `OrderSide`")
                    else:
                        last_trade = self._cache.trade_tick(instrument.id)
                        if last_trade is not None:
                            last_px = last_trade.price
                        else:
                            self._log.warning(
                                f"Cannot check MARKET order risk: no prices for {instrument.id}",
                            )
                            continue  # Cannot check order risk
            elif order.order_type == OrderType.STOP_MARKET or order.order_type == OrderType.MARKET_IF_TOUCHED:
                last_px = order.trigger_price
            elif order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
                if order.trigger_price is None:
                    self._log.warning(
                        f"Cannot check {order_type_to_str(order.order_type)} order risk: "
                        f"no trigger price was set",  # TODO: Use last_trade += offset
                    )
                    continue  # Cannot assess risk
                else:
                    last_px = order.trigger_price
            else:
                last_px = order.price

            notional = instrument.notional_value(order.quantity, last_px, use_quote_for_inverse=True)
            if self.debug:
                self._log.debug(f"Notional: {order_balance_impact!r}", LogColor.MAGENTA)

            if max_notional and notional._mem.raw > max_notional._mem.raw:
                self._deny_order(
                    order=order,
                    reason=f"NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional={max_notional}, notional={notional}",
                )
                return False  # Denied

            # Check MIN notional instrument limit
            if (
                instrument.min_notional is not None
                and instrument.min_notional.currency == notional.currency
                and notional._mem.raw < instrument.min_notional._mem.raw
            ):
                self._deny_order(
                    order=order,
                    reason=f"NOTIONAL_LESS_THAN_MIN_FOR_INSTRUMENT: min_notional={instrument.min_notional} , notional={notional}",
                )
                return False  # Denied

            # Check MAX notional instrument limit
            if (
                instrument.max_notional is not None
                and instrument.max_notional.currency == notional.currency
                and notional._mem.raw > instrument.max_notional._mem.raw
            ):
                self._deny_order(
                    order=order,
                    reason=f"NOTIONAL_GREATER_THAN_MAX_FOR_INSTRUMENT: max_notional={instrument.max_notional}, notional={notional}",
                )
                return False  # Denied

            order_balance_impact = account.balance_impact(instrument, order.quantity, last_px, order.side)
            if self.debug:
                self._log.debug(f"Balance impact: {order_balance_impact!r}", LogColor.MAGENTA)

            if free is not None and (free._mem.raw + order_balance_impact._mem.raw) < 0:
                self._deny_order(
                    order=order,
                    reason=f"NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, balance_impact={order_balance_impact}",
                )
                return False  # Denied

            if base_currency is None:
                base_currency = instrument.get_base_currency()

            if order.is_buy_c():
                if cum_notional_buy is None:
                    cum_notional_buy = Money(-order_balance_impact, order_balance_impact.currency)
                else:
                    cum_notional_buy._mem.raw += -order_balance_impact._mem.raw

                if self.debug:
                    self._log.debug(f"Cumulative notional BUY: {cum_notional_buy!r}")
                if free is not None and cum_notional_buy._mem.raw > free._mem.raw:
                    self._deny_order(
                        order=order,
                        reason=f"CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_buy}",
                    )
                    return False  # Denied
            elif order.is_sell_c():
                if account.base_currency is not None:
                    if cum_notional_sell is None:
                        cum_notional_sell = Money(order_balance_impact, order_balance_impact.currency)
                    else:
                        cum_notional_sell._mem.raw += order_balance_impact._mem.raw

                    if self.debug:
                        self._log.debug(f"Cumulative notional SELL: {cum_notional_sell!r}")
                    if free is not None and cum_notional_sell._mem.raw > free._mem.raw:
                        self._deny_order(
                            order=order,
                            reason=f"CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_sell}",
                        )
                        return False  # Denied
                elif base_currency is not None and account.type == AccountType.CASH:
                    cash_value = Money(order.quantity.as_f64_c(), base_currency)
                    if self.debug:
                        total = account.balance_total(base_currency)
                        locked = account.balance_locked(base_currency)
                        free = account.balance_free(base_currency)
                        self._log.debug(f"Cash value: {cash_value!r}", LogColor.MAGENTA)
                        self._log.debug(f"Total: {total!r}", LogColor.MAGENTA)
                        self._log.debug(f"Locked: {locked!r}", LogColor.MAGENTA)
                        self._log.debug(f"Free: {free!r}", LogColor.MAGENTA)

                    if cum_notional_sell is None:
                        cum_notional_sell = cash_value
                    else:
                        cum_notional_sell._mem.raw += cash_value._mem.raw

                    if self.debug:
                        self._log.debug(f"Cumulative notional SELL: {cum_notional_sell!r}")
                    if free is not None and cum_notional_sell._mem.raw > free._mem.raw:
                        self._deny_order(
                            order=order,
                            reason=f"CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_sell}",
                        )
                        return False  # Denied

        # Finally
        return True  # Passed

    cpdef str _check_price(self, Instrument instrument, Price price):
        if price is None:
            # Nothing to check
            return None
        if price.precision > instrument.price_precision:
            # Check failed
            return f"price {price} invalid (precision {price.precision} > {instrument.price_precision})"
        if instrument.instrument_class != InstrumentClass.OPTION:
            if price.raw_int_c() <= 0:
                # Check failed
                return f"price {price} invalid (not positive)"

    cpdef str _check_quantity(self, Instrument instrument, Quantity quantity):
        if quantity is None:
            # Nothing to check
            return None
        if quantity._mem.precision > instrument.size_precision:
            # Check failed
            return f"quantity {quantity} invalid (precision {quantity._mem.precision} > {instrument.size_precision})"
        if instrument.max_quantity and quantity > instrument.max_quantity:
            # Check failed
            return f"quantity {quantity} invalid (> maximum trade size of {instrument.max_quantity})"
        if instrument.min_quantity and quantity < instrument.min_quantity:
            # Check failed
            return f"quantity {quantity} invalid (< minimum trade size of {instrument.min_quantity})"

# -- DENIALS --------------------------------------------------------------------------------------

    cpdef void _deny_command(self, TradingCommand command, str reason):
        if isinstance(command, SubmitOrder):
            self._deny_order(command.order, reason=reason)
        elif isinstance(command, SubmitOrderList):
            self._deny_order_list(command.order_list, reason=reason)
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"Cannot deny command {command}")  # pragma: no cover (design-time error)

    # Needs to be `cpdef` due being called from throttler
    cpdef void _deny_new_order(self, TradingCommand command):
        if isinstance(command, SubmitOrder):
            self._deny_order(command.order, reason="Exceeded MAX_ORDER_SUBMIT_RATE")
        elif isinstance(command, SubmitOrderList):
            self._deny_order_list(command.order_list, reason="Exceeded MAX_ORDER_SUBMIT_RATE")

    # Needs to be `cpdef` due being called from throttler
    cpdef void _deny_modify_order(self, ModifyOrder command):
        cdef Order order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"Order with {command.client_order_id!r} not found")
            return
        self._reject_modify_order(order, reason="Exceeded MAX_ORDER_MODIFY_RATE")

    cpdef void _deny_order(self, Order order, str reason):
        self._log.warning(f"SubmitOrder for {order.client_order_id.to_str()} DENIED: {reason}")

        if order is None:
            # Nothing to deny
            return

        if order.status_c() != OrderStatus.INITIALIZED:
            # Already denied or duplicated (INITIALIZED -> DENIED only valid state transition)
            return

        if not self._cache.order_exists(order.client_order_id):
            self._cache.add_order(order)

        # Generate event
        cdef OrderDenied denied = OrderDenied(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.send(endpoint="ExecEngine.process", msg=denied)

    cpdef void _deny_order_list(self, OrderList order_list, str reason):
        cdef Order order
        for order in order_list.orders:
            if not order.is_closed_c():
                self._deny_order(order=order, reason=reason)

# -- EGRESS ---------------------------------------------------------------------------------------

    cpdef void _execution_gateway(self, Instrument instrument, TradingCommand command):
        # Check TradingState
        cdef Order order
        if self.trading_state == TradingState.HALTED:
            if isinstance(command, SubmitOrder):
                self._deny_command(
                    command=command,
                    reason=f"TradingState.HALTED",
                )
                return  # Denied
            elif isinstance(command, SubmitOrderList):
                self._deny_order_list(
                    order_list=command.order_list,
                    reason="TradingState.HALTED",
                )
                return  # Denied
        elif self.trading_state == TradingState.REDUCING:
            if isinstance(command, SubmitOrder):
                order = command.order
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
            elif isinstance(command, SubmitOrderList):
                for order in command.order_list.orders:
                    if order.is_buy_c() and self._portfolio.is_net_long(instrument.id):
                        self._deny_order_list(
                            order_list=command.order_list,
                            reason=f"OrderList contains BUY when TradingState.REDUCING and LONG {instrument.id}",
                        )
                        return  # Denied
                    elif order.is_sell_c() and self._portfolio.is_net_short(instrument.id):
                        self._deny_order_list(
                            order_list=command.order_list,
                            reason=f"OrderList contains SELL when TradingState.REDUCING and SHORT {instrument.id}",
                        )
                        return  # Denied

        # All checks passed: send to ORDER_RATE throttler
        self._order_submit_throttler.send(command)

    # Needs to be `cpdef` due being called from throttler
    cpdef void _send_to_execution(self, TradingCommand command):
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)

    cpdef void _reject_modify_order(self, Order order, str reason):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderModifyRejected denied = OrderModifyRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

        self._msgbus.send(endpoint="ExecEngine.process", msg=denied)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void _handle_event(self, Event event):
        if self.debug:
            self._log.debug(f"{RECV}{EVT} {event}", LogColor.MAGENTA)
        self.event_count += 1
