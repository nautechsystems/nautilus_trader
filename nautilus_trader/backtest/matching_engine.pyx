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

from libc.limits cimport INT_MAX
from libc.limits cimport INT_MIN
from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.core.uuid cimport UUID4
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
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetType
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetTypeParser
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderEvent
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
    log : LoggerAdapter
        The logger adapter for the matching engine.
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
        LoggerAdapter log not None,
    ):
        self._clock = clock
        self._log = log
        self._msgbus = msgbus

        self.venue = instrument.id.venue
        self.instrument = instrument
        self.product_id = product_id
        self.book_type = book_type
        self.oms_type = oms_type
        self.cache = cache

        self._reject_stop_orders = reject_stop_orders
        self._fill_model = fill_model
        self._book = OrderBook.create(
            instrument=instrument,
            book_type=book_type,
            simulated=True,
        )
        self._account_ids: dict[TraderId, AccountId]  = {}

        # Market
        self._last: Optional[Price] = None
        self._last_bid: Optional[Price] = None
        self._last_ask: Optional[Price] = None
        self._last_bid_bar: Optional[Bar] = None
        self._last_ask_bar: Optional[Bar] = None
        self._order_index: dict[ClientOrderId, Order] = {}
        self._orders_bid: list[Order] = []
        self._orders_ask: list[Order] = []
        self._oto_orders: dict[ClientOrderId, ClientOrderId] = {}
        self._bar_execution: bool = False

        self._position_count = 0
        self._order_count = 0
        self._execution_count = 0

    cpdef void reset(self) except *:
        self._log.debug(f"Resetting OrderMatchingEngine {self.instrument.id}...")

        self._book.clear()
        self._account_ids.clear()
        self._last: Optional[Price] = None
        self._last_bid: Optional[Price] = None
        self._last_ask: Optional[Price] = None
        self._last_bid_bar: Optional[Bar] = None
        self._last_ask_bar: Optional[Bar] = None
        self._order_index.clear()
        self._orders_bid.clear()
        self._orders_ask.clear()
        self._oto_orders.clear()
        self._bar_execution: bool = False

        self._position_count = 0
        self._order_count = 0
        self._execution_count = 0

        self._log.info(f"Reset OrderMatchingEngine {self.instrument.id}.")

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
        return self._orders_bid

    cpdef list get_open_ask_orders(self):
        """
        Return the open ask orders at the exchange.

        Returns
        -------
        list[Order]

        """
        return self._orders_ask

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void process_order_book(self, OrderBookData data) except *:
        """
        Process the exchanges market for the given order book data.

        Parameters
        ----------
        data : OrderBookData
            The order book data to process.

        """
        Condition.not_none(data, "data")

        self._book.apply(data)

        self.iterate(data.ts_init)

        if not self._log.is_bypassed:
            self._log.debug(f"Processed {repr(data)}")

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

        if self.book_type == BookType.L1_TBBO:
            self._book.update_quote_tick(tick)

        self.iterate(tick.ts_init)

        if not self._log.is_bypassed:
            self._log.debug(f"Processed {repr(tick)}")

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

        if self.book_type == BookType.L1_TBBO:
            self._book.update_trade_tick(tick)

        self._last = tick.price

        self.iterate(tick.ts_init)

        if not self._log.is_bypassed:
            self._log.debug(f"Processed {repr(tick)}")

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

        if self.book_type != BookType.L1_TBBO:
            return  # Can only process an L1 book with bars

        # Turn ON bar execution mode (temporary until unify execution)
        self._bar_execution = True

        cdef PriceType price_type = bar.type.spec.price_type
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

        if not self._log.is_bypassed:
            self._log.debug(f"Processed {repr(bar)}")

    cdef void _process_trade_ticks_from_bar(self, Bar bar) except *:
        cdef Quantity size = Quantity(bar.volume.as_double() / 4.0, bar._mem.volume.precision)

        # Create reusable tick
        cdef TradeTick tick = TradeTick(
            bar.type.instrument_id,
            bar.open,
            size,
            <OrderSide>AggressorSide.BUY if self._last is None or bar._mem.open.raw > self._last._mem.raw else <OrderSide>AggressorSide.SELL,
            self._generate_trade_id(),
            bar.ts_event,
            bar.ts_event,
        )

        # Open
        if self._last is None or bar._mem.open.raw != self._last._mem.raw:  # Direct memory comparison
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._last = bar.open

        # High
        if bar._mem.high.raw > self._last._mem.raw:  # Direct memory comparison
            tick._mem.price = bar._mem.high  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.BUY  # Direct memory assignment
            tick._mem.trade_id = self._generate_trade_id()._mem
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._last = bar.high

        # Low
        if bar._mem.low.raw < self._last._mem.raw:  # Direct memory comparison
            tick._mem.price = bar._mem.low  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.SELL
            tick._mem.trade_id = self._generate_trade_id()._mem
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._last = bar.low

        # Close
        if bar._mem.close.raw != self._last._mem.raw:  # Direct memory comparison
            tick._mem.price = bar._mem.close  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.BUY if bar._mem.close.raw > self._last._mem.raw else <OrderSide>AggressorSide.SELL
            tick._mem.trade_id = self._generate_trade_id()._mem
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._last = bar.close

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

