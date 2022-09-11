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

from decimal import Decimal
from heapq import heappush
from typing import Dict, Optional

from nautilus_trader.config.error import InvalidConfiguration

from libc.limits cimport INT_MAX
from libc.limits cimport INT_MIN
from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.backtest.execution_client cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.models cimport LatencyModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.account_type cimport AccountTypeParser
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSide
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.c_enums.contingency_type cimport ContingencyType
from nautilus_trader.model.c_enums.depth_type cimport DepthType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.oms_type cimport OMSTypeParser
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
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.crypto_future cimport CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual cimport CryptoPerpetual
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.data cimport Order as OrderBookOrder
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.trailing_stop_limit cimport TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market cimport TrailingStopMarketOrder
from nautilus_trader.model.position cimport Position


cdef class SimulatedExchange:
    """
    Provides a simulated financial market exchange.

    Parameters
    ----------
    venue : Venue
        The venue to simulate.
    oms_type : OMSType {``HEDGING``, ``NETTING``}
        The order management system type used by the exchange.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    starting_balances : list[Money]
        The starting balances for the exchange.
    default_leverage : Decimal
        The account default leverage (for margin accounts).
    leverages : Dict[InstrumentId, Decimal]
        The instrument specific leverage configuration (for margin accounts).
    cache : CacheFacade
        The read-only cache for the exchange.
    fill_model : FillModel
        The fill model for the exchange.
    latency_model : LatencyModel, optional
        The latency model for the exchange.
    clock : TestClock
        The clock for the exchange.
    logger : Logger
        The logger for the exchange.
    book_type : BookType
        The order book type for the exchange.
    frozen_account : bool, default False
        If the account for this exchange is frozen (balances will not change).
    reject_stop_orders : bool, default True
        If stop orders are rejected on submission if in the market.

    Raises
    ------
    ValueError
        If `instruments` is empty.
    ValueError
        If `instruments` contains a type other than `Instrument`.
    ValueError
        If `starting_balances` is empty.
    ValueError
        If `starting_balances` contains a type other than `Money`.
    ValueError
        If `base_currency` and multiple starting balances.
    ValueError
        If `modules` contains a type other than `SimulationModule`.
    """

    def __init__(
        self,
        Venue venue not None,
        OMSType oms_type,
        AccountType account_type,
        Currency base_currency: Optional[Currency],
        list starting_balances not None,
        default_leverage not None: Decimal,
        leverages not None: Dict[InstrumentId, Decimal],
        list instruments not None,
        list modules not None,
        CacheFacade cache not None,
        TestClock clock not None,
        Logger logger not None,
        FillModel fill_model not None,
        LatencyModel latency_model = None,
        BookType book_type = BookType.L1_TBBO,
        bint frozen_account = False,
        bint reject_stop_orders = True,
    ):
        Condition.list_type(instruments, Instrument, "instruments", "Instrument")
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(starting_balances, Money, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules", "SimulationModule")
        if base_currency:
            Condition.true(len(starting_balances) == 1, "single-currency account has multiple starting currencies")
        if default_leverage and default_leverage > 1 or leverages:
            Condition.true(account_type == AccountType.MARGIN, "leverages defined when account type is not `MARGIN`")

        self._clock = clock
        self._log = LoggerAdapter(
            component_name=f"{type(self).__name__}({venue})",
            logger=logger,
        )

        self.id = venue
        self.oms_type = oms_type
        self._log.info(f"OMSType={OMSTypeParser.to_str(oms_type)}")
        self.book_type = book_type

        self.cache = cache
        self.exec_client = None  # Initialized when execution client registered

        # Accounting
        self.account_type = account_type
        self.base_currency = base_currency
        self.starting_balances = starting_balances
        self.default_leverage = default_leverage
        self.leverages = leverages
        self.is_frozen_account = frozen_account

        # Execution
        self.reject_stop_orders = reject_stop_orders
        self.fill_model = fill_model
        self.latency_model = latency_model
        self._bar_execution = False

        # Load modules
        self.modules = []
        for module in modules:
            Condition.not_in(module, self.modules, "module", "modules")
            module.register_exchange(self)
            self.modules.append(module)
            self._log.info(f"Loaded {module}.")

        # InstrumentId indexer for venue_order_ids
        self._instrument_indexer = {}  # type: dict[InstrumentId, int]

        # Load instruments
        self.instruments: Dict[InstrumentId, Instrument] = {}
        for instrument in instruments:
            self.add_instrument(instrument)

        # Markets
        self._books = {}             # type: dict[InstrumentId, OrderBook]
        self._last = {}              # type: dict[InstrumentId, Price]
        self._last_bids = {}         # type: dict[InstrumentId, Price]
        self._last_asks = {}         # type: dict[InstrumentId, Price]
        self._last_bid_bars = {}     # type: dict[InstrumentId, Bar]
        self._last_ask_bars = {}     # type: dict[InstrumentId, Bar]
        self._order_index = {}       # type: dict[ClientOrderId, Order]
        self._orders_bid = {}        # type: dict[InstrumentId, list[Order]]
        self._orders_ask = {}        # type: dict[InstrumentId, list[Order]]
        self._oto_orders = {}        # type: dict[ClientOrderId, ClientOrderId]

        self._symbol_pos_count = {}  # type: dict[InstrumentId, int]
        self._symbol_ord_count = {}  # type: dict[InstrumentId, int]
        self._executions_count = 0
        self._message_queue = Queue()
        self._inflight_queue = []
        self._inflight_counter = {}  # type: dict[uint64_t, int]

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"id={self.id}, "
            f"oms_type={OMSTypeParser.to_str(self.oms_type)}, "
            f"account_type={AccountTypeParser.to_str(self.account_type)})"
        )

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, BacktestExecClient client) except *:
        """
        Register the given execution client with the simulated exchange.

        Parameters
        ----------
        client : BacktestExecClient
            The client to register

        """
        Condition.not_none(client, "client")

        self.exec_client = client

        self._log.info(f"Registered ExecutionClient-{client}.")

    cpdef void set_fill_model(self, FillModel fill_model) except *:
        """
        Set the fill model to the given model.

        Parameters
        ----------
        fill_model : FillModel
            The fill model to set.

        """
        Condition.not_none(fill_model, "fill_model")

        self.fill_model = fill_model

        self._log.info("Changed fill model.")

    cpdef void set_latency_model(self, LatencyModel latency_model) except *:
        """
        Change the latency model for this exchange.

        Parameters
        ----------
        latency_model : LatencyModel
            The latency model to set.

        """
        Condition.not_none(latency_model, "latency_model")

        self.latency_model = latency_model

        self._log.info("Changed latency model.")

    cpdef void initialize_account(self) except *:
        """
        Initialize the account to the starting balances.
        """
        self._generate_fresh_account_state()

    cpdef void add_instrument(self, Instrument instrument) except *:
        """
        Add the given instrument to the venue.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        Raises
        ------
        ValueError
            If `instrument.id.venue` is not equal to the venue ID.
        KeyError
            If `instrument` is already contained within the venue.
            This is to enforce correct internal identifier indexing.
        InvalidConfiguration
            If `instrument` is invalid for this venue.

        """
        Condition.not_none(instrument, "instrument")
        Condition.equal(instrument.id.venue, self.id, "instrument.id.venue", "self.id")
        Condition.not_in(instrument.id, self.instruments, "instrument.id", "self.instruments")

        # Validate instrument
        if isinstance(instrument, (CryptoPerpetual, CryptoFuture)):
            if self.account_type == AccountType.CASH:
                raise InvalidConfiguration(
                    f"Cannot add a `{type(instrument).__name__}` type instrument "
                    f"to a venue with a `CASH` account type. Please add to a "
                    f"venue with a `MARGIN` account type.",
                )

        self.instruments[instrument.id] = instrument

        index = len(self._instrument_indexer) + 1
        self._instrument_indexer[instrument.id] = index

        self._log.info(f"Loaded instrument {instrument.id}.")

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self, InstrumentId instrument_id):
        """
        Return the best bid price for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        Price or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderBook order_book = self._books.get(instrument_id)
        if order_book is None:
            return None
        best_bid_price = order_book.best_bid_price()
        if best_bid_price is None:
            return None
        return Price(best_bid_price, order_book.price_precision)

    cpdef Price best_ask_price(self, InstrumentId instrument_id):
        """
        Return the best ask price for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        Price or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderBook order_book = self._books.get(instrument_id)
        if order_book is None:
            return None
        best_ask_price = order_book.best_ask_price()
        if best_ask_price is None:
            return None
        return Price(best_ask_price, order_book.price_precision)

    cpdef OrderBook get_book(self, InstrumentId instrument_id):
        """
        Return the order book for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        OrderBook

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderBook book = self._books.get(instrument_id)
        if book is None:
            instrument = self.instruments.get(instrument_id)
            if instrument is None:
                raise RuntimeError(
                    f"cannot create OrderBook: no instrument for {instrument_id}"
                )
            # Create order book
            book = OrderBook.create(
                instrument=instrument,
                book_type=self.book_type,
                simulated=True,
            )

            # Add to books
            self._books[instrument_id] = book

        return book

    cpdef dict get_books(self):
        """
        Return all order books with the exchange.

        Returns
        -------
        dict[InstrumentId, OrderBook]

        """
        return self._books.copy()

    cpdef list get_open_orders(self, InstrumentId instrument_id = None):
        """
        Return the open orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        return (
            self.get_open_bid_orders(instrument_id)
            + self.get_open_ask_orders(instrument_id)
        )

    cpdef list get_open_bid_orders(self, InstrumentId instrument_id = None):
        """
        Return the open bid orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        cdef list bids = []
        if instrument_id is None:
            for orders in self._orders_bid.values():
                for o in orders:
                    bids.append(o)
            return bids
        else:
            return [o for o in self._orders_bid.get(instrument_id, [])]

    cpdef list get_open_ask_orders(self, InstrumentId instrument_id = None):
        """
        Return the open ask orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        cdef list asks = []
        if instrument_id is None:
            for orders in self._orders_ask.values():
                for o in orders:
                    asks.append(o)
            return asks
        else:
            return [o for o in self._orders_ask.get(instrument_id, [])]

    cpdef Account get_account(self):
        """
        Return the account for the registered client (if registered).

        Returns
        -------
        Account or ``None``

        """
        if not self.exec_client:
            return None

        return self.exec_client.get_account()

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment) except *:
        """
        Adjust the account at the exchange with the given adjustment.

        Parameters
        ----------
        adjustment : Money
            The adjustment for the account.

        """
        Condition.not_none(adjustment, "adjustment")

        if self.is_frozen_account:
            return  # Nothing to adjust

        cdef Account account = self.cache.account_for_venue(self.exec_client.venue)
        if account is None:
            self._log.error(
                f"Cannot adjust account: no account found for {self.exec_client.venue}"
            )
            return

        cdef AccountBalance balance = account.balance(adjustment.currency)
        if balance is None:
            self._log.error(
                f"Cannot adjust account: no balance found for {adjustment.currency}"
            )
            return

        balance.total = Money(balance.total + adjustment, adjustment.currency)
        balance.free = Money(balance.free + adjustment, adjustment.currency)

        cdef list margins = []
        if account.is_margin_account:
            margins = list(account.margins().values())

        # Generate and handle event
        self.exec_client.generate_account_state(
            balances=[balance],
            margins=margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

    cpdef void send(self, TradingCommand command) except *:
        """
        Send the given trading command into the exchange.

        Parameters
        ----------
        command : TradingCommand
            The command to send.

        """
        Condition.not_none(command, "command")

        if self.latency_model is None:
            self._message_queue.put_nowait(command)
        else:
            heappush(self._inflight_queue, self.generate_inflight_command(command))

    cdef tuple generate_inflight_command(self, TradingCommand command):
        cdef uint64_t ts
        if isinstance(command, (SubmitOrder, SubmitOrderList)):
            ts = command.ts_init + self.latency_model.insert_latency_nanos
        elif isinstance(command, ModifyOrder):
            ts = command.ts_init + self.latency_model.update_latency_nanos
        elif isinstance(command, (CancelOrder, CancelAllOrders)):
            ts = command.ts_init + self.latency_model.cancel_latency_nanos
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid command, was {command}")
        if ts not in self._inflight_counter:
            self._inflight_counter[ts] = 0
        self._inflight_counter[ts] += 1
        cdef (uint64_t, uint64_t) key = (ts, self._inflight_counter[ts])
        return key, command

    cpdef void process_order_book(self, OrderBookData data) except *:
        """
        Process the exchanges market for the given order book data.

        Parameters
        ----------
        data : OrderBookData
            The order book data to process.

        """
        Condition.not_none(data, "data")

        self._clock.set_time(data.ts_init)
        self.get_book(data.instrument_id).apply(data)

        self._iterate_matching_engine(
            data.instrument_id,
            data.ts_init,
        )

        if not self._log.is_bypassed:
            self._log.debug(f"Processed {data}")

    cpdef void process_quote_tick(self, QuoteTick tick) except *:
        """
        Process the exchanges market for the given quote tick.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : QuoteTick
            The tick to process.

        """
        Condition.not_none(tick, "tick")

        self._clock.set_time(tick.ts_init)

        cdef OrderBook book = self.get_book(tick.instrument_id)
        if book.type == BookType.L1_TBBO:
            book.update_quote_tick(tick)

        self._iterate_matching_engine(
            tick.instrument_id,
            tick.ts_init,
        )

        if not self._log.is_bypassed:
            self._log.debug(f"Processed {tick}")

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

        self._clock.set_time(tick.ts_init)

        cdef OrderBook book = self.get_book(tick.instrument_id)
        if book.type == BookType.L1_TBBO:
            book.update_trade_tick(tick)

        self._iterate_matching_engine(
            tick.instrument_id,
            tick.ts_init,
        )

        self._last[tick.instrument_id] = tick.price

        if not self._log.is_bypassed:
            self._log.debug(f"Processed {tick}")

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

        self._clock.set_time(bar.ts_init)

        cdef OrderBook book = self.get_book(bar.type.instrument_id)
        if book.type != BookType.L1_TBBO:
            return  # Can only process an L1 book with bars

        # Turn ON bar execution mode (temporary until unify execution)
        self._bar_execution = True

        cdef PriceType price_type = bar.type.spec.price_type
        if price_type == PriceType.LAST or price_type == PriceType.MID:
            self._process_trade_ticks_from_bar(book, bar)
        elif price_type == PriceType.BID:
            self._last_bid_bars[bar.type.instrument_id] = bar
            self._process_quote_ticks_from_bar(book)
        elif price_type == PriceType.ASK:
            self._last_ask_bars[bar.type.instrument_id] = bar
            self._process_quote_ticks_from_bar(book)
        else:  # pragma: no cover (design-time error)
            raise RuntimeError("invalid price type")

        if not self._log.is_bypassed:
            self._log.debug(f"Processed {bar}")

    cdef void _process_trade_ticks_from_bar(self, OrderBook book, Bar bar) except *:
        cdef Quantity size = Quantity(bar.volume.as_double() / 4.0, bar._mem.volume.precision)
        cdef Price last = self._last.get(book.instrument_id)

        # Create reusable tick
        cdef TradeTick tick = TradeTick(
            bar.type.instrument_id,
            bar.open,
            size,
            <OrderSide>AggressorSide.BUY if last is None or bar._mem.open.raw > last._mem.raw else <OrderSide>AggressorSide.SELL,
            self._generate_trade_id(),
            bar.ts_event,
            bar.ts_event,
        )

        # Open
        if last is None or bar._mem.open.raw != last._mem.raw:  # Direct memory comparison
            book.update_trade_tick(tick)
            self._iterate_matching_engine(
                tick.instrument_id,
                tick.ts_init,
            )
            last = bar.open

        # High
        if bar._mem.high.raw > last._mem.raw:  # Direct memory comparison
            tick._mem.price = bar._mem.high  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.BUY  # Direct memory assignment
            tick._mem.trade_id = self._generate_trade_id()._mem
            book.update_trade_tick(tick)
            self._iterate_matching_engine(
                tick.instrument_id,
                tick.ts_init,
            )
            last = bar.high

        # Low
        if bar._mem.low.raw < last._mem.raw:  # Direct memory comparison
            tick._mem.price = bar._mem.low  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.SELL
            tick._mem.trade_id = self._generate_trade_id()._mem
            book.update_trade_tick(tick)
            self._iterate_matching_engine(
                tick.instrument_id,
                tick.ts_init,
            )
            last = bar.low

        # Close
        if bar._mem.close.raw != last._mem.raw:  # Direct memory comparison
            tick._mem.price = bar._mem.close  # Direct memory assignment
            tick._mem.aggressor_side = <OrderSide>AggressorSide.BUY if bar._mem.close.raw > last._mem.raw else <OrderSide>AggressorSide.SELL
            tick._mem.trade_id = self._generate_trade_id()._mem
            book.update_trade_tick(tick)
            self._iterate_matching_engine(
                tick.instrument_id,
                tick.ts_init,
            )
            last = bar.close

        self._last[book.instrument_id] = last

    cdef void _process_quote_ticks_from_bar(self, OrderBook book) except *:
        cdef Bar last_bid_bar = self._last_bid_bars.get(book.instrument_id)
        cdef Bar last_ask_bar = self._last_ask_bars.get(book.instrument_id)

        if last_bid_bar is None or last_ask_bar is None:
            return  # Wait for next bar

        if last_bid_bar.ts_event != last_ask_bar.ts_event:
            return  # Wait for next bar

        cdef Quantity bid_size = Quantity(last_bid_bar.volume.as_double() / 4.0, last_bid_bar._mem.volume.precision)
        cdef Quantity ask_size = Quantity(last_ask_bar.volume.as_double() / 4.0, last_ask_bar._mem.volume.precision)

        # Create reusable tick
        cdef QuoteTick tick = QuoteTick(
            book.instrument_id,
            last_bid_bar.open,
            last_ask_bar.open,
            bid_size,
            ask_size,
            last_bid_bar.ts_event,
            last_ask_bar.ts_init,
        )

        # Open
        book.update_quote_tick(tick)
        self._iterate_matching_engine(
            tick.instrument_id,
            tick.ts_init,
        )

        # High
        tick._mem.bid = last_bid_bar._mem.high  # Direct memory assignment
        tick._mem.ask = last_ask_bar._mem.high  # Direct memory assignment
        book.update_quote_tick(tick)
        self._iterate_matching_engine(
            tick.instrument_id,
            tick.ts_init,
        )

        # Low
        tick._mem.bid = last_bid_bar._mem.low  # Assigning memory directly
        tick._mem.ask = last_ask_bar._mem.low  # Assigning memory directly
        book.update_quote_tick(tick)
        self._iterate_matching_engine(
            tick.instrument_id,
            tick.ts_init,
        )

        # Close
        tick._mem.bid = last_bid_bar._mem.close  # Assigning memory directly
        tick._mem.ask = last_ask_bar._mem.close  # Assigning memory directly
        book.update_quote_tick(tick)
        self._iterate_matching_engine(
            tick.instrument_id,
            tick.ts_init,
        )

    cpdef void process(self, uint64_t now_ns) except *:
        """
        Process the exchange to the gives time.

        All pending commands will be processed along with all simulation modules.

        Parameters
        ----------
        now_ns : uint64_t
            The UNIX timestamp (nanoseconds) now.

        """
        self._clock.set_time(now_ns)

        cdef:
            uint64_t ts
        while self._inflight_queue:
            # Peek at timestamp of next in-flight message
            ts = self._inflight_queue[0][0][0]
            if ts <= now_ns:
                # Place message on queue to be processed
                self._message_queue.put_nowait(self._inflight_queue.pop(0)[1])
                self._inflight_counter.pop(ts, None)
            else:
                break

        cdef:
            TradingCommand command
            Order order
            list orders
        while self._message_queue.count > 0:
            command = self._message_queue.get_nowait()
            if isinstance(command, SubmitOrder):
                self._process_order(command.order)
            elif isinstance(command, SubmitOrderList):
                for order in command.list.orders:
                    self._process_order(order)
            elif isinstance(command, ModifyOrder):
                order = self._order_index.get(command.client_order_id)
                if order is None:
                    self._generate_order_modify_rejected(
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        command.venue_order_id,
                        f"{repr(command.client_order_id)} not found",
                    )
                    continue
                self._generate_order_pending_update(order)
                self._update_order(
                    order,
                    command.quantity,
                    command.price,
                    command.trigger_price,
                )
            elif isinstance(command, CancelOrder):
                order = self._order_index.pop(command.client_order_id, None)
                if order is None:
                    self._generate_order_cancel_rejected(
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        command.venue_order_id,
                        f"{repr(command.client_order_id)} not found",
                    )
                    continue
                if order.is_inflight_c() or order.is_open_c():
                    self._generate_order_pending_cancel(order)
                    self._cancel_order(order)
            elif isinstance(command, CancelAllOrders):
                orders = (
                    self._orders_bid.get(command.instrument_id, [])
                    + self._orders_ask.get(command.instrument_id, [])
                )
                for order in orders:
                    if order.is_inflight_c() or order.is_open_c():
                        self._generate_order_pending_cancel(order)
                        self._cancel_order(order)

        # Iterate over modules
        cdef SimulationModule module
        for module in self.modules:
            module.process(now_ns)

        self._last_bids.clear()
        self._last_asks.clear()

    cpdef void reset(self) except *:
        """
        Reset the simulated exchange.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        for module in self.modules:
            module.reset()

        self._generate_fresh_account_state()

        self._books.clear()
        self._last.clear()
        self._last_bids.clear()
        self._last_asks.clear()
        self._last_bid_bars.clear()
        self._last_ask_bars.clear()
        self._order_index.clear()
        self._orders_bid.clear()
        self._orders_ask.clear()

        self._symbol_pos_count.clear()
        self._symbol_ord_count.clear()
        self._executions_count = 0
        self._message_queue = Queue()
        self._inflight_queue.clear()
        self._inflight_counter.clear()

        self._log.info("Reset.")

# -- COMMAND HANDLING -----------------------------------------------------------------------------

    cdef void _process_order(self, Order order) except *:
        if order.client_order_id in self._order_index:
            return  # Already processed

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
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(
                f"{OrderTypeParser.to_str(order.type)} "
                f"orders are not supported for backtesting in this version",
            )

    cdef void _process_market_order(self, MarketOrder order) except *:
        # Check market exists
        if order.side == OrderSide.BUY and not self.best_ask_price(order.instrument_id):
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self.best_bid_price(order.instrument_id):
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Immediately fill marketable order
        self._fill_market_order(order, LiquiditySide.TAKER)

    cdef void _process_market_to_limit_order(self, MarketToLimitOrder order) except *:
        # Check market exists
        if order.side == OrderSide.BUY and not self.best_ask_price(order.instrument_id):
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self.best_bid_price(order.instrument_id):
            self._generate_order_rejected(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        # Order is valid and accepted
        self._accept_order(order)

        # Immediately fill marketable order
        self._fill_market_order(order, LiquiditySide.TAKER)

    cdef void _process_limit_order(self, LimitOrder order) except *:
        if order.is_post_only and self._is_limit_marketable(order.instrument_id, order.side, order.price):
            self._generate_order_rejected(
                order,
                f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                f"limit px of {order.price} would have been a TAKER: "
                f"bid={self.best_bid_price(order.instrument_id)}, "
                f"ask={self.best_ask_price(order.instrument_id)}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

        # Check for immediate fill
        if self._is_limit_matched(order.instrument_id, order.side, order.price):
            # Filling as liquidity taker
            self._fill_limit_order(order, LiquiditySide.TAKER)
        elif order.time_in_force == TimeInForce.FOK or order.time_in_force == TimeInForce.IOC:
            self._cancel_order(order)

    cdef void _process_stop_market_order(self, Order order) except *:
        if self._is_stop_marketable(order.instrument_id, order.side, order.trigger_price):
            if self.reject_stop_orders:
                self._generate_order_rejected(
                    order,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"stop px of {order.trigger_price} was in the market: "
                    f"bid={self.best_bid_price(order.instrument_id)}, "
                    f"ask={self.best_ask_price(order.instrument_id)}",
                )
                return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

    cdef void _process_stop_limit_order(self, Order order) except *:
        if self._is_stop_marketable(order.instrument_id, order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self.best_bid_price(order.instrument_id)}, "
                f"ask={self.best_ask_price(order.instrument_id)}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

    cdef void _process_trailing_stop_market_order(self, TrailingStopMarketOrder order) except *:
        if order.has_trigger_price_c() and self._is_stop_marketable(order.instrument_id, order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self.best_bid_price(order.instrument_id)}, "
                f"ask={self.best_ask_price(order.instrument_id)}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)

        if order.trigger_price is None:
            self._manage_trailing_stop(order)

    cdef void _process_trailing_stop_limit_order(self, TrailingStopLimitOrder order) except *:
        if order.has_trigger_price_c() and self._is_stop_marketable(order.instrument_id, order.side, order.trigger_price):
            self._generate_order_rejected(
                order,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"trigger stop px of {order.trigger_price} was in the market: "
                f"bid={self.best_bid_price(order.instrument_id)}, "
                f"ask={self.best_ask_price(order.instrument_id)}",
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
    ) except *:
        if self._is_limit_marketable(order.instrument_id, order.side, price):
            if order.is_post_only:
                self._generate_order_modify_rejected(
                    order.strategy_id,
                    order.instrument_id,
                    order.client_order_id,
                    order.venue_order_id,
                    f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"new limit px of {price} would have been a TAKER: "
                    f"bid={self.best_bid_price(order.instrument_id)}, "
                    f"ask={self.best_ask_price(order.instrument_id)}",
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
        if self._is_stop_marketable(order.instrument_id, order.side, trigger_price):
            self._generate_order_modify_rejected(
                order.strategy_id,
                order.instrument_id,
                order.client_order_id,
                order.venue_order_id,
                f"{order.type_string_c()} {order.side_string_c()} order "
                f"new stop px of {trigger_price} was in the market: "
                f"bid={self.best_bid_price(order.instrument_id)}, "
                f"ask={self.best_ask_price(order.instrument_id)}",
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
            if self._is_stop_marketable(order.instrument_id, order.side, price):
                self._generate_order_modify_rejected(
                    order.strategy_id,
                    order.instrument_id,
                    order.client_order_id,
                    order.venue_order_id,
                    f"{order.type_string_c()} {order.side_string_c()} order "
                    f"new trigger stop px of {price} was in the market: "
                    f"bid={self.best_bid_price(order.instrument_id)}, "
                    f"ask={self.best_ask_price(order.instrument_id)}",
                )
                return  # Cannot update order
        else:
            # Updating limit price
            if self._is_limit_marketable(order.instrument_id, order.side, price):
                if order.is_post_only:
                    self._generate_order_modify_rejected(
                        order.strategy_id,
                        order.instrument_id,
                        order.client_order_id,
                        order.venue_order_id,
                        f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order  "
                        f"new limit px of {price} would have been a TAKER: "
                        f"bid={self.best_bid_price(order.instrument_id)}, "
                        f"ask={self.best_ask_price(order.instrument_id)}",
                    )
                    return  # Cannot update order
                else:
                    self._generate_order_updated(order, qty, price, None)
                    self._fill_limit_order(order, LiquiditySide.TAKER)  # Immediate fill as TAKER
                    return  # Filled

        self._generate_order_updated(order, qty, price, trigger_price or order.trigger_price)

# -- EVENT HANDLING -------------------------------------------------------------------------------

    cdef void _accept_order(self, Order order) except *:
        self._add_order(order)
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
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderType, was {order.type}")

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
            order.venue_order_id = self._generate_venue_order_id(order.instrument_id)

        cdef:
            list orders_bid
            list orders_ask
        if order.is_buy_c():
            orders_bid = self._orders_bid.get(order.instrument_id)
            if orders_bid and order in orders_bid:
                orders_bid.remove(order)
        elif order.is_sell_c():
            orders_ask = self._orders_ask.get(order.instrument_id)
            if orders_ask and order in orders_ask:
                orders_ask.remove(order)

        self._generate_order_canceled(order)

        if order.contingency_type == ContingencyType.OCO and cancel_ocos:
            self._cancel_oco_orders(order)

    cdef void _cancel_oco_orders(self, Order order) except*:
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
        self._generate_order_expired(order)

        if order.contingency_type == ContingencyType.OCO:
            self._cancel_oco_orders(order)

# -- ORDER MATCHING ENGINE ------------------------------------------------------------------------

    cdef void _add_order(self, Order order) except *:
        # Index order
        self._order_index[order.client_order_id] = order

        cdef:
            list orders_bid
            list orders_ask
        if order.is_buy_c():
            orders_bid = self._orders_bid.get(order.instrument_id)
            if not orders_bid:
                orders_bid = []
                self._orders_bid[order.instrument_id] = orders_bid
            orders_bid.append(order)
            orders_bid.sort(key=lambda o: o.price if (o.type == OrderType.LIMIT or o.type == OrderType.MARKET_TO_LIMIT) or (o.type == OrderType.STOP_LIMIT and o.is_triggered) else o.trigger_price or INT_MIN, reverse=True)  # noqa  TODO(cs): Will refactor!
        elif order.is_sell_c():
            orders_ask = self._orders_ask.get(order.instrument_id)
            if not orders_ask:
                orders_ask = []
                self._orders_ask[order.instrument_id] = orders_ask
            orders_ask.append(order)
            orders_ask.sort(key=lambda o: o.price if (o.type == OrderType.LIMIT or o.type == OrderType.MARKET_TO_LIMIT) or (o.type == OrderType.STOP_LIMIT and o.is_triggered) else o.trigger_price or INT_MAX)  # noqa  TODO(cs): Will refactor!

    cdef void _delete_order(self, Order order) except *:
        self._order_index.pop(order.client_order_id, None)

        cdef:
            list orders_bid
            list orders_ask
        if order.is_buy_c():
            orders_bid = self._orders_bid.get(order.instrument_id)
            if orders_bid:
                orders_bid.remove(order)
        elif order.is_sell_c():
            orders_ask = self._orders_ask.get(order.instrument_id)
            if orders_ask:
                orders_ask.remove(order)

    cdef void _iterate_matching_engine(
        self, InstrumentId instrument_id,
        uint64_t timestamp_ns,
    ) except *:
        # Iterate bids
        cdef list orders_bid = self._orders_bid.get(instrument_id)
        if orders_bid:
            self._iterate_side(orders_bid.copy(), timestamp_ns)  # Copy list for safe loop

        # Iterate asks
        cdef list orders_ask = self._orders_ask.get(instrument_id)
        if orders_ask:
            self._iterate_side(orders_ask.copy(), timestamp_ns)  # Copy list for safe loop

    cdef void _iterate_side(self, list orders, uint64_t timestamp_ns) except *:
        cdef Price
        cdef Order order
        for order in orders:
            if not order.is_open_c():
                continue  # Orders state has changed since the loop started
            elif order.expire_time_ns > 0 and timestamp_ns >= order.expire_time_ns:
                self._delete_order(order)
                self._expire_order(order)
                continue
            # Check for order match
            self._match_order(order)

            if order.is_open_c() and (order.type == OrderType.TRAILING_STOP_MARKET or order.type == OrderType.TRAILING_STOP_LIMIT):
                self._manage_trailing_stop(order)

    cdef void _match_order(self, Order order) except *:
        if order.type == OrderType.LIMIT or order.type == OrderType.MARKET_TO_LIMIT:
            self._match_limit_order(order)
        elif (
            order.type == OrderType.STOP_MARKET
            or order.type == OrderType.MARKET_IF_TOUCHED
            or order.type == OrderType.TRAILING_STOP_MARKET
        ):
            self._match_stop_market_order(order)
        elif (
            order.type == OrderType.STOP_LIMIT
            or order.type == OrderType.LIMIT_IF_TOUCHED
            or order.type == OrderType.TRAILING_STOP_LIMIT
        ):
            self._match_stop_limit_order(order)
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderType, was {order.type}")

    cdef void _match_limit_order(self, LimitOrder order) except *:
        if self._is_limit_matched(order.instrument_id, order.side, order.price):
            self._fill_limit_order(order, LiquiditySide.MAKER)

    cdef void _match_stop_market_order(self, Order order) except *:
        if self._is_stop_triggered(order.instrument_id, order.side, order.trigger_price):
            # Triggered stop places market order
            self._fill_market_order(order, LiquiditySide.TAKER)

    cdef void _match_stop_limit_order(self, Order order) except *:
        if order.is_triggered:
            if self._is_limit_matched(order.instrument_id, order.side, order.price):
                self._fill_limit_order(order, LiquiditySide.MAKER)
            return

        if self._is_stop_triggered(order.instrument_id, order.side, order.trigger_price):
            self._generate_order_triggered(order)
            # Check for immediate fill
            if not self._is_limit_marketable(order.instrument_id, order.side, order.price):
                return

            if order.is_post_only:  # Would be liquidity taker
                self._delete_order(order)  # Remove order from open orders
                self._generate_order_rejected(
                    order,
                    f"POST_ONLY {order.type_string_c()} {order.side_string_c()} order "
                    f"limit px of {order.price} would have been a TAKER: "
                    f"bid={self.best_bid_price(order.instrument_id)}, "
                    f"ask={self.best_ask_price(order.instrument_id)}",
                )
            else:
                self._fill_limit_order(order, LiquiditySide.TAKER)  # Fills as TAKER

    cdef bint _is_limit_marketable(self, InstrumentId instrument_id, OrderSide side, Price order_price) except *:
        cdef Price bid
        cdef Price ask
        if side == OrderSide.BUY:
            ask = self.best_ask_price(instrument_id)
            if ask is None:
                return False  # No market
            return order_price._mem.raw >= ask._mem.raw  # Match with LIMIT sells
        elif side == OrderSide.SELL:
            bid = self.best_bid_price(instrument_id)
            if bid is None:  # No market
                return False
            return order_price._mem.raw <= bid._mem.raw  # Match with LIMIT buys
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderSide, was {side}")

    cdef bint _is_limit_matched(self, InstrumentId instrument_id, OrderSide side, Price price) except *:
        cdef Price bid
        cdef Price ask
        if side == OrderSide.BUY:
            ask = self.best_ask_price(instrument_id)
            if ask is None:
                return False  # No market
            return price._mem.raw > ask._mem.raw or (ask._mem.raw == price._mem.raw and self.fill_model.is_limit_filled())
        elif side == OrderSide.SELL:
            bid = self.best_bid_price(instrument_id)
            if bid is None:
                return False  # No market
            return price._mem.raw < bid._mem.raw or (bid._mem.raw == price._mem.raw and self.fill_model.is_limit_filled())
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderSide, was {side}")

    cdef bint _is_stop_marketable(self, InstrumentId instrument_id, OrderSide side, Price price) except *:
        cdef Price bid
        cdef Price ask
        if side == OrderSide.BUY:
            ask = self.best_ask_price(instrument_id)
            if ask is None:
                return False  # No market
            return ask._mem.raw >= price._mem.raw  # Match with LIMIT sells
        elif side == OrderSide.SELL:
            bid = self.best_bid_price(instrument_id)
            if bid is None:
                return False  # No market
            return bid._mem.raw <= price._mem.raw  # Match with LIMIT buys
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderSide, was {side}")

    cdef bint _is_stop_triggered(self, InstrumentId instrument_id, OrderSide side, Price price) except *:
        cdef Price bid
        cdef Price ask
        if side == OrderSide.BUY:
            ask = self.best_ask_price(instrument_id)
            if ask is None:
                return False  # No market
            return ask._mem.raw > price._mem.raw or (ask._mem.raw == price._mem.raw and self.fill_model.is_stop_filled())
        elif side == OrderSide.SELL:
            bid = self.best_bid_price(instrument_id)
            if bid is None:
                return False  # No market
            return bid._mem.raw < price._mem.raw or (bid._mem.raw == price._mem.raw and self.fill_model.is_stop_filled())
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderSide, was {side}")

    cdef list _determine_limit_price_and_volume(self, Order order):
        if self._bar_execution:
            if order.is_buy_c():
                self._last_bids[order.instrument_id] = order.price
            elif order.is_sell_c():
                self._last_asks[order.instrument_id] = order.price
            self._last[order.instrument_id] = order.price
            return [(order.price, order.leaves_qty)]
        cdef OrderBook book = self.get_book(order.instrument_id)
        cdef OrderBookOrder submit_order = OrderBookOrder(price=order.price, size=order.leaves_qty, side=order.side)
        if order.is_buy_c():
            return book.asks.simulate_order_fills(order=submit_order, depth_type=DepthType.VOLUME)
        elif order.is_sell_c():
            return book.bids.simulate_order_fills(order=submit_order, depth_type=DepthType.VOLUME)

    cdef list _determine_market_price_and_volume(self, Order order):
        cdef Price price
        if self._bar_execution:
            if order.type == OrderType.MARKET or order.type == OrderType.MARKET_IF_TOUCHED:
                if order.is_buy_c():
                    price = self._last_asks.get(order.instrument_id)
                    if price is None:
                        price = self.best_ask_price(order.instrument_id)
                    self._last[order.instrument_id] = price
                    if price is not None:
                        return [(price, order.leaves_qty)]
                    else:  # pragma: no cover (design-time error)
                        raise RuntimeError(
                            "Market best ASK price was None when filling MARKET order",
                        )
                elif order.is_sell_c():
                    price = self._last_bids.get(order.instrument_id)
                    if price is None:
                        price = self.best_bid_price(order.instrument_id)
                    self._last[order.instrument_id] = price
                    if price is not None:
                        return [(price, order.leaves_qty)]
                    else:  # pragma: no cover (design-time error)
                        raise RuntimeError(
                            "Market best BID price was None when filling MARKET order",
                        )
            else:
                price = order.price if order.type == OrderType.LIMIT else order.trigger_price
                if order.is_buy_c():
                    self._last_asks[order.instrument_id] = price
                elif order.is_sell_c():
                    self._last_bids[order.instrument_id] = price
                self._last[order.instrument_id] = price
                return [(price, order.leaves_qty)]
        price = Price.from_int_c(INT_MAX if order.side == OrderSide.BUY else INT_MIN)
        cdef OrderBookOrder submit_order = OrderBookOrder(price=price, size=order.leaves_qty, side=order.side)
        cdef OrderBook book = self.get_book(order.instrument_id)
        if order.is_buy_c():
            return book.asks.simulate_order_fills(order=submit_order)
        elif order.is_sell_c():
            return book.bids.simulate_order_fills(order=submit_order)

    cdef void _fill_market_order(self, Order order, LiquiditySide liquidity_side) except *:
        cdef PositionId position_id = self._get_position_id(order)
        cdef Position position = None
        if position_id is not None:
            position = self.cache.position(position_id)
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
            position_id=position_id,
            position=position,
        )

    cdef void _fill_limit_order(self, Order order, LiquiditySide liquidity_side) except *:
        cdef PositionId position_id = self._get_position_id(order)
        cdef Position position = None
        if position_id is not None:
            position = self.cache.position(position_id)
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
            position_id=position_id,
            position=position,
        )

    cdef void _apply_fills(
        self,
        Order order,
        LiquiditySide liquidity_side,
        list fills,
        PositionId position_id,
        Position position,
    ) except *:
        if not fills:
            return  # No fills

        if not self._log.is_bypassed:
            self._log.debug(
                f"Applying fills to {order}, "
                f"position_id={position_id}, "
                f"position={position}, "
                f"fills={fills}.",
            )

        cdef Instrument instrument = self.instruments[order.instrument_id]

        cdef:
            uint64_t raw_org_qty
            uint64_t raw_adj_qty
            Price fill_px
            Quantity fill_qty
            Quantity updated_qty
        for fill_px, fill_qty in fills:
            if order.filled_qty._mem.raw == 0:
                if order.type == OrderType.MARKET_TO_LIMIT:
                    self._generate_order_updated(order, qty=order.quantity, price=fill_px, trigger_price=None)
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
            if self.book_type == BookType.L1_TBBO and self.fill_model.is_slipped():
                if order.side == OrderSide.BUY:
                    fill_px = fill_px.add(instrument.price_increment)
                elif order.side == OrderSide.SELL:
                    fill_px = fill_px.sub(instrument.price_increment)
                else:  # pragma: no cover (design-time error)
                    raise ValueError(f"invalid OrderSide, was {order.side}")
            if order.is_reduce_only and fill_qty._mem.raw > position.quantity._mem.raw:
                # Adjust fill to honor reduce only execution
                raw_org_qty = fill_qty._mem.raw
                raw_adj_qty = fill_qty._mem.raw - (fill_qty._mem.raw - position.quantity._mem.raw)
                fill_qty = Quantity.from_raw_c(raw_adj_qty, fill_qty._mem.precision)
                updated_qty = Quantity.from_raw_c(order.quantity._mem.raw - (raw_org_qty - raw_adj_qty), fill_qty._mem.precision)
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
                instrument=instrument,
                order=order,
                venue_position_id=position_id,
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
                or order.type == OrderType.MARKET_TO_LIMIT
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
                fill_px = fill_px.add(instrument.price_increment)
            elif order.side == OrderSide.SELL:
                fill_px = fill_px.sub(instrument.price_increment)
            else:  # pragma: no cover (design-time error)
                raise ValueError(f"invalid OrderSide, was {order.side}")

            self._fill_order(
                instrument=instrument,
                order=order,
                venue_position_id=position_id,
                position=position,
                last_qty=order.leaves_qty,
                last_px=fill_px,
                liquidity_side=liquidity_side,
            )

    cdef void _fill_order(
        self,
        Instrument instrument,
        Order order,
        PositionId venue_position_id,
        Position position: Optional[Position],
        Quantity last_qty,
        Price last_px,
        LiquiditySide liquidity_side,
    ) except *:
        # Calculate commission
        cdef Money commission = self.exec_client.get_account().calculate_commission(
            instrument=instrument,
            last_qty=order.quantity,
            last_px=last_px,
            liquidity_side=liquidity_side,
        )

        self._generate_order_filled(
            order=order,
            venue_position_id=None if self.oms_type == OMSType.NETTING else venue_position_id,
            last_qty=last_qty,
            last_px=last_px,
            quote_currency=instrument.quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
        )

        if order.is_passive_c() and order.is_closed_c():
            # Remove order from market
            self._delete_order(order)

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
                        venue=self.id,
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
            return

        # Check reduce only orders for position
        for order in self.cache.orders_for_position(venue_position_id):
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
            Price last
            Price bid
            Price ask
            Price temp_trigger_price
            Price temp_price
        if (
            order.trigger_type == TriggerType.DEFAULT
            or order.trigger_type == TriggerType.LAST
            or order.trigger_type == TriggerType.MARK
        ):
            last = self._last.get(order.instrument_id)
            if last is None:
                raise RuntimeError(
                    f"cannot process trailing stop, "
                    f"no LAST price for {order.instrument_id} "
                    f"(please add trade ticks or use bars)",
                )
            if order.is_buy_c():
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw > temp_price._mem.raw:
                        new_price = temp_price
            elif order.is_sell_c():
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        last=last,
                    )
                    if price is None or price._mem.raw < temp_price._mem.raw:
                        new_price = temp_price
        elif order.trigger_type == TriggerType.BID_ASK:
            bid = self.best_bid_price(order.instrument_id)
            ask = self.best_ask_price(order.instrument_id)

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

            if order.is_buy_c():
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
            elif order.is_sell_c():
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
            last = self._last.get(order.instrument_id)
            bid = self.best_bid_price(order.instrument_id)
            ask = self.best_ask_price(order.instrument_id)

            if last is None:
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

            if order.is_buy_c():
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw > temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                    trigger_price = new_trigger_price  # Set trigger to new trigger
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        last=last,
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
            elif order.is_sell_c():
                temp_trigger_price = self._calculate_new_trailing_price_last(
                    order=order,
                    trailing_offset_type=order.trailing_offset_type,
                    offset=float(order.trailing_offset),
                    last=last,
                )
                if trigger_price is None or trigger_price._mem.raw < temp_trigger_price._mem.raw:
                    new_trigger_price = temp_trigger_price
                    trigger_price = new_trigger_price  # Set trigger to new trigger
                if order.type == OrderType.TRAILING_STOP_LIMIT:
                    temp_price = self._calculate_new_trailing_price_last(
                        order=order,
                        trailing_offset_type=order.trailing_offset_type,
                        offset=float(order.limit_offset),
                        last=last,
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
                f"TriggerType.{TriggerTypeParser.to_str(order.trigger_type)} "
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
        if trailing_offset_type == TrailingOffsetType.DEFAULT or trailing_offset_type == TrailingOffsetType.PRICE:
            if order.is_buy_c():
                return Price(last.as_f64_c() + offset, precision=last._mem.precision)
            elif order.is_sell_c():
                return Price(last.as_f64_c() - offset, precision=last._mem.precision)
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            if order.is_buy_c():
                offset = last.as_f64_c() * (offset / 100) / 100
                return Price(last.as_f64_c() + offset, precision=last._mem.precision)
            elif order.is_sell_c():
                offset = last.as_f64_c() * (offset / 100) / 100
                return Price(last.as_f64_c() - offset, precision=last._mem.precision)
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"TrailingOffsetType.{TrailingOffsetTypeParser.to_str(trailing_offset_type)} "
                f"not currently supported",
            )

    cdef Price _calculate_new_trailing_price_bid_ask(
        self,
        Order order,
        TrailingOffsetType trailing_offset_type,
        double offset,
        Price bid,
        Price ask,
    ):
        if trailing_offset_type == TrailingOffsetType.DEFAULT or trailing_offset_type == TrailingOffsetType.PRICE:
            if order.is_buy_c():
                return Price(ask.as_f64_c() + offset, precision=ask._mem.precision)
            elif order.is_sell_c():
                return Price(bid.as_f64_c() - offset, precision=bid._mem.precision)
        elif trailing_offset_type == TrailingOffsetType.BASIS_POINTS:
            if order.is_buy_c():
                offset = ask.as_f64_c() * (offset / 100) / 100
                return Price(ask.as_f64_c() + offset, precision=ask._mem.precision)
            elif order.is_sell_c():
                offset = bid.as_f64_c() * (offset / 100) / 100
                return Price(bid.as_f64_c() - offset, precision=bid._mem.precision)
        else:
            raise RuntimeError(
                f"cannot process trailing stop, "
                f"TrailingOffsetType.{TrailingOffsetTypeParser.to_str(trailing_offset_type)} "
                f"not currently supported",
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
                return self._generate_venue_position_id(order.instrument_id)
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

    cdef PositionId _generate_venue_position_id(self, InstrumentId instrument_id):
        cdef int pos_count = self._symbol_pos_count.get(instrument_id, 0)
        pos_count += 1
        self._symbol_pos_count[instrument_id] = pos_count
        return PositionId(f"{self.id.value}-{self._instrument_indexer[instrument_id]}-{pos_count:03d}")

    cdef VenueOrderId _generate_venue_order_id(self, InstrumentId instrument_id):
        cdef int ord_count = self._symbol_ord_count.get(instrument_id, 0)
        ord_count += 1
        self._symbol_ord_count[instrument_id] = ord_count
        return VenueOrderId(f"{self.id.value}-{self._instrument_indexer[instrument_id]}-{ord_count:03d}")

    cdef TradeId _generate_trade_id(self):
        self._executions_count += 1
        return TradeId(f"{self.id.value}-{self._executions_count}")

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_fresh_account_state(self) except *:
        cdef list balances = [
            AccountBalance(
                total=money,
                locked=Money(0, money.currency),
                free=money,
            )
            for money in self.starting_balances
        ]

        self.exec_client.generate_account_state(
            balances=balances,
            margins=[],
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        # Set leverages
        cdef Account account = self.get_account()
        if account.is_margin_account:
            account.set_default_leverage(self.default_leverage)
            # Set instrument specific leverages
            for instrument_id, leverage in self.leverages.items():
                account.set_leverage(instrument_id, leverage)

    cdef void _generate_order_rejected(self, Order order, str reason) except *:
        self.exec_client.generate_order_rejected(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            ts_event=self._clock.timestamp_ns(),
        )

    cdef void _generate_order_accepted(self, Order order) except *:
        self.exec_client.generate_order_accepted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=self._generate_venue_order_id(order.instrument_id),
            ts_event=self._clock.timestamp_ns(),
        )

    cdef void _generate_order_pending_update(self, Order order) except *:
        self.exec_client.generate_order_pending_update(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

    cdef void _generate_order_pending_cancel(self, Order order) except *:
        self.exec_client.generate_order_pending_cancel(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

    cdef void _generate_order_modify_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    ) except *:
        self.exec_client.generate_order_modify_rejected(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            reason=reason,
            ts_event=self._clock.timestamp_ns(),
        )

    cdef void _generate_order_cancel_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    ) except *:
        self.exec_client.generate_order_cancel_rejected(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            reason=reason,
            ts_event=self._clock.timestamp_ns(),
        )

    cdef void _generate_order_updated(
        self,
        Order order,
        Quantity qty,
        Price price,
        Price trigger_price,
    ) except *:
        cdef VenueOrderId venue_order_id = order.venue_order_id
        cdef bint venue_order_id_modified = False
        if venue_order_id is None:
            venue_order_id = self._generate_venue_order_id(order.instrument_id)
            venue_order_id_modified = True

        self.exec_client.generate_order_updated(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            quantity=qty,
            price=price,
            trigger_price=trigger_price,
            ts_event=self._clock.timestamp_ns(),
            venue_order_id_modified=venue_order_id_modified,
        )

    cdef void _generate_order_canceled(self, Order order) except *:
        self.exec_client.generate_order_canceled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

    cdef void _generate_order_triggered(self, Order order) except *:
        self.exec_client.generate_order_triggered(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

    cdef void _generate_order_expired(self, Order order) except *:
        self.exec_client.generate_order_expired(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_event=order.expire_time_ns,
        )

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
        cdef VenueOrderId venue_order_id = order.venue_order_id
        if venue_order_id is None:
            venue_order_id = self._generate_venue_order_id(order.instrument_id)
        self.exec_client.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=venue_position_id,
            trade_id=self._generate_trade_id(),
            order_side=order.side,
            order_type=order.type,
            last_qty=last_qty,
            last_px=last_px,
            quote_currency=quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            ts_event=self._clock.timestamp_ns(),
        )
