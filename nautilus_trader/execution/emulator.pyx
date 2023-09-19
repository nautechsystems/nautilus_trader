# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Optional

from nautilus_trader.config.common import OrderEmulatorConfig

from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.manager cimport OrderManager
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.execution.trailing cimport TrailingStopCalculator
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport OrderStatus
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.enums_c cimport trigger_type_to_str
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEmulated
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderReleased
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.msgbus.bus cimport MessageBus


cdef tuple SUPPORTED_TRIGGERS = (TriggerType.DEFAULT, TriggerType.BID_ASK, TriggerType.LAST_TRADE)


cdef class OrderEmulator(Actor):
    """
    Provides order emulation for specified trigger types.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the order emulator.
    msgbus : MessageBus
        The message bus for the order emulator.
    cache : Cache
        The cache for the order emulator.
    clock : Clock
        The clock for the order emulator.
    logger : Logger
        The logger for the order emulator.
    config : OrderEmulatorConfig, optional
        The configuration for the order emulator.

    """

    def __init__(
        self,
        TraderId trader_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        config: Optional[OrderEmulatorConfig] = None,
    ):
        if config is None:
            config = OrderEmulatorConfig()
        Condition.type(config, OrderEmulatorConfig, "config")
        super().__init__()

        self.register_base(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._manager = OrderManager(
            clock=clock,
            logger=logger,
            msgbus=msgbus,
            cache=cache,
            component_name=type(self).__name__,
            submit_order_handler=self._handle_submit_order,
            cancel_order_handler=self._cancel_order,
            debug=config.debug,
        )

        self._matching_cores: dict[InstrumentId, MatchingCore]  = {}

        self._subscribed_quotes: set[InstrumentId] = set()
        self._subscribed_trades: set[InstrumentId] = set()
        self._subscribed_strategies: set[StrategyId] = set()
        self._monitored_positions: set[PositionId] = set()

        # Settings
        self.debug: bool = config.debug

        # Counters
        self.command_count: int = 0
        self.event_count: int = 0

        # Register endpoints
        self._msgbus.register(endpoint="OrderEmulator.execute", handler=self.execute)

    @property
    def subscribed_quotes(self) -> list[InstrumentId]:
        """
        Return the subscribed quote feeds for the emulator.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_quotes))

    @property
    def subscribed_trades(self) -> list[InstrumentId]:
        """
        Return the subscribed trade feeds for the emulator.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_trades))

    def get_submit_order_commands(self) -> dict[ClientOrderId, SubmitOrder]:
        """
        Return the emulators cached submit order commands.

        Returns
        -------
        dict[ClientOrderId, SubmitOrder]

        """
        return self._manager.get_submit_order_commands()

    def get_matching_core(self, InstrumentId instrument_id) -> Optional[MatchingCore]:
        """
        Return the emulators matching core for the given instrument ID.

        Returns
        -------
        MatchingCore or ``None``

        """
        return self._matching_cores.get(instrument_id)

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void on_start(self):
        cdef list emulated_orders = self.cache.orders_emulated()
        if not emulated_orders:
            self._log.info("No emulated orders to reactivate.")
            return

        cdef:
            Order order
            SubmitOrder command
            PositionId position_id
            ClientId client_id
        for order in emulated_orders:
            if order.status_c() not in (OrderStatus.INITIALIZED, OrderStatus.EMULATED):
                continue  # No longer emulated

            if order.parent_order_id is not None:
                parent_order = self.cache.order(order.parent_order_id)
                if parent_order is None:
                    self._log.error("Cannot handle order: parent {order.parent_order_id!r} not found.")
                    continue
                position_id = parent_order.position_id
                if parent_order.is_closed_c() and (position_id is None or self.cache.is_position_closed(position_id)):
                    self._manager.cancel_order(order=order)
                    continue  # Parent already closed
                if parent_order.contingency_type == ContingencyType.OTO:
                    if parent_order.is_active_local_c() or parent_order.filled_qty == 0:
                        continue  # Process contingency order later once parent triggered

            position_id = self.cache.position_id(order.client_order_id)
            client_id = self.cache.client_id(order.client_order_id)
            command = SubmitOrder(
                trader_id=self.trader_id,
                strategy_id=order.strategy_id,
                order=order,
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                position_id=position_id,
                client_id=client_id,
            )

            self._handle_submit_order(command)

    cpdef void on_event(self, Event event):
        """
        Handle the given `event`.

        Parameters
        ----------
        event : Event
            The received event to handle.

        """
        Condition.not_none(event, "event")

        if self.debug:
            self._log.info(f"{RECV}{EVT} {event}.", LogColor.MAGENTA)
        self.event_count += 1

        if isinstance(event, OrderRejected):
            self._manager.handle_order_rejected(event)
        elif isinstance(event, OrderCanceled):
            self._manager.handle_order_canceled(event)
        elif isinstance(event, OrderExpired):
            self._manager.handle_order_expired(event)
        elif isinstance(event, OrderUpdated):
            self._manager.handle_order_updated(event)
        elif isinstance(event, OrderFilled):
            self._manager.handle_order_filled(event)
        elif isinstance(event, PositionEvent):
            self._manager.handle_position_event(event)

        if not isinstance(event, OrderEvent):
            return

        cdef Order order = self.cache.order(event.client_order_id)
        if order is None:
            return  # Order not in cache yet

        cdef MatchingCore matching_core = None
        if order.is_closed_c():
            matching_core = self._matching_cores.get(order.instrument_id)
            if matching_core is not None:
                matching_core.delete_order(order)

    cpdef void on_stop(self):
        pass

    cpdef void on_reset(self):
        self._manager.reset()
        self._matching_cores.clear()

        self.command_count = 0
        self.event_count = 0

    cpdef void on_dispose(self):
        pass

