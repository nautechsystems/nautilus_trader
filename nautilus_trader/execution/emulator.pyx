# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.trailing_calculator cimport TrailingStopCalculator
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.msgbus.bus cimport MessageBus


cdef tuple SUPPORTED_TRIGGERS = (TriggerType.DEFAULT, TriggerType.BID_ASK, TriggerType.LAST)


cdef class OrderEmulator(Actor):
    """
    Provides order emulation for specified trigger types.
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
        super().__init__()

        self.register_base(
            trader_id=trader_id,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._commands: dict[ClientOrderId, SubmitOrder] = {}
        self._matching_cores: dict[InstrumentId, MatchingCore]  = {}

        self._subscribed_quotes: set[InstrumentId] = set()
        self._subscribed_trades: set[InstrumentId] = set()

        # Register endpoints
        self._msgbus.register(endpoint="OrderEmulator.execute", handler=self.execute)

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _start(self) except *:
        cdef list emulated_orders = self.cache.orders_emulated()
        if not emulated_orders:
            self._log.info("No emulated orders to reactivate.")
            return

        cdef int emulated_count = len(emulated_orders)
        self._log.info(f"Reactivating {emulated_count} emulated order{'' if emulated_count == 1 else 's'}...")

        cdef:
            Order order
            SubmitOrder command
        for order in emulated_orders:
            command = self.cache.load_submit_order_command(order.client_order_id)
            if command is None:
                self._log.error(
                    f"Cannot load `SubmitOrder` command for {repr(order.client_order_id)}: not found in cache."
                )
                continue
            self._log.info(f"Loaded {command}.", LogColor.BLUE)
            self._handle_submit_order(command)

    cpdef void _stop(self) except *:
        pass

    cpdef void _reset(self) except *:
        self._commands.clear()
        self._matching_cores.clear()

    cpdef void _dispose(self) except *:
        pass

# -------------------------------------------------------------------------------------------------

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

    def get_commands(self) -> dict[ClientOrderId, SubmitOrder]:
        """
        Return the emulators cached commands.

        Returns
        -------
        dict[ClientOrderId, SubmitOrder]

        """
        return self._commands.copy()

    def get_matching_core(self, InstrumentId instrument_id) -> Optional[MatchingCore]:
        """
        Return the emulators matching core for the given instrument ID.

        Returns
        -------
        MatchingCore or ``None``

        """
        return self._matching_cores.get(instrument_id)

    cpdef void execute(self, TradingCommand command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : TradingCommand
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._log.debug(f"{RECV}{CMD} {command}.", LogColor.MAGENTA)

        if isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, ModifyOrder):
            self._handle_modify_order(command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(command)
        elif isinstance(command, CancelAllOrders):
            self._handle_cancel_all_orders(command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cdef void _handle_submit_order(self, SubmitOrder command) except *:
        cdef Order order = command.order
        cdef TriggerType emulation_trigger = command.order.emulation_trigger
        Condition.not_equal(emulation_trigger, TriggerType.NONE, "command.order.emulation_trigger", "TriggerType.NONE")
        Condition.not_in(command.order.client_order_id, self._commands, "command.order.client_order_id", "self._commands")

        if emulation_trigger not in SUPPORTED_TRIGGERS:
            self._log.error(
                f"Cannot emulate order: `TriggerType` {TriggerTypeParser.to_str(emulation_trigger)} "
                f"not supported.",
            )
            self._cancel_order(matching_core=None, order=order)
            return

        # Cache command
        self._commands[order.client_order_id] = command

        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)
        if matching_core is None:
            instrument = self.cache.instrument(command.instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot emulate order: no instrument for {command.instrument_id}.",
                )
                self._cancel_order(matching_core=None, order=order)
                return

            matching_core = MatchingCore(
                instrument=instrument,
                trigger_stop_order=self.trigger_stop_order,
                fill_market_order=self.fill_market_order,
                fill_limit_order=self.fill_limit_order,
            )
            self._matching_cores[instrument.id] = matching_core

        # Hold in matching core
        matching_core.add_order(order)

        # Check data subscription
        if emulation_trigger == TriggerType.DEFAULT or emulation_trigger == TriggerType.BID_ASK:
            if command.instrument_id not in self._subscribed_quotes:
                self.subscribe_quote_ticks(command.instrument_id)
                self._subscribed_quotes.add(command.instrument_id)
        elif emulation_trigger == TriggerType.LAST:
            if command.instrument_id not in self._subscribed_trades:
                self.subscribe_trade_ticks(command.instrument_id)
                self._subscribed_trades.add(command.instrument_id)
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `TriggerType`, was {emulation_trigger}",
            )

        # Manage trailing stop
        if order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
            self._update_trailing_stop_order(matching_core, order)

        self.log.info(f"Emulating {command.order.info()}...")

    cdef void _handle_modify_order(self, ModifyOrder command) except *:
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
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=None,  # Not yet assigned by any venue
            account_id=order.account_id,  # Probably None
            quantity=command.quantity or order.quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _handle_cancel_order(self, CancelOrder command) except *:
        cdef Order order = self.cache.order(command.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot cancel order: order for {repr(order.client_order_id)} not found.",
            )
            return

        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `CancelOrder`: no matching core for {command.instrument_id}.")
            return

        if not matching_core.order_exists(command.client_order_id):
            # Order not held by the emulator
            self._send_exec_command(command)
        else:
            self._cancel_order(matching_core, order)

    cdef void _handle_cancel_all_orders(self, CancelAllOrders command) except *:
        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)
        if matching_core is None:
            # No orders to cancel
            return

        cdef list orders
        if command.order_side == OrderSide.NONE:
            orders = matching_core.get_orders()
        elif command.order_side == OrderSide.BUY:
            orders = matching_core.get_orders_bid()
        elif command.order_side == OrderSide.SELL:
            orders = matching_core.get_orders_ask()
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `OrderSide`, was {command.order_side}",
            )

        cdef Order order
        for order in orders:
            self._cancel_order(matching_core, order)

    cdef void _cancel_order(self, MatchingCore matching_core, Order order) except *:
        # Remove emulation trigger
        order.emulation_trigger = TriggerType.NONE

        if matching_core is not None:
            matching_core.delete_order(order)

        cdef SubmitOrder command = self._commands.pop(order.client_order_id, None)
        if command is None:
            self._log.warning(
                f"`SubmitOrder` command for {repr(order.client_order_id)} not found.",
            )

        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderCanceled event = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,  # Probably None
            account_id=order.account_id,  # Probably None
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._send_exec_event(event)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void trigger_stop_order(self, Order order) except *:
        cdef OrderTriggered event
        if (
            order.order_type == OrderType.STOP_LIMIT
            or order.order_type == OrderType.TRAILING_STOP_LIMIT
        ):
            # Generate event
            event = OrderTriggered(
                trader_id=order.trader_id,
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,  # Probably None
                account_id=order.account_id,  # Probably None
                event_id=UUID4(),
                ts_event=self._clock.timestamp_ns(),
                ts_init=self._clock.timestamp_ns(),
            )
            self._send_exec_event(event)

        if (
            order.order_type == OrderType.STOP_MARKET
            or order.order_type == OrderType.MARKET_IF_TOUCHED
            or order.order_type == OrderType.TRAILING_STOP_MARKET
        ):
            # Liquidity side is ignored in this case
            self.fill_market_order(order, LiquiditySide.TAKER)
        elif (
            order.order_type == OrderType.STOP_LIMIT
            or order.order_type == OrderType.LIMIT_IF_TOUCHED
            or order.order_type == OrderType.TRAILING_STOP_LIMIT
        ):
            # Liquidity side is ignored in this case
            self.fill_limit_order(order, LiquiditySide.TAKER)
        else:
            raise RuntimeError("invalid `OrderType`")  # pragma: no cover (design-time error)

    cpdef void fill_market_order(self, Order order, LiquiditySide liquidity_side) except *:
        self.log.info(f"Releasing {order}...")

        # Fetch command
        cdef SubmitOrder command = self._commands.pop(order.client_order_id, None)
        if command is None:
            self._log.error(
                f"`SubmitOrder` command for {repr(order.client_order_id)} not found.",
            )
            return

        cdef MatchingCore matching_core = self._matching_cores.get(order.instrument_id)
        if matching_core is None:
            raise RuntimeError(f"No matching core for {order.instrument_id}")

        matching_core.delete_order(order)

        cdef MarketOrder transformed = self._transform_to_market_order(order)

        # Cast to writable cache
        cdef Cache cache = <Cache>self.cache
        cache.add_order(transformed, command.position_id, override=True)

        # Replace commands order with transformed order
        command.order = transformed

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=transformed.last_event_c(),
        )

        self._send_exec_command(command)

    cpdef void fill_limit_order(self, Order order, LiquiditySide liquidity_side) except *:
        if order.order_type == OrderType.LIMIT:
            self.fill_market_order(order, liquidity_side)
            return

        self.log.info(f"Releasing {order}...")

        # Fetch command
        cdef SubmitOrder command = self._commands.pop(order.client_order_id, None)
        if command is None:
            self._log.error(
                f"`SubmitOrder` command for {repr(order.client_order_id)} not found.",
            )
            return

        cdef MatchingCore matching_core = self._matching_cores.get(order.instrument_id)
        if matching_core is None:
            raise RuntimeError(f"No matching core for {order.instrument_id}")

        matching_core.delete_order(order)

        cdef LimitOrder transformed = self._transform_to_limit_order(order)

        # Cast to writable cache
        cdef Cache cache = <Cache>self.cache
        cache.add_order(transformed, command.position_id, override=True)

        # Replace commands order with transformed order
        command.order = transformed

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=transformed.last_event_c(),
        )

        self._send_exec_command(command)

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(tick)}...")

        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `QuoteTick`: no matching core for {tick.instrument_id}.")
            return

        matching_core.set_bid(tick._mem.bid)
        matching_core.set_ask(tick._mem.ask)

        self._iterate_orders(matching_core)

    cpdef void on_trade_tick(self, TradeTick tick) except *:
        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(tick)}...")

        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `TradeTick`: no matching core for {tick.instrument_id}.")
            return

        matching_core.set_last(tick._mem.price)
        if tick.instrument_id not in self._subscribed_quotes:
            matching_core.set_bid(tick._mem.price)
            matching_core.set_ask(tick._mem.price)

        self._iterate_orders(matching_core)

    cdef void _iterate_orders(self, MatchingCore matching_core) except *:
        matching_core.iterate(self._clock.timestamp_ns())

        cdef list orders = matching_core.get_orders()
        cdef Order order
        for order in orders:
            if order.is_closed_c():
                continue

            # Manage trailing stop
            if order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
                self._update_trailing_stop_order(matching_core, order)

    cdef void _update_trailing_stop_order(self, MatchingCore matching_core, Order order) except *:
        cdef Instrument instrument = self.cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot update order: no instrument for {order.instrument_id}.",
            )
            return

        # TODO(cs): Improve efficiency of this ---------------------------------
        cdef Price bid = None
        cdef Price ask = None
        cdef Price last = None
        if matching_core.is_bid_initialized:
            bid = Price.from_raw_c(matching_core.bid_raw, instrument.price_precision)
        if matching_core.is_ask_initialized:
            ask = Price.from_raw_c(matching_core.ask_raw, instrument.price_precision)
        if matching_core.is_last_initialized:
            last = Price.from_raw_c(matching_core.last_raw, instrument.price_precision)

        cdef QuoteTick quote_tick = self.cache.quote_tick(instrument.id)
        cdef TradeTick trade_tick = self.cache.trade_tick(instrument.id)
        if bid is None and quote_tick is not None:
            bid = quote_tick.bid
        if ask is None and quote_tick is not None:
            ask = quote_tick.ask
        if last is None and trade_tick is not None:
            last = trade_tick.price
        # TODO(cs): ------------------------------------------------------------

        cdef tuple output
        try:
            output = TrailingStopCalculator.calculate(
                instrument=instrument,
                order=order,
                bid=bid,
                ask=ask,
                last=last,
            )
        except RuntimeError as e:
            self._log.warning(f"Cannot calculate trailing stop order: {e}")
            return

        cdef Price new_trigger_price = output[0]
        cdef Price new_price = output[1]
        if new_trigger_price is None and new_price is None:
            return  # No updates

        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
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
            ts_event=timestamp,
            ts_init=timestamp,
        )
        order.apply(event)

        self._send_risk_event(event)

    cdef MarketOrder _transform_to_market_order(self, Order order):
        cdef list original_events = order.events_c()
        cdef MarketOrder transformed = MarketOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            order_side=order.side,
            quantity=order.quantity,
            time_in_force=order.time_in_force,
            reduce_only=order.is_reduce_only,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            contingency_type=order.contingency_type,
            order_list_id=order.order_list_id,
            linked_order_ids=order.linked_order_ids,
            parent_order_id=order.parent_order_id,
            tags=order.tags,
        )

        self._hydrate_initial_events(original=order, transformed=transformed)

        return transformed

    cdef LimitOrder _transform_to_limit_order(self, Order order):
        cdef LimitOrder transformed = LimitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            order_side=order.side,
            quantity=order.quantity,
            price=order.price,
            time_in_force=order.time_in_force,
            expire_time_ns=order.expire_time_ns,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            post_only=order.is_post_only,
            reduce_only=order.is_reduce_only,
            display_qty=order.display_qty,
            contingency_type=order.contingency_type,
            order_list_id=order.order_list_id,
            linked_order_ids=order.linked_order_ids,
            parent_order_id=order.parent_order_id,
            tags=order.tags,
        )

        self._hydrate_initial_events(original=order, transformed=transformed)

        return transformed

    cdef void _hydrate_initial_events(self, Order original, Order transformed) except *:
        cdef list original_events = original.events_c()

        cdef OrderEvent event
        for event in reversed(original_events):
            # Insert each event to the beginning of the events list in reverse
            # to preserve correct order of events.
            transformed._events.insert(0, event)

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_risk_command(self, TradingCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="RiskEngine.execute", msg=command)

    cdef void _send_exec_command(self, TradingCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)

    cdef void _send_risk_event(self, OrderEvent event) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{EVT}{SENT} {event}.")
        self._msgbus.send(endpoint="RiskEngine.process", msg=event)

    cdef void _send_exec_event(self, OrderEvent event) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{EVT}{SENT} {event}.")
        self._msgbus.send(endpoint="ExecEngine.process", msg=event)
