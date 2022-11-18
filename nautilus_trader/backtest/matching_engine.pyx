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

from cpython.object cimport PyObject
from libc.limits cimport INT_MAX
from libc.limits cimport INT_MIN
from libc.stdint cimport uint64_t

from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport price_new
from nautilus_trader.core.rust.model cimport trade_id_new
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.trailing cimport TrailingStopCalculator
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSide
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyType
from nautilus_trader.model.c_enums.depth_type cimport DepthType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_status cimport OrderStatus
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderPendingCancel
from nautilus_trader.model.events.order cimport OrderPendingUpdate
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.data cimport Order as OrderBookOrder
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class OrderMatchingEngine:
    """
    Provides an order matching engine for a single market.

    Parameters
    ----------
    instrument : Instrument
        The market instrument for the matching engine.
    product_id : int
        The product ID for the instrument.
    fill_model : FillModel
        The fill model for the matching engine.
    book_type : BookType
        The order book type for the engine.
    oms_type : OMSType
        The order management system type for the matching engine. Determines
        the generation and handling of venue position IDs.
    reject_stop_orders : bool
        If stop orders are rejected if already in the market on submitting.
    msgbus : MessageBus
        The message bus for the matching engine.
    cache : CacheFacade
        The read-only cache for the matching engine.
    clock : TestClock
        The clock for the matching engine.
    logger : Logger
        The logger for the matching engine.
    """

    def __init__(
        self,
        Instrument instrument not None,
        int product_id,
        FillModel fill_model not None,
        BookType book_type,
        OMSType oms_type,
        bint reject_stop_orders,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        TestClock clock not None,
        Logger logger not None,
    ):
        self._clock = clock
        self._log = LoggerAdapter(
            component_name=f"{type(self).__name__}({instrument.id.venue})",
            logger=logger,
        )
        self.msgbus = msgbus
        self.cache = cache

        self.venue = instrument.id.venue
        self.instrument = instrument
        self.product_id = product_id
        self.book_type = book_type
        self.oms_type = oms_type

        self._reject_stop_orders = reject_stop_orders
        self._fill_model = fill_model
        self._book = OrderBook.create(
            instrument=instrument,
            book_type=book_type,
            simulated=True,
        )
        self._account_ids: dict[TraderId, AccountId]  = {}

        # Market
        self._core = MatchingCore(
            instrument=instrument,
            trigger_stop_order=self._trigger_stop_order,
            fill_market_order=self._fill_market_order,
            fill_limit_order=self._fill_limit_order,
        )

        self._last_bid_bar: Optional[Bar] = None
        self._last_ask_bar: Optional[Bar] = None
        self._bar_execution: bool = False

        self._position_count = 0
        self._order_count = 0
        self._execution_count = 0

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"venue={self.venue.value}, "
            f"instrument_id={self.instrument.id.value}, "
            f"product_id={self.product_id})"
        )

    cpdef void reset(self) except *:
        self._log.debug(f"Resetting OrderMatchingEngine {self.instrument.id}...")

        self._book.clear()
        self._account_ids.clear()
        self._core.reset()
        self._last_bid_bar: Optional[Bar] = None
        self._last_ask_bar: Optional[Bar] = None
        self._bar_execution: bool = False

        self._position_count = 0
        self._order_count = 0
        self._execution_count = 0

        self._log.info(f"Reset OrderMatchingEngine {self.instrument.id}.")

    cpdef void set_fill_model(self, FillModel fill_model) except *:
        """
        Set the fill model to the given model.

        Parameters
        ----------
        fill_model : FillModel
            The fill model to set.

        """
        Condition.not_none(fill_model, "fill_model")

        self._fill_model = fill_model

        self._log.debug(f"Changed `FillModel` to {self._fill_model}.")

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self):
        """
        Return the best bid price for the given instrument ID (if found).

        Returns
        -------
        Price or ``None``

        """
        best_bid_price = self._book.best_bid_price()
        if best_bid_price is None:
            return None
        return Price(best_bid_price, self.instrument.price_precision)

    cpdef Price best_ask_price(self):
        """
        Return the best ask price for the given instrument ID (if found).

        Returns
        -------
        Price or ``None``

        """
        best_ask_price = self._book.best_ask_price()
        if best_ask_price is None:
            return None
        return Price(best_ask_price, self.instrument.price_precision)

    cpdef OrderBook get_book(self):
        """
        Return the internal order book.

        Returns
        -------
        OrderBook

        """
        return self._book

    cpdef list get_open_orders(self):
        """
        Return the open orders in the matching engine.

        Returns
        -------
        list[Order]

        """
        return self.get_open_bid_orders() + self.get_open_ask_orders()

    cpdef list get_open_bid_orders(self):
        """
        Return the open bid orders in the matching engine.

        Returns
        -------
        list[Order]

        """
        return self._core.get_orders_bid()

    cpdef list get_open_ask_orders(self):
        """
        Return the open ask orders at the exchange.

        Returns
        -------
        list[Order]

        """
        return self._core.get_orders_ask()

    cpdef bint order_exists(self, ClientOrderId client_order_id) except *:
        return self._core.order_exists(client_order_id)