# -- COMMAND HANDLING -----------------------------------------------------------------------------

    cpdef bint order_exists(self, ClientOrderId client_order_id) except *:
        return client_order_id in self._order_index

    cpdef void process_order(self, Order order, AccountId account_id) except *:
        if order.client_order_id in self._order_index:
            return  # Already processed

        # Index identifiers
        self._account_ids[order.trader_id] = account_id

        # Check contingency orders
        cdef ClientOrderId client_order_id
        if order.contingency_type == ContingencyType.OTO:
            assert order.linked_order_ids is not None
            for client_order_id in order.linked_order_ids:
                self._oto_orders[client_order_id] = order.client_order_id

        cdef Order parent
        if order.parent_order_id is not None:
            if order.client_order_id in self._oto_orders:
                parent = self.cache.order(order.parent_order_id)
                assert parent is not None, "OTO parent not found"
                if parent.status_c() == OrderStatus.REJECTED and order.is_open_c():
                    self._generate_order_rejected(
                        order,
                        f"REJECT OTO from {parent.client_order_id}",
                    )
                    return  # Order rejected
                elif parent.status_c() == OrderStatus.ACCEPTED:
                    self._log.info(
                        f"Pending OTO {order.client_order_id} "
                        f"triggers from {parent.client_order_id}",
                    )
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

        if order.type == OrderType.MARKET:
            self._process_market_order(order)
        elif order.type == OrderType.MARKET_TO_LIMIT:
            self._process_market_to_limit_order(order)
        elif order.type == OrderType.LIMIT:
            self._process_limit_order(order)
        elif order.type == OrderType.STOP_MARKET or order.type == OrderType.MARKET_IF_TOUCHED:
            self._process_stop_market_order(order)
        elif order.type == OrderType.STOP_LIMIT or order.type == OrderType.LIMIT_IF_TOUCHED:
            self._process_stop_limit_order(order)
        elif order.type == OrderType.TRAILING_STOP_MARKET:
            self._process_trailing_stop_market_order(order)
        elif order.type == OrderType.TRAILING_STOP_LIMIT:
            self._process_trailing_stop_limit_order(order)
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"{OrderTypeParser.to_str(order.type)} "
                f"orders are not supported for backtesting in this version",
            )

    cpdef void process_modify(self, ModifyOrder command, AccountId account_id) except *:
        cdef Order order = self._order_index.get(command.client_order_id)
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
        cdef Order order = self._order_index.pop(command.client_order_id, None)
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
        for order in self._orders_bid + self._orders_ask:
            if order.is_inflight_c() or order.is_open_c():
                self._generate_order_pending_cancel(order)
                self._cancel_order(order)

    cdef void _process_market_order(self, MarketOrder order) except*:
        # Check market exists
        if order.side == OrderSide.BUY and not self.best_ask_price():
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self.best_bid_price():
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Immediately fill marketable order
        self.fill_market_order(order, LiquiditySide.TAKER)

    cdef void _process_market_to_limit_order(self, MarketToLimitOrder order) except*:
        # Check market exists
        if order.side == OrderSide.BUY and not self.best_ask_price():
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self.best_bid_price():
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Order is valid and accepted
        self._accept_order(order)

        # Immediately fill marketable order
        self.fill_market_order(order, LiquiditySide.TAKER)

    cdef void _process_limit_order(self, LimitOrder order) except*:
        if order.is_post_only and self.is_limit_marketable(order.side, order.price):
            self._generate_order_rejected(
                order,
                f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                f"limit px of {order.price} would have been a TAKER: "
                f"bid={self.best_bid_price()}, "
                f"ask={self.best_ask_price()}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

        # Check for immediate fill
        if self.is_limit_matched(order.side, order.price):
            # Filling as liquidity taker
            self.fill_limit_order(order, LiquiditySide.TAKER)
        elif order.time_in_force == TimeInForce.FOK or order.time_in_force == TimeInForce.IOC:
            self._cancel_order(order)

    cdef void _process_stop_market_order(self, Order order) except*:
        if self.is_stop_marketable(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"stop px of {order.trigger_price} was in the market: "
                    f"bid={self.best_bid_price()}, "
                    f"ask={self.best_ask_price()}",
                )
                return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

    cdef void _process_stop_limit_order(self, Order order) except*:
        if self.is_stop_marketable(order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self.best_bid_price()}, "
                f"ask={self.best_ask_price()}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

    cdef void _process_trailing_stop_market_order(self, TrailingStopMarketOrder order) except*:
        if order.has_trigger_price_c() and self.is_stop_marketable(order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self.best_bid_price()}, "
                f"ask={self.best_ask_price()}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

        if order.trigger_price is None:
            self._manage_trailing_stop(order)

    cdef void _process_trailing_stop_limit_order(self, TrailingStopLimitOrder order) except*:
        if order.has_trigger_price_c() and self.is_stop_marketable(order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self.best_bid_price()}, "
                f"ask={self.best_ask_price()}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

        if order.trigger_price is None:
            self._manage_trailing_stop(order)

    cdef void _update_limit_order(
        self,
        Order order,
        Quantity qty,
        Price price,
    ) except*:
        if self.is_limit_marketable(order.side, price):
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
                    f"bid={self.best_bid_price()}, "
                    f"ask={self.best_ask_price()}",
                )
                return  # Cannot update order

            self._generate_order_updated(order, qty, price, None)
            self.fill_limit_order(order, LiquiditySide.TAKER)  # Immediate fill as TAKER
            return  # Filled

        self._generate_order_updated(order, qty, price, None)

    cdef void _update_stop_market_order(
        self,
        Order order,
        Quantity qty,
        Price trigger_price,
    ) except*:
        if self.is_stop_marketable(order.side, trigger_price):
            self._generate_order_modify_rejected(
                trader_id=order.trader_id,
                strategy_id=order.strategy_id,
                account_id=order.account_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=f"{order.type_string_c()} {order.side_string_c()} order "
                f"new stop px of {trigger_price} was in the market: "
                f"bid={self.best_bid_price()}, "
                f"ask={self.best_ask_price()}",
            )
            return  # Cannot update order

        self._generate_order_updated(order, qty, None, trigger_price)

    cdef void _update_stop_limit_order(
        self,
        Order order,
        Quantity qty,
        Price price,
        Price trigger_price,
    ) except*:
        if not order.is_triggered:
            # Updating stop price
            if self.is_stop_marketable(order.side, price):
                self._generate_order_modify_rejected(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    account_id=order.account_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=f"{order.type_string_c()} {order.side_string_c()} order "
                    f"new trigger stop px of {price} was in the market: "
                    f"bid={self.best_bid_price()}, "
                    f"ask={self.best_ask_price()}",
                )
                return  # Cannot update order
        else:
            # Updating limit price
            if self.is_limit_marketable(order.side, price):
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
                        f"bid={self.best_bid_price()}, "
                        f"ask={self.best_ask_price()}",
                    )
                    return  # Cannot update order
                else:
                    self._generate_order_updated(order, qty, price, None)
                    self.fill_limit_order(order, LiquiditySide.TAKER)  # Immediate fill as TAKER
                    return  # Filled

        self._generate_order_updated(order, qty, price, trigger_price or order.trigger_price)