# -------------------------------------------------------------------------------------------------

    cpdef void execute(self, TradingCommand command):
        """
        Execute the given command.

        Parameters
        ----------
        command : TradingCommand
            The command to execute.

        """
        Condition.not_none(command, "command")

        if self.debug:
            self._log.info(f"{RECV}{CMD} {command}.", LogColor.MAGENTA)
        self.command_count += 1

        if isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, SubmitOrderList):
            self._handle_submit_order_list(command)
        elif isinstance(command, ModifyOrder):
            self._handle_modify_order(command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(command)
        elif isinstance(command, CancelAllOrders):
            self._handle_cancel_all_orders(command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cpdef MatchingCore create_matching_core(
        self,
        InstrumentId instrument_id,
        Price price_increment,
    ):
        """
        Create an internal matching core for the given `instrument`.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the matching core.
        price_increment : Price
            The minimum price increment (tick size) for the matching core.

        Returns
        -------
        MatchingCore

        Raises
        ------
        KeyError
            If a matching core for the given `instrument_id` already exists.

        """
        Condition.not_in(instrument_id, self._matching_cores, "instrument_id", "self._matching_cores")

        matching_core = MatchingCore(
            instrument_id=instrument_id,
            price_increment=price_increment,
            trigger_stop_order=self._trigger_stop_order,
            fill_market_order=self._fill_market_order,
            fill_limit_order=self._fill_limit_order,
        )

        self._matching_cores[instrument_id] = matching_core

        if self.debug:
            self._log.info(f"Created matching core for {instrument_id}.", LogColor.MAGENTA)

        return matching_core

    cdef void _handle_submit_order(self, SubmitOrder command):
        cdef Order order = command.order
        cdef TriggerType emulation_trigger = command.order.emulation_trigger
        Condition.not_equal(emulation_trigger, TriggerType.NO_TRIGGER, "command.order.emulation_trigger", "TriggerType.NO_TRIGGER")
        Condition.not_in(command.order.client_order_id, self._manager.get_submit_order_commands(), "command.order.client_order_id", "self._commands_submit_order")

        if emulation_trigger not in SUPPORTED_TRIGGERS:
            self._log.error(
                f"Cannot emulate order: `TriggerType` {trigger_type_to_str(emulation_trigger)} not supported.")
            self._manager.cancel_order(order=order)
            return

        self._check_monitoring(command.strategy_id, command.position_id)

        cdef InstrumentId trigger_instrument_id = order.instrument_id if order.trigger_instrument_id is None else order.trigger_instrument_id
        cdef MatchingCore matching_core = self._matching_cores.get(trigger_instrument_id)
        if matching_core is None:
            if trigger_instrument_id.is_synthetic():
                synthetic = self.cache.synthetic(trigger_instrument_id)
                if synthetic is None:
                    self._log.error(
                        f"Cannot emulate order: no synthetic instrument {trigger_instrument_id} for trigger.",
                    )
                    self._manager.cancel_order(order=order)
                    return
                matching_core = self.create_matching_core(synthetic.id, synthetic.price_increment)
            else:
                instrument = self.cache.instrument(trigger_instrument_id)
                if instrument is None:
                    self._log.error(
                        f"Cannot emulate order: no instrument {trigger_instrument_id} for trigger.",
                    )
                    self._manager.cancel_order(order=order)
                    return
                matching_core = self.create_matching_core(instrument.id, instrument.price_increment)

        # Update trailing stop
        if order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
            self._update_trailing_stop_order(matching_core, order)
            if order.trigger_price is None:
                self.log.error(
                    "Cannot handle trailing stop order with no `trigger_price` and no market updates.",
                )
                self._manager.cancel_order(order)
                return

        # Cache command
        self._manager.cache_submit_order_command(command)

        # Check if immediately marketable (initial match)
        matching_core.match_order(order, initial=True)

        # Check data subscription
        if emulation_trigger == TriggerType.DEFAULT or emulation_trigger == TriggerType.BID_ASK:
            if trigger_instrument_id not in self._subscribed_quotes:
                self.subscribe_quote_ticks(trigger_instrument_id)
                self._subscribed_quotes.add(trigger_instrument_id)
        elif emulation_trigger == TriggerType.LAST_TRADE:
            if trigger_instrument_id not in self._subscribed_trades:
                self.subscribe_trade_ticks(trigger_instrument_id)
                self._subscribed_trades.add(trigger_instrument_id)
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `TriggerType`, was {emulation_trigger}",  # pragma: no cover (design-time error)
            )

        if order.client_order_id not in self._manager.get_submit_order_commands():
            return  # Already released

        # Hold in matching core
        matching_core.add_order(order)

        cdef OrderEmulated event
        if order.status_c() == OrderStatus.INITIALIZED:
            # Generate event
            event = OrderEmulated(
                trader_id=order.trader_id,
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                event_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            order.apply(event)
            self.cache.update_order(order)

            self._manager.send_risk_event(event)

            # Publish event
            self._msgbus.publish_c(
                topic=f"events.order.{order.strategy_id.to_str()}",
                msg=event,
            )

        self.log.info(f"Emulating {command.order}.", LogColor.MAGENTA)

    cdef void _handle_submit_order_list(self, SubmitOrderList command):
        self._check_monitoring(command.strategy_id, command.position_id)

        cdef Order order
        for order in command.order_list.orders:
            if order.parent_order_id is not None:
                parent_order = self.cache.order(order.parent_order_id)
                assert parent_order, f"Parent order for {repr(order.client_order_id)} not found"
                if parent_order.contingency_type == ContingencyType.OTO:
                    continue  # Process contingency order later once parent triggered
            self._manager.create_new_submit_order(
                order=order,
                position_id=command.position_id,
                client_id=command.client_id,
            )

    cdef void _handle_modify_order(self, ModifyOrder command):
        cdef Order order = self.cache.order(command.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot modify order: {repr(order.client_order_id)} not found.",
            )
            return

        cdef Price price = command.price
        if price is None and order.has_price_c():
            price = order.price

        cdef Price trigger_price = command.trigger_price
        if trigger_price is None and order.has_trigger_price_c():
            trigger_price = order.trigger_price

        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,  # Could be None
            account_id=order.account_id,  # Could be None
            quantity=command.quantity or order.quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self._manager.send_exec_event(event)

        cdef InstrumentId trigger_instrument_id = order.instrument_id if order.trigger_instrument_id is None else order.trigger_instrument_id
        cdef MatchingCore matching_core = self._matching_cores.get(trigger_instrument_id)
        if matching_core is None:
            self._log.error(
                f"Cannot handle `ModifyOrder`: no matching core for trigger instrument {trigger_instrument_id}.",
            )
            return

        matching_core.match_order(order)
        if order.side == OrderSide.BUY:
            matching_core.sort_bid_orders()
        elif order.side == OrderSide.SELL:
            matching_core.sort_ask_orders()
        else:
            raise RuntimeError("invalid `OrderSide`")  # pragma: no cover (design-time error)

    cdef void _handle_cancel_order(self, CancelOrder command):
        cdef Order order = self.cache.order(command.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot cancel order: {repr(command.client_order_id)} not found.",
            )
            return

        cdef InstrumentId trigger_instrument_id = order.instrument_id if order.trigger_instrument_id is None else order.trigger_instrument_id
        cdef MatchingCore matching_core = self._matching_cores.get(trigger_instrument_id)
        if matching_core is None:
            self._manager.cancel_order(order)
            return

        if not matching_core.order_exists(order.client_order_id) and order.is_open_c() and not order.is_pending_cancel_c():
            # Order not held in the emulator
            self._manager.send_exec_command(command)
        else:
            self._manager.cancel_order(order)

    cdef void _handle_cancel_all_orders(self, CancelAllOrders command):
        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)
        if matching_core is None:
            # No orders to cancel
            return

        cdef list orders
        if command.order_side == OrderSide.NO_ORDER_SIDE:
            orders = matching_core.get_orders()
        elif command.order_side == OrderSide.BUY:
            orders = matching_core.get_orders_bid()
        elif command.order_side == OrderSide.SELL:
            orders = matching_core.get_orders_ask()
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `OrderSide`, was {command.order_side}",  # pragma: no cover (design-time error)
            )

        cdef Order order
        for order in orders:
            self._manager.cancel_order(order)

    cpdef void _check_monitoring(self, StrategyId strategy_id, PositionId position_id):
        if strategy_id not in self._subscribed_strategies:
            # Subscribe to all strategy events
            self._msgbus.subscribe(topic=f"events.order.{strategy_id.to_str()}", handler=self.on_event)
            self._msgbus.subscribe(topic=f"events.position.{strategy_id.to_str()}", handler=self.on_event)
            self._subscribed_strategies.add(strategy_id)
            self._log.info(f"Subscribed to strategy {strategy_id.to_str()} order and position events.", LogColor.BLUE)

        if position_id is not None and position_id not in self._monitored_positions:
            self._monitored_positions.add(position_id)

    cpdef void _cancel_order(self, Order order):
        if order is None:
            self._log.error(
                f"Cannot cancel order: order for {repr(order.client_order_id)} not found.",
            )
            return

        if self.debug:
            self._log.info(f"Cancelling order {order.client_order_id!r}.", LogColor.MAGENTA)

        # Remove emulation trigger
        order.emulation_trigger = TriggerType.NO_TRIGGER

        cdef InstrumentId trigger_instrument_id = order.instrument_id if order.trigger_instrument_id is None else order.trigger_instrument_id
        cdef MatchingCore matching_core = self._matching_cores.get(trigger_instrument_id)
        if matching_core is not None:
            matching_core.delete_order(order)