# -- DATA PROCESSING ------------------------------------------------------------------------------

    cpdef void process_order_book(self, OrderBookData data) except *:
        """
        Process the exchanges market for the given order book data.

        Parameters
        ----------
        data : OrderBookData
            The order book data to process.

        """
        Condition.not_none(data, "data")

        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(data)}...")

        self._book.apply(data)

        self.iterate(data.ts_init)

    cpdef void process_quote_tick(self, QuoteTick tick)  except *:
        """
        Process the exchanges market for the given quote tick.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : QuoteTick
            The tick to process.

        """
        Condition.not_none(tick, "tick")

        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(tick)}...")

        if self.book_type == BookType.L1_TBBO:
            self._book.update_quote_tick(tick)

        self.iterate(tick.ts_init)

    cpdef void process_trade_tick(self, TradeTick tick) except *:
        """
        Process the exchanges market for the given trade tick.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : TradeTick
            The tick to process.

        """
        Condition.not_none(tick, "tick")

        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(tick)}...")

        if self.book_type == BookType.L1_TBBO:
            self._book.update_trade_tick(tick)

        self._core.set_last(tick._mem.price)

        self.iterate(tick.ts_init)

    cpdef void process_bar(self, Bar bar) except *:
        """
        Process the exchanges market for the given bar.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        """
        Condition.not_none(bar, "bar")

        if not self._log.is_bypassed:
            self._log.debug(f"Processing {repr(bar)}...")

        if self.book_type != BookType.L1_TBBO:
            return  # Can only process an L1 book with bars

        # Turn ON bar execution mode (temporary until unify execution)
        self._bar_execution = True

        cdef PriceType price_type = bar.bar_type.spec.price_type
        if price_type == PriceType.LAST or price_type == PriceType.MID:
            self._process_trade_ticks_from_bar(bar)
        elif price_type == PriceType.BID:
            self._last_bid_bar = bar
            self._process_quote_ticks_from_bar()
        elif price_type == PriceType.ASK:
            self._last_ask_bar = bar
            self._process_quote_ticks_from_bar()
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `PriceType`, was {price_type}",
            )

    cdef void _process_trade_ticks_from_bar(self, Bar bar) except *:
        cdef Quantity size = Quantity(bar.volume.as_double() / 4.0, bar._mem.volume.precision)

        # Create reusable tick
        cdef TradeTick tick = TradeTick(
            bar.bar_type.instrument_id,
            bar.open,
            size,
            <OrderSide>AggressorSide.BUY if not self._core.is_last_initialized or bar._mem.open.raw > self._core.last_raw else <OrderSide>AggressorSide.SELL,
            self._generate_trade_id(),
            bar.ts_event,
            bar.ts_event,
        )

        # Open
        if not self._core.is_last_initialized or bar._mem.open.raw != self._core.last_raw:  # Direct memory comparison
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last(bar._mem.open)

        cdef str trade_id_str  # Assigned below

        # High
        if bar._mem.high.raw > self._core.last_raw:  # Direct memory comparison
            tick._mem.price = bar._mem.high  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.BUY  # Direct memory assignment
            trade_id_str = self._generate_trade_id_str()
            tick._mem.trade_id = trade_id_new(<PyObject *>trade_id_str)
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last(bar._mem.high)

        # Low
        if bar._mem.low.raw < self._core.last_raw:  # Direct memory comparison
            tick._mem.price = bar._mem.low  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.SELL
            trade_id_str = self._generate_trade_id_str()
            tick._mem.trade_id = trade_id_new(<PyObject *>trade_id_str)
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last(bar._mem.low)

        # Close
        if bar._mem.close.raw != self._core.last_raw:  # Direct memory comparison
            tick._mem.price = bar._mem.close  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.BUY if bar._mem.close.raw > self._core.last_raw else <OrderSide>AggressorSide.SELL
            trade_id_str = self._generate_trade_id_str()
            tick._mem.trade_id = trade_id_new(<PyObject *>trade_id_str)
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last(bar._mem.close)

    cdef void _process_quote_ticks_from_bar(self) except *:
        if self._last_bid_bar is None or self._last_ask_bar is None:
            return  # Wait for next bar

        if self._last_bid_bar.ts_event != self._last_ask_bar.ts_event:
            return  # Wait for next bar

        cdef Quantity bid_size = Quantity(self._last_bid_bar.volume.as_double() / 4.0, self._last_bid_bar._mem.volume.precision)
        cdef Quantity ask_size = Quantity(self._last_ask_bar.volume.as_double() / 4.0, self._last_ask_bar._mem.volume.precision)

        # Create reusable tick
        cdef QuoteTick tick = QuoteTick(
            self._book.instrument_id,
            self._last_bid_bar.open,
            self._last_ask_bar.open,
            bid_size,
            ask_size,
            self._last_bid_bar.ts_event,
            self._last_ask_bar.ts_init,
        )

        # Open
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

        # High
        tick._mem.bid = self._last_bid_bar._mem.high  # Direct memory assignment
        tick._mem.ask = self._last_ask_bar._mem.high  # Direct memory assignment
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

        # Low
        tick._mem.bid = self._last_bid_bar._mem.low  # Assigning memory directly
        tick._mem.ask = self._last_ask_bar._mem.low  # Assigning memory directly
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

        # Close
        tick._mem.bid = self._last_bid_bar._mem.close  # Assigning memory directly
        tick._mem.ask = self._last_ask_bar._mem.close  # Assigning memory directly
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

