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

"""
This module defines a trading strategy class which allows users to implement
their own customized trading strategies

A user can inherit from `Strategy` and optionally override any of the
"on" named event methods. The class is not entirely initialized in a stand-alone
way, the intended usage is to pass strategies to a `Trader` so that they can be
fully "wired" into the platform. Exceptions will be raised if a `Strategy`
attempts to operate without a managing `Trader` instance.

"""

from typing import Optional

import cython

from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import StrategyConfig

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.enums_c cimport ComponentState
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.algorithm cimport ExecAlgorithmSpecification
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport QueryOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport oms_type_from_str
from nautilus_trader.model.enums_c cimport order_side_to_str
from nautilus_trader.model.enums_c cimport position_side_to_str
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport VALID_LIMIT_ORDER_TYPES
from nautilus_trader.model.orders.base cimport VALID_STOP_ORDER_TYPES
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class Strategy(Actor):
    """
    The base class for all trading strategies.

    This class allows traders to implement their own customized trading strategies.
    A trading strategy can configure its own order management system type, which
    determines how positions are handled by the `ExecutionEngine`.

    Strategy OMS (Order Management System) types:
     - ``UNSPECIFIED``: No specific type has been configured, will therefore
       default to the native OMS type for each venue.
     - ``HEDGING``: A position ID will be assigned for each new position which
       is opened per instrument.
     - ``NETTING``: There will only be a single position for the strategy per
       instrument. The position ID naming convention is `{instrument_id}-{strategy_id}`.

    Parameters
    ----------
    config : StrategyConfig, optional
        The trading strategy configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `StrategyConfig`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: Optional[StrategyConfig] = None):
        if config is None:
            config = StrategyConfig()
        Condition.type(config, StrategyConfig, "config")

        super().__init__()
        # Assign strategy ID after base class initialized
        component_id = type(self).__name__ if config.strategy_id is None else config.strategy_id
        self.id = StrategyId(f"{component_id}-{config.order_id_tag}")
        self.order_id_tag = str(config.order_id_tag)

        # Configuration
        self.config = config
        self.oms_type = oms_type_from_str(str(config.oms_type).upper()) if config.oms_type else OmsType.UNSPECIFIED
        self._manage_gtd_expiry = False

        # Indicators
        self._indicators: list[Indicator] = []
        self._indicators_for_quotes: dict[InstrumentId, list[Indicator]] = {}
        self._indicators_for_trades: dict[InstrumentId, list[Indicator]] = {}
        self._indicators_for_bars: dict[BarType, list[Indicator]] = {}

        # Public components
        self.clock = self._clock
        self.cache = None          # Initialized when registered
        self.portfolio = None      # Initialized when registered
        self.order_factory = None  # Initialized when registered

        # Register warning events
        self.register_warning_event(OrderDenied)
        self.register_warning_event(OrderRejected)
        self.register_warning_event(OrderCancelRejected)
        self.register_warning_event(OrderModifyRejected)

    def to_importable_config(self) -> ImportableStrategyConfig:
        """
        Returns an importable configuration for this strategy.

        Returns
        -------
        ImportableStrategyConfig

        """
        return ImportableStrategyConfig(
            strategy_path=self.fully_qualified_name(),
            config_path=self.config.fully_qualified_name(),
            config=self.config.dict(),
        )

    @property
    def registered_indicators(self):
        """
        Return the registered indicators for the strategy.

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

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void on_start(self) except *:
        # Should override in subclass
        self.log.warning(
            "The `Strategy.on_start` handler was called when not overridden. "
            "It's expected that any actions required when starting the strategy "
            "occur here, such as subscribing/requesting data.",
        )

    cpdef void on_stop(self) except *:
        # Should override in subclass
        self.log.warning(
            "The `Strategy.on_stop` handler was called when not overridden. "
            "It's expected that any actions required when stopping the strategy "
            "occur here, such as unsubscribing from data.",
        )

    cpdef void on_resume(self) except *:
        # Should override in subclass
        self.log.warning(
            "The `Strategy.on_resume` handler was called when not overridden. "
            "It's expected that any actions required when resuming the strategy "
            "following a stop occur here."
        )

    cpdef void on_reset(self) except *:
        # Should override in subclass
        self.log.warning(
            "The `Strategy.on_reset` handler was called when not overridden. "
            "It's expected that any actions required when resetting the strategy "
            "occur here, such as resetting indicators and other state."
        )