# -------------------------------------------------------------------------------------------------

    cpdef void _trigger_stop_order(self, Order order):
        if (
            order.order_type == OrderType.STOP_LIMIT
            or order.order_type == OrderType.LIMIT_IF_TOUCHED
            or order.order_type == OrderType.TRAILING_STOP_LIMIT
        ):
            self._fill_limit_order(order)
        elif (
            order.order_type == OrderType.STOP_MARKET
            or order.order_type == OrderType.MARKET_IF_TOUCHED
            or order.order_type == OrderType.TRAILING_STOP_MARKET
        ):
            self._fill_market_order(order)
        else:
            raise RuntimeError(f"invalid `OrderType`, was {order.type_string_c()}")  # pragma: no cover (design-time error)

    cpdef void _fill_market_order(self, Order order):
        # Fetch command
        cdef SubmitOrder command = self._manager.pop_submit_order_command(order.client_order_id)
        if command is None:
            raise RuntimeError("invalid operation `_fill_market_order` with no command")  # pragma: no cover (design-time error)

        cdef InstrumentId trigger_instrument_id = order.instrument_id if order.trigger_instrument_id is None else order.trigger_instrument_id
        cdef MatchingCore matching_core = self._matching_cores.get(trigger_instrument_id)
        if matching_core is not None:
            matching_core.delete_order(order)

        order.emulation_trigger = TriggerType.NO_TRIGGER
        cdef MarketOrder transformed = MarketOrder.transform(order, self.clock.timestamp_ns())

        # Cast to writable cache
        cdef Cache cache = <Cache>self.cache
        cache.add_order(
            transformed,
            command.position_id,
            command.client_id,
            override=True,
        )

        # Replace commands order with transformed order
        command.order = transformed

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=transformed.last_event_c(),
        )

        # Determine triggered price
        if order.side == OrderSide.BUY:
            released_price = matching_core.ask
        elif order.side == OrderSide.SELL:
            released_price = matching_core.bid
        else:
            raise RuntimeError("invalid `OrderSide`")  # pragma: no cover (design-time error)

        # Generate event
        cdef OrderReleased event = OrderReleased(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            released_price=released_price,
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        transformed.apply(event)
        self.cache.update_order(transformed)

        self._manager.send_risk_event(event)

        self.log.info(f"Releasing {transformed}...", LogColor.MAGENTA)

        # Publish event
        self._msgbus.publish_c(
            topic=f"events.order.{transformed.strategy_id.to_str()}",
            msg=event,
        )

        if order.exec_algorithm_id is not None:
            self._manager.send_algo_command(command)
        else:
            self._manager.send_exec_command(command)

    cpdef void _fill_limit_order(self, Order order):
        if order.order_type == OrderType.LIMIT:
            self._fill_market_order(order)
            return

        # Fetch command
        cdef SubmitOrder command = self._manager.pop_submit_order_command(order.client_order_id)
        if command is None:
            return  # Order already released

        cdef InstrumentId trigger_instrument_id = order.instrument_id if order.trigger_instrument_id is None else order.trigger_instrument_id
        cdef MatchingCore matching_core = self._matching_cores.get(trigger_instrument_id)
        if matching_core is not None:
            matching_core.delete_order(order)

        order.emulation_trigger = TriggerType.NO_TRIGGER
        cdef LimitOrder transformed = LimitOrder.transform(order, self.clock.timestamp_ns())

        # Cast to writable cache
        cdef Cache cache = <Cache>self.cache
        cache.add_order(
            transformed,
            command.position_id,
            command.client_id,
            override=True,
        )

        # Replace commands order with transformed order
        command.order = transformed

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=transformed.last_event_c(),
        )

        # Determine triggered price
        if order.side == OrderSide.BUY:
            released_price = matching_core.ask
        elif order.side == OrderSide.SELL:
            released_price = matching_core.bid
        else:
            raise RuntimeError("invalid `OrderSide`")  # pragma: no cover (design-time error)

        # Generate event
        cdef OrderReleased event = OrderReleased(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            released_price=released_price,
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        transformed.apply(event)
        self.cache.update_order(transformed)

        self._manager.send_risk_event(event)

        self.log.info(f"Releasing {transformed}...", LogColor.MAGENTA)

        # Publish event
        self._msgbus.publish_c(
            topic=f"events.order.{transformed.strategy_id.to_str()}",
            msg=event,
        )

        if order.exec_algorithm_id is not None:
            self._manager.send_algo_command(command)
        else:
            self._manager.send_exec_command(command)

    cpdef void on_quote_tick(self, QuoteTick tick):
        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(tick)}...", LogColor.CYAN)

        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `QuoteTick`: no matching core for instrument {tick.instrument_id}.")
            return

        matching_core.set_bid_raw(tick._mem.bid_price.raw)
        matching_core.set_ask_raw(tick._mem.ask_price.raw)

        self._iterate_orders(matching_core)

    cpdef void on_trade_tick(self, TradeTick tick):
        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(tick)}...", LogColor.CYAN)

        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `TradeTick`: no matching core for instrument {tick.instrument_id}.")
            return

        matching_core.set_last_raw(tick._mem.price.raw)
        if tick.instrument_id not in self._subscribed_quotes:
            matching_core.set_bid_raw(tick._mem.price.raw)
            matching_core.set_ask_raw(tick._mem.price.raw)

        self._iterate_orders(matching_core)

    cdef void _iterate_orders(self, MatchingCore matching_core):
        matching_core.iterate(self._clock.timestamp_ns())

        cdef list orders = matching_core.get_orders()
        cdef Order order
        for order in orders:
            if order.is_closed_c():
                continue

            # Manage trailing stop
            if order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
                self._update_trailing_stop_order(matching_core, order)

    cdef void _update_trailing_stop_order(self, MatchingCore matching_core, Order order):
        # TODO(cs): Improve efficiency of this ---------------------------------
        cdef Price bid = None
        cdef Price ask = None
        cdef Price last = None
        if matching_core.is_bid_initialized:
            bid = Price.from_raw_c(matching_core.bid_raw, matching_core.price_precision)
        if matching_core.is_ask_initialized:
            ask = Price.from_raw_c(matching_core.ask_raw, matching_core.price_precision)
        if matching_core.is_last_initialized:
            last = Price.from_raw_c(matching_core.last_raw, matching_core.price_precision)

        cdef QuoteTick quote_tick = self.cache.quote_tick(matching_core.instrument_id)
        cdef TradeTick trade_tick = self.cache.trade_tick(matching_core.instrument_id)
        if bid is None and quote_tick is not None:
            bid = quote_tick.bid_price
        if ask is None and quote_tick is not None:
            ask = quote_tick.ask_price
        if last is None and trade_tick is not None:
            last = trade_tick.price
        # TODO(cs): ------------------------------------------------------------

        cdef tuple output
        try:
            output = TrailingStopCalculator.calculate(
                price_increment=matching_core.price_increment,
                order=order,
                bid=bid,
                ask=ask,
                last=last,
            )
        except RuntimeError as e:  # pragma: no cover (design-time error)
            self._log.warning(f"Cannot calculate trailing stop order: {e}")
            return

        cdef Price new_trigger_price = output[0]
        cdef Price new_price = output[1]
        if new_trigger_price is None and new_price is None:
            return  # No updates

        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=None,  # Not yet assigned by any venue
            account_id=order.account_id,  # Probably None
            quantity=order.quantity,
            price=new_price,
            trigger_price=new_trigger_price,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        order.apply(event)
        self.cache.update_order(order)

        self._manager.send_risk_event(event)