# -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef void process_order(self, Order order, AccountId account_id) except *:
        if self._core.order_exists(order.client_order_id):
            return  # Already processed

        # Index identifiers
        self._account_ids[order.trader_id] = account_id

        cdef Order parent
        if order.parent_order_id is not None:
            parent = self.cache.order(order.parent_order_id)
            assert parent is not None and parent.contingency_type == ContingencyType.OTO, "OTO parent not found"
            if parent.status_c() == OrderStatus.REJECTED and order.is_open_c():
                self._generate_order_rejected(order, f"REJECT OTO from {parent.client_order_id}")
                return  # Order rejected
            elif parent.status_c() == OrderStatus.ACCEPTED:
                self._log.info(f"Pending OTO {order.client_order_id} triggers from {parent.client_order_id}")
                return  # Pending trigger

        # Check reduce-only instruction
        cdef Position position
        if order.is_reduce_only:
            position = self.cache.position_for_order(order.client_order_id)
            if (
                not position
                or position.is_closed_c()
                or (order.is_buy_c() and position.is_long_c())
                or (order.is_sell_c() and position.is_short_c())
            ):
                self._generate_order_rejected(
                    order,
                    f"REDUCE_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"would have increased position.",
                )
                return  # Reduce only

        if order.order_type == OrderType.MARKET:
            self._process_market_order(order)
        elif order.order_type == OrderType.MARKET_TO_LIMIT:
            self._process_market_to_limit_order(order)
        elif order.order_type == OrderType.LIMIT:
            self._process_limit_order(order)
        elif order.order_type == OrderType.STOP_MARKET or order.order_type == OrderType.MARKET_IF_TOUCHED:
            self._process_stop_market_order(order)
        elif order.order_type == OrderType.STOP_LIMIT or order.order_type == OrderType.LIMIT_IF_TOUCHED:
            self._process_stop_limit_order(order)
        elif order.order_type == OrderType.TRAILING_STOP_MARKET:
            self._process_trailing_stop_market_order(order)
        elif order.order_type == OrderType.TRAILING_STOP_LIMIT:
            self._process_trailing_stop_limit_order(order)
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"{OrderTypeParser.to_str(order.order_type)} "
                f"orders are not supported for backtesting in this version",
            )

    cpdef void process_modify(self, ModifyOrder command, AccountId account_id) except *:
        cdef Order order = self._core.get_order(command.client_order_id)
        if order is None:
            self._generate_order_modify_rejected(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                account_id=account_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"{repr(command.client_order_id)} not found",
            )
        else:
            self._generate_order_pending_update(order)
            self._update_order(
                order,
                command.quantity,
                command.price,
                command.trigger_price,
            )

    cpdef void process_cancel(self, CancelOrder command, AccountId account_id) except *:
        cdef Order order = self._core.get_order(command.client_order_id)
        if order is None:
            self._generate_order_cancel_rejected(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                account_id=account_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"{repr(command.client_order_id)} not found",
            )
        else:
            if order.is_inflight_c() or order.is_open_c():
                self._generate_order_pending_cancel(order)
                self._cancel_order(order)

    cpdef void process_cancel_all(self, CancelAllOrders command, AccountId account_id) except *:
        cdef Order order
        for order in self._core.get_orders():
            if command.order_side != OrderSide.NONE and command.order_side != order.side:
                continue
            if order.is_inflight_c() or order.is_open_c():
                self._generate_order_pending_cancel(order)
                self._cancel_order(order)

    cdef void _process_market_order(self, MarketOrder order) except *:
        # Check market exists
        if order.side == OrderSide.BUY and not self._core.is_ask_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self._core.is_bid_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Immediately fill marketable order
        self._fill_market_order(order, LiquiditySide.TAKER)

    cdef void _process_market_to_limit_order(self, MarketToLimitOrder order) except *:
        # Check market exists
        if order.side == OrderSide.BUY and not self._core.is_ask_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self._core.is_bid_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Order is valid and accepted
        self._accept_order(order)

        # Immediately fill marketable order
        self._fill_market_order(order, LiquiditySide.TAKER)

    cdef void _process_limit_order(self, LimitOrder order) except *:
        if order.is_post_only and self._core.is_limit_matched(order.side, order.price):
            self._generate_order_rejected(
                order,
                f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                f"limit px of {order.price} would have been a TAKER: "
                f"bid={self._core.bid}, "
                f"ask={self._core.ask}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

        # Check for immediate fill
        if self._core.is_limit_matched(order.side, order.price):
            # Filling as liquidity taker
            self._fill_limit_order(order, LiquiditySide.TAKER)
        elif order.time_in_force == TimeInForce.FOK or order.time_in_force == TimeInForce.IOC:
            self._cancel_order(order)

    cdef void _process_stop_market_order(self, Order order) except *:
        if self._core.is_stop_triggered(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

    cdef void _process_stop_limit_order(self, Order order) except *:
        if self._core.is_stop_triggered(order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self._core.bid}, "
                f"ask={self._core.ask}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

    cdef void _process_trailing_stop_market_order(self, TrailingStopMarketOrder order) except *:
        if order.has_trigger_price_c() and self._core.is_stop_triggered(order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self._core.bid}, "
                f"ask={self._core.ask}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

        if order.trigger_price is None:
            self._update_trailing_stop_order(order)

    cdef void _process_trailing_stop_limit_order(self, TrailingStopLimitOrder order) except *:
        if order.has_trigger_price_c() and self._core.is_stop_triggered(order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self._core.bid}, "
                f"ask={self._core.ask}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

        if order.trigger_price is None:
            self._update_trailing_stop_order(order)

    cdef void _update_limit_order(
        self,
        Order order,
        Quantity qty,
        Price price,
    ) except *:
        if self._core.is_limit_matched(order.side, price):
            if order.is_post_only:
                self._generate_order_modify_rejected(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    account_id=order.account_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"new limit px of {price} would have been a TAKER: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Cannot update order

            self._generate_order_updated(order, qty, price, None)
            self._fill_limit_order(order, LiquiditySide.TAKER)  # Immediate fill as TAKER
            return  # Filled

        self._generate_order_updated(order, qty, price, None)

    cdef void _update_stop_market_order(
        self,
        Order order,
        Quantity qty,
        Price trigger_price,
    ) except *:
        if self._core.is_stop_triggered(order.side, trigger_price):
            self._generate_order_modify_rejected(
                trader_id=order.trader_id,
                strategy_id=order.strategy_id,
                account_id=order.account_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=f"{order.type_string_c()} {order.side_string_c()} order "
                f"new stop px of {trigger_price} was in the market: "
                f"bid={self._core.bid}, "
                f"ask={self._core.ask}",
            )
            return  # Cannot update order

        self._generate_order_updated(order, qty, None, trigger_price)

    cdef void _update_stop_limit_order(
        self,
        Order order,
        Quantity qty,
        Price price,
        Price trigger_price,
    ) except *:
        if not order.is_triggered:
            # Updating stop price
            if self._core.is_stop_triggered(order.side, price):
                self._generate_order_modify_rejected(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    account_id=order.account_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=f"{order.type_string_c()} {order.side_string_c()} order "
                    f"new trigger stop px of {price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Cannot update order
        else:
            # Updating limit price
            if self._core.is_limit_matched(order.side, price):
                if order.is_post_only:
                    self._generate_order_modify_rejected(
                        trader_id=order.trader_id,
                        strategy_id=order.strategy_id,
                        account_id=order.account_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                        reason=f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order  "
                        f"new limit px of {price} would have been a TAKER: "
                        f"bid={self._core.bid}, "
                        f"ask={self._core.ask}",
                    )
                    return  # Cannot update order
                else:
                    self._generate_order_updated(order, qty, price, None)
                    self._fill_limit_order(order, LiquiditySide.TAKER)  # Immediate fill as TAKER
                    return  # Filled

        self._generate_order_updated(order, qty, price, trigger_price or order.trigger_price)

    cdef void _update_trailing_stop_order(self, Order order) except *:
        cdef tuple output = TrailingStopCalculator.calculate(
            instrument=self.instrument,
            order=order,
            bid=self._core.bid,
            ask=self._core.ask,
            last=self._core.last,
        )

        cdef Price new_trigger_price = output[0]
        cdef Price new_price = output[1]
        if new_trigger_price is None and new_price is None:
            return  # No updates

        self._generate_order_updated(
            order=order,
            quantity=order.quantity,
            price=new_price,
            trigger_price=new_trigger_price,
        )

# -- ORDER PROCESSING -----------------------------------------------------------------------------

    cpdef void iterate(self, uint64_t timestamp_ns) except *:
        self._clock.set_time(timestamp_ns)

        # TODO: Convert order book to use ints rather than doubles
        cdef list bid_levels = self._book.bids.levels
        cdef list ask_levels = self._book.asks.levels

        if bid_levels:
            self._core.set_bid(price_new(bid_levels[0].price, self.instrument.price_precision))
        if ask_levels:
            self._core.set_ask(price_new(ask_levels[0].price, self.instrument.price_precision))

        self._core.iterate(timestamp_ns)

        cdef list orders = self._core.get_orders()
        cdef Order order
        for order in orders:
            if order.is_closed_c():
                continue

            # Check expiry
            if order.expire_time_ns > 0 and timestamp_ns >= order.expire_time_ns:
                self._core.delete_order(order)
                self._expire_order(order)
                continue

            # Manage trailing stop
            if order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
                self._update_trailing_stop_order(order)

    cpdef list _determine_limit_price_and_volume(self, Order order):
        cdef Price price
        if self._bar_execution:
            price = order.price
            if order.side == OrderSide.BUY:
                self._core.set_bid(price._mem)
            elif order.side == OrderSide.SELL:
                self._core.set_ask(price._mem)
            else:
                raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)
            self._core.set_last(price._mem)
            return [(order.price, order.leaves_qty)]
        cdef OrderBookOrder submit_order = OrderBookOrder(price=order.price, size=order.leaves_qty, side=order.side)
        if order.side == OrderSide.BUY:
            return self._book.asks.simulate_order_fills(order=submit_order, depth_type=DepthType.VOLUME)
        elif order.side == OrderSide.SELL:
            return self._book.bids.simulate_order_fills(order=submit_order, depth_type=DepthType.VOLUME)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)

    cpdef list _determine_market_price_and_volume(self, Order order):
        cdef Price price
        if self._bar_execution:
            if order.order_type == OrderType.MARKET or order.order_type == OrderType.MARKET_IF_TOUCHED:
                if order.side == OrderSide.BUY:
                    if self._core.is_ask_initialized:
                        price = self._core.ask
                    else:
                        price = self.best_ask_price()
                    if price is not None:
                        self._core.set_last(price._mem)
                        return [(price, order.leaves_qty)]
                    else:
                        raise RuntimeError(  # pragma: no cover (design-time error)
                            "Market best ASK price was None when filling MARKET order",
                        )
                elif order.side == OrderSide.SELL:
                    if self._core.is_bid_initialized:
                        price = self._core.bid
                    else:
                        price = self.best_bid_price()
                    if price is not None:
                        self._core.set_last(price._mem)
                        return [(price, order.leaves_qty)]
                    else:
                        raise RuntimeError(  # pragma: no cover (design-time error)
                            "Market best BID price was None when filling MARKET order",
                        )
            else:
                price = order.price if order.order_type == OrderType.LIMIT else order.trigger_price
                if order.side == OrderSide.BUY:
                    self._core.set_ask(price._mem)
                elif order.side == OrderSide.SELL:
                    self._core.set_bid(price._mem)
                else:
                    raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)
                self._core.set_last(price._mem)
                return [(price, order.leaves_qty)]
        price = Price.from_int_c(INT_MAX if order.side == OrderSide.BUY else INT_MIN)
        cdef OrderBookOrder submit_order = OrderBookOrder(price=price, size=order.leaves_qty, side=order.side)
        if order.side == OrderSide.BUY:
            return self._book.asks.simulate_order_fills(order=submit_order)
        elif order.side == OrderSide.SELL:
            return self._book.bids.simulate_order_fills(order=submit_order)

    cpdef void _fill_market_order(self, Order order, LiquiditySide liquidity_side) except *:
        cdef PositionId venue_position_id = self._get_position_id(order)
        cdef Position position = None
        if venue_position_id is not None:
            position = self.cache.position(venue_position_id)
        if order.is_reduce_only and position is None:
            self._log.warning(
                f"Canceling REDUCE_ONLY {order.type_string_c()} "
                f"as would increase position.",
            )
            self._cancel_order(order)
            return  # Order canceled

        self._apply_fills(
            order=order,
            liquidity_side=liquidity_side,
            fills=self._determine_market_price_and_volume(order),
            venue_position_id=venue_position_id,
            position=position,
        )

    cpdef void _fill_limit_order(self, Order order, LiquiditySide liquidity_side) except *:
        cdef Price price = order.price
        if self._fill_model:
            if order.side == OrderSide.BUY and self._core.bid_raw == price._mem.raw and not self._fill_model.is_limit_filled():
                return  # Not filled
            elif order.side == OrderSide.SELL and self._core.ask_raw == price._mem.raw and not self._fill_model.is_limit_filled():
                return  # Not filled

        cdef PositionId venue_position_id = self._get_position_id(order)
        cdef Position position = None
        if venue_position_id is not None:
            position = self.cache.position(venue_position_id)
        if order.is_reduce_only and position is None:
            self._log.warning(
                f"Canceling REDUCE_ONLY {order.type_string_c()} "
                f"as would increase position.",
            )
            self._cancel_order(order)
            return  # Order canceled

        self._apply_fills(
            order=order,
            liquidity_side=liquidity_side,
            fills=self._determine_limit_price_and_volume(order),
            venue_position_id=venue_position_id,
            position=position,
        )

    cpdef void _apply_fills(
        self,
        Order order,
        LiquiditySide liquidity_side,
        list fills,
        PositionId venue_position_id,  # Can be None
        Position position,  # Can be None
    ) except*:
        if not fills:
            return  # No fills

        if self.oms_type == OMSType.NETTING:
            venue_position_id = None  # No position IDs generated by the venue

        if not self._log.is_bypassed:
            self._log.debug(
                f"Applying fills to {order}, "
                f"venue_position_id={venue_position_id}, "
                f"position={position}, "
                f"fills={fills}.",
            )

        cdef:
            uint64_t raw_org_qty
            uint64_t raw_adj_qty
            Price fill_px
            Quantity fill_qty
            Quantity updated_qty
        for fill_px, fill_qty in fills:
            if order.filled_qty._mem.raw == 0:
                if order.order_type == OrderType.MARKET_TO_LIMIT:
                    self._generate_order_updated(
                        order,
                        qty=order.quantity,
                        price=fill_px,
                        trigger_price=None,
                    )
                if order.time_in_force == TimeInForce.FOK and fill_qty._mem.raw < order.quantity._mem.raw:
                    # FOK order cannot fill the entire quantity - cancel
                    self._cancel_order(order)
                    return
            elif order.time_in_force == TimeInForce.IOC:
                # IOC order has already filled at one price - cancel remaining
                self._cancel_order(order)
                return

            if order.is_reduce_only and order.leaves_qty._mem.raw == 0:
                return  # Done early
            if order.order_type == OrderType.STOP_MARKET:
                fill_px = order.trigger_price  # TODO: Temporary strategy for market moving through price
            if self.book_type == BookType.L1_TBBO and self._fill_model.is_slipped():
                if order.side == OrderSide.BUY:
                    fill_px = fill_px.add(self.instrument.price_increment)
                elif order.side == OrderSide.SELL:
                    fill_px = fill_px.sub(self.instrument.price_increment)
                else:
                    raise ValueError(
                        f"invalid `OrderSide`, was {order.side}",  # pragma: no cover (design-time error)
                    )
            if order.is_reduce_only and fill_qty._mem.raw > position.quantity._mem.raw:
                # Adjust fill to honor reduce only execution
                raw_org_qty = fill_qty._mem.raw
                raw_adj_qty = fill_qty._mem.raw - (
                            fill_qty._mem.raw - position.quantity._mem.raw)
                fill_qty = Quantity.from_raw_c(raw_adj_qty, fill_qty._mem.precision)
                updated_qty = Quantity.from_raw_c(
                    order.quantity._mem.raw - (raw_org_qty - raw_adj_qty),
                    fill_qty._mem.precision)
                if updated_qty._mem.raw > 0:
                    self._generate_order_updated(
                        order=order,
                        qty=updated_qty,
                        price=None,
                        trigger_price=None,
                    )
            if not fill_qty._mem.raw > 0:
                return  # Done
            self._fill_order(
                order=order,
                venue_position_id=venue_position_id,
                position=position,
                last_qty=fill_qty,
                last_px=fill_px,
                liquidity_side=liquidity_side,
            )

        if (
            order.is_open_c()
            and self.book_type == BookType.L1_TBBO
            and (
            order.order_type == OrderType.MARKET
            or order.order_type == OrderType.MARKET_IF_TOUCHED
            or order.order_type == OrderType.STOP_MARKET
        )
        ):
            if order.time_in_force == TimeInForce.IOC:
                # IOC order has already filled at one price - cancel remaining
                self._cancel_order(order)
                return

            # Exhausted simulated book volume (continue aggressive filling into next level)
            fill_px = fills[-1][0]
            if order.side == OrderSide.BUY:
                fill_px = fill_px.add(self.instrument.price_increment)
            elif order.side == OrderSide.SELL:
                fill_px = fill_px.sub(self.instrument.price_increment)
            else:
                raise ValueError(
                    f"invalid `OrderSide`, was {order.side}",  # pragma: no cover (design-time error)
                )

            self._fill_order(
                order=order,
                venue_position_id=venue_position_id,
                position=position,
                last_qty=order.leaves_qty,
                last_px=fill_px,
                liquidity_side=liquidity_side,
            )

    cpdef void _fill_order(
        self,
        Order order,
        PositionId venue_position_id,  # Can be None
        Position position: Optional[Position],
        Quantity last_qty,
        Price last_px,
        LiquiditySide liquidity_side,
    ) except *:
        # Calculate commission
        cdef double notional = self.instrument.notional_value(
            quantity=last_qty,
            price=last_px,
            inverse_as_quote=False,
        ).as_f64_c()

        cdef double commission_f64
        if liquidity_side == LiquiditySide.MAKER:
            commission_f64 = notional * float(self.instrument.maker_fee)
        elif liquidity_side == LiquiditySide.TAKER:
            commission_f64 = notional * float(self.instrument.taker_fee)
        else:
            raise ValueError(
                f"invalid `LiquiditySide`, was {LiquiditySideParser.to_str(liquidity_side)}"
            )

        cdef Money commission
        if self.instrument.is_inverse:  # and not inverse_as_quote:
            commission = Money(commission_f64, self.instrument.base_currency)
        else:
            commission = Money(commission_f64, self.instrument.quote_currency)

        self._generate_order_filled(
            order=order,
            venue_position_id=venue_position_id,
            last_qty=last_qty,
            last_px=last_px,
            quote_currency=self.instrument.quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
        )

        if order.is_passive_c() and order.is_closed_c():
            # Remove order from market
            self._core.delete_order(order)

        # Check contingency orders
        cdef ClientOrderId client_order_id
        cdef Order child_order
        if order.contingency_type == ContingencyType.OTO:
            for client_order_id in order.linked_order_ids:
                child_order = self.cache.order(client_order_id)
                assert child_order is not None, "OTO child order not found"
                if child_order.position_id is None:
                    self.cache.add_position_id(
                        position_id=order.position_id,
                        venue=self.venue,
                        client_order_id=client_order_id,
                        strategy_id=child_order.strategy_id,
                    )
                    self._log.debug(
                        f"Indexed {repr(order.position_id)} "
                        f"for {repr(child_order.client_order_id)}",
                    )
                if not child_order.is_open_c():
                    self._accept_order(child_order)
        elif order.contingency_type == ContingencyType.OCO:
            for client_order_id in order.linked_order_ids:
                oco_order = self.cache.order(client_order_id)
                assert oco_order is not None, "OCO order not found"
                self._cancel_order(oco_order)
        elif order.contingency_type == ContingencyType.OUO:
            for client_order_id in order.linked_order_ids:
                ouo_order = self.cache.order(client_order_id)
                assert ouo_order is not None, "OUO order not found"
                if order.is_closed_c() and ouo_order.is_open_c():
                    self._cancel_order(ouo_order)
                elif order.leaves_qty._mem.raw != ouo_order.leaves_qty._mem.raw:
                    self._update_order(
                        ouo_order,
                        order.leaves_qty,
                        price=ouo_order.price if ouo_order.has_price_c() else None,
                        trigger_price=ouo_order.trigger_price if ouo_order.has_trigger_price_c() else None,
                        update_contingencies=False,
                    )

        if position is None:
            return  # Fill completed

        # Check reduce only orders for position
        for order in self.cache.orders_for_position(position.id):
            if (
                order.is_reduce_only
                and order.is_open_c()
                and order.is_passive_c()
            ):
                if position.quantity._mem.raw == 0:
                    self._cancel_order(order)
                elif order.leaves_qty._mem.raw != position.quantity._mem.raw:
                    self._update_order(
                        order,
                        position.quantity,
                        price=order.price if order.has_price_c() else None,
                        trigger_price=order.trigger_price if order.has_trigger_price_c() else None,
                    )

# -- IDENTIFIER GENERATORS ------------------------------------------------------------------------

    cdef PositionId _get_position_id(self, Order order, bint generate=True):
        cdef PositionId position_id
        if OMSType.HEDGING:
            position_id = self.cache.position_id(order.client_order_id)
            if position_id is not None:
                return position_id
            if generate:
                # Generate a venue position ID
                return self._generate_venue_position_id()
        ####################################################################
        # NETTING OMS (position ID will be `{instrument_id}-{strategy_id}`)
        ####################################################################
        cdef list positions_open = self.cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=order.instrument_id,
        )
        if positions_open:
            return positions_open[0].id
        else:
            return None

    cdef PositionId _generate_venue_position_id(self):
        self._position_count += 1
        return PositionId(
            f"{self.venue.to_str()}-{self.product_id}-{self._position_count:03d}")

    cdef VenueOrderId _generate_venue_order_id(self):
        self._order_count += 1
        return VenueOrderId(
            f"{self.venue.to_str()}-{self.product_id}-{self._order_count:03d}")

    cdef TradeId _generate_trade_id(self):
        self._execution_count += 1
        return TradeId(self._generate_trade_id_str())

    cdef str _generate_trade_id_str(self):
        return f"{self.venue.to_str()}-{self.product_id}-{self._execution_count:03d}"