# -- REGISTRATION ---------------------------------------------------------------------------------

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
        Condition.not_none(portfolio, "portfolio")
        Condition.not_none(msgbus, "msgbus")
        Condition.not_none(cache, "cache")
        Condition.not_none(clock, "clock")
        Condition.not_none(logger, "logger")

        self.register_base(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

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

        cdef set order_list_ids = self.cache.order_list_ids(
            venue=None,
            instrument_id=None,
            strategy_id=self.id,
        )

        cdef int order_id_count = len(client_order_ids)
        cdef int order_list_id_count = len(order_list_ids)
        self.order_factory.set_client_order_id_count(order_id_count)
        self.order_factory.set_order_list_id_count(order_list_id_count)
        self.log.info(f"Set ClientOrderIdGenerator client_order_id count to {order_id_count}.")
        self.log.info(f"Set ClientOrderIdGenerator order_list_id count to {order_list_id_count}.")

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

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _reset(self) except *:
        if self.order_factory:
            self.order_factory.reset()

        self._indicators.clear()
        self._indicators_for_quotes.clear()
        self._indicators_for_trades.clear()
        self._indicators_for_bars.clear()

        self.on_reset()

# -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef void submit_order(
        self,
        Order order,
        PositionId position_id = None,
        bint manage_gtd_expiry = False,
        ExecAlgorithmSpecification exec_algorithm_spec = None,
        ClientId client_id = None,
    ) except *:
        """
        Submit the given order with optional position ID, execution algorithm
        and routing instructions.

        A `SubmitOrder` command will be created and sent to the `RiskEngine`.

        Parameters
        ----------
        order : Order
            The order to submit.
        position_id : PositionId, optional
            The position ID to submit the order against. If a position does not
            yet exist, then any position opened will have this identifier assigned.
        manage_gtd_expiry : bool, default False
            If any GTD time in force order expiry should be managed by the strategy.
        exec_algorithm_spec : ExecAlgorithmSpecification, optional
            The execution algorithm specification for the order.
        client_id : ClientId, optional
            The specific execution client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by the order will have this position ID assigned. This may
        not be what you intended.

        """
        Condition.true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=order.init_event_c(),
        )

        cdef SubmitOrder command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            position_id=position_id,
            exec_algorithm_spec=exec_algorithm_spec,
            client_id=client_id,
        )

        self.cache.add_submit_order_command(command)

        if manage_gtd_expiry and order.time_in_force == TimeInForce.GTD:
            self._set_gtd_expiry(order)

        self._send_risk_command(command)

    cpdef void submit_order_list(
        self,
        OrderList order_list,
        PositionId position_id = None,
        bint manage_gtd_expiry = False,
        list exec_algorithm_specs = None,
        ClientId client_id = None
    ) except *:
        """
        Submit the given order list with optional position ID, execution algorithm
        and routing instructions.

        A `SubmitOrderList` command with be created and sent to the `RiskEngine`.

        Parameters
        ----------
        order_list : OrderList
            The order list to submit.
        position_id : PositionId, optional
            The position ID to submit the order against. If a position does not
            yet exist, then any position opened will have this identifier assigned.
        manage_gtd_expiry : bool, default False
            If any GTD time in force order expiry should be managed by the strategy.
        exec_algorithm_specs : list[ExecAlgorithmSpecification], optional
            The execution algorithm specifications for the orders.
        client_id : ClientId, optional
            The specific execution client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by an order will have this position ID assigned. This may
        not be what you intended.

        """
        Condition.true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order_list, "order_list")

        # Publish initialized events
        cdef Order order
        for order in order_list.orders:
            self._msgbus.publish_c(
                    topic=f"events.order.{order.strategy_id.to_str()}",
                msg=order.init_event_c(),
            )

        cdef SubmitOrderList command = SubmitOrderList(
            trader_id=self.trader_id,
            strategy_id=self.id,
            order_list=order_list,
            position_id=position_id,
            exec_algorithm_specs=exec_algorithm_specs,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
        )

        self.cache.add_submit_order_list_command(command)

        if manage_gtd_expiry:
            for order in command.order_list.orders:
                if order.time_in_force == TimeInForce.GTD:
                    self._set_gtd_expiry(order)

        self._send_risk_command(command)

    cpdef void modify_order(
        self,
        Order order,
        Quantity quantity = None,
        Price price = None,
        Price trigger_price = None,
        ClientId client_id = None,
    ) except *:
        """
        Modify the given order with optional parameters and routing instructions.

        An `ModifyOrder` command is created and then sent to the
        `ExecutionEngine`. Either one or both values must differ from the
        original order for the command to be valid.

        Will use an Order Cancel/Replace Request (a.k.a Order Modification)
        for FIX protocols, otherwise if order update is not available for
        the API, then will cancel and replace with a new order using the
        original `ClientOrderId`.

        Parameters
        ----------
        order : Order
            The order to update.
        quantity : Quantity, optional
            The updated quantity for the given order.
        price : Price, optional
            The updated price for the given order (if applicable).
        trigger_price : Price, optional
            The updated trigger price for the given order (if applicable).
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        Raises
        ------
        ValueError
            If `price` is not ``None`` and order does not have a `price`.
        ValueError
            If `trigger` is not ``None`` and order does not have a `trigger_price`.

        Warnings
        --------
        If the order is already closed or at `PENDING_CANCEL` status
        then the command will not be generated, and a warning will be logged.

        References
        ----------
        https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html

        """
        Condition.true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")

        cdef bint updating = False  # Set validation flag (must become true)

        if quantity is not None and quantity != order.quantity:
            updating = True

        if price is not None:
            Condition.true(
                order.order_type in VALID_LIMIT_ORDER_TYPES,
                fail_msg=f"{order.type_string_c()} orders do not have a LIMIT price",
            )
            if price != order.price:
                updating = True

        if trigger_price is not None:
            Condition.true(
                order.order_type in VALID_STOP_ORDER_TYPES,
                fail_msg=f"{order.type_string_c()} orders do not have a STOP trigger price",
            )
            if trigger_price != order.trigger_price:
                updating = True

        if not updating:
            self.log.error(
                "Cannot create command ModifyOrder: "
                "quantity, price and trigger were either None "
                "or the same as existing values.",
            )
            return

        if order.is_closed_c() or order.is_pending_cancel_c():
            self.log.warning(
                f"Cannot create command ModifyOrder: "
                f"state is {order.status_string_c()}, {order}.",
            )
            return  # Cannot send command

        cdef ModifyOrder command = ModifyOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
        )

        self._send_risk_command(command)

    cpdef void cancel_order(self, Order order, ClientId client_id = None) except *:
        """
        Cancel the given order with optional routing instructions.

        A `CancelOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Logs an error if no `VenueOrderId` has been assigned to the order.

        Parameters
        ----------
        order : Order
            The order to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")

        if order.is_closed_c() or order.is_pending_cancel_c():
            self.log.warning(
                f"Cannot cancel order: state is {order.status_string_c()}, {order}.",
            )
            return  # Cannot send command

        cdef CancelOrder command = CancelOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
        )

        self._send_risk_command(command)

    cpdef void cancel_all_orders(
        self,
        InstrumentId instrument_id,
        OrderSide order_side = OrderSide.NO_ORDER_SIDE,
        ClientId client_id = None,
    ) except *:
        """
        Cancel all orders for this strategy for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the orders to cancel.
        order_side : OrderSide, default ``NO_ORDER_SIDE`` (both sides)
            The side of the orders to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(instrument_id, "instrument_id")

        cdef list open_orders = self.cache.orders_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
            side=order_side,
        )

        cdef list emulated_orders = self.cache.orders_emulated(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
            side=order_side,
        )

        cdef str order_side_str = " " + order_side_to_str(order_side) if order_side != OrderSide.NO_ORDER_SIDE else ""
        if not open_orders and not emulated_orders:
            self.log.info(
                f"No open or emulated{order_side_str} "
                f"{instrument_id.value} orders to cancel.")
            return

        cdef int open_count = len(open_orders)
        if open_count:
            self.log.info(
                f"Canceling {open_count} open{order_side_str} "
                f"{instrument_id.value} order{'' if open_count == 1 else 's'}...",
            )

        cdef int emulated_count = len(emulated_orders)
        if emulated_count:
            self.log.info(
                f"Canceling {emulated_count} emulated{order_side_str} "
                f"{instrument_id.value} order{'' if emulated_count == 1 else 's'}...",
            )

        cdef CancelAllOrders command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument_id,
            order_side=order_side,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
        )

        self._send_risk_command(command)

    cpdef void close_position(
        self,
        Position position,
        ClientId client_id = None,
        str tags = None,
    ) except *:
        """
        Close the given position.

        A closing `MarketOrder` for the position will be created, and then sent
        to the `ExecutionEngine` via a `SubmitOrder` command.

        Parameters
        ----------
        position : Position
            The position to close.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        tags : str, optional
            The tags for the market order closing the position.

        """
        Condition.true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(position, "position")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self.order_factory, "self.order_factory")

        if position.is_closed_c():
            self.log.warning(
                f"Cannot close position "
                f"(the position is already closed), {position}."
            )
            return  # Invalid command

        # Create closing order
        cdef MarketOrder order = self.order_factory.market(
            instrument_id=position.instrument_id,
            order_side=Order.closing_side_c(position.side),
            quantity=position.quantity,
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
            tags=tags,
        )

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=order.init_event_c(),
        )

        # Create command
        cdef SubmitOrder command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            position_id=position.id,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
        )

        self._send_risk_command(command)

    cpdef void close_all_positions(
        self,
        InstrumentId instrument_id,
        PositionSide position_side = PositionSide.NO_POSITION_SIDE,
        ClientId client_id = None,
        str tags = None,
    ) except *:
        """
        Close all positions for the given instrument ID for this strategy.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the positions to close.
        position_side : PositionSide, default ``NO_POSITION_SIDE`` (both sides)
            The side of the positions to close.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        tags : str, optional
            The tags for the market orders closing the positions.

        """
        # instrument_id can be None
        Condition.true(self.trader_id is not None, "The strategy has not been registered")

        cdef list positions_open = self.cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
            side=position_side,
        )

        cdef str position_side_str = " " + position_side_to_str(position_side) if position_side != PositionSide.NO_POSITION_SIDE else ""
        if not positions_open:
            self.log.info(f"No open{position_side_str} positions to close.")
            return

        cdef int count = len(positions_open)
        self.log.info(
            f"Closing {count} open{position_side_str} position{'' if count == 1 else 's'}...",
        )

        cdef Position position
        for position in positions_open:
            self.close_position(position, client_id, tags)

    cpdef void query_order(self, Order order, ClientId client_id = None) except *:
        """
        Query the given order with optional routing instructions.

        A `QueryOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Logs an error if no `VenueOrderId` has been assigned to the order.

        Parameters
        ----------
        order : Order
            The order to query.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")

        cdef QueryOrder command = QueryOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
        )

        self._send_exec_command(command)

    cdef str _get_gtd_expiry_timer_name(self, ClientOrderId client_order_id):
        return f"GTD-EXPIRY:{client_order_id.to_str()}"

    cdef void _set_gtd_expiry(self, Order order) except *:
        self._log.info(
            f"Setting managed GTD expiry timer for {order.client_order_id} @ {order.expire_time}.",
            LogColor.BLUE,
        )
        cdef str timer_name = self._get_gtd_expiry_timer_name(order.client_order_id)
        self._clock.set_time_alert_ns(
            name=timer_name,
            alert_time_ns=order.expire_time_ns,
            callback=self._expire_gtd_order,
        )
        # For now, we flip this opt-in flag
        self._manage_gtd_expiry = True

    cdef void _cancel_gtd_expiry(self, Order order) except *:
        cdef str timer_name = self._get_gtd_expiry_timer_name(order.client_order_id)
        if timer_name in self._clock.timer_names:
            self._log.info(
                f"Canceling managed GTD expiry timer for {order.client_order_id} @ {order.expire_time}.",
                LogColor.BLUE,
            )
            self._clock.cancel_timer(name=timer_name)

    cpdef void _expire_gtd_order(self, TimeEvent event) except *:
        cdef ClientOrderId client_order_id = ClientOrderId(event.to_str().partition(":")[2])
        cdef Order order = self.cache.order(client_order_id)
        if order is None:
            self._log.warning(
                f"Order with {repr(client_order_id)} not found in the cache to apply {event}."
            )

        if order.is_closed_c():
            self._log.warning(f"GTD expired order {order.client_order_id} was already closed.")
            return  # Already closed

        self._log.info(f"Expiring GTD order {order.client_order_id}.", LogColor.BLUE)
        self.cancel_order(order)

    # -- HANDLERS -------------------------------------------------------------------------------------

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """
        Handle the given quote tick.

        If state is ``RUNNING`` then passes to `on_quote_tick`.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(tick, "tick")

        # Update indicators
        cdef list indicators = self._indicators_for_quotes.get(tick.instrument_id)
        if indicators:
            self._handle_indicators_for_quote(indicators, tick)

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_quote_tick(tick)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(tick)}", e)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_quote_ticks(self, list ticks) except *:
        """
        Handle the given historical quote tick data by handling each tick individually.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The ticks received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef QuoteTick first = ticks[0] if length > 0 else None
        cdef InstrumentId instrument_id = first.instrument_id if first is not None else None

        if length > 0:
            self._log.info(f"Received <QuoteTick[{length}]> data for {instrument_id}.")
        else:
            self._log.warning("Received <QuoteTick[]> data with no ticks.")
            return

        # Update indicators
        cdef list indicators = self._indicators_for_quotes.get(first.instrument_id)

        cdef:
            int i
            QuoteTick tick
        for i in range(length):
            tick = ticks[i]
            if indicators:
                self._handle_indicators_for_quote(indicators, tick)
            self.handle_historical_data(tick)

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """
        Handle the given trade tick.

        If state is ``RUNNING`` then passes to `on_trade_tick`.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(tick, "tick")

        # Update indicators
        cdef list indicators = self._indicators_for_trades.get(tick.instrument_id)
        if indicators:
            self._handle_indicators_for_trade(indicators, tick)

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_trade_tick(tick)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(tick)}", e)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_trade_ticks(self, list ticks) except *:
        """
        Handle the given historical trade tick data by handling each tick individually.

        Parameters
        ----------
        ticks : list[TradeTick]
            The ticks received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef TradeTick first = ticks[0] if length > 0 else None
        cdef InstrumentId instrument_id = first.instrument_id if first is not None else None

        if length > 0:
            self._log.info(f"Received <TradeTick[{length}]> data for {instrument_id}.")
        else:
            self._log.warning("Received <TradeTick[]> data with no ticks.")
            return

        # Update indicators
        cdef list indicators = self._indicators_for_trades.get(first.instrument_id)

        cdef:
            int i
            TradeTick tick
        for i in range(length):
            tick = ticks[i]
            if indicators:
                self._handle_indicators_for_trade(indicators, tick)
            self.handle_historical_data(tick)

    cpdef void handle_bar(self, Bar bar) except *:
        """
        Handle the given bar data.

        If state is ``RUNNING`` then passes to `on_bar`.

        Parameters
        ----------
        bar : Bar
            The bar received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(bar, "bar")

        # Update indicators
        cdef list indicators = self._indicators_for_bars.get(bar.bar_type)
        if indicators:
            self._handle_indicators_for_bar(indicators, bar)

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_bar(bar)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(bar)}", e)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_bars(self, list bars) except *:
        """
        Handle the given historical bar data by handling each bar individually.

        Parameters
        ----------
        bars : list[Bar]
            The bars to handle.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)
        cdef Bar first = bars[0] if length > 0 else None
        cdef Bar last = bars[length - 1] if length > 0 else None

        if length > 0:
            self._log.info(f"Received <Bar[{length}]> data for {first.bar_type}.")
        else:
            self._log.error(f"Received <Bar[{length}]> data for unknown bar type.")
            return

        if length > 0 and first.ts_init > last.ts_init:
            raise RuntimeError(f"cannot handle <Bar[{length}]> data: incorrectly sorted")

        # Update indicators
        cdef list indicators = self._indicators_for_bars.get(first.bar_type)

        cdef:
            int i
            Bar bar
        for i in range(length):
            bar = bars[i]
            if indicators:
                self._handle_indicators_for_bar(indicators, bar)
            self.handle_historical_data(bar)

    cpdef void handle_event(self, Event event) except *:
        """
        Handle the given event.

        If state is ``RUNNING`` then passes to `on_event`.

        Parameters
        ----------
        event : Event
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")

        if type(event) in self._warning_events:
            self.log.warning(f"{RECV}{EVT} {event}.")
        else:
            self.log.info(f"{RECV}{EVT} {event}.")

        cdef Order order
        if self._manage_gtd_expiry and isinstance(event, OrderEvent):
            order = self.cache.order(event.client_order_id)
            if order is not None and order.is_closed_c():
                self._cancel_gtd_expiry(order)

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as e:
                self.log.exception(f"Error on handling {repr(event)}", e)
                raise

# -- HANDLERS -------------------------------------------------------------------------------------

    cdef void _handle_indicators_for_quote(self, list indicators, QuoteTick tick) except *:
        cdef Indicator indicator
        for indicator in indicators:
            indicator.handle_quote_tick(tick)

    cdef void _handle_indicators_for_trade(self, list indicators, TradeTick tick) except *:
        cdef Indicator indicator
        for indicator in indicators:
            indicator.handle_trade_tick(tick)

    cdef void _handle_indicators_for_bar(self, list indicators, Bar bar) except *:
        cdef Indicator indicator
        for indicator in indicators:
            indicator.handle_bar(bar)

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_risk_command(self, TradingCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="RiskEngine.execute", msg=command)

    cdef void _send_exec_command(self, TradingCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)
