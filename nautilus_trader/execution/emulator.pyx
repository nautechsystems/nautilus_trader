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
from nautilus_trader.execution.algorithm cimport ExecAlgorithmSpecification
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
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.enums_c cimport trigger_type_to_str
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderRejected
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
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._matching_cores: dict[InstrumentId, MatchingCore]  = {}
        self._commands_submit_order: dict[ClientOrderId, SubmitOrder] = {}
        self._commands_submit_order_list: dict[OrderListId, SubmitOrderList] = {}

        self._subscribed_quotes: set[InstrumentId] = set()
        self._subscribed_trades: set[InstrumentId] = set()
        self._subscribed_strategies: set[StrategyId] = set()
        self._monitored_positions: set[PositionId] = set()

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
        return self._commands_submit_order.copy()

    def get_submit_order_list_commands(self) -> dict[OrderListId, SubmitOrderList]:
        """
        Return the emulators cached submit order list commands.

        Returns
        -------
        dict[OrderListId, SubmitOrderList]

        """
        return self._commands_submit_order_list.copy()

    def get_matching_core(self, InstrumentId instrument_id) -> Optional[MatchingCore]:
        """
        Return the emulators matching core for the given instrument ID.

        Returns
        -------
        MatchingCore or ``None``

        """
        return self._matching_cores.get(instrument_id)

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void on_start(self) except *:
        cdef list emulated_orders = self.cache.orders_emulated()
        if not emulated_orders:
            self._log.info("No emulated orders to reactivate.")
            return

        cdef int emulated_count = len(emulated_orders)
        self._log.info(f"Reactivating {emulated_count} emulated order{'' if emulated_count == 1 else 's'}...")

        cdef:
            Order order
            TradingCommand command
        for order in emulated_orders:
            if order.order_list_id is not None:
                command = self.cache.load_submit_order_list_command(order.order_list_id)
                if command is None:
                    self._log.error(
                        f"Cannot load `SubmitOrderList` command for {repr(order.order_list_id)}: not found in cache."
                    )
                    continue
                self._log.info(f"Loaded {command}.", LogColor.BLUE)
                self._handle_submit_order_list(command)
            else:
                command = self.cache.load_submit_order_command(order.client_order_id)
                if command is None:
                    self._log.error(
                        f"Cannot load `SubmitOrder` command for {repr(order.client_order_id)}: not found in cache."
                    )
                    continue
                self._log.info(f"Loaded {command}.", LogColor.BLUE)
                self._handle_submit_order(command)

    cpdef void on_event(self, Event event) except *:
        self._log.info(f"Received {event}", LogColor.MAGENTA)
        if isinstance(event, OrderRejected):
            self._handle_order_rejected(event)
        elif isinstance(event, OrderCanceled):
            self._handle_order_canceled(event)
        elif isinstance(event, OrderExpired):
            self._handle_order_expired(event)
        elif isinstance(event, OrderUpdated):
            self._handle_order_updated(event)
        elif isinstance(event, OrderFilled):
            self._handle_order_filled(event)

    cpdef void on_stop(self) except *:
        pass

    cpdef void on_reset(self) except *:
        self._commands_submit_order.clear()
        self._commands_submit_order_list.clear()
        self._matching_cores.clear()

    cpdef void on_dispose(self) except *:
        pass