# -------------------------------------------------------------------------------------------------
    cpdef void add_order(self, Order order) except *:
        # Index order
        self._order_index[order.client_order_id] = order
        self._add_order(order)

    cdef void _add_order(self, Order order) except *:
        if order.side == OrderSide.BUY:
            self._orders_bid.append(order)
            self._orders_bid.sort(key=lambda o: o.price if (o.type == OrderType.LIMIT or o.type == OrderType.MARKET_TO_LIMIT) or (o.type == OrderType.STOP_LIMIT and o.is_triggered) else o.trigger_price or INT_MIN, reverse=True)  # noqa  TODO(cs): Will refactor!
        elif order.side == OrderSide.SELL:
            self._orders_ask.append(order)
            self._orders_ask.sort(key=lambda o: o.price if (o.type == OrderType.LIMIT or o.type == OrderType.MARKET_TO_LIMIT) or (o.type == OrderType.STOP_LIMIT and o.is_triggered) else o.trigger_price or INT_MAX)  # noqa  TODO(cs): Will refactor!

    cpdef void delete_order(self, Order order) except *:
        self._order_index.pop(order.client_order_id, None)

        if order.side == OrderSide.BUY:
            self._orders_bid.remove(order)
        elif order.side == OrderSide.SELL:
            self._orders_ask.remove(order)

    cpdef void iterate(self, uint64_t timestamp_ns) except *:
        self._clock.set_time(timestamp_ns)

        # Iterate bids
        if self._orders_bid:
            self._iterate_side(self._orders_bid.copy(), timestamp_ns)  # Copy list for safe loop

        # Iterate asks
        if self._orders_ask:
            self._iterate_side(self._orders_ask.copy(), timestamp_ns)  # Copy list for safe loop

    cdef void _iterate_side(self, list orders, uint64_t timestamp_ns) except *:
        cdef Order order
        for order in orders:
            if not order.is_open_c():
                continue  # Orders state has changed since the loop started
            elif order.expire_time_ns > 0 and timestamp_ns >= order.expire_time_ns:
                self.delete_order(order)
                self._expire_order(order)
                continue
            # Check for order match
            self.match_order(order)

            if order.is_open_c() and (order.type == OrderType.TRAILING_STOP_MARKET or order.type == OrderType.TRAILING_STOP_LIMIT):
                self._manage_trailing_stop(order)

    cpdef void match_order(self, Order order) except *:
        if order.type == OrderType.LIMIT or order.type == OrderType.MARKET_TO_LIMIT:
            self.match_limit_order(order)
        elif (
            order.type == OrderType.STOP_MARKET
            or order.type == OrderType.MARKET_IF_TOUCHED
            or order.type == OrderType.TRAILING_STOP_MARKET
        ):
            self.match_stop_market_order(order)
        elif (
            order.type == OrderType.STOP_LIMIT
            or order.type == OrderType.LIMIT_IF_TOUCHED
            or order.type == OrderType.TRAILING_STOP_LIMIT
        ):
            self.match_stop_limit_order(order)
        else:
            raise ValueError(f"invalid `OrderType` was {order.type}")  # pragma: no cover (design-time error)

    cpdef void match_limit_order(self, Order order) except *:
        if self.is_limit_matched(order.side, order.price):
            self.fill_limit_order(order, LiquiditySide.MAKER)

    cpdef void match_stop_market_order(self, Order order) except *:
        if self.is_stop_triggered(order.side, order.trigger_price):
            # Triggered stop places market order
            self.fill_market_order(order, LiquiditySide.TAKER)

    cpdef void match_stop_limit_order(self, Order order) except *:
        if order.is_triggered:
            if self.is_limit_matched(order.side, order.price):
                self.fill_limit_order(order, LiquiditySide.MAKER)
            return

        if self.is_stop_triggered(order.side, order.trigger_price):
            self._generate_order_triggered(order)
            # Check for immediate fill
            if not self.is_limit_marketable(order.side, order.price):
                return

            if order.is_post_only:  # Would be liquidity taker
                self.delete_order(order)  # Remove order from open orders
                self._generate_order_rejected(
                    order,
                    f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"limit px of {order.price} would have been a TAKER: "
                    f"bid={self.best_bid_price()}, "
                    f"ask={self.best_ask_price()}",
                )
            else:
                self.fill_limit_order(order, LiquiditySide.TAKER)  # Fills as TAKER

    cpdef bint is_limit_marketable(self, OrderSide side, Price order_price) except *:
        cdef Price bid
        cdef Price ask
        if side == OrderSide.BUY:
            ask = self.best_ask_price()
            if ask is None:
                return False  # No market
            return order_price._mem.raw >= ask._mem.raw  # Match with LIMIT sells
        elif side == OrderSide.SELL:
            bid = self.best_bid_price()
            if bid is None:  # No market
                return False
            return order_price._mem.raw <= bid._mem.raw  # Match with LIMIT buys
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef bint is_limit_matched(self, OrderSide side, Price price) except *:
        cdef Price bid
        cdef Price ask
        if side == OrderSide.BUY:
            ask = self.best_ask_price()
            if ask is None:
                return False  # No market
            return price._mem.raw > ask._mem.raw or (ask._mem.raw == price._mem.raw and self._fill_model.is_limit_filled())
        elif side == OrderSide.SELL:
            bid = self.best_bid_price()
            if bid is None:
                return False  # No market
            return price._mem.raw < bid._mem.raw or (bid._mem.raw == price._mem.raw and self._fill_model.is_limit_filled())
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef bint is_stop_marketable(self, OrderSide side, Price price) except *:
        cdef Price bid
        cdef Price ask
        if side == OrderSide.BUY:
            ask = self.best_ask_price()
            if ask is None:
                return False  # No market
            return ask._mem.raw >= price._mem.raw  # Match with LIMIT sells
        elif side == OrderSide.SELL:
            bid = self.best_bid_price()
            if bid is None:
                return False  # No market
            return bid._mem.raw <= price._mem.raw  # Match with LIMIT buys
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef bint is_stop_triggered(self, OrderSide side, Price price) except *:
        cdef Price bid
        cdef Price ask
        if side == OrderSide.BUY:
            ask = self.best_ask_price()
            if ask is None:
                return False  # No market
            return ask._mem.raw > price._mem.raw or (ask._mem.raw == price._mem.raw and self._fill_model.is_stop_filled())
        elif side == OrderSide.SELL:
            bid = self.best_bid_price()
            if bid is None:
                return False  # No market
            return bid._mem.raw < price._mem.raw or (bid._mem.raw == price._mem.raw and self._fill_model.is_stop_filled())
        else:
            raise ValueError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef list determine_limit_price_and_volume(self, Order order):
        if self._bar_execution:
            if order.side == OrderSide.BUY:
                self._last_bid = order.price
            elif order.side == OrderSide.SELL:
                self._last_ask = order.price
            self._last = order.price
            return [(order.price, order.leaves_qty)]
        cdef OrderBookOrder submit_order = OrderBookOrder(price=order.price, size=order.leaves_qty, side=order.side)
        if order.side == OrderSide.BUY:
            return self._book.asks.simulate_order_fills(order=submit_order, depth_type=DepthType.VOLUME)
        elif order.side == OrderSide.SELL:
            return self._book.bids.simulate_order_fills(order=submit_order, depth_type=DepthType.VOLUME)

    cpdef list determine_market_price_and_volume(self, Order order):
        cdef Price price
        if self._bar_execution:
            if order.type == OrderType.MARKET or order.type == OrderType.MARKET_IF_TOUCHED:
                if order.side == OrderSide.BUY:
                    price = self._last_ask
                    if price is None:
                        price = self.best_ask_price()
                    self._last = price
                    if price is not None:
                        return [(price, order.leaves_qty)]
                    else:
                        raise RuntimeError(  # pragma: no cover (design-time error)
                            "Market best ASK price was None when filling MARKET order",
                        )
                elif order.side == OrderSide.SELL:
                    price = self._last_bid
                    if price is None:
                        price = self.best_bid_price()
                    self._last = price
                    if price is not None:
                        return [(price, order.leaves_qty)]
                    else:
                        raise RuntimeError(  # pragma: no cover (design-time error)
                            "Market best BID price was None when filling MARKET order",
                        )
            else:
                price = order.price if order.type == OrderType.LIMIT else order.trigger_price
                if order.side == OrderSide.BUY:
                    self._last_ask = price
                elif order.side == OrderSide.SELL:
                    self._last_bid = price
                self._last = price
                return [(price, order.leaves_qty)]
        price = Price.from_int_c(INT_MAX if order.side == OrderSide.BUY else INT_MIN)
        cdef OrderBookOrder submit_order = OrderBookOrder(price=price, size=order.leaves_qty, side=order.side)
        if order.side == OrderSide.BUY:
            return self._book.asks.simulate_order_fills(order=submit_order)
        elif order.side == OrderSide.SELL:
            return self._book.bids.simulate_order_fills(order=submit_order)

    cdef void fill_market_order(self, Order order, LiquiditySide liquidity_side) except *:
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

        self.apply_fills(
            order=order,
            liquidity_side=liquidity_side,
            fills=self.determine_market_price_and_volume(order),
            venue_position_id=venue_position_id,
            position=position,
        )

    cdef void fill_limit_order(self, Order order, LiquiditySide liquidity_side) except *:
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

        self.apply_fills(
            order=order,
            liquidity_side=liquidity_side,
            fills=self.determine_limit_price_and_volume(order),
            venue_position_id=venue_position_id,
            position=position,
        )

    cdef void apply_fills(
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
                if order.type == OrderType.MARKET_TO_LIMIT:
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
            if order.type == OrderType.STOP_MARKET:
                fill_px = order.trigger_price  # TODO: Temporary strategy for market moving through price
            if self.book_type == BookType.L1_TBBO and self._fill_model.is_slipped():
                if order.side == OrderSide.BUY:
                    fill_px = fill_px.add(self.instrument.price_increment)
                elif order.side == OrderSide.SELL:
                    fill_px = fill_px.sub(self.instrument.price_increment)
                else:
                    raise ValueError(
                        f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)
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
            self.fill_order(
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
            order.type == OrderType.MARKET
            or order.type == OrderType.MARKET_IF_TOUCHED
            or order.type == OrderType.STOP_MARKET
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
                raise ValueError(  # pragma: no cover (design-time error)
                    f"invalid `OrderSide`, was {order.side}",
                )

            self.fill_order(
                order=order,
                venue_position_id=venue_position_id,
                position=position,
                last_qty=order.leaves_qty,
                last_px=fill_px,
                liquidity_side=liquidity_side,
            )

    cdef void fill_order(
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
            self.delete_order(order)

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
                if order.is_closed_c() and oco_order.is_open_c():
                    self._cancel_order(oco_order)
                elif order.leaves_qty._mem.raw != oco_order.leaves_qty._mem.raw:
                    self._update_order(
                        oco_order,
                        order.leaves_qty,
                        price=oco_order.price if oco_order.has_price_c() else None,
                        trigger_price=oco_order.trigger_price if oco_order.has_trigger_price_c() else None,
                        update_ocos=False,
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

    cdef void _manage_trailing_stop(self, Order order) except *:
        cdef int64_t trailing_offset_raw = int(order.trailing_offset * int(FIXED_SCALAR))
        cdef int64_t limit_offset_raw = 0

        cdef Price trigger_price = order.trigger_price
        cdef Price price = None
        cdef Price new_trigger_price = None
        cdef Price new_price = None

        if order.type == OrderType.TRAILING_STOP_LIMIT:
            price = order.price
            limit_offset_raw = int(order.limit_offset * int(FIXED_SCALAR))

        cdef:
            Price bid
            Price ask
            Price temp_trigger_price
            Price temp_price
        if (
            order.trigger_type == TriggerType.DEFAULT
            or order.trigger_type == TriggerType.LAST
            or order.trigger_type == TriggerType.MARK
        ):
            if self._last is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(please add trade ticks or use bars)",
                )
            if order.side == OrderSide.BUY:
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    last=self._last,
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        last=self._last,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    last=self._last,
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        last=self._last,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        elif order.trigger_type == TriggerType.BID_ASK:
            bid = self.best_bid_price()
            ask = self.best_ask_price()

            if bid is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no BID price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )
            if ask is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no ASK price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )

            if order.side == OrderSide.BUY:
                temp_trigger_price = self._calculate_new_trailing_price_bid_ask(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    bid=bid,
                    ask=ask
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_bid_ask(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = self._calculate_new_trailing_price_bid_ask(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    bid=bid,
                    ask=ask
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_bid_ask(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        elif order.trigger_type == TriggerType.LAST_OR_BID_ASK:
            bid = self.best_bid_price()
            ask = self.best_ask_price()

            if self._last is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(please add trade ticks or use bars)",
                )
            if bid is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no BID price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )
            if ask is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no ASK price for {order.instrument_id} "
                    f"(please add quote ticks or use bars)",
                )

            if order.side == OrderSide.BUY:
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    last=self._last,
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                    trigger_price = new_trigger_price  # Set trigger to new trigger
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        last=self._last,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
                        price = new_price  # Set price to new price

                temp_trigger_price = self._calculate_new_trailing_price_bid_ask(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    bid=bid,
                    ask=ask
                )
                if trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_bid_ask(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.side == OrderSide.SELL:
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    last=self._last,
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                    trigger_price = new_trigger_price  # Set trigger to new trigger
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        last=self._last,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
                        price = new_price  # Set price to new price

                temp_trigger_price = self._calculate_new_trailing_price_bid_ask(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    bid=bid,
                    ask=ask
                )
                if trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_bid_ask(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        bid=bid,
                        ask=ask,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"`TriggerType.{TriggerTypeParser.to_str(order.trigger_type)}` "
                f"not currently supported",
            )

        if new_trigger_price is None and new_price is None:
            return  # No updates

        self._generate_order_updated(
            order,
            qty=order.quantity,
            price=new_price,
            trigger_price=new_trigger_price,
        )

    cdef Price _calculate_new_trailing_price_last(
        self,
        Order order,
        TrailingOffsetType trailing_offset_type,
        double offset,
        Price last,
    ):
        cdef double last_f64 = last.as_f64_c()
        cdef Instrument instrument = None

        if trailing_offset_type == TrailingOffsetType.DEFAULT or trailing_offset_type == TrailingOffsetType.PRICE:
            pass  # Offset already calculated
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            offset = last_f64 * (offset / 100) / 100
        elif trailing_offset_type == TrailingOffsetType.TICKS:
            instrument = self.cache.instrument(order.instrument_id)
            if instrument is None:
                raise RuntimeError(
                    f"cannot calculate trailing stop price, "
                    f"no instrument for {order.instrument_id}",
                )
            offset *= instrument.price_increment.as_f64_c()
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"TrailingOffsetType.{TrailingOffsetTypeParser.to_str(trailing_offset_type)} "
                f"not currently supported",
            )

        if order.side == OrderSide.BUY:
            return Price(last_f64 + offset, precision=last._mem.precision)
        elif order.side == OrderSide.SELL:
            return Price(last_f64 - offset, precision=last._mem.precision)

    cdef Price _calculate_new_trailing_price_bid_ask(
        self,
        Order order,
        TrailingOffsetType trailing_offset_type,
        double offset,
        Price bid,
        Price ask,
    ):
        cdef double ask_f64 = ask.as_f64_c()
        cdef double bid_f64 = bid.as_f64_c()
        cdef Instrument instrument = None

        if trailing_offset_type == TrailingOffsetType.DEFAULT or trailing_offset_type == TrailingOffsetType.PRICE:
            pass  # Offset already calculated
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            if order.side == OrderSide.BUY:
                offset = ask_f64 * (offset / 100) / 100
            elif order.side == OrderSide.SELL:
                offset = bid_f64 * (offset / 100) / 100
        elif trailing_offset_type == TrailingOffsetType.TICKS:
            instrument = self.cache.instrument(order.instrument_id)
            if instrument is None:
                raise RuntimeError(
                    f"cannot calculate trailing stop price, "
                    f"no instrument for {order.instrument_id}",
                )
            offset *= instrument.price_increment.as_f64_c()
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"`TrailingOffsetType.{TrailingOffsetTypeParser.to_str(trailing_offset_type)}` "
                f"not currently supported",
            )

        if order.side == OrderSide.BUY:
            return Price(ask_f64 + offset, precision=ask._mem.precision)
        elif order.side == OrderSide.SELL:
            return Price(bid_f64 - offset, precision=bid._mem.precision)

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
            f"{self.venue.value}-{self.product_id}-{self._position_count:03d}")

    cdef VenueOrderId _generate_venue_order_id(self):
        self._order_count += 1
        return VenueOrderId(
            f"{self.venue.value}-{self.product_id}-{self._order_count:03d}")

    cdef TradeId _generate_trade_id(self):
        self._execution_count += 1
        return TradeId(
            f"{self.venue.value}-{self.product_id}-{self._execution_count:03d}",
        )