# -- EVENT HANDLING -------------------------------------------------------------------------------

    cpdef void _accept_order(self, Order order) except *:
        self._core.add_order(order)
        self._generate_order_accepted(order)

    cpdef void _expire_order(self, Order order) except *:
        if order.contingency_type != ContingencyType.NONE:
            self._cancel_contingency_orders(order)

        self._generate_order_expired(order)

    cpdef void _cancel_order(self, Order order, bint cancel_contingencies=True) except *:
        if order.venue_order_id is None:
            order.venue_order_id = self._generate_venue_order_id()

        self._core.delete_order(order)

        self._generate_order_canceled(order)

        if order.contingency_type != ContingencyType.NONE and cancel_contingencies:
            self._cancel_contingency_orders(order)

    cpdef void _update_order(
        self,
        Order order,
        Quantity qty,
        Price price = None,
        Price trigger_price = None,
        bint update_contingencies = True,
    ) except *:
        if qty is None:
            qty = order.quantity

        if order.order_type == OrderType.LIMIT or order.order_type == OrderType.MARKET_TO_LIMIT:
            if price is None:
                price = order.price
            self._update_limit_order(order, qty, price)
        elif order.order_type == OrderType.STOP_MARKET or order.order_type == OrderType.MARKET_IF_TOUCHED:
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_stop_market_order(order, qty, trigger_price)
        elif order.order_type == OrderType.STOP_LIMIT or order.order_type == OrderType.LIMIT_IF_TOUCHED:
            if price is None:
                price = order.price
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_stop_limit_order(order, qty, price, trigger_price)
        else:
            raise ValueError(
                f"invalid `OrderType` was {order.order_type}")  # pragma: no cover (design-time error)

        if order.contingency_type != ContingencyType.NONE and update_contingencies:
            self._update_contingency_orders(order)

    cpdef void _trigger_stop_order(self, Order order) except *:
        cdef Price trigger_price = order.trigger_price
        if self._fill_model:
            if order.side == OrderSide.BUY and self._core.ask_raw == trigger_price._mem.raw and not self._fill_model.is_stop_filled():
                return  # Not triggered
            elif order.side == OrderSide.SELL and self._core.bid_raw == trigger_price._mem.raw and not self._fill_model.is_stop_filled():
                return  # Not triggered

        self._generate_order_triggered(order)
        # Check for immediate fill
        if self._core.is_limit_matched(order.side, order.price) and order.is_post_only:
            # Would be liquidity taker
            self._core.delete_order(order)
            self._generate_order_rejected(
                order,
                f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                f"limit px of {order.price} would have been a TAKER: "
                f"bid={self._core.bid}, "
                f"ask={self._core.ask}",
            )

    cpdef void _update_contingency_orders(self, Order order) except *:
        self._log.debug(f"Updating OUO orders from {order.client_order_id}")
        cdef ClientOrderId client_order_id
        cdef Order ouo_order
        for client_order_id in order.linked_order_ids:
            ouo_order = self.cache.order(client_order_id)
            assert ouo_order is not None, "OUO order not found"
            if ouo_order.leaves_qty._mem.raw != order.leaves_qty._mem.raw:
                self._update_order(
                    ouo_order,
                    order.leaves_qty,
                    price=ouo_order.price if ouo_order.has_price_c() else None,
                    trigger_price=ouo_order.trigger_price if ouo_order.has_trigger_price_c() else None,
                    update_contingencies=False,
                )

    cpdef void _cancel_contingency_orders(self, Order order) except *:
        self._log.debug(f"Canceling contingency orders from {order.client_order_id}")
        # Iterate all contingency orders and cancel if active
        cdef ClientOrderId client_order_id
        cdef Order contingency_order
        for client_order_id in order.linked_order_ids:
            contingency_order = self.cache.order(client_order_id)
            assert contingency_order is not None, "Contigency order not found"
            if contingency_order.is_open_c():
                self._cancel_order(contingency_order, cancel_contingencies=False)

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_order_rejected(self, Order order, str reason) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderRejected event = OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            reason=reason,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_accepted(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderAccepted event = OrderAccepted(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=self._generate_venue_order_id(),
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_pending_update(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderPendingUpdate event = OrderPendingUpdate(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_pending_cancel(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderPendingCancel event = OrderPendingCancel(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_modify_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        AccountId account_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    ) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderModifyRejected event = OrderModifyRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_cancel_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        AccountId account_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    ) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderCancelRejected event = OrderCancelRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cpdef void _generate_order_updated(
        self,
        Order order,
        Quantity quantity,
        Price price,
        Price trigger_price,
    ) except *:
        cdef VenueOrderId venue_order_id = order.venue_order_id
        cdef bint venue_order_id_modified = False
        if venue_order_id is None:
            venue_order_id = self._generate_venue_order_id()
            venue_order_id_modified = True

        # Check venue_order_id against cache, only allow modification when `venue_order_id_modified=True`
        if not venue_order_id_modified:
            existing = self.cache.venue_order_id(order.client_order_id)
            if existing is not None:
                Condition.equal(existing, order.venue_order_id, "existing", "order.venue_order_id")
            else:
                self._log.warning(f"{order.venue_order_id} does not match existing {repr(existing)}")

        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_canceled(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderCanceled event = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_triggered(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderTriggered event = OrderTriggered(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_expired(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderExpired event = OrderExpired(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.client_order_id],
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_filled(
        self,
        Order order,
        PositionId venue_position_id,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side
    ) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderFilled event = OrderFilled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id or self._generate_venue_order_id(),
            account_id=order.account_id or self._account_ids[order.trader_id],
            trade_id=self._generate_trade_id(),
            position_id=venue_position_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=last_qty,
            last_px=last_px,
            currency=quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)