# -------------------------------------------------------------------------------------------------

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

    cpdef MatchingCore create_matching_core(self, Instrument instrument):
        """
        Create an internal matching core for the given `instrument`.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the matching core.

        Returns
        -------
        MatchingCore

        Raises
        ------
        RuntimeError
            If a matching core for the given `instrument` already exists.

        """
        if instrument.id in self._matching_cores:
            raise RuntimeError(f"A matching core already exists for {instrument.id}.")

        matching_core = MatchingCore(
            instrument=instrument,
            trigger_stop_order=self._trigger_stop_order,
            fill_market_order=self._fill_market_order,
            fill_limit_order=self._fill_limit_order,
        )

        self._matching_cores[instrument.id] = matching_core
        self._log.debug(f"Created matching core for {instrument.id.to_str()}.")

        return matching_core

    cdef void _handle_submit_order(self, SubmitOrder command) except *:
        cdef Order order = command.order
        cdef TriggerType emulation_trigger = command.order.emulation_trigger
        Condition.not_equal(emulation_trigger, TriggerType.NO_TRIGGER, "command.order.emulation_trigger", "TriggerType.NO_TRIGGER")
        Condition.not_in(command.order.client_order_id, self._commands_submit_order, "command.order.client_order_id", "self._commands_submit_order")

        if emulation_trigger not in SUPPORTED_TRIGGERS:
            self._log.error(
                f"Cannot emulate order: `TriggerType` {trigger_type_to_str(emulation_trigger)} "
                f"not supported.",
            )
            self._cancel_order(matching_core=None, order=order)
            return

        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)
        if matching_core is None:
            instrument = self.cache.instrument(command.instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot emulate order: no instrument for {command.instrument_id}.",
                )
                self._cancel_order(matching_core=None, order=order)
                return
            matching_core = self.create_matching_core(instrument)

        # Update trailing stop
        if order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
            self._update_trailing_stop_order(matching_core, order)
            if order.trigger_price is None:
                self.log.error(
                    "Cannot handle trailing stop order with no `trigger_price` and no market updates.",
                )
                self._cancel_order(None, order)
                return

        # Cache command
        self._commands_submit_order[order.client_order_id] = command

        # Check if immediately marketable (initial match)
        matching_core.match_order(order, initial=True)

        # Check data subscription
        if emulation_trigger == TriggerType.DEFAULT or emulation_trigger == TriggerType.BID_ASK:
            if command.instrument_id not in self._subscribed_quotes:
                self.subscribe_quote_ticks(command.instrument_id)
                self._subscribed_quotes.add(command.instrument_id)
        elif emulation_trigger == TriggerType.LAST_TRADE:
            if command.instrument_id not in self._subscribed_trades:
                self.subscribe_trade_ticks(command.instrument_id)
                self._subscribed_trades.add(command.instrument_id)
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `TriggerType`, was {emulation_trigger}",  # pragma: no cover (design-time error)
            )

        if order.client_order_id not in self._commands_submit_order:
            return  # Already released

        # Hold in matching core
        matching_core.add_order(order)

        self.log.info(f"Emulating {command.order}.", LogColor.MAGENTA)

    cdef void _handle_submit_order_list(self, SubmitOrderList command) except *:
        Condition.not_in(command.order_list.id, self._commands_submit_order_list, "command.order_list.id", "self._commands_submit_order_list")

        # Cache command
        self._commands_submit_order_list[command.order_list.id] = command

        # Setup event monitoring
        if command.strategy_id not in self._subscribed_strategies:
            # Subscribe to all strategy events
            self._log.info(f"Subscribing to strategy {command.strategy_id.to_str()} order and position events.", LogColor.BLUE)
            self._msgbus.subscribe(topic=f"events.order.{command.strategy_id.to_str()}", handler=self.on_event)
            self._msgbus.subscribe(topic=f"events.position.{command.strategy_id.to_str()}", handler=self.on_event)
            self._subscribed_strategies.add(command.strategy_id)

        if command.position_id is not None:
            self._monitored_positions.add(command.position_id)

        # Index all execution algorithm specs
        cdef dict exec_algorithm_index = {}
        if command.exec_algorithm_specs:
            exec_algorithm_index = {eas.client_order_id: eas for eas in command.exec_algorithm_specs}

        cdef Order order
        for order in command.order_list.orders:
            if order.parent_order_id is not None:
                parent_order = self.cache.order(order.parent_order_id)
                assert parent_order, f"Parent order for {repr(order.client_order_id)} not found"
                if parent_order.contingency_type == ContingencyType.OTO:
                    continue  # Process contingency order later once triggered
            self._create_new_submit_order(
                order=order,
                position_id=command.position_id,
                exec_algorithm_spec=exec_algorithm_index.get(order.client_order_id),
                client_id=command.client_id,
            )

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
            venue_order_id=order.venue_order_id,  # Could be None
            account_id=order.account_id,  # Could be None
            quantity=command.quantity or order.quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)
        if matching_core is None:
            raise RuntimeError(f"Cannot handle `ModifyOrder`: no matching core for {command.instrument_id}.")  # pragma: no cover (design-time error)

        matching_core.match_order(order)
        if order.side == OrderSide.BUY:
            matching_core.sort_bid_orders()
        elif order.side == OrderSide.SELL:
            matching_core.sort_ask_orders()

    cdef void _handle_cancel_order(self, CancelOrder command) except *:
        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)
        if matching_core is None:
            raise RuntimeError(f"Cannot handle `CancelOrder`: no matching core for {command.instrument_id}.")  # pragma: no cover (design-time error)

        cdef Order order = self.cache.order(command.client_order_id)

        if not matching_core.order_exists(command.client_order_id):
            # Order not held in the emulator
            self._send_exec_command(command)
        else:
            self._cancel_order(matching_core, order)

    cdef void _handle_cancel_all_orders(self, CancelAllOrders command) except *:
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
            self._cancel_order(matching_core, order)

    cdef void _create_new_submit_order(
        self,
        Order order,
        PositionId position_id,
        ExecAlgorithmSpecification exec_algorithm_spec,
        ClientId client_id,
    ) except *:
        cdef SubmitOrder submit = SubmitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            order=order,
            position_id=position_id,
            exec_algorithm_spec=exec_algorithm_spec,
            client_id=client_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        if order.emulation_trigger == TriggerType.NO_TRIGGER:
            # Immediately send back to RiskEngine
            self._send_risk_command(submit)
        else:
            # Emulate
            self._handle_submit_order(submit)

    cdef void _cancel_order(self, MatchingCore matching_core, Order order) except *:
        if order is None:
            self._log.error(
                f"Cannot cancel order: order for {repr(order.client_order_id)} not found.",
            )
            return

        # Remove emulation trigger
        order.emulation_trigger = TriggerType.NO_TRIGGER

        if matching_core is None:
            matching_core = self._matching_cores.get(order.instrument_id)
        if matching_core is not None:
            matching_core.delete_order(order)

        self._commands_submit_order.pop(order.client_order_id, None)

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

    cdef void _handle_order_rejected(self, OrderRejected rejected) except *:
        cdef Order order = self.cache.order(rejected.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderRejected`: "
                f"order for {repr(rejected.client_order_id)} not found. {rejected}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self._handle_contingencies(order)

    cdef void _handle_order_canceled(self, OrderCanceled canceled) except *:
        cdef Order order = self.cache.order(canceled.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderCanceled`: "
                f"order for {repr(canceled.client_order_id)} not found. {canceled}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self._handle_contingencies(order)

    cdef void _handle_order_expired(self, OrderExpired expired) except *:
        cdef Order order = self.cache.order(expired.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderExpired`: "
                f"order for {repr(expired.client_order_id)} not found. {expired}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self._handle_contingencies(order)

    cdef void _handle_order_updated(self, OrderUpdated updated) except *:
        cdef Order order = self.cache.order(updated.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderUpdated`: "
                f"order for {repr(updated.client_order_id)} not found. {updated}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self._handle_contingencies(order)

    cdef void _handle_order_filled(self, OrderFilled filled) except *:
        cdef Order order = self.cache.order(filled.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderFilled`: "
                f"order for {repr(filled.client_order_id)} not found. {filled}",
            )
            return

        cdef MatchingCore matching_core = None
        if order.is_closed_c():
            matching_core = self._matching_cores.get(order.instrument_id)
            if matching_core is not None:
                matching_core.delete_order(order)

        cdef dict exec_algorithm_index = {}
        cdef:
            ClientOrderId client_order_id
            SubmitOrderList submit_order_list
            Order contingent_order
        if order.contingency_type == ContingencyType.OTO:
            assert order.linked_order_ids
            submit_order_list = self._commands_submit_order_list.get(order.order_list_id)
            assert submit_order_list
            # Index all execution algorithm specs
            if submit_order_list.exec_algorithm_specs:
                exec_algorithm_index = {eas.client_order_id: eas for eas in submit_order_list.exec_algorithm_specs}
            for client_order_id in order.linked_order_ids:
                child_order = self.cache.order(client_order_id)
                assert child_order
                self._create_new_submit_order(
                    order=child_order,
                    position_id=submit_order_list.position_id,
                    exec_algorithm_spec=exec_algorithm_index.get(child_order.client_order_id),
                    client_id=submit_order_list.client_id,
                )
        elif order.contingency_type == ContingencyType.OCO:
            # Cancel all OCO orders
            for client_order_id in order.linked_order_ids:
                contingent_order = self.cache.order(client_order_id)
                assert contingent_order
                if contingent_order.client_order_id != order.client_order_id and not contingent_order.is_closed_c():
                    self._cancel_order(matching_core, contingent_order)
        elif order.contingency_type == ContingencyType.OUO:
            self._handle_contingencies(order)

    cdef void _handle_contingencies(self, Order order) except *:
        assert order.linked_order_ids

        cdef MatchingCore matching_core = None
        if order.is_closed_c():
            matching_core = self._matching_cores.get(order.instrument_id)
            if matching_core is not None:
                matching_core.delete_order(order)

        cdef ClientOrderId client_order_id
        cdef Order contingent_order
        for client_order_id in order.linked_order_ids:
            contingent_order = self.cache.order(client_order_id)
            assert contingent_order
            if client_order_id == order.client_order_id:
                continue  # Already being handled
            if contingent_order.is_closed_c() or contingent_order.emulation_trigger == TriggerType.NO_TRIGGER:
                self._commands_submit_order.pop(order.client_order_id, None)
                continue  # Already completed

            if order.is_closed_c():
                self._cancel_order(matching_core, contingent_order)
            elif order.quantity._mem.raw != contingent_order.quantity._mem.raw:
                self._update_order_quantity(contingent_order, order.quantity)
            elif order.leaves_qty._mem.raw != contingent_order.leaves_qty._mem.raw:
                self._update_order_quantity(contingent_order, order.leaves_qty)

    cdef void _update_order_quantity(self, Order order, Quantity new_quantity) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=None,  # Not yet assigned by any venue
            account_id=order.account_id,  # Probably None
            quantity=new_quantity,
            price=None,
            trigger_price=None,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        order.apply(event)

        self._send_risk_event(event)

# -------------------------------------------------------------------------------------------------

    cpdef void _trigger_stop_order(self, Order order) except *:
        cdef OrderTriggered event
        if (
            order.order_type == OrderType.STOP_LIMIT
            or order.order_type == OrderType.LIMIT_IF_TOUCHED
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
            # TODO(cs): Determine a way of publishing event without applying
            order.apply(event)
            # self._send_exec_event(event)
            self._fill_limit_order(order)
        elif (
            order.order_type == OrderType.STOP_MARKET
            or order.order_type == OrderType.MARKET_IF_TOUCHED
            or order.order_type == OrderType.TRAILING_STOP_MARKET
        ):
            self._fill_market_order(order)
        else:
            raise RuntimeError("invalid `OrderType`")  # pragma: no cover (design-time error)

    cpdef void _fill_market_order(self, Order order) except *:
        # Fetch command
        cdef SubmitOrder command = self._commands_submit_order.pop(order.client_order_id, None)
        if command is None:
            self._log.debug(
                f"`SubmitOrder` command for {repr(order.client_order_id)} not found.",
            )
            return

        self.log.info(f"Releasing {order}...")

        cdef MatchingCore matching_core = self._matching_cores.get(order.instrument_id)
        if matching_core is None:
            raise RuntimeError(f"No matching core for {order.instrument_id}")

        matching_core.delete_order(order)

        cdef MarketOrder transformed = MarketOrder.transform(order, self.clock.timestamp_ns())

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

    cpdef void _fill_limit_order(self, Order order) except *:
        if order.order_type == OrderType.LIMIT:
            self._fill_market_order(order)
            return

        # Fetch command
        cdef SubmitOrder command = self._commands_submit_order.pop(order.client_order_id, None)
        if command is None:
            self._log.debug(
                f"`SubmitOrder` command for {repr(order.client_order_id)} not found.",
            )
            return

        self.log.info(f"Releasing {order}...")

        cdef MatchingCore matching_core = self._matching_cores.get(order.instrument_id)
        if matching_core is None:
            raise RuntimeError(f"No matching core for {order.instrument_id}")

        matching_core.delete_order(order)

        cdef LimitOrder transformed = LimitOrder.transform(order, self.clock.timestamp_ns())

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
            self._log.debug(f"Processing {repr(tick)}...", LogColor.CYAN)

        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `QuoteTick`: no matching core for {tick.instrument_id}.")
            return

        matching_core.set_bid_raw(tick._mem.bid.raw)
        matching_core.set_ask_raw(tick._mem.ask.raw)

        self._iterate_orders(matching_core)

    cpdef void on_trade_tick(self, TradeTick tick) except *:
        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(tick)}...", LogColor.CYAN)

        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `TradeTick`: no matching core for {tick.instrument_id}.")
            return

        matching_core.set_last_raw(tick._mem.price.raw)
        if tick.instrument_id not in self._subscribed_quotes:
            matching_core.set_bid_raw(tick._mem.price.raw)
            matching_core.set_ask_raw(tick._mem.price.raw)

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
