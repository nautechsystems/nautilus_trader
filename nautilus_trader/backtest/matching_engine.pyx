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

import uuid

from cpython.datetime cimport timedelta
from libc.stdint cimport uint64_t

from nautilus_trader.backtest.models cimport FeeModel
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TestClock
from nautilus_trader.common.component cimport is_logging_initialized
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport AggregationSource
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport InstrumentCloseType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport MarketStatus
from nautilus_trader.core.rust.model cimport MarketStatusAction
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport orderbook_best_ask_price
from nautilus_trader.core.rust.model cimport orderbook_best_bid_price
from nautilus_trader.core.rust.model cimport orderbook_has_ask
from nautilus_trader.core.rust.model cimport orderbook_has_bid
from nautilus_trader.core.rust.model cimport price_new
from nautilus_trader.core.rust.model cimport trade_id_new
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.trailing cimport TrailingStopCalculator
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport BookOrder
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.functions cimport aggressor_side_to_str
from nautilus_trader.model.functions cimport liquidity_side_to_str
from nautilus_trader.model.functions cimport order_type_to_str
from nautilus_trader.model.functions cimport time_in_force_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport EXPIRING_INSTRUMENT_TYPES
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.equity cimport Equity
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.limit_if_touched cimport LimitIfTouchedOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_if_touched cimport MarketIfTouchedOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_limit cimport TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market cimport TrailingStopMarketOrder
from nautilus_trader.model.position cimport Position


cdef class OrderMatchingEngine:
    """
    Provides an order matching engine for a single market.

    Parameters
    ----------
    instrument : Instrument
        The market instrument for the matching engine.
    raw_id : uint32_t
        The raw integer ID for the instrument.
    fill_model : FillModel
        The fill model for the matching engine.
    fee_model : FeeModel
        The fee model for the matching engine.
    book_type : BookType
        The order book type for the engine.
    oms_type : OmsType
        The order management system type for the matching engine. Determines
        the generation and handling of venue position IDs.
    account_type : AccountType
        The account type for the matching engine. Determines allowable
        executions based on the instrument.
    msgbus : MessageBus
        The message bus for the matching engine.
    cache : CacheFacade
        The read-only cache for the matching engine.
    clock : TestClock
        The clock for the matching engine.
    logger : Logger
        The logger for the matching engine.
    bar_execution : bool, default True
        If bars should be processed by the matching engine (and move the market).
    trade_execution : bool, default False
        If trades should be processed by the matching engine (and move the market).
    reject_stop_orders : bool, default True
        If stop orders are rejected if already in the market on submitting.
    support_gtd_orders : bool, default True
        If orders with GTD time in force will be supported by the venue.
    support_contingent_orders : bool, default True
        If contingent orders will be supported/respected by the venue.
        If False, then its expected the strategy will be managing any contingent orders.
    use_position_ids : bool, default True
        If venue position IDs will be generated on order fills.
    use_random_ids : bool, default False
        If all venue generated identifiers will be random UUID4's.
    use_reduce_only : bool, default True
        If the `reduce_only` execution instruction on orders will be honored.
    auction_match_algo : Callable[[Ladder, Ladder], Tuple[List, List], optional
        The auction matching algorithm.
    bar_adaptive_high_low_ordering : bool, default False
        Determines whether the processing order of bar prices is adaptive based on a heuristic.
        This setting is only relevant when `bar_execution` is True.
        If False, bar prices are always processed in the fixed order: Open, High, Low, Close.
        If True, the processing order adapts with the heuristic:
        - If High is closer to Open than Low then the processing order is Open, High, Low, Close.
        - If Low is closer to Open than High then the processing order is Open, Low, High, Close.

    """

    def __init__(
        self,
        Instrument instrument not None,
        uint32_t raw_id,
        FillModel fill_model not None,
        FeeModel fee_model not None,
        BookType book_type,
        OmsType oms_type,
        AccountType account_type,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        TestClock clock not None,
        bint reject_stop_orders = True,
        bint support_gtd_orders = True,
        bint support_contingent_orders = True,
        bint use_position_ids = True,
        bint use_random_ids = False,
        bint use_reduce_only = True,
        bint bar_execution = True,
        bint bar_adaptive_high_low_ordering = False,
        bint trade_execution = False,
        # auction_match_algo = default_auction_match
    ) -> None:
        self._clock = clock
        self._log = Logger(name=f"{type(self).__name__}({instrument.id.venue})")
        self.msgbus = msgbus
        self.cache = cache

        self.venue = instrument.id.venue
        self.instrument = instrument
        self.raw_id = raw_id
        self.book_type = book_type
        self.oms_type = oms_type
        self.account_type = account_type
        self.market_status = MarketStatus.OPEN

        self._instrument_has_expiration = instrument.instrument_class in EXPIRING_INSTRUMENT_TYPES
        self._instrument_close = None
        self._reject_stop_orders = reject_stop_orders
        self._support_gtd_orders = support_gtd_orders
        self._support_contingent_orders = support_contingent_orders
        self._use_position_ids = use_position_ids
        self._use_random_ids = use_random_ids
        self._use_reduce_only = use_reduce_only
        self._bar_execution = bar_execution
        self._bar_adaptive_high_low_ordering = bar_adaptive_high_low_ordering
        self._trade_execution = trade_execution

        # self._auction_match_algo = auction_match_algo
        self._fill_model = fill_model
        self._fee_model = fee_model
        self._book = OrderBook(
            instrument_id=instrument.id,
            book_type=book_type,
        )
        self._opening_auction_book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L3_MBO,
        )
        self._closing_auction_book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L3_MBO,
        )

        self._account_ids: dict[TraderId, AccountId]  = {}
        self._execution_bar_types: dict[InstrumentId, BarType]  =  {}
        self._execution_bar_deltas: dict[BarType, timedelta]  =  {}
        self._cached_filled_qty: dict[ClientOrderId, Quantity] = {}

        # Market
        self._core = MatchingCore(
            instrument_id=instrument.id,
            price_increment=instrument.price_increment,
            trigger_stop_order=self.trigger_stop_order,
            fill_market_order=self.fill_market_order,
            fill_limit_order=self.fill_limit_order,
        )

        self._target_bid = 0
        self._target_ask = 0
        self._target_last = 0
        self._has_targets = False
        self._last_bid_bar: Bar | None = None
        self._last_ask_bar: Bar | None = None

        self._position_count = 0
        self._order_count = 0
        self._execution_count = 0

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"venue={self.venue.value}, "
            f"instrument_id={self.instrument.id.value}, "
            f"raw_id={self.raw_id})"
        )

    cpdef void reset(self):
        self._log.debug(f"Resetting OrderMatchingEngine {self.instrument.id}")

        self._book.clear(0, 0)
        self._account_ids.clear()
        self._execution_bar_types.clear()
        self._execution_bar_deltas.clear()
        self._cached_filled_qty.clear()
        self._core.reset()
        self._target_bid = 0
        self._target_ask = 0
        self._target_last = 0
        self._has_targets = False
        self._last_bid_bar = None
        self._last_ask_bar = None

        self._position_count = 0
        self._order_count = 0
        self._execution_count = 0

        self._log.info(f"Reset OrderMatchingEngine {self.instrument.id}")

    cpdef void set_fill_model(self, FillModel fill_model):
        """
        Set the fill model to the given model.

        Parameters
        ----------
        fill_model : FillModel
            The fill model to set.

        """
        Condition.not_none(fill_model, "fill_model")

        self._fill_model = fill_model

        self._log.debug(f"Changed `FillModel` to {self._fill_model}")

    cpdef void update_instrument(self, Instrument instrument):
        """
        Update the matching engines current instrument definition with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument definition to update.

        """
        Condition.not_none(instrument, "instrument")
        Condition.equal(instrument.id, self.instrument.id, "instrument.id", "self.instrument.id")

        self.instrument = instrument

        self._log.debug(f"Updated instrument definition for {instrument.id}")

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self):
        """
        Return the best bid price for the given instrument ID (if found).

        Returns
        -------
        Price or ``None``

        """
        return self._book.best_bid_price()

    cpdef Price best_ask_price(self):
        """
        Return the best ask price for the given instrument ID (if found).

        Returns
        -------
        Price or ``None``

        """
        return self._book.best_ask_price()

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

    cpdef bint order_exists(self, ClientOrderId client_order_id):
        return self._core.order_exists(client_order_id)

