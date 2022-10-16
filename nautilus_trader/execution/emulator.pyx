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

from typing import List, Optional

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
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
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

    cpdef void _reset(self) except *:
        self._commands.clear()
        self._matching_cores.clear()

# -------------------------------------------------------------------------------------------------

    @property
    def subscribed_quotes(self) -> List[InstrumentId]:
        """
        Return the subscribed quote feeds for the emulator.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_quotes))

    @property
    def subscribed_trades(self) -> List[InstrumentId]:
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
        cdef TriggerType emulation_trigger = command.order.emulation_trigger
        Condition.not_equal(emulation_trigger, TriggerType.NONE, "command.order.emulation_trigger", "TriggerType.NONE")
        Condition.not_in(command.order.client_order_id, self._commands, "command.order.client_order_id", "self._commands")

        if command.order.emulation_trigger not in SUPPORTED_TRIGGERS:
            raise RuntimeError(
                f"cannot emulate order: `TriggerType` {TriggerTypeParser.to_str(command.emulation_trigger)} "
                f"not supported."
            )

        # Cache command
        self._commands[command.order.client_order_id] = command

        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)

        if matching_core is None:
            instrument = self.cache.instrument(command.instrument_id)
            if instrument is None:
                raise RuntimeError(f"cannot find instrument for {instrument.id}")

            matching_core = MatchingCore(
                instrument=instrument,
                trigger_stop_order=self.trigger_stop_order,
                fill_market_order=self.fill_market_order,
                fill_limit_order=self.fill_limit_order,
            )
            self._matching_cores[instrument.id] = matching_core

        # Hold in matching core
        matching_core.add_order(command.order)

        # Check data subscription
        if emulation_trigger == TriggerType.DEFAULT or emulation_trigger == TriggerType.BID_ASK:
            if command.instrument_id not in self._subscribed_quotes:
                self.subscribe_quote_ticks(command.instrument_id)
                self._subscribed_quotes.add(command.instrument_id)
        elif emulation_trigger == TriggerType.LAST:
            if command.instrument_id not in self._subscribed_trades:
                self.subscribe_trade_ticks(command.instrument_id)
                self._subscribed_trades.add(command.instrument_id)

    cdef void _handle_modify_order(self, ModifyOrder command) except *:
        cdef Order order = self.cache.order(command.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot modify order: order for {repr(order.client_order_id)} not found.",
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
            account_id=order.account_id,
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
            raise ValueError(
                f"invalid `OrderSide`, was {command.order_side}",  # pragma: no cover (design-time error)
            )

        cdef Order order
        for order in orders:
            self._cancel_order(matching_core, order)

    cdef void _cancel_order(self, MatchingCore matching_core, Order order) except *:
        matching_core.delete_order(order)
        cdef SubmitOrder command = self._commands.pop(order.client_order_id, None)
        if command is None:
            self._log.warning(
                f"Could not find held `SubmitOrder command for {repr(order.client_order_id)}",
            )

        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderCanceled event = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._send_exec_event(event)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void trigger_stop_order(self, Order order) except *:
        pass

    cpdef void fill_market_order(self, Order order, LiquiditySide liquidity_side) except *:
        pass

    cpdef void fill_limit_order(self, Order order, LiquiditySide liquidity_side) except *:
        pass

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `QuoteTick`: no matching core for {tick.instrument_id}.")
            return

        matching_core.bid = tick.bid
        matching_core.ask = tick.ask
        matching_core.iterate(self._clock.timestamp_ns())

    cpdef void on_trade_tick(self, TradeTick tick) except *:
        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `TradeTick`: no matching core for {tick.instrument_id}.")
            return

        matching_core.last = tick.last
        if tick.instrument_id not in self._subscribed_quotes:
            matching_core.bid = tick.last
            matching_core.ask = tick.last
        matching_core.iterate(self._clock.timestamp_ns())

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_risk_command(self, TradingCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="RiskEngine.execute", msg=command)

    cdef void _send_exec_command(self, TradingCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)

    cdef void _send_exec_event(self, OrderEvent event) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{EVT}{SENT} {event}.")
        self._msgbus.send(endpoint="ExecEngine.process", msg=event)
