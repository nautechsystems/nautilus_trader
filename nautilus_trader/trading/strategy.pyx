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
This module defines a trading strategy class which allows users to implement
their own customized trading strategies

A user can inherit from `TradingStrategy` and optionally override any of the
"on" named event methods. The class is not entirely initialized in a stand-alone
way, the intended usage is to pass strategies to a `Trader` so that they can be
fully "wired" into the platform. Exceptions will be raised if a `TradingStrategy`
attempts to operate without a managing `Trader` instance.

"""

from typing import Optional

import pydantic

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.message cimport Event
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.c_enums.oms_type cimport OMSTypeParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport ModifyOrder
from nautilus_trader.model.commands.trading cimport SubmitBracketOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.base cimport PassiveOrder
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.bus cimport MessageBus


# class ImportableStrategyConfig(pydantic.BaseModel):
#     """
#     Represents the trading strategy configuration for one specific backtest run.
#
#     name : str
#         The fully-qualified name of the module.
#     path : str
#         The path to the source code.
#
#     """
#
#     module: str
#     cls: str
#     path: Optional[str]
#     source: Optional[bytes]
#     config: Optional[TradingStrategyConfig]


class TradingStrategyConfig(pydantic.BaseModel):
    """
    The base model for all trading strategy configurations.

    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OMSType
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).
    """

    order_id_tag: str = "000"
    oms_type: str = "HEDGING"