# -- DATA PROCESSING ------------------------------------------------------------------------------

    cpdef void process_order_book_delta(self, OrderBookDelta delta):
        """
        Process the exchanges market for the given order book delta.

        Parameters
        ----------
        delta : OrderBookDelta
            The order book delta to process.

        """
        Condition.not_none(delta, "delta")

        if is_logging_initialized():
            self._log.debug(f"Processing {delta!r}")

        self._book.apply_delta(delta)

        # TODO: WIP to introduce flags
        # if data.flags == TimeInForce.GTC:
        #     self._book.apply(data)
        # elif data.flags == TimeInForce.AT_THE_OPEN:
        #     self._opening_auction_book.apply(data)
        # elif data.flags == TimeInForce.AT_THE_CLOSE:
        #     self._closing_auction_book.apply(data)
        # else:
        #     raise RuntimeError(data.time_in_force)

        self.iterate(delta.ts_init)

    cpdef void process_order_book_deltas(self, OrderBookDeltas deltas):
        """
        Process the exchanges market for the given order book deltas.

        Parameters
        ----------
        delta : OrderBookDeltas
            The order book deltas to process.

        """
        Condition.not_none(deltas, "deltas")

        if is_logging_initialized():
            self._log.debug(f"Processing {deltas!r}")

        self._book.apply_deltas(deltas)

        # TODO: WIP to introduce flags
        # if data.flags == TimeInForce.GTC:
        #     self._book.apply(data)
        # elif data.flags == TimeInForce.AT_THE_OPEN:
        #     self._opening_auction_book.apply(data)
        # elif data.flags == TimeInForce.AT_THE_CLOSE:
        #     self._closing_auction_book.apply(data)
        # else:
        #     raise RuntimeError(data.time_in_force)

        self.iterate(deltas.ts_init)

    cpdef void process_quote_tick(self, QuoteTick tick) :
        """
        Process the exchanges market for the given quote tick.

        The internal order book will only be updated if the venue `book_type` is 'L1_MBP'.

        Parameters
        ----------
        tick : QuoteTick
            The tick to process.

        Raises
        ------
        RuntimeError
            If a price precision does not match the instrument for the matching engine.
        RuntimeError
            If a size precision does not match the instrument for the matching engine.

        """
        Condition.not_none(tick, "tick")

        if is_logging_initialized():
            self._log.debug(f"Processing {tick!r}")

        # Validate precisions
        if tick._mem.bid_price.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {tick.bid_price.precision=} did not match {self.instrument.price_precision=}",
            )
        if tick._mem.ask_price.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {tick.ask_price.precision=} did not match {self.instrument.price_precision=}",
            )
        if tick._mem.bid_size.precision != self.instrument.size_precision:
            raise RuntimeError(
                f"invalid {tick.bid_size.precision=} did not match {self.instrument.size_precision=}",
            )
        if tick._mem.ask_size.precision != self.instrument.size_precision:
            raise RuntimeError(
                f"invalid {tick.ask_size.precision=} did not match {self.instrument.size_precision=}",
            )

        if self.book_type == BookType.L1_MBP:
            self._book.update_quote_tick(tick)

        self.iterate(tick.ts_init)

    cpdef void process_trade_tick(self, TradeTick tick):
        """
        Process the exchanges market for the given trade tick.

        The internal order book will only be updated if the venue `book_type` is 'L1_MBP'.

        Parameters
        ----------
        tick : TradeTick
            The tick to process.

        Raises
        ------
        RuntimeError
            If the trades price precision does not match the instrument for the matching engine.
        RuntimeError
            If the trades size precision does not match the instrument for the matching engine.

        """
        Condition.not_none(tick, "tick")

        if is_logging_initialized():
            self._log.debug(f"Processing {tick!r}")

        # Validate precisions
        if tick._mem.price.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {tick.price.precision=} did not match {self.instrument.price_precision=}",
            )
        if tick._mem.size.precision != self.instrument.size_precision:
            raise RuntimeError(
                f"invalid {tick.size.precision=} did not match {self.instrument.size_precision=}",
            )

        if self.book_type == BookType.L1_MBP:
            self._book.update_trade_tick(tick)

        cdef AggressorSide aggressor_side = AggressorSide.NO_AGGRESSOR
        cdef int64_t price_raw = tick._mem.price.raw

        self._core.set_last_raw(price_raw)

        if self._trade_execution:
            aggressor_side = tick.aggressor_side
            if aggressor_side == AggressorSide.BUYER:
                self._core.set_ask_raw(price_raw)
                if price_raw < self._core.bid_raw:
                    self._core.set_bid_raw(price_raw)
            elif aggressor_side == AggressorSide.SELLER:
                self._core.set_bid_raw(price_raw)
                if price_raw > self._core.ask_raw:
                    self._core.set_ask_raw(price_raw)
            else:
                aggressor_side_str = aggressor_side_to_str(aggressor_side)
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"invalid `AggressorSide` for trade execution, was {aggressor_side_str}",  # pragma: no cover
                )

        self.iterate(tick.ts_init, aggressor_side)

    cpdef void process_bar(self, Bar bar):
        """
        Process the exchanges market for the given bar.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        Raises
        ------
        RuntimeError
            If a price precision does not match the instrument for the matching engine.
        RuntimeError
            If a size precision does not match the instrument for the matching engine.

        """
        Condition.not_none(bar, "bar")

        if not self._bar_execution:
            return

        if self.book_type != BookType.L1_MBP:
            return  # Can only process an L1 book with bars

        cdef BarType bar_type = bar.bar_type
        if bar_type.aggregation_source == AggregationSource.INTERNAL:
            return  # Do not process internally aggregated bars

        if bar_type.spec.aggregation == BarAggregation.MONTH:
            return  # Do not process monthly bars (there is no available `timedelta`)

        # Validate precisions
        if bar._mem.open.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {bar.open.precision=} did not match {self.instrument.price_precision=}",
            )
        if bar._mem.high.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {bar.high.precision=} did not match {self.instrument.price_precision=}",
            )
        if bar._mem.low.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {bar.low.precision=} did not match {self.instrument.price_precision=}",
            )
        if bar._mem.close.precision != self.instrument.price_precision:
            raise RuntimeError(
                f"invalid {bar.close.precision=} did not match {self.instrument.price_precision=}",
            )
        if bar._mem.volume.precision != self.instrument.size_precision:
            raise RuntimeError(
                f"invalid {bar.volume.precision=} did not match {self.instrument.size_precision=}",
            )

        cdef InstrumentId instrument_id = bar_type.instrument_id
        cdef BarType execution_bar_type = self._execution_bar_types.get(instrument_id)

        if execution_bar_type is None:
            execution_bar_type = bar_type
            self._execution_bar_types[instrument_id] = bar_type
            self._execution_bar_deltas[bar_type] = bar_type.spec.timedelta

        if execution_bar_type != bar_type:
            bar_type_timedelta = self._execution_bar_deltas.get(bar_type)
            if bar_type_timedelta is None:
                bar_type_timedelta = bar_type.spec.timedelta
                self._execution_bar_deltas[bar_type] = bar_type_timedelta
            if self._execution_bar_deltas[execution_bar_type] >= bar_type_timedelta:
                self._execution_bar_types[instrument_id] = bar_type
            else:
                return

        if is_logging_initialized():
            self._log.debug(f"Processing {bar!r}")

        cdef PriceType price_type = bar_type.spec.price_type
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
                f"invalid `PriceType`, was {price_type}",  # pragma: no cover
            )

    cpdef void process_status(self, MarketStatusAction status):
        """
        Process the exchange status.

        Parameters
        ----------
        status : MarketStatusAction
            The status action to process.

        """
        # # TODO: Reimplement
        if (self.market_status, status) == (MarketStatus.CLOSED, MarketStatusAction.TRADING):
            self.market_status = MarketStatus.OPEN
        elif (self.market_status, status) == (MarketStatus.CLOSED, MarketStatusAction.PRE_OPEN):
            # Nothing to do on pre-market open.
            self.market_status = MarketStatus.OPEN
        # elif (self.market_status, status) == (MarketStatus.PRE_OPEN, MarketStatusAction.PAUSE):
        #     # Opening auction period, run auction match on pre-open auction orderbook
        #     self.process_auction_book(self._opening_auction_book)
        #     self.market_status = status
        # elif (self.market_status, status) == (MarketStatus.PAUSE, MarketStatusAction.OPEN):
        #     # Normal market open
        #     self.market_status = status
        # elif (self.market_status, status) == (MarketStatus.OPEN, MarketStatusAction.PAUSE):
        #     # Closing auction period, run auction match on closing auction orderbook
        #     self.process_auction_book(self._closing_auction_book)
        #     self.market_status = status
        # elif (self.market_status, status) == (MarketStatus.PAUSE, MarketStatusAction.CLOSED):
        #     # Market closed - nothing to do for now
        #     # TODO - should we implement some sort of closing price message here?
        #     self.market_status = status

    cpdef void process_instrument_close(self, InstrumentClose close):
        """
        Process the instrument close.

        Parameters
        ----------
        close : InstrumentClose
            The close price to process.

        """
        if close.instrument_id != self.instrument.id:
            self._log.warning(f"Received instrument close for unknown instrument_id: {close.instrument_id}")
            return

        if close.close_type == InstrumentCloseType.CONTRACT_EXPIRED:
            self._instrument_close = close
            self.iterate(close.ts_init)

    cpdef void process_auction_book(self, OrderBook book):
        Condition.not_none(book, "book")

        cdef:
            list traded_bids
            list traded_asks
        # Perform an auction match on this auction order book
        # traded_bids, traded_asks = self._auction_match_algo(book.bids, book.asks)

        cdef set client_order_ids = {c.value for c in self.cache.client_order_ids()}

        # cdef:
        #     BookOrder order
        #     Order real_order
        #     PositionId venue_position_id
        # # Check filled orders from auction for any client orders and emit fills
        # for order in traded_bids + traded_asks:
        #     if order.order_id in client_order_ids:
        #         real_order = self.cache.order(ClientOrderId(order.order_id))
        #         venue_position_id = self._get_position_id(real_order)
        #         self._generate_order_filled(
        #             real_order,
        #             self._get_venue_order_id(real_order),
        #             venue_position_id,
        #             Quantity(order.size, self.instrument.size_precision),
        #             Price(order.price, self.instrument.price_precision),
        #             self.instrument.quote_currency,
        #             Money(0.0, self.instrument.quote_currency),
        #             LiquiditySide.NO_LIQUIDITY_SIDE,
        #         )

    cdef void _process_trade_ticks_from_bar(self, Bar bar):
        cdef double size_value = max(bar.volume.as_double() / 4.0, self.instrument.size_increment.as_double())
        cdef Quantity size = Quantity(size_value, bar._mem.volume.precision)

        # Create base tick template
        cdef TradeTick tick = self._create_base_trade_tick(bar, size)

        # Process each price point
        cdef bint process_high_first = (
            not self._bar_adaptive_high_low_ordering
            or abs(bar._mem.high.raw - bar._mem.open.raw) < abs(bar._mem.low.raw - bar._mem.open.raw)
        )

        self._process_trade_bar_open(bar, tick)
        if process_high_first:
            self._process_trade_bar_high(bar, tick)
            self._process_trade_bar_low(bar, tick)
        else:
            self._process_trade_bar_low(bar, tick)
            self._process_trade_bar_high(bar, tick)
        self._process_trade_bar_close(bar, tick)

    cdef TradeTick _create_base_trade_tick(self, Bar bar, Quantity size):
        return TradeTick(
            bar.bar_type.instrument_id,
            bar.open,
            size,
            AggressorSide.BUYER if not self._core.is_last_initialized or bar._mem.open.raw > self._core.last_raw else AggressorSide.SELLER,
            self._generate_trade_id(),
            bar.ts_event,
            bar.ts_event,
        )

    cdef void _process_trade_bar_open(self, Bar bar, TradeTick tick):
        if not self._core.is_last_initialized or bar._mem.open.raw != self._core.last_raw:
            if is_logging_initialized():
                self._log.debug(f"Updating with open {bar.open}")
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last_raw(bar._mem.open.raw)

    cdef void _process_trade_bar_high(self, Bar bar, TradeTick tick):
        if bar._mem.high.raw > self._core.last_raw:
            if is_logging_initialized():
                self._log.debug(f"Updating with high {bar.high}")
            tick._mem.price = bar._mem.high
            tick._mem.aggressor_side = AggressorSide.BUYER
            tick._mem.trade_id = trade_id_new(pystr_to_cstr(self._generate_trade_id_str()))
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last_raw(bar._mem.high.raw)

    cdef void _process_trade_bar_low(self, Bar bar, TradeTick tick):
        if bar._mem.low.raw < self._core.last_raw:
            if is_logging_initialized():
                self._log.debug(f"Updating with low {bar.low}")
            tick._mem.price = bar._mem.low
            tick._mem.aggressor_side = AggressorSide.SELLER
            tick._mem.trade_id = trade_id_new(pystr_to_cstr(self._generate_trade_id_str()))
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last_raw(bar._mem.low.raw)

    cdef void _process_trade_bar_close(self, Bar bar, TradeTick tick):
        if bar._mem.close.raw != self._core.last_raw:
            if is_logging_initialized():
                self._log.debug(f"Updating with close {bar.close}")
            tick._mem.price = bar._mem.close
            tick._mem.aggressor_side = AggressorSide.BUYER if bar._mem.close.raw > self._core.last_raw else AggressorSide.SELLER
            tick._mem.trade_id = trade_id_new(pystr_to_cstr(self._generate_trade_id_str()))
            self._book.update_trade_tick(tick)
            self.iterate(tick.ts_init)
            self._core.set_last_raw(bar._mem.close.raw)

    cdef void _process_quote_ticks_from_bar(self):
        if self._last_bid_bar is None or self._last_ask_bar is None:
            return  # Wait for next bar

        if self._last_bid_bar.ts_event != self._last_ask_bar.ts_event:
            return  # Wait for next bar

        cdef double size_increment_f64 = self.instrument.size_increment.as_double()
        cdef double bid_size_value = max(self._last_bid_bar.volume.as_double() / 4.0, size_increment_f64)
        cdef double ask_size_value = max(self._last_ask_bar.volume.as_double() / 4.0, size_increment_f64)
        cdef Quantity bid_size = Quantity(bid_size_value, self._last_bid_bar._mem.volume.precision)
        cdef Quantity ask_size = Quantity(ask_size_value, self._last_ask_bar._mem.volume.precision)

        # Create base tick template
        cdef QuoteTick tick = self._create_base_quote_tick(bid_size, ask_size)

        # Process each price point
        cdef bint process_high_first = (
            not self._bar_adaptive_high_low_ordering
            or abs(self._last_bid_bar._mem.high.raw - self._last_bid_bar._mem.open.raw) < abs(self._last_bid_bar._mem.low.raw - self._last_bid_bar._mem.open.raw)
        )

        self._process_quote_bar_open(tick)
        if process_high_first:
            self._process_quote_bar_high(tick)
            self._process_quote_bar_low(tick)
        else:
            self._process_quote_bar_low(tick)
            self._process_quote_bar_high(tick)
        self._process_quote_bar_close(tick)

        self._last_bid_bar = None
        self._last_ask_bar = None

    cdef QuoteTick _create_base_quote_tick(self, Quantity bid_size, Quantity ask_size):
        return QuoteTick(
            self._book.instrument_id,
            self._last_bid_bar.open,
            self._last_ask_bar.open,
            bid_size,
            ask_size,
            self._last_bid_bar.ts_event,
            self._last_ask_bar.ts_init,
        )

    cdef void _process_quote_bar_open(self, QuoteTick tick):
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

    cdef void _process_quote_bar_high(self, QuoteTick tick):
        tick._mem.bid_price = self._last_bid_bar._mem.high
        tick._mem.ask_price = self._last_ask_bar._mem.high
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

    cdef void _process_quote_bar_low(self, QuoteTick tick):
        tick._mem.bid_price = self._last_bid_bar._mem.low
        tick._mem.ask_price = self._last_ask_bar._mem.low
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

    cdef void _process_quote_bar_close(self, QuoteTick tick):
        tick._mem.bid_price = self._last_bid_bar._mem.close
        tick._mem.ask_price = self._last_ask_bar._mem.close
        self._book.update_quote_tick(tick)
        self.iterate(tick.ts_init)

    # -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef void process_order(self, Order order, AccountId account_id):
        if self._core.order_exists(order.client_order_id):
            return  # Already processed

        # Index identifiers
        self._account_ids[order.trader_id] = account_id

        cdef uint64_t now_ns
        if self._instrument_has_expiration:
            now_ns = self._clock.timestamp_ns()
            if now_ns < self.instrument.activation_ns:
                self._generate_order_rejected(
                    order,
                    f"Contract {self.instrument.id} not yet active, "
                    f"activation {format_iso8601(unix_nanos_to_dt(self.instrument.activation_ns))}"
                )
                return
            elif now_ns > self.instrument.expiration_ns:
                self._generate_order_rejected(
                    order,
                    f"Contract {self.instrument.id} has expired, "
                    f"expiration {format_iso8601(unix_nanos_to_dt(self.instrument.expiration_ns))}"
                )
                return

        cdef:
            Order parent
            Order contingenct_order
            ClientOrderId client_order_id
        if self._support_contingent_orders and order.parent_order_id is not None:
            parent = self.cache.order(order.parent_order_id)
            assert parent is not None and parent.contingency_type == ContingencyType.OTO, "OTO parent not found"
            if parent.status_c() == OrderStatus.REJECTED and order.is_open_c():
                self._generate_order_rejected(order, f"REJECT OTO from {parent.client_order_id}")
                return  # Order rejected
            elif parent.status_c() == OrderStatus.ACCEPTED or parent.status_c() == OrderStatus.TRIGGERED:
                self._log.info(f"Pending OTO {order.client_order_id} triggers from {parent.client_order_id}")
                return  # Pending trigger

            if order.linked_order_ids is not None:
                # Check contingent orders are still open
                for client_order_id in order.linked_order_ids:
                    contingent_order = self.cache.order(client_order_id)
                    if contingent_order is None:
                        raise RuntimeError(f"Cannot find contingent order for {client_order_id!r}")  # pragma: no cover
                    if order.contingency_type == ContingencyType.OCO or order.contingency_type == ContingencyType.OUO:
                        if not order.is_closed_c() and contingent_order.is_closed_c():
                            self._generate_order_rejected(order, f"Contingent order {client_order_id} already closed")
                            return  # Order rejected

        # Check order quantity precision
        if order.quantity._mem.precision != self.instrument.size_precision:
            self._generate_order_rejected(
                order,
                f"Invalid size precision for order {order.client_order_id}, "
                f"was {order.quantity.precision} "
                f"when {self.instrument.id} size precision is {self.instrument.size_precision}"
            )
            return  # Invalid order

        cdef Price price
        if order.has_price_c():
            # Check order price precision
            price = order.price
            if price._mem.precision != self.instrument.price_precision:
                self._generate_order_rejected(
                    order,
                    f"Invalid price precision for order {order.client_order_id}, "
                    f"was {price.precision} "
                    f"when {self.instrument.id} price precision is {self.instrument.price_precision}"
                )
                return  # Invalid order

        cdef Price trigger_price
        if order.has_trigger_price_c():
            # Check order trigger price precision
            trigger_price = order.trigger_price
            if trigger_price._mem.precision != self.instrument.price_precision:
                self._generate_order_rejected(
                    order,
                    f"Invalid trigger price precision for order {order.client_order_id}, "
                    f"was {trigger_price.precision} "
                    f"when {self.instrument.id} price precision is {self.instrument.price_precision}"
                )
                return  # Invalid order

        cdef Position position = self.cache.position_for_order(order.client_order_id)

        cdef PositionId position_id
        if position is None and self.oms_type == OmsType.NETTING:
            position_id = PositionId(f"{order.instrument_id}-{order.strategy_id}")
            position = self.cache.position(position_id)

        # Check not shorting an equity without a MARGIN account
        if (
            order.side == OrderSide.SELL
            and self.account_type != AccountType.MARGIN
            and isinstance(self.instrument, Equity)
            and (position is None or not order.would_reduce_only(position.side, position.quantity))
        ):
            self._generate_order_rejected(
                order,
                f"SHORT SELLING not permitted on a CASH account with position {position} and order {order!r}"
            )
            return  # Cannot short sell

        # Check reduce-only instruction
        if self._use_reduce_only and order.is_reduce_only and not order.is_closed_c():
            if (
                not position
                or position.is_closed_c()
                or (order.is_buy_c() and position.is_long_c())
                or (order.is_sell_c() and position.is_short_c())
            ):
                self._generate_order_rejected(
                    order,
                    f"REDUCE_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"would have increased position",
                )
                return  # Reduce only

        if order.order_type == OrderType.MARKET:
            self._process_market_order(order)
        elif order.order_type == OrderType.MARKET_TO_LIMIT:
            self._process_market_to_limit_order(order)
        elif order.order_type == OrderType.LIMIT:
            self._process_limit_order(order)
        elif order.order_type == OrderType.STOP_MARKET:
            self._process_stop_market_order(order)
        elif order.order_type == OrderType.STOP_LIMIT:
            self._process_stop_limit_order(order)
        elif order.order_type == OrderType.MARKET_IF_TOUCHED:
            self._process_market_if_touched_order(order)
        elif order.order_type == OrderType.LIMIT_IF_TOUCHED:
            self._process_limit_if_touched_order(order)
        elif order.order_type == OrderType.TRAILING_STOP_MARKET:
            self._process_trailing_stop_market_order(order)
        elif order.order_type == OrderType.TRAILING_STOP_LIMIT:
            self._process_trailing_stop_limit_order(order)
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"{order_type_to_str(order.order_type)} "  # pragma: no cover
                f"orders are not supported for backtesting in this version",  # pragma: no cover
            )

    cpdef void process_modify(self, ModifyOrder command, AccountId account_id):
        cdef Order order = self._core.get_order(command.client_order_id)
        if order is None:
            self._generate_order_modify_rejected(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                account_id=account_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"{command.client_order_id!r} not found",
            )
        else:
            self.update_order(
                order,
                command.quantity,
                command.price,
                command.trigger_price,
            )

    cpdef void process_cancel(self, CancelOrder command, AccountId account_id):
        cdef Order order = self._core.get_order(command.client_order_id)
        if order is None:
            self._generate_order_cancel_rejected(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                account_id=account_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"{command.client_order_id!r} not found",
            )
        else:
            if order.is_inflight_c() or order.is_open_c():
                self.cancel_order(order)

    cpdef void process_batch_cancel(self, BatchCancelOrders command, AccountId account_id):
        cdef CancelOrder cancel
        for cancel in command.cancels:
            self.process_cancel(cancel, account_id)

    cpdef void process_cancel_all(self, CancelAllOrders command, AccountId account_id):
        cdef Order order
        for order in self.cache.orders_open(venue=None, instrument_id=command.instrument_id):
            if command.order_side != OrderSide.NO_ORDER_SIDE and command.order_side != order.side:
                continue
            if order.is_inflight_c() or order.is_open_c():
                self.cancel_order(order)

    cdef void _process_market_order(self, MarketOrder order):
        # Check AT_THE_OPEN/AT_THE_CLOSE time in force
        if order.time_in_force == TimeInForce.AT_THE_OPEN or order.time_in_force == TimeInForce.AT_THE_CLOSE:
            self._log.error(
                f"Market auction for time in force {time_in_force_to_str(order.time_in_force)} "
                "is not currently supported",
            )
            # TODO: This functionality needs reimplementing
            # self._process_auction_market_order(order)
            return

        # Check market exists
        if order.side == OrderSide.BUY and not self._core.is_ask_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self._core.is_bid_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Immediately fill marketable order
        self.fill_market_order(order)

    cdef void _process_market_to_limit_order(self, MarketToLimitOrder order):
        # Check market exists
        if order.side == OrderSide.BUY and not self._core.is_ask_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self._core.is_bid_initialized:
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Immediately fill marketable order
        self.fill_market_order(order)

        if order.is_open_c():
            self.accept_order(order)

    cdef void _process_limit_order(self, LimitOrder order):
        # Check AT_THE_OPEN/AT_THE_CLOSE time in force
        if order.time_in_force == TimeInForce.AT_THE_OPEN or order.time_in_force == TimeInForce.AT_THE_CLOSE:
            self._process_auction_limit_order(order)
            return

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
        self.accept_order(order)

        # Check for immediate fill
        if self._core.is_limit_matched(order.side, order.price):
            # Filling as liquidity taker
            if order.liquidity_side == LiquiditySide.NO_LIQUIDITY_SIDE:
                order.liquidity_side = LiquiditySide.TAKER
            self.fill_limit_order(order)
        elif order.time_in_force == TimeInForce.FOK or order.time_in_force == TimeInForce.IOC:
            self.cancel_order(order)

    cdef void _process_stop_market_order(self, StopMarketOrder order):
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
            self.fill_market_order(order)
            return

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_stop_limit_order(self, StopLimitOrder order):
        if self._core.is_stop_triggered(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"trigger stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price
            self.accept_order(order)
            self._generate_order_triggered(order)

            # Check if immediately marketable
            if self._core.is_limit_matched(order.side, order.price):
                order.liquidity_side = LiquiditySide.TAKER
                self.fill_limit_order(order)
            return

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_market_if_touched_order(self, MarketIfTouchedOrder order):
        if self._core.is_touch_triggered(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price
            self.fill_market_order(order)
            return

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_limit_if_touched_order(self, LimitIfTouchedOrder order):
        if self._core.is_touch_triggered(order.side, order.trigger_price):
            if self._reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"trigger stop px of {order.trigger_price} was in the market: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return  # Invalid price
            self.accept_order(order)
            self._generate_order_triggered(order)

            # Check if immediately marketable
            if self._core.is_limit_matched(order.side, order.price):
                order.liquidity_side = LiquiditySide.TAKER
                self.fill_limit_order(order)
            return

        # Order is valid and accepted
        self.accept_order(order)

    cdef void _process_trailing_stop_market_order(self, TrailingStopMarketOrder order):
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
        self.accept_order(order)

    cdef void _process_trailing_stop_limit_order(self, TrailingStopLimitOrder order):
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
        self.accept_order(order)

    cdef void _process_auction_market_order(self, MarketOrder order):
        cdef:
            Instrument instrument = self.instrument
            BookOrder book_order = BookOrder(
                side=order.side,
                price=instrument.max_price if order.is_buy_c() else instrument.min_price,
                size=order.quantity,
                order_id=self._clock.timestamp_ns(),
            )
        self._process_auction_book_order(book_order, time_in_force=order.time_in_force)

    cdef void _process_auction_limit_order(self, LimitOrder order):
        cdef:
            Instrument instrument = self.instrument
            BookOrder book_order = BookOrder(
                price=order.price,
                size=order.quantity,
                side=order.side,
                order_id=self._clock.timestamp_ns(),
            )
        self._process_auction_book_order(book_order, time_in_force=order.time_in_force)

    cdef void _process_auction_book_order(self, BookOrder order, TimeInForce time_in_force):
        if time_in_force == TimeInForce.AT_THE_OPEN:
            self._opening_auction_book.add(order, 0, 0, 0)
        elif time_in_force == TimeInForce.AT_THE_CLOSE:
            self._closing_auction_book.add(order, 0, 0, 0)
        else:
            raise RuntimeError(time_in_force)

    cdef void _update_limit_order(
        self,
        Order order,
        Quantity qty,
        Price price,
    ):
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
            order.liquidity_side = LiquiditySide.TAKER
            self.fill_limit_order(order)  # Immediate fill as TAKER
            return  # Filled

        self._generate_order_updated(order, qty, price, None)

    cdef void _update_stop_market_order(
        self,
        StopMarketOrder order,
        Quantity qty,
        Price trigger_price,
    ):
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
        StopLimitOrder order,
        Quantity qty,
        Price price,
        Price trigger_price,
    ):
        if not order.is_triggered:
            # Updating stop price
            if self._core.is_stop_triggered(order.side, trigger_price):
                self._generate_order_modify_rejected(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    account_id=order.account_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=f"{order.type_string_c()} {order.side_string_c()} order "
                    f"new trigger stop px of {trigger_price} was in the market: "
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
                    order.liquidity_side = LiquiditySide.TAKER
                    self.fill_limit_order(order)  # Immediate fill as TAKER
                    return  # Filled

        self._generate_order_updated(order, qty, price, trigger_price or order.trigger_price)

    cdef void _update_market_if_touched_order(
        self,
        Order order,
        Quantity qty,
        Price trigger_price,
    ):
        if self._core.is_touch_triggered(order.side, trigger_price):
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

    cdef void _update_limit_if_touched_order(
        self,
        Order order,
        Quantity qty,
        Price price,
        Price trigger_price,
    ):
        if not order.is_triggered:
            # Updating stop price
            if self._core.is_touch_triggered(order.side, trigger_price):
                self._generate_order_modify_rejected(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    account_id=order.account_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=f"{order.type_string_c()} {order.side_string_c()} order "
                           f"new trigger stop px of {trigger_price} was in the market: "
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
                    order.liquidity_side = LiquiditySide.TAKER
                    self.fill_limit_order(order)  # Immediate fill as TAKER
                    return  # Filled

        self._generate_order_updated(order, qty, price, trigger_price or order.trigger_price)

    cdef void _update_trailing_stop_order(self, Order order):
        cdef tuple output = TrailingStopCalculator.calculate(
            price_increment=self.instrument.price_increment,
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

    cpdef void iterate(self, uint64_t timestamp_ns, AggressorSide aggressor_side = AggressorSide.NO_AGGRESSOR):
        """
        Iterate the matching engine by processing the bid and ask order sides
        and advancing time up to the given UNIX `timestamp_ns`.

        Parameters
        ----------
        timestamp_ns : uint64_t
            UNIX timestamp to advance the matching engine time to.
        aggressor_side : AggressorSide, default 'NO_AGGRESSOR'
            The aggressor side for trade execution processing.

        """
        self._clock.set_time(timestamp_ns)

        cdef Price_t bid
        cdef Price_t ask

        if orderbook_has_bid(&self._book._mem) and aggressor_side == AggressorSide.NO_AGGRESSOR:
            bid = orderbook_best_bid_price(&self._book._mem)
            self._core.set_bid_raw(bid.raw)
        if orderbook_has_ask(&self._book._mem) and aggressor_side == AggressorSide.NO_AGGRESSOR:
            ask = orderbook_best_ask_price(&self._book._mem)
            self._core.set_ask_raw(ask.raw)

        self._core.iterate(timestamp_ns)

        cdef list orders = self._core.get_orders()
        cdef Order order
        for order in orders:
            if order.is_closed_c():
                self._cached_filled_qty.pop(order.client_order_id, None)
                continue

            # Check expiry
            if self._support_gtd_orders:
                if order.expire_time_ns > 0 and timestamp_ns >= order.expire_time_ns:
                    self._core.delete_order(order)
                    self._cached_filled_qty.pop(order.client_order_id, None)
                    self.expire_order(order)
                    continue

            # Manage trailing stop
            if order.order_type == OrderType.TRAILING_STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_LIMIT:
                self._update_trailing_stop_order(order)

            # Move market back to targets
            if self._has_targets:
                self._core.set_bid_raw(self._target_bid)
                self._core.set_ask_raw(self._target_ask)
                self._core.set_last_raw(self._target_last)
                self._has_targets = False

        # Reset any targets after iteration
        self._target_bid = 0
        self._target_ask = 0
        self._target_last = 0
        self._has_targets = False

        # Instrument expiration
        if (self._instrument_has_expiration and timestamp_ns >= self.instrument.expiration_ns) or self._instrument_close is not None:
            self._log.info(f"{self.instrument.id} reached expiration")

            # Cancel all open orders
            for order in self.get_open_orders():
                self.cancel_order(order)

            # Close all open positions
            for position in self.cache.positions_open(None, self.instrument.id):
                order = MarketOrder(
                    trader_id=position.trader_id,
                    strategy_id=position.strategy_id,
                    instrument_id=position.instrument_id,
                    client_order_id=ClientOrderId(str(uuid.uuid4())),
                    order_side=Order.closing_side_c(position.side),
                    quantity=position.quantity,
                    init_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                    reduce_only=True,
                    tags=[f"EXPIRATION_{self.venue}_CLOSE"],
                )
                self.cache.add_order(order, position_id=position.id)
                self.fill_market_order(order)

    cpdef list determine_limit_price_and_volume(self, Order order):
        """
        Return the projected fills for the given *limit* order filling passively
        from its limit price.

        The list may be empty if no fills.

        Parameters
        ----------
        order : Order
            The order to determine fills for.

        Returns
        -------
        list[tuple[Price, Quantity]]

        Raises
        ------
        ValueError
            If the `order` does not have a LIMIT `price`.

        """
        Condition.is_true(order.has_price_c(), "order has no limit `price`")

        cdef list fills = self._book.simulate_fills(
            order,
            price_prec=self.instrument.price_precision,
            size_prec=self.instrument.size_precision,
            is_aggressive=False,
        )

        cdef Price triggered_price = order.get_triggered_price_c()
        cdef Price price = order.price

        if (
            fills
            and triggered_price is not None
            and order.liquidity_side == LiquiditySide.TAKER
        ):
            ########################################################################
            # Filling as TAKER from a trigger
            ########################################################################
            if order.side == OrderSide.BUY and price._mem.raw > triggered_price._mem.raw:
                fills[0] = (triggered_price, fills[0][1])
                self._has_targets = True
                self._target_bid = self._core.bid_raw
                self._target_ask = self._core.ask_raw
                self._target_last = self._core.last_raw
                self._core.set_ask_raw(price._mem.raw)
                self._core.set_last_raw(price._mem.raw)
            elif order.side == OrderSide.SELL and price._mem.raw < triggered_price._mem.raw:
                fills[0] = (triggered_price, fills[0][1])
                self._has_targets = True
                self._target_bid = self._core.bid_raw
                self._target_ask = self._core.ask_raw
                self._target_last = self._core.last_raw
                self._core.set_bid_raw(price._mem.raw)
                self._core.set_last_raw(price._mem.raw)

        cdef tuple[Price, Quantity] fill
        cdef Price last_px
        if (
            fills
            and order.liquidity_side == LiquiditySide.MAKER
        ):
            ########################################################################
            # Filling as MAKER
            ########################################################################
            price = order.price
            if order.side == OrderSide.BUY:
                if triggered_price and price > triggered_price:
                    price = triggered_price
                for fill in fills:
                    last_px = fill[0]
                    if last_px._mem.raw < price._mem.raw:
                        # Marketable BUY would have filled at limit
                        self._has_targets = True
                        self._target_bid = self._core.bid_raw
                        self._target_ask = self._core.ask_raw
                        self._target_last = self._core.last_raw
                        self._core.set_ask_raw(price._mem.raw)
                        self._core.set_last_raw(price._mem.raw)
                        last_px._mem.raw = price._mem.raw
            elif order.side == OrderSide.SELL:
                if triggered_price and price < triggered_price:
                    price = triggered_price
                for fill in fills:
                    last_px = fill[0]
                    if last_px._mem.raw > price._mem.raw:
                        # Marketable SELL would have filled at limit
                        self._has_targets = True
                        self._target_bid = self._core.bid_raw
                        self._target_ask = self._core.ask_raw
                        self._target_last = self._core.last_raw
                        self._core.set_bid_raw(price._mem.raw)
                        self._core.set_last_raw(price._mem.raw)
                        last_px._mem.raw = price._mem.raw
            else:
                raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)

        return fills

    cpdef list determine_market_price_and_volume(self, Order order):
        """
        Return the projected fills for the given *marketable* order filling
        aggressively into the opposite order side.

        The list may be empty if no fills.

        Parameters
        ----------
        order : Order
            The order to determine fills for.

        Returns
        -------
        list[tuple[Price, Quantity]]

        """
        cdef list fills = self._book.simulate_fills(
            order,
            price_prec=self.instrument.price_precision,
            size_prec=self.instrument.size_precision,
            is_aggressive=True,
        )

        cdef Price price
        cdef Price triggered_price
        if self._book.book_type == BookType.L1_MBP and fills:
            triggered_price = order.get_triggered_price_c()
            if order.order_type == OrderType.MARKET or order.order_type == OrderType.MARKET_TO_LIMIT or order.order_type == OrderType.MARKET_IF_TOUCHED:
                if order.side == OrderSide.BUY:
                    if self._core.is_ask_initialized:
                        price = self._core.ask
                    else:
                        price = self.best_ask_price()
                    if triggered_price:
                        price = triggered_price
                    if price is not None:
                        self._core.set_last_raw(price._mem.raw)
                        fills[0] = (price, fills[0][1])
                    else:
                        raise RuntimeError(  # pragma: no cover (design-time error)
                            "Market best ASK price was None when filling MARKET order",  # pragma: no cover
                        )
                elif order.side == OrderSide.SELL:
                    if self._core.is_bid_initialized:
                        price = self._core.bid
                    else:
                        price = self.best_bid_price()
                    if triggered_price:
                        price = triggered_price
                    if price is not None:
                        self._core.set_last_raw(price._mem.raw)
                        fills[0] = (price, fills[0][1])
                    else:
                        raise RuntimeError(  # pragma: no cover (design-time error)
                            "Market best BID price was None when filling MARKET order",  # pragma: no cover
                        )
            else:
                price = order.price if (order.order_type == OrderType.LIMIT or order.order_type == OrderType.LIMIT_IF_TOUCHED) else order.trigger_price
                if triggered_price:
                    price = triggered_price
                if order.side == OrderSide.BUY:
                    self._core.set_ask_raw(price._mem.raw)
                elif order.side == OrderSide.SELL:
                    self._core.set_bid_raw(price._mem.raw)
                else:
                    raise RuntimeError(f"invalid `OrderSide`, was {order.side}")  # pragma: no cover (design-time error)
                self._core.set_last_raw(price._mem.raw)
                fills[0] = (price, fills[0][1])

        return fills

    cpdef void fill_market_order(self, Order order):
        """
        Fill the given *marketable* order.

        Parameters
        ----------
        order : Order
            The order to fill.

        """
        cdef Quantity cached_filled_qty = self._cached_filled_qty.get(order.client_order_id)
        if cached_filled_qty is not None and cached_filled_qty._mem.raw >= order.quantity._mem.raw:
            self._log.debug(
                f"Ignoring fill as already filled pending application of events: "
                f"{cached_filled_qty=}, {order.quantity=}, {order.filled_qty=}, {order.leaves_qty=}",
            )
            return

        cdef PositionId venue_position_id = self._get_position_id(order)
        cdef Position position = None
        if venue_position_id is not None:
            position = self.cache.position(venue_position_id)
        if self._use_reduce_only and order.is_reduce_only and position is None:
            self._log.warning(
                f"Canceling REDUCE_ONLY {order.type_string_c()} "
                f"as would increase position",
            )
            self.cancel_order(order)
            return  # Order canceled

        order.liquidity_side = LiquiditySide.TAKER
        cdef list fills = self.determine_market_price_and_volume(order)

        self.apply_fills(
            order=order,
            fills=fills,
            liquidity_side=order.liquidity_side,
            venue_position_id=venue_position_id,
            position=position,
        )

    cpdef void fill_limit_order(self, Order order):
        """
        Fill the given limit order.

        Parameters
        ----------
        order : Order
            The order to fill.

        Raises
        ------
        ValueError
            If the `order` does not have a LIMIT `price`.

        """
        Condition.is_true(order.has_price_c(), "order has no limit `price`")

        cdef Quantity cached_filled_qty = self._cached_filled_qty.get(order.client_order_id)
        if cached_filled_qty is not None and cached_filled_qty._mem.raw >= order.quantity._mem.raw:
            self._log.debug(
                f"Ignoring fill as already filled pending application of events: "
                f"{cached_filled_qty=}, {order.quantity=}, {order.filled_qty=}, {order.leaves_qty=}",
            )
            return

        cdef Price price = order.price
        if order.liquidity_side == LiquiditySide.MAKER and self._fill_model:
            if order.side == OrderSide.BUY and self._core.bid_raw == price._mem.raw and not self._fill_model.is_limit_filled():
                return  # Not filled
            elif order.side == OrderSide.SELL and self._core.ask_raw == price._mem.raw and not self._fill_model.is_limit_filled():
                return  # Not filled

        cdef PositionId venue_position_id = self._get_position_id(order)
        cdef Position position = None
        if venue_position_id is not None:
            position = self.cache.position(venue_position_id)
        if self._use_reduce_only and order.is_reduce_only and position is None:
            self._log.warning(
                f"Canceling REDUCE_ONLY {order.type_string_c()} "
                f"as would increase position",
            )
            self.cancel_order(order)
            return  # Order canceled

        cdef list fills = self.determine_limit_price_and_volume(order)

        self.apply_fills(
            order=order,
            fills=fills,
            liquidity_side=order.liquidity_side,
            venue_position_id=venue_position_id,
            position=position,
        )

    cpdef void apply_fills(
        self,
        Order order,
        list fills,
        LiquiditySide liquidity_side,
        PositionId venue_position_id: PositionId | None = None,
        Position position: Position | None = None,
    ):
        """
        Apply the given list of fills to the given order. Optionally provide
        existing position details.

        - If the `fills` list is empty, an error will be logged.
        - Market orders will be rejected if no opposing orders are available to fulfill them.

        Parameters
        ----------
        order : Order
            The order to fill.
        fills : list[tuple[Price, Quantity]]
            The fills to apply to the order.
        liquidity_side : LiquiditySide
            The liquidity side for the fill(s).
        venue_position_id :  PositionId, optional
            The current venue position ID related to the order (if assigned).
        position : Position, optional
            The current position related to the order (if any).

        Raises
        ------
        ValueError
            If `liquidity_side` is ``NO_LIQUIDITY_SIDE``.

        Warnings
        --------
        The `liquidity_side` will override anything previously set on the order.

        """
        Condition.not_none(order, "order")
        Condition.not_none(fills, "fills")
        Condition.not_equal(liquidity_side, LiquiditySide.NO_LIQUIDITY_SIDE, "liquidity_side", "NO_LIQUIDITY_SIDE")

        order.liquidity_side = liquidity_side

        cdef:
            Price fill_px
            Quantity fill_qty
            QuantityRaw total_size_raw = 0
        if order.time_in_force == TimeInForce.FOK:
            # Check FOK requirement
            for fill in fills:
                fill_px, fill_qty = fill
                total_size_raw += fill_qty._mem.raw

            if order.leaves_qty._mem.raw > total_size_raw:
                self.cancel_order(order)
                return  # Cannot fill full size - so kill/cancel

        if not fills:
            if order.status_c() == OrderStatus.SUBMITTED:
                self._generate_order_rejected(order, f"no market with no fills for {order.instrument_id}")
            else:
                self._log.error(
                    "Cannot fill order: no fills from book when fills were expected (check data)",
                )
            return  # No fills

        if self.oms_type == OmsType.NETTING:
            venue_position_id = None  # No position IDs generated by the venue

        if is_logging_initialized():
            self._log.debug(
                "Market: "
                f"bid={self._book.best_bid_size()} @ {self._book.best_bid_price()}, "
                f"ask={self._book.best_ask_size()} @ {self._book.best_ask_price()}, "
                f"last={self._core.last}",
            )
            self._log.debug(
                f"Applying fills to {order}, "
                f"venue_position_id={venue_position_id}, "
                f"position={position}, "
                f"fills={fills}",
            )

        cdef:
            bint initial_market_to_limit_fill = False
            Price last_fill_px = None
        for fill_px, fill_qty in fills:
            # Validate price precision
            if fill_px._mem.precision != self.instrument.price_precision:
                raise RuntimeError(
                    f"Invalid price precision for fill {fill_px.precision} "
                    f"when instrument price precision is {self.instrument.price_precision}. "
                    f"Check that the data price precision matches the {self.instrument.id} instrument"
                )
            # Validate size precision
            if fill_qty._mem.precision != self.instrument.size_precision:
                raise RuntimeError(
                    f"Invalid size precision for fill {fill_qty.precision} "
                    f"when instrument size precision is {self.instrument.size_precision}. "
                    f"Check that the data size precision matches the {self.instrument.id} instrument"
                )

            if order.filled_qty._mem.raw == 0:
                if order.order_type == OrderType.MARKET_TO_LIMIT:
                    self._generate_order_updated(
                        order,
                        qty=order.quantity,
                        price=fill_px,
                        trigger_price=None,
                    )
                    initial_market_to_limit_fill = True

            if self.book_type == BookType.L1_MBP and self._fill_model.is_slipped():
                if order.side == OrderSide.BUY:
                    fill_px = fill_px.add(self.instrument.price_increment)
                elif order.side == OrderSide.SELL:
                    fill_px = fill_px.sub(self.instrument.price_increment)
                else:
                    raise ValueError(  # pragma: no cover (design-time error)
                        f"invalid `OrderSide`, was {order.side}",  # pragma: no cover (design-time error)
                    )

            # Check reduce only order
            if self._use_reduce_only and order.is_reduce_only and fill_qty._mem.raw > position.quantity._mem.raw:
                if position.quantity._mem.raw == 0:
                    return  # Done

                # Adjust fill to honor reduce only execution (fill remaining position size only)
                fill_qty = Quantity.from_raw_c(position.quantity._mem.raw, fill_qty._mem.precision)

                self._generate_order_updated(
                    order=order,
                    qty=fill_qty,
                    price=None,
                    trigger_price=None,
                )

            if fill_qty._mem.raw == 0:
                if len(fills) == 1 and order.status_c() == OrderStatus.SUBMITTED:
                    self._generate_order_rejected(order, f"no market for {order.instrument_id}")
                return  # Done

            self.fill_order(
                order=order,
                last_px=fill_px,
                last_qty=fill_qty,
                liquidity_side=order.liquidity_side,
                venue_position_id=venue_position_id,
                position=position,
            )
            if order.order_type == OrderType.MARKET_TO_LIMIT and initial_market_to_limit_fill:
                return  # Filled initial level

            last_fill_px = fill_px

        if order.time_in_force == TimeInForce.IOC and order.is_open_c():
            # IOC order has filled all available size
            self.cancel_order(order)
            return

        if (
            order.is_open_c()
            and self.book_type == BookType.L1_MBP
            and (
            order.order_type == OrderType.MARKET
            or order.order_type == OrderType.MARKET_IF_TOUCHED
            or order.order_type == OrderType.STOP_MARKET
        )
        ):
            # Exhausted simulated book volume (continue aggressive filling into next level)
            # This is a very basic implementation of slipping by a single tick, in the future
            # we will implement more detailed fill modeling.
            if order.side == OrderSide.BUY:
                fill_px = last_fill_px.add(self.instrument.price_increment)
            elif order.side == OrderSide.SELL:
                fill_px = last_fill_px.sub(self.instrument.price_increment)
            else:
                raise ValueError(  # pragma: no cover (design-time error)
                    f"invalid `OrderSide`, was {order.side}",  # pragma: no cover (design-time error)
                )

            self.fill_order(
                order=order,
                last_px=fill_px,
                last_qty=order.leaves_qty,
                liquidity_side=order.liquidity_side,
                venue_position_id=venue_position_id,
                position=position,
            )


    cpdef void fill_order(
        self,
        Order order,
        Price last_px,
        Quantity last_qty,
        LiquiditySide liquidity_side,
        PositionId venue_position_id: PositionId | None = None,
        Position position: Position | None = None,
    ):
        """
        Apply the given list of fills to the given order. Optionally provide
        existing position details.

        Parameters
        ----------
        order : Order
            The order to fill.
        last_px : Price
            The fill price for the order.
        last_qty : Quantity
            The fill quantity for the order.
        liquidity_side : LiquiditySide
            The liquidity side for the fill.
        venue_position_id :  PositionId, optional
            The current venue position ID related to the order (if assigned).
        position : Position, optional
            The current position related to the order (if any).

        Raises
        ------
        ValueError
            If `liquidity_side` is ``NO_LIQUIDITY_SIDE``.

        Warnings
        --------
        The `liquidity_side` will override anything previously set on the order.

        """
        Condition.not_none(order, "order")
        Condition.not_none(last_px, "last_px")
        Condition.not_none(last_qty, "last_qty")
        Condition.not_equal(liquidity_side, LiquiditySide.NO_LIQUIDITY_SIDE, "liquidity_side", "NO_LIQUIDITY_SIDE")

        order.liquidity_side = liquidity_side

        cdef Quantity cached_filled_qty = self._cached_filled_qty.get(order.client_order_id)
        cdef Quantity leaves_qty = None
        if cached_filled_qty is None:
            self._cached_filled_qty[order.client_order_id] = Quantity.from_raw_c(last_qty._mem.raw, last_qty._mem.precision)
        else:
            leaves_qty = Quantity.from_raw_c(order.quantity._mem.raw - cached_filled_qty._mem.raw, last_qty._mem.precision)
            last_qty = Quantity.from_raw_c(min(leaves_qty._mem.raw, last_qty._mem.raw), last_qty._mem.precision)
            cached_filled_qty._mem.raw += last_qty._mem.raw

        # Calculate commission
        cdef Money commission = self._fee_model.get_commission(
            order=order,
            fill_qty=last_qty,
            fill_px=last_px,
            instrument=self.instrument,
        )

        self._generate_order_filled(
            order=order,
            venue_order_id=self._get_venue_order_id(order),
            venue_position_id=venue_position_id,
            last_qty=last_qty,
            last_px=last_px,
            quote_currency=self.instrument.quote_currency,
            commission=commission,
            liquidity_side=order.liquidity_side,
        )

        if order.is_passive_c() and order.is_closed_c():
            # Remove order from market
            self._core.delete_order(order)
            self._cached_filled_qty.pop(order.client_order_id, None)

        if not self._support_contingent_orders:
            return

        # Check contingent orders
        cdef ClientOrderId client_order_id
        cdef Order child_order
        if order.contingency_type == ContingencyType.OTO:
            for client_order_id in order.linked_order_ids:
                child_order = self.cache.order(client_order_id)
                assert child_order is not None, "OTO child order not found"
                if child_order.is_closed_c():
                    continue
                if child_order.is_active_local_c():
                    continue  # Order is not on the exchange yet
                if child_order.position_id is None and order.position_id is not None:
                    self.cache.add_position_id(
                        position_id=order.position_id,
                        venue=self.venue,
                        client_order_id=client_order_id,
                        strategy_id=child_order.strategy_id,
                    )
                    self._log.debug(
                        f"Indexed {order.position_id!r} "
                        f"for {child_order.client_order_id!r}",
                    )
                if not child_order.is_open_c() or (child_order.status_c() == OrderStatus.PENDING_UPDATE and child_order._previous_status == OrderStatus.SUBMITTED):
                    self.process_order(
                        order=child_order,
                        account_id=order.account_id or self._account_ids[order.trader_id],
                    )
        elif order.contingency_type == ContingencyType.OCO:
            for client_order_id in order.linked_order_ids:
                oco_order = self.cache.order(client_order_id)
                assert oco_order is not None, "OCO order not found"
                if oco_order.is_closed_c():
                    continue
                if oco_order.is_active_local_c():
                    continue  # Order is not on the exchange yet
                self.cancel_order(oco_order)
        elif order.contingency_type == ContingencyType.OUO:
            for client_order_id in order.linked_order_ids:
                ouo_order = self.cache.order(client_order_id)
                assert ouo_order is not None, "OUO order not found"
                if ouo_order.is_active_local_c():
                    continue  # Order is not on the exchange yet
                if order.is_closed_c() and ouo_order.is_open_c():
                    self.cancel_order(ouo_order)
                elif order.leaves_qty._mem.raw != 0 and order.leaves_qty._mem.raw != ouo_order.leaves_qty._mem.raw:
                    self.update_order(
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
                self._use_reduce_only
                and order.is_reduce_only
                and order.is_open_c()
                and order.is_passive_c()
            ):
                if position.quantity._mem.raw == 0:
                    self.cancel_order(order)
                elif order.leaves_qty._mem.raw != position.quantity._mem.raw:
                    self.update_order(
                        order,
                        position.quantity,
                        price=order.price if order.has_price_c() else None,
                        trigger_price=order.trigger_price if order.has_trigger_price_c() else None,
                    )

# -- IDENTIFIER GENERATORS ------------------------------------------------------------------------

    cdef VenueOrderId _get_venue_order_id(self, Order order):
        # Check existing on order
        cdef VenueOrderId venue_order_id = order.venue_order_id
        if venue_order_id is not None:
            return venue_order_id

        # Check exiting in cache
        venue_order_id = self.cache.venue_order_id(order.client_order_id)
        if venue_order_id is not None:
            return venue_order_id

        venue_order_id = self._generate_venue_order_id()
        self.cache.add_venue_order_id(order.client_order_id, venue_order_id)

        return venue_order_id

    cdef PositionId _get_position_id(self, Order order, bint generate=True):
        cdef PositionId position_id
        if self.oms_type == OmsType.HEDGING:
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
        if not self._use_position_ids:
            return None

        self._position_count += 1
        if self._use_random_ids:
            return PositionId(str(uuid.uuid4()))
        else:
            return PositionId(f"{self.venue.to_str()}-{self.raw_id}-{self._position_count:03d}")

    cdef VenueOrderId _generate_venue_order_id(self):
        self._order_count += 1
        if self._use_random_ids:
            return VenueOrderId(str(uuid.uuid4()))
        else:
            return VenueOrderId(f"{self.venue.to_str()}-{self.raw_id}-{self._order_count:03d}")

    cdef TradeId _generate_trade_id(self):
        self._execution_count += 1
        return TradeId(self._generate_trade_id_str())

    cdef str _generate_trade_id_str(self):
        if self._use_random_ids:
            return str(uuid.uuid4())
        else:
            return f"{self.venue.to_str()}-{self.raw_id}-{self._execution_count:03d}"

# -- EVENT HANDLING -------------------------------------------------------------------------------

    cpdef void accept_order(self, Order order):
        if order.is_closed_c():
            return  # Temporary guard to prevent invalid processing

        # Check if order already accepted (being added back into the matching engine)
        if not order.status_c() == OrderStatus.ACCEPTED:
            self._generate_order_accepted(order, venue_order_id=self._get_venue_order_id(order))

            if (
                order.order_type == OrderType.TRAILING_STOP_MARKET
                or order.order_type == OrderType.TRAILING_STOP_LIMIT
            ):
                if order.trigger_price is None:
                    self._update_trailing_stop_order(order)

        self._core.add_order(order)

    cpdef void expire_order(self, Order order):
        if self._support_contingent_orders and order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self._cancel_contingent_orders(order)

        self._generate_order_expired(order)

    cpdef void cancel_order(self, Order order, bint cancel_contingencies=True):
        if order.is_active_local_c():
            self._log.error(
                f"Cannot cancel an order with {order.status_string_c()} from the matching engine",
            )
            return

        self._core.delete_order(order)
        self._cached_filled_qty.pop(order.client_order_id, None)

        self._generate_order_canceled(order, venue_order_id=self._get_venue_order_id(order))

        if self._support_contingent_orders and order.contingency_type != ContingencyType.NO_CONTINGENCY and cancel_contingencies:
            self._cancel_contingent_orders(order)

    cpdef void update_order(
        self,
        Order order,
        Quantity qty,
        Price price = None,
        Price trigger_price = None,
        bint update_contingencies = True,
    ):
        if qty is None:
            qty = order.quantity

        if order.order_type == OrderType.LIMIT or order.order_type == OrderType.MARKET_TO_LIMIT:
            if price is None:
                price = order.price
            self._update_limit_order(order, qty, price)
        elif order.order_type == OrderType.STOP_MARKET:
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_stop_market_order(order, qty, trigger_price)
        elif order.order_type == OrderType.STOP_LIMIT:
            if price is None:
                price = order.price
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_stop_limit_order(order, qty, price, trigger_price)
        elif order.order_type == OrderType.MARKET_IF_TOUCHED:
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_market_if_touched_order(order, qty, trigger_price)
        elif order.order_type == OrderType.LIMIT_IF_TOUCHED:
            if price is None:
                price = order.price
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_limit_if_touched_order(order, qty, price, trigger_price)
        elif order.order_type == OrderType.TRAILING_STOP_MARKET:
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_market_if_touched_order(order, qty, trigger_price)
        elif order.order_type == OrderType.TRAILING_STOP_LIMIT:
            if price is None:
                price = order.price
            if trigger_price is None:
                trigger_price = order.trigger_price
            self._update_limit_if_touched_order(order, qty, price, trigger_price)
        else:
            raise ValueError(
                f"invalid `OrderType` was {order.order_type}")  # pragma: no cover (design-time error)

        if self._support_contingent_orders and order.contingency_type != ContingencyType.NO_CONTINGENCY and update_contingencies:
            self._update_contingent_orders(order)

    cpdef void trigger_stop_order(self, Order order):
        # Always STOP_LIMIT or LIMIT_IF_TOUCHED orders
        cdef Price trigger_price = order.trigger_price
        cdef Price price = order.price

        if self._fill_model:
            if order.side == OrderSide.BUY and self._core.ask_raw == trigger_price._mem.raw and not self._fill_model.is_stop_filled():
                return  # Not triggered
            elif order.side == OrderSide.SELL and self._core.bid_raw == trigger_price._mem.raw and not self._fill_model.is_stop_filled():
                return  # Not triggered

        self._generate_order_triggered(order)

        # Check for immediate fill
        if order.side == OrderSide.BUY and trigger_price._mem.raw > price._mem.raw > self._core.ask_raw:
            order.liquidity_side = LiquiditySide.MAKER
            self.fill_limit_order(order)
            return
        elif order.side == OrderSide.SELL and trigger_price._mem.raw < price._mem.raw < self._core.bid_raw:
            order.liquidity_side = LiquiditySide.MAKER
            self.fill_limit_order(order)
            return

        if self._core.is_limit_matched(order.side, price):
            if order.is_post_only:
                # Would be liquidity taker
                self._core.delete_order(order)
                self._cached_filled_qty.pop(order.client_order_id, None)
                self._generate_order_rejected(
                    order,
                    f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"limit px of {order.price} would have been a TAKER: "
                    f"bid={self._core.bid}, "
                    f"ask={self._core.ask}",
                )
                return
            order.liquidity_side = LiquiditySide.TAKER
            self.fill_limit_order(order)

    cdef void _update_contingent_orders(self, Order order):
        self._log.debug(f"Updating OUO orders from {order.client_order_id}", LogColor.MAGENTA)
        cdef ClientOrderId client_order_id
        cdef Order ouo_order
        for client_order_id in order.linked_order_ids:
            ouo_order = self.cache.order(client_order_id)
            assert ouo_order is not None, "OUO order not found"
            if ouo_order.is_active_local_c():
                continue  # Order is not on the exchange yet
            if ouo_order.order_type == OrderType.MARKET or ouo_order.is_closed_c():
                continue
            if order.leaves_qty._mem.raw == 0:
                self.cancel_order(ouo_order)
            elif ouo_order.leaves_qty._mem.raw != order.leaves_qty._mem.raw:
                self.update_order(
                    ouo_order,
                    order.leaves_qty,
                    price=ouo_order.price if ouo_order.has_price_c() else None,
                    trigger_price=ouo_order.trigger_price if ouo_order.has_trigger_price_c() else None,
                    update_contingencies=False,
                )

    cdef void _cancel_contingent_orders(self, Order order):
        # Iterate all contingent orders and cancel if active
        cdef ClientOrderId client_order_id
        cdef Order contingent_order
        for client_order_id in order.linked_order_ids:
            contingent_order = self.cache.order(client_order_id)
            assert contingent_order is not None, "Contingency order not found"
            if contingent_order.is_active_local_c():
                continue  # Order is not on the exchange yet
            if not contingent_order.is_closed_c():
                self.cancel_order(contingent_order, cancel_contingencies=False)

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_order_rejected(self, Order order, str reason):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderRejected event = OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_accepted(self, Order order, VenueOrderId venue_order_id):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderAccepted event = OrderAccepted(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
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
    ):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderModifyRejected event = OrderModifyRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
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
    ):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderCancelRejected event = OrderCancelRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cpdef void _generate_order_updated(
        self,
        Order order,
        Quantity quantity,
        Price price,
        Price trigger_price,
    ):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_canceled(self, Order order, VenueOrderId venue_order_id):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderCanceled event = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_triggered(self, Order order):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderTriggered event = OrderTriggered(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.trader_id],
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_expired(self, Order order):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderExpired event = OrderExpired(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id or self._account_ids[order.client_order_id],
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)

    cdef void _generate_order_filled(
        self,
        Order order,
        VenueOrderId venue_order_id,
        PositionId venue_position_id,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side
    ):
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderFilled event = OrderFilled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
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
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.msgbus.send(endpoint="ExecEngine.process", msg=event)