# -- EVENT HANDLING -------------------------------------------------------------------------------

    cdef void _accept_order(self, Order order) except *:
        self.add_order(order)
        self._generate_order_accepted(order)

    cdef void _update_order(
        self,
        Order order,
        Quantity qty,
        Price price = None,
        Price trigger_price = None,
        bint update_ocos = True,
    ) except *:
        if qty is None:
            qty = order.quantity

        if order.type == OrderType.LIMIT or order.type == OrderType.MARKET_TO_LIMIT:
            if price is None:
                price = order.price
            self._update_limit_order(order, qty, price)
        elif order.type == OrderType.STOP_MARKET or order.type == OrderType.MARKET_IF_TOUCHED:
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_stop_market_order(order, qty, trigger_price)
        elif order.type == OrderType.STOP_LIMIT or order.type == OrderType.LIMIT_IF_TOUCHED:
            if price is None:
                price = order.price
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_stop_limit_order(order, qty, price, trigger_price)
        else:
            raise ValueError(
                f"invalid `OrderType` was {order.type}")  # pragma: no cover (design-time error)

        if order.contingency_type == ContingencyType.OCO and update_ocos:
            self._update_oco_orders(order)

    cdef void _update_oco_orders(self, Order order) except *:
        self._log.debug(f"Updating OCO orders from {order.client_order_id}")
        cdef ClientOrderId client_order_id
        cdef Order oco_order
        for client_order_id in order.linked_order_ids:
            oco_order = self.cache.order(client_order_id)
            assert oco_order is not None, "OCO order not found"
            if oco_order.leaves_qty._mem.raw != order.leaves_qty._mem.raw:
                self._update_order(
                    oco_order,
                    order.leaves_qty,
                    price=oco_order.price if oco_order.has_price_c() else None,
                    trigger_price=oco_order.trigger_price if oco_order.has_trigger_price_c() else None,
                    update_ocos=False,
                )

    cdef void _cancel_order(self, Order order, bint cancel_ocos=True) except *:
        if order.venue_order_id is None:
            order.venue_order_id = self._generate_venue_order_id()

        if order.side == OrderSide.BUY:
            if order in self._orders_bid:
                self._orders_bid.remove(order)
        elif order.side == OrderSide.SELL:
            if order in self._orders_ask:
                self._orders_ask.remove(order)

        self._generate_order_canceled(order)

        if order.contingency_type == ContingencyType.OCO and cancel_ocos:
            self._cancel_oco_orders(order)

    cdef void _cancel_oco_orders(self, Order order) except *:
        self._log.debug(f"Canceling OCO orders from {order.client_order_id}")
        # Iterate all contingency orders and cancel if active
        cdef ClientOrderId client_order_id
        cdef Order oco_order
        for client_order_id in order.linked_order_ids:
            oco_order = self.cache.order(client_order_id)
            assert oco_order is not None, "OCO order not found"
            if oco_order.is_open_c():
                self._cancel_order(oco_order, cancel_ocos=False)

    cdef void _expire_order(self, Order order) except *:
        if order.contingency_type == ContingencyType.OCO:
            self._cancel_oco_orders(order)

        self._generate_order_expired(order)

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_order_rejected(self, Order order, str reason) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderRejected event = OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

    cdef void _generate_order_accepted(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderAccepted event = OrderAccepted(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=self._generate_venue_order_id(),
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

    cdef void _generate_order_pending_update(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderPendingUpdate event = OrderPendingUpdate(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

    cdef void _generate_order_pending_cancel(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderPendingCancel event = OrderPendingCancel(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

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
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

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
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

    cdef void _generate_order_updated(
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
            account_id=order.account_id or self._account_ids[order.trader_id],
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

    cdef void _generate_order_canceled(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderCanceled event = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

    cdef void _generate_order_triggered(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderTriggered event = OrderTriggered(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

    cdef void _generate_order_expired(self, Order order) except *:
        # Generate event
        cdef uint64_t timestamp = self._clock.timestamp_ns()
        cdef OrderExpired event = OrderExpired(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=order.account_id or self._account_ids[order.client_order_id],
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

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
            account_id=order.account_id or self._account_ids[order.trader_id],
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id or self._generate_venue_order_id(),
            trade_id=self._generate_trade_id(),
            position_id=venue_position_id,
            order_side=order.side,
            order_type=order.type,
            last_qty=last_qty,
            last_px=last_px,
            currency=quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._emit_order_event(event)

    cdef void _emit_order_event(self, OrderEvent event) except *:
        self._msgbus.send(endpoint="ExecEngine.process", msg=event)