cdef class TradingStrategy(Actor):
    """
    The abstract base class for all trading strategies.

    This class allows traders to implement their own customized trading strategies.
    A trading strategy can configure its own order management system type, which
    determines how positions are handled by the `ExecutionEngine`.

    Strategy OMS (Order Management System) types:
     - ``HEDGING``: A position ID will be assigned for each new position which
       is opened per instrument.
     - ``NETTING``: There will only ever be a single position for the strategy
       per instrument. The position ID will be `{instrument_id}-{strategy_id}`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: Optional[TradingStrategyConfig]=None):
        """
        Initialize a new instance of the ``TradingStrategy`` class.

        Parameters
        ----------
        config : TradingStrategyConfig, optional
            The trading strategy configuration.

        Raises
        ------
        TypeError
            If config is not of type `TradingStrategyConfig`.

        """
        if config is None:
            config = TradingStrategyConfig()
        Condition.type(config, TradingStrategyConfig, "config")

        self.oms_type = OMSTypeParser.from_str(config.oms_type)

        # Assign strategy ID
        strategy_id = StrategyId(f"{type(self).__name__}-{config.order_id_tag}")
        super().__init__(component_id=strategy_id, config=config.dict())

        # Indicators
        self._indicators = []             # type: list[Indicator]
        self._indicators_for_quotes = {}  # type: dict[InstrumentId, list[Indicator]]
        self._indicators_for_trades = {}  # type: dict[InstrumentId, list[Indicator]]
        self._indicators_for_bars = {}    # type: dict[BarType, list[Indicator]]

        # Public components
        self.clock = self._clock
        self.uuid_factory = self._uuid_factory
        self.log = self._log
        self.cache = None          # Initialized when registered
        self.portfolio = None      # Initialized when registered
        self.order_factory = None  # Initialized when registered

        # Register warning events
        self.register_warning_event(OrderDenied)
        self.register_warning_event(OrderRejected)
        self.register_warning_event(OrderCancelRejected)
        self.register_warning_event(OrderModifyRejected)

    @property
    def registered_indicators(self):
        """
        The registered indicators for the strategy.

        Returns
        -------
        list[Indicator]

        """
        return self._indicators.copy()

    cpdef bint indicators_initialized(self) except *:
        """
        Return a value indicating whether all indicators are initialized.

        Returns
        -------
        bool
            True if all initialized, else False

        """
        if not self._indicators:
            return False

        cdef Indicator indicator
        for indicator in self._indicators:
            if not indicator.initialized:
                return False
        return True

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef dict on_save(self):
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        return {}  # Optionally override in subclass

    cpdef void on_load(self, dict state) except *:
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register(
        self,
        TraderId trader_id,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
        Logger logger,
    ) except *:
        """
        Register the strategy with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID for the strategy.
        portfolio : PortfolioFacade
            The read-only portfolio for the strategy.
        msgbus : MessageBus
            The message bus for the strategy.
        cache : CacheFacade
            The read-only cache for the strategy.
        clock : Clock
            The clock for the strategy.
        logger : Logger
            The logger for the strategy.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(trader_id, "trader_id")
        Condition.not_none(clock, "clock")
        Condition.not_none(logger, "logger")

        self.register_base(
            trader_id=trader_id,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self.clock = self._clock
        self.log = self._log
        self.portfolio = portfolio  # Assigned as PortfolioFacade

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.id,
            clock=self.clock,
        )

        cdef set client_order_ids = self.cache.client_order_ids(
            venue=None,
            instrument_id=None,
            strategy_id=self.id,
        )

        cdef int order_id_count = len(client_order_ids)
        self.order_factory.set_count(order_id_count)
        self.log.info(f"Set ClientOrderIdGenerator count to {order_id_count}.")

        # Required subscriptions
        self._msgbus.subscribe(topic=f"events.order.{self.id}", handler=self.handle_event)
        self._msgbus.subscribe(topic=f"events.position.{self.id}", handler=self.handle_event)

    cpdef void register_indicator_for_quote_ticks(self, InstrumentId instrument_id, Indicator indicator) except *:
        """
        Register the given indicator with the strategy to receive quote tick
        data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for tick updates.
        indicator : Indicator
            The indicator to register.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if instrument_id not in self._indicators_for_quotes:
            self._indicators_for_quotes[instrument_id] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_quotes[instrument_id]:
            self._indicators_for_quotes[instrument_id].append(indicator)
            self.log.info(f"Registered Indicator {indicator} for {instrument_id} quote ticks.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {instrument_id} quote ticks.")

    cpdef void register_indicator_for_trade_ticks(self, InstrumentId instrument_id, Indicator indicator) except *:
        """
        Register the given indicator with the strategy to receive trade tick
        data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for tick updates.
        indicator : indicator
            The indicator to register.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if instrument_id not in self._indicators_for_trades:
            self._indicators_for_trades[instrument_id] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_trades[instrument_id]:
            self._indicators_for_trades[instrument_id].append(indicator)
            self.log.info(f"Registered Indicator {indicator} for {instrument_id} trade ticks.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {instrument_id} trade ticks.")

    cpdef void register_indicator_for_bars(self, BarType bar_type, Indicator indicator) except *:
        """
        Register the given indicator with the strategy to receive bar data for the
        given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for bar updates.
        indicator : Indicator
            The indicator to register.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if bar_type not in self._indicators_for_bars:
            self._indicators_for_bars[bar_type] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_bars[bar_type]:
            self._indicators_for_bars[bar_type].append(indicator)
            self.log.info(f"Registered Indicator {indicator} for {bar_type} bars.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {bar_type} bars.")

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _reset(self) except *:
        if self.order_factory:
            self.order_factory.reset()

        self._indicators.clear()
        self._indicators_for_quotes.clear()
        self._indicators_for_trades.clear()
        self._indicators_for_bars.clear()

        self.on_reset()

# -- STRATEGY COMMANDS -----------------------------------------------------------------------------

    cpdef dict save(self):
        """
        Return the strategy state dictionary to be saved.

        Calls `on_save`.

        Raises
        ------
        RuntimeError
            If strategy is not registered with a trader.

        Warnings
        --------
        Exceptions raised will be caught, logged, and reraised.

        """
        if not self.is_initialized_c():
            self.log.error(
                "Cannot save: strategy has not been registered with a trader.",
            )
            return
        try:
            self.log.debug("Saving state...")
            user_state = self.on_save()
            if len(user_state) > 0:
                self.log.info(f"Saved state: {user_state}.", color=LogColor.BLUE)
            else:
                self.log.info("No user state to save.", color=LogColor.BLUE)
            return user_state
        except Exception as ex:
            self.log.exception(ex)
            raise  # Otherwise invalid state information could be saved

    cpdef void load(self, dict state) except *:
        """
        Load the strategy state from the give state dictionary.

        Calls `on_load` and passes the state.

        Parameters
        ----------
        state : dict[str, object]
            The state dictionary.

        Raises
        ------
        RuntimeError
            If strategy is not registered with a trader.

        Warnings
        --------
        Exceptions raised will be caught, logged, and reraised.

        """
        Condition.not_none(state, "state")

        if not state:
            self.log.info("No user state to load.", color=LogColor.BLUE)
            return

        try:
            self.log.debug(f"Loading state...")
            self.on_load(state)
            self.log.info(f"Loaded state {state}.", color=LogColor.BLUE)
        except Exception as ex:
            self.log.exception(ex)
            raise

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void publish_data(self, Data data) except *:
        """
        Publish the strategy data to the message bus.

        Parameters
        ----------
        data : Data
            The strategy data to publish.

        """
        Condition.not_none(data, "data")

        self._msgbus.publish_c(
            topic=f"data.strategy.{type(data).__name__}.{self.id}",
            msg=data,
        )

# -- TRADING COMMANDS ------------------------------------------------------------------------------

    cpdef void submit_order(
        self,
        Order order,
        PositionId position_id=None,
    ) except *:
        """
        Submit the given order with optional position ID and routing instructions.

        A `SubmitOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Parameters
        ----------
        order : Order
            The order to submit.
        position_id : PositionId, optional
            The position ID to submit the order against.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "self.trader_id")

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.value}",
            msg=order.init_event_c(),
        )

        cdef SubmitOrder command = SubmitOrder(
            self.trader_id,
            self.id,
            position_id,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void submit_bracket_order(self, BracketOrder bracket_order) except *:
        """
        Submit the given bracket order with optional routing instructions.

        A `SubmitBracketOrder` command with be created and sent to the
        `ExecutionEngine`.

        Parameters
        ----------
        bracket_order : BracketOrder
            The bracket order to submit.

        """
        Condition.not_none(bracket_order, "bracket_order")
        Condition.not_none(self.trader_id, "self.trader_id")

        # Publish initialized events
        self._msgbus.publish_c(
            topic=f"events.order.{bracket_order.entry.strategy_id.value}",
            msg=bracket_order.entry.init_event_c(),
        )
        self._msgbus.publish_c(
            topic=f"events.order.{bracket_order.stop_loss.strategy_id.value}",
            msg=bracket_order.stop_loss.init_event_c(),
        )
        self._msgbus.publish_c(
            topic=f"events.order.{bracket_order.take_profit.strategy_id.value}",
            msg=bracket_order.take_profit.init_event_c(),
        )

        cdef SubmitBracketOrder command = SubmitBracketOrder(
            self.trader_id,
            self.id,
            bracket_order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void modify_order(
        self,
        PassiveOrder order,
        Quantity quantity=None,
        Price price=None,
        Price trigger=None,
    ) except *:
        """
        Modify the given order with optional parameters and routing instructions.

        An `ModifyOrder` command is created and then sent to the
        `ExecutionEngine`. Either one or both values must differ from the
        original order for the command to be valid.

        Will use an Order Cancel/Replace Request (a.k.a Order Modification)
        for FIX protocols, otherwise if order update is not available with
        the API, then will cancel - then replace with a new order using the
        original `ClientOrderId`.

        Parameters
        ----------
        order : PassiveOrder
            The order to update.
        quantity : Quantity, optional
            The updated quantity for the given order.
        price : Price, optional
            The updated price for the given order.
        trigger : Price, optional
            The updated trigger price for the given order.

        Raises
        ------
        ValueError
            If trigger is not `None` and order.type != ``STOP_LIMIT``.

        References
        ----------
        https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "self.trader_id")
        if trigger is not None:
            Condition.equal(order.type, OrderType.STOP_LIMIT, "order.type", "STOP_LIMIT")

        cdef bint updating = False  # Set validation flag (must become true)

        if quantity is not None and quantity != order.quantity:
            updating = True

        if price is not None and price != order.price:
            updating = True

        if trigger is not None:
            if order.is_triggered_c():
                self.log.warning(
                    f"Cannot create command ModifyOrder: "
                    f"Order with {repr(order.client_order_id)} already triggered.",
                )
                return
            if trigger != order.trigger:
                updating = True

        if not updating:
            self.log.error(
                "Cannot create command ModifyOrder: "
                "quantity, price and trigger were either None or the same as existing values.",
            )
            return

        if order.account_id is None:
            self.log.error(
                f"Cannot create command ModifyOrder: "
                f"no account assigned to order yet, {order}.",
            )
            return  # Cannot send command

        if (
            order.is_completed_c()
            or order.is_pending_update_c()
            or order.is_pending_cancel_c()
        ):
            self.log.warning(
                f"Cannot create command ModifyOrder: state is {order.status_string_c()}, {order}.",
            )
            return  # Cannot send command

        cdef ModifyOrder command = ModifyOrder(
            self.trader_id,
            self.id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            quantity,
            price,
            trigger,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void cancel_order(self, Order order) except *:
        """
        Cancel the given order with optional routing instructions.

        A `CancelOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Logs an error if no `VenueOrderId` has been assigned to the order.

        Parameters
        ----------
        order : Order
            The order to cancel.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "self.trader_id")

        if order.venue_order_id is None:
            self.log.error(
                f"Cannot cancel order: no venue_order_id assigned yet, {order}.",
            )
            return  # Cannot send command

        if order.is_completed_c() or order.is_pending_cancel_c():
            self.log.warning(
                f"Cannot cancel order: state is {order.status_string_c()}, {order}.",
            )
            return  # Cannot send command

        cdef CancelOrder command = CancelOrder(
            self.trader_id,
            self.id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void cancel_all_orders(self, InstrumentId instrument_id) except *:
        """
        Cancel all orders for this strategy for the given instrument ID.

        All working orders in turn will have a `CancelOrder` command created and
        then sent to the `ExecutionEngine`.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument for the orders to cancel.

        """
        # instrument_id can be None

        cdef list working_orders = self.cache.orders_working(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
        )

        if not working_orders:
            self.log.info("No working orders to cancel.")
            return

        cdef int count = len(working_orders)
        self.log.info(
            f"Cancelling {count} working order{'' if count == 1 else 's'}...",
        )

        cdef Order order
        for order in working_orders:
            self.cancel_order(order)

    cpdef void flatten_position(self, Position position) except *:
        """
        Flatten the given position.

        A closing `MarketOrder` for the position will be created, and then sent
        to the `ExecutionEngine` via a `SubmitOrder` command.

        Parameters
        ----------
        position : Position
            The position to flatten.

        """
        Condition.not_none(position, "position")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self.order_factory, "self.order_factory")

        if position.is_closed_c():
            self.log.warning(
                f"Cannot flatten position "
                f"(the position is already closed), {position}."
            )
            return  # Invalid command

        # Create flattening order
        cdef MarketOrder order = self.order_factory.market(
            position.instrument_id,
            Order.flatten_side_c(position.side),
            position.quantity,
        )

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.value}",
            msg=order.init_event_c(),
        )

        # Create command
        cdef SubmitOrder command = SubmitOrder(
            self.trader_id,
            self.id,
            position.id,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void flatten_all_positions(self, InstrumentId instrument_id) except *:
        """
        Flatten all positions for the given instrument ID for this strategy.

        All open positions in turn will have a closing `MarketOrder` created and
        then sent to the `ExecutionEngine` via `SubmitOrder` commands.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument for the positions to flatten.

        """
        # instrument_id can be None

        cdef list positions_open = self.cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
        )

        if not positions_open:
            self.log.info("No open positions to flatten.")
            return

        cdef int count = len(positions_open)
        self.log.info(f"Flattening {count} open position{'' if count == 1 else 's'}...")

        cdef Position position
        for position in positions_open:
            self.flatten_position(position)

# -- HANDLERS --------------------------------------------------------------------------------------

    cpdef void handle_quote_tick(self, QuoteTick tick, bint is_historical=False) except *:
        """
        Handle the given tick.

        Calls `on_quote_tick` if state is ``RUNNING``.

        Parameters
        ----------
        tick : QuoteTick
            The received tick.
        is_historical : bool
            If tick is historical then it won't be passed to `on_quote_tick`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(tick, "tick")

        # Update indicators
        cdef list indicators = self._indicators_for_quotes.get(tick.instrument_id)  # Could be None
        cdef Indicator indicator
        if indicators:
            for indicator in indicators:
                indicator.handle_quote_tick(tick)

        if is_historical:
            return  # Don't pass to on_tick()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_quote_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
                raise

    cpdef void handle_trade_tick(self, TradeTick tick, bint is_historical=False) except *:
        """
        Handle the given tick.

        Calls `on_trade_tick` if state is ``RUNNING``.

        Parameters
        ----------
        tick : TradeTick
            The received trade tick.
        is_historical : bool
            If tick is historical then it won't be passed to `on_trade_tick`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(tick, "tick")

        # Update indicators
        cdef list indicators = self._indicators_for_trades.get(tick.instrument_id)  # Could be None
        cdef Indicator indicator
        if indicators:
            for indicator in indicators:
                indicator.handle_trade_tick(tick)

        if is_historical:
            return  # Don't pass to on_tick()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_trade_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
                raise

    cpdef void handle_bar(self, Bar bar, bint is_historical=False) except *:
        """
        Handle the given bar data.

        Calls `on_bar` if state is ``RUNNING``.

        Parameters
        ----------
        bar : Bar
            The bar received.
        is_historical : bool
            If bar is historical then it won't be passed to `on_bar`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(bar, "bar")

        # Update indicators
        cdef list indicators = self._indicators_for_bars.get(bar.type)
        cdef Indicator indicator
        if indicators:
            for indicator in indicators:
                indicator.handle_bar(bar)

        if is_historical:
            return  # Don't pass to on_bar()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_bar(bar)
            except Exception as ex:
                self.log.exception(ex)
                raise

    cpdef void handle_event(self, Event event) except *:
        """
        Handle the given event.

        Calls `on_event` if state is ``RUNNING``.

        Parameters
        ----------
        event : Event
            The received event.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")

        if type(event) in self._warning_events:
            self.log.warning(f"{RECV}{EVT} {event}.")
        else:
            self.log.info(f"{RECV}{EVT} {event}.")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as ex:
                self.log.exception(ex)
                raise

# -- EGRESS ----------------------------------------------------------------------------------------

    cdef void _send_exec_cmd(self, TradingCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="RiskEngine.execute", msg=command)
