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

from decimal import Decimal

from libc.limits cimport INT_MAX
from libc.limits cimport INT_MIN
from libc.stdint cimport int64_t

from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.model.c_enums.depth_type cimport DepthType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.oms_type cimport OMSTypeParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.order cimport Order as OrderBookOrder
from nautilus_trader.model.orders.base cimport PassiveOrder
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.trading.calculators cimport ExchangeRateCalculator


cdef class SimulatedExchange:
    """
    Provides a simulated financial market exchange.
    """

    def __init__(
        self,
        Venue venue not None,
        OMSType oms_type,
        bint is_frozen_account,
        list starting_balances not None,
        list instruments not None,
        list modules not None,
        ExecutionCache exec_cache not None,
        FillModel fill_model not None,
        TestClock clock not None,
        Logger logger not None,
        OrderBookLevel exchange_order_book_level=OrderBookLevel.L1,
    ):
        """
        Initialize a new instance of the `SimulatedExchange` class.

        Parameters
        ----------
        venue : Venue
            The venue to simulate for the backtest.
        oms_type : OMSType
            The order management system type used by the exchange (HEDGING or NETTING).
        is_frozen_account : bool
            If the account for this exchange is frozen (balances will not change).
        starting_balances : list[Money]
            The starting balances for the exchange.
        exec_cache : ExecutionCache
            The execution cache for the backtest.
        fill_model : FillModel
            The fill model for the backtest.
        clock : TestClock
            The clock for the component.
        logger : Logger
            The logger for the component.

        Raises
        ------
        ValueError
            If instruments is empty.
        ValueError
            If instruments contains a type other than Instrument.
        ValueError
            If starting_balances is empty.
        ValueError
            If starting_balances contains a type other than Money.
        ValueError
            If modules contains a type other than SimulationModule.

        """
        Condition.not_empty(instruments, "instruments")
        Condition.list_type(instruments, Instrument, "instruments", "Instrument")
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(starting_balances, Money, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules", "SimulationModule")

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(
            component=f"{type(self).__name__}({venue})",
            logger=logger,
        )

        self.id = venue
        self.oms_type = oms_type
        self._log.info(f"OMSType={OMSTypeParser.to_str(oms_type)}")
        self.exchange_order_book_level = exchange_order_book_level

        self.exec_cache = exec_cache
        self.exec_client = None  # Initialized when execution client registered

        self.is_frozen_account = is_frozen_account
        self.starting_balances = starting_balances
        self.default_currency = None if len(starting_balances) > 1 else starting_balances[0].currency
        self.account_balances = {b.currency: b for b in starting_balances}
        self.account_balances_free = {b.currency: b for b in starting_balances}
        self.account_balances_locked = {b.currency: Money(0, b.currency) for b in starting_balances}
        self.total_commissions = {}  # type: dict[Currency, Money]

        self.xrate_calculator = ExchangeRateCalculator()
        self.fill_model = fill_model

        # Load modules
        self.modules = []
        for module in modules:
            Condition.not_in(module, self.modules, "module", "self._modules")
            module.register_exchange(self)
            self.modules.append(module)
            self._log.info(f"Loaded {module}.")

        # InstrumentId indexer for venue_order_ids
        self._instrument_indexer = {}  # type: dict[InstrumentId, int]

        # Load instruments
        self.instruments = {}
        for instrument in instruments:
            Condition.equal(instrument.venue, self.id, "instrument.venue", "self.id")
            self.instruments[instrument.id] = instrument
            index = len(self._instrument_indexer) + 1
            self._instrument_indexer[instrument.id] = index
            self._log.info(f"Loaded instrument {instrument.id.value}.")

        self._books = {}                # type: dict[InstrumentId, OrderBook]
        self._instrument_orders = {}    # type: dict[InstrumentId, dict[ClientOrderId, PassiveOrder]]
        self._working_orders = {}       # type: dict[ClientOrderId, PassiveOrder]
        self._position_index = {}       # type: dict[ClientOrderId, PositionId]
        self._child_orders = {}         # type: dict[ClientOrderId, list[Order]]
        self._oco_orders = {}           # type: dict[ClientOrderId, ClientOrderId]
        self._position_oco_orders = {}  # type: dict[PositionId, list[ClientOrderId]]
        self._symbol_pos_count = {}     # type: dict[InstrumentId, int]
        self._symbol_ord_count = {}     # type: dict[InstrumentId, int]
        self._executions_count = 0

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.id})"

    cpdef Price best_bid_price(self, InstrumentId instrument_id):
        """
        Return the best bid price for the given instrument identifier (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the price.

        Returns
        -------
        Price or None

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
        Return the best ask price for the given instrument identifier (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the price.

        Returns
        -------
        Price or None

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderBook order_book = self._books.get(instrument_id)
        if order_book is None:
            return None
        best_ask_price = order_book.best_ask_price()
        if best_ask_price is None:
            return None
        return Price(best_ask_price, order_book.price_precision)

    cpdef object get_xrate(
        self,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type,
    ):
        """
        Return the exchange rate for the given parameters.

        Parameters
        ----------
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.
        price_type : PriceType
            The price type to use for the calculation.

        Returns
        -------
        Decimal

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")
        return self.xrate_calculator.get_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type,
            bid_quotes=self._build_current_bid_rates(),
            ask_quotes=self._build_current_ask_rates(),
        )

    cpdef OrderBook get_book(self, InstrumentId instrument_id):
        """
        Return the order book for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the price.

        Returns
        -------
        OrderBook

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Instrument instrument
        cdef OrderBook book = self._books.get(instrument_id)
        if book is None:
            instrument = self.instruments.get(instrument_id)
            if instrument is None:
                raise RuntimeError(f"Cannot create OrderBook: "
                                   f"no instrument for {instrument_id.value}")
            book = OrderBook.create(
                instrument=instrument,
                level=self.exchange_order_book_level,
            )
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

    cpdef dict get_working_orders(self):
        """
        Return the working orders inside the exchange.

        Returns
        -------
        dict[ClientOrderId, Order]

        """
        return self._working_orders.copy()

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
        self._generate_account_event()

        self._log.info(f"Registered {client}.")

    cpdef void set_fill_model(self, FillModel fill_model) except *:
        """
        Set the fill model to the given model.

        fill_model : FillModel
            The fill model to set.

        """
        Condition.not_none(fill_model, "fill_model")

        self.fill_model = fill_model

        self._log.info("Changed fill model.")

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

        balance = self.account_balances[adjustment.currency]
        self.account_balances[adjustment.currency] = Money(balance + adjustment, adjustment.currency)

        # Generate and handle event
        self._generate_account_event()

    cpdef void process_order_book(self, OrderBookData data) except *:
        """
        Process the exchanges market for the given order book data.

        Parameters
        ----------
        data : OrderBookData
            The order book data to process.

        """
        Condition.not_none(data, "data")

        self._clock.set_time(data.timestamp_ns)
        self.get_book(data.instrument_id).apply(data)

        self._iterate_matching_engine(
            data.instrument_id,
            data.timestamp_ns,
        )

    cpdef void process_tick(self, Tick tick) except *:
        """
        Process the exchanges market for the given tick.

        Market dynamics are simulated by auctioning working orders.

        Parameters
        ----------
        tick : Tick
            The tick to process.

        """
        Condition.not_none(tick, "tick")

        self._clock.set_time(tick.timestamp_ns)

        cdef OrderBook book = self.get_book(tick.instrument_id)
        if book.level == OrderBookLevel.L1:
            book.update_top(tick)

        self._iterate_matching_engine(
            tick.instrument_id,
            tick.timestamp_ns,
        )

    cpdef void process_modules(self, int64_t now_ns) except *:
        """
        Process the simulation modules by advancing their time.

        Parameters
        ----------
        now_ns : int64
            The Unix timestamp (nanos) now.

        """
        self._clock.set_time(now_ns)

        # Iterate through modules
        cdef SimulationModule module
        for module in self.modules:
            module.process(now_ns)

    cpdef void check_residuals(self) except *:
        """
        Check for any residual objects and log warnings if any are found.
        """
        self._log.debug("Checking residuals...")

        for order_list in self._child_orders.values():
            for order in order_list:
                self._log.warning(f"Residual child-order {order}")

        for order_id in self._oco_orders.values():
            self._log.warning(f"Residual OCO {order_id}")

    cpdef void reset(self) except *:
        """
        Reset the simulated exchange.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        for module in self.modules:
            module.reset()

        self.account_balances = {b.currency: b for b in self.starting_balances}
        self.account_balances_free = {b.currency: b for b in self.starting_balances}
        self.account_balances_locked = {b.currency: Money(0, b.currency) for b in self.starting_balances}
        self.total_commissions = {}

        self._generate_account_event()

        self._books.clear()
        self._instrument_orders.clear()
        self._working_orders.clear()
        self._position_index.clear()
        self._child_orders.clear()
        self._oco_orders.clear()
        self._position_oco_orders.clear()
        self._symbol_pos_count.clear()
        self._symbol_ord_count.clear()
        self._executions_count = 0

        self._log.info("Reset.")

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void handle_submit_order(self, SubmitOrder command) except *:
        Condition.not_none(command, "command")

        if command.position_id.not_null():
            self._position_index[command.order.client_order_id] = command.position_id

        self._generate_order_submitted(command.order)
        self._process_order(command.order)

    cpdef void handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        Condition.not_none(command, "command")

        cdef PositionId position_id = self._generate_position_id(command.bracket_order.entry.instrument_id)

        cdef list bracket_orders = [command.bracket_order.stop_loss]
        self._position_oco_orders[position_id] = []
        if command.bracket_order.take_profit is not None:
            bracket_orders.append(command.bracket_order.take_profit)
            self._oco_orders[command.bracket_order.take_profit.client_order_id] = command.bracket_order.stop_loss.client_order_id
            self._oco_orders[command.bracket_order.stop_loss.client_order_id] = command.bracket_order.take_profit.client_order_id
            self._position_oco_orders[position_id].append(command.bracket_order.take_profit)

        self._child_orders[command.bracket_order.entry.client_order_id] = bracket_orders
        self._position_oco_orders[position_id].append(command.bracket_order.stop_loss)

        self._generate_order_submitted(command.bracket_order.entry)
        self._generate_order_submitted(command.bracket_order.stop_loss)
        if command.bracket_order.take_profit is not None:
            self._generate_order_submitted(command.bracket_order.take_profit)

        self._process_order(command.bracket_order.entry)

    cpdef void handle_cancel_order(self, CancelOrder command) except *:
        Condition.not_none(command, "command")

        cdef PassiveOrder order = self._working_orders.pop(command.client_order_id, None)
        if order is None:
            self._generate_order_cancel_rejected(
                command.client_order_id,
                "cancel order",
                f"{repr(command.client_order_id)} not found",
            )
        else:
            self._cancel_order(order)

    cpdef void handle_update_order(self, UpdateOrder command) except *:
        Condition.not_none(command, "command")

        cdef PassiveOrder order = self._working_orders.get(command.client_order_id)
        if order is None:
            self._generate_order_update_rejected(
                command.client_order_id,
                "update order",
                f"{repr(command.client_order_id)} not found",
            )
        else:
            self._update_order(order, command.quantity, command.price)

# --------------------------------------------------------------------------------------------------

    cdef inline dict _build_current_bid_rates(self):
        return {
            instrument_id.symbol.value: Decimal(f"{book.best_bid_price():.{book.price_precision}f}")
            for instrument_id, book in self._books.items() if book.best_bid_price()
        }

    cdef inline dict _build_current_ask_rates(self):
        return {
            instrument_id.symbol.value: Decimal(f"{book.best_ask_price():.{book.price_precision}f}")
            for instrument_id, book in self._books.items() if book.best_ask_price()
        }

    cdef inline PositionId _generate_position_id(self, InstrumentId instrument_id):
        cdef int pos_count = self._symbol_pos_count.get(instrument_id, 0)
        pos_count += 1
        self._symbol_pos_count[instrument_id] = pos_count
        return PositionId(f"{self._instrument_indexer[instrument_id]}-{pos_count:03d}")

    cdef inline VenueOrderId _generate_venue_order_id(self, InstrumentId instrument_id):
        cdef int ord_count = self._symbol_ord_count.get(instrument_id, 0)
        ord_count += 1
        self._symbol_ord_count[instrument_id] = ord_count
        return VenueOrderId(f"{self._instrument_indexer[instrument_id]}-{ord_count:03d}")

    cdef inline ExecutionId _generate_execution_id(self):
        self._executions_count += 1
        return ExecutionId(f"{self._executions_count}")

# -- EVENT HANDLING --------------------------------------------------------------------------------

    cdef inline void _reject_order(self, Order order, str reason) except *:
        # Generate event
        self._generate_order_rejected(order, reason)
        self._check_oco_order(order.client_order_id)
        self._clean_up_child_orders(order.client_order_id)

    cdef inline void _update_order(self, PassiveOrder order, Quantity qty, Price price) except *:
        if qty <= 0:
            self._generate_order_update_rejected(
                order.client_order_id,
                "update order",
                f"new quantity {qty} invalid",
            )
            return  # Cannot update order

        cdef Price bid = self.best_bid_price(order.instrument_id)  # Market must exist
        cdef Price ask = self.best_ask_price(order.instrument_id)  # Market must exist

        if order.type == OrderType.LIMIT:
            self._update_limit_order(order, qty, price)
        elif order.type == OrderType.STOP_MARKET:
            self._update_stop_market_order(order, qty, price)
        elif order.type == OrderType.STOP_LIMIT:
            self._update_stop_limit_order(order, qty, price)
        else:
            raise RuntimeError(f"Invalid order type")

    cdef inline void _cancel_order(self, PassiveOrder order) except *:
        cdef dict instrument_orders = self._instrument_orders.get(order.instrument_id)
        if instrument_orders is not None:
            # Assumption that order exists in instrument_orders
            # Will raise KeyError if not found by `pop`.
            instrument_orders.pop(order.client_order_id)

        self._generate_order_cancelled(order)
        self._check_oco_order(order.client_order_id)

    cdef inline void _expire_order(self, PassiveOrder order) except *:
        self._generate_order_expired(order)

        cdef ClientOrderId first_child_order_id
        cdef ClientOrderId other_oco_order_id
        if order.client_order_id in self._child_orders:
            # Remove any unprocessed OCO child orders
            first_child_order_id = self._child_orders[order.client_order_id][0].client_order_id
            if first_child_order_id in self._oco_orders:
                other_oco_order_id = self._oco_orders[first_child_order_id]
                del self._oco_orders[first_child_order_id]
                del self._oco_orders[other_oco_order_id]
        else:
            self._check_oco_order(order.client_order_id)
        self._clean_up_child_orders(order.client_order_id)

    cdef inline void _generate_account_event(self) except *:
        cdef dict info
        if self.default_currency is None:
            info = {}
        else:
            info = {"default_currency": self.default_currency.code}
        # Generate event
        self.exec_client.generate_account_state(
            balances=list(self.account_balances.values()),
            balances_free=list(self.account_balances_free.values()),
            balances_locked=list(self.account_balances_locked.values()),
            info=info,
        )

    cdef inline void _generate_order_submitted(self, Order order) except *:
        # Generate event
        self.exec_client.generate_order_submitted(
            client_order_id=order.client_order_id,
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _generate_order_accepted(self, Order order) except *:
        # Generate event
        self.exec_client.generate_order_accepted(
            client_order_id=order.client_order_id,
            venue_order_id=self._generate_venue_order_id(order.instrument_id),
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _generate_order_rejected(self, Order order, str reason) except *:
        # Generate event
        self.exec_client.generate_order_rejected(
            client_order_id=order.client_order_id,
            reason=reason,
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _generate_order_cancel_rejected(
        self,
        ClientOrderId client_order_id,
        str response,
        str reason,
    ) except *:
        # Generate event
        self.exec_client.generate_order_cancel_rejected(
            client_order_id=client_order_id,
            response=response,
            reason=reason,
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _generate_order_update_rejected(
        self,
        ClientOrderId client_order_id,
        str response,
        str reason,
    ) except *:
        # Generate event
        self.exec_client.generate_order_update_rejected(
            client_order_id=client_order_id,
            response=response,
            reason=reason,
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _generate_order_triggered(self, StopLimitOrder order) except *:
        # Generate event
        self.exec_client.generate_order_triggered(
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _generate_order_updated(
        self,
        PassiveOrder order,
        Quantity qty,
        Price price,
    ) except *:
        # Generate event
        self.exec_client.generate_order_updated(
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            quantity=qty,
            price=price,
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _generate_order_cancelled(self, PassiveOrder order) except *:
        # Generate event
        self.exec_client.generate_order_cancelled(
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _generate_order_expired(self, PassiveOrder order) except *:
        # Generate event
        self.exec_client.generate_order_expired(
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            timestamp_ns=order.expire_time_ns,
        )

    cdef inline void _process_order(self, Order order) except *:
        Condition.not_in(order.client_order_id, self._working_orders, "order.client_order_id", "working_orders")

        cdef Instrument instrument = self.instruments[order.instrument_id]

        # Check order size is valid or reject
        if instrument.max_quantity and order.quantity > instrument.max_quantity:
            self._reject_order(
                order,
                f"order quantity of {order.quantity} exceeds the "
                f"maximum trade size of {instrument.max_quantity}",
            )
            return  # Cannot accept order
        if instrument.min_quantity and order.quantity < instrument.min_quantity:
            self._reject_order(
                order,
                f"order quantity of {order.quantity} is less than the "
                f"minimum trade size of {instrument.min_quantity}",
            )
            return  # Cannot accept order

        if order.type == OrderType.MARKET:
            self._process_market_order(order)
        elif order.type == OrderType.LIMIT:
            self._process_limit_order(order)
        elif order.type == OrderType.STOP_MARKET:
            self._process_stop_market_order(order)
        elif order.type == OrderType.STOP_LIMIT:
            self._process_stop_limit_order(order)
        else:
            raise RuntimeError(f"Invalid order type")

    cdef inline void _process_market_order(self, MarketOrder order) except *:
        # Check market exists
        if order.side == OrderSide.BUY and not self.best_ask_price(order.instrument_id):
            self._reject_order(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not self.best_bid_price(order.instrument_id):
            self._reject_order(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        self._generate_order_accepted(order)

        # Immediately fill marketable order
        self._aggressively_fill_order(order, LiquiditySide.TAKER)

    cdef inline void _process_limit_order(self, LimitOrder order) except *:
        if order.is_post_only:
            if self._is_limit_marketable(order.instrument_id, order.side, order.price):
                self._reject_order(
                    order,
                    f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"limit px of {order.price} would have been a TAKER: "
                    f"bid={self.best_bid_price(order.instrument_id)}, "
                    f"ask={self.best_ask_price(order.instrument_id)}",
                )
                return  # Invalid price

        # Order is valid and accepted
        self._add_order(order)
        self._generate_order_accepted(order)

        # Check for immediate fill
        if not order.is_post_only and self._is_limit_matched(order.instrument_id, order.side, order.price):
            self._passively_fill_order(order, LiquiditySide.TAKER)  # Fills as liquidity taker

    cdef inline void _process_stop_market_order(self, StopMarketOrder order) except *:
        if self._is_stop_marketable(order.instrument_id, order.side, order.price):
            self._reject_order(
                order,
                f"STOP {OrderSideParser.to_str(order.side)} order "
                f"stop px of {order.price} was in the market: "
                f"bid={self.best_bid_price(order.instrument_id)}, "
                f"ask={self.best_ask_price(order.instrument_id)}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._add_order(order)
        self._generate_order_accepted(order)

    cdef inline void _process_stop_limit_order(self, StopLimitOrder order) except *:
        if self._is_stop_marketable(order.instrument_id, order.side, order.trigger):
            self._reject_order(
                order,
                f"STOP_LIMIT {OrderSideParser.to_str(order.side)} order "
                f"trigger stop px of {order.trigger} was in the market: "
                f"bid={self.best_bid_price(order.instrument_id)}, "
                f"ask={self.best_ask_price(order.instrument_id)}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._add_order(order)
        self._generate_order_accepted(order)

    cdef inline void _update_limit_order(
        self,
        LimitOrder order,
        Quantity qty,
        Price price,
    ) except *:
        if self._is_limit_marketable(order.instrument_id, order.side, price):
            if order.is_post_only:
                self._generate_order_update_rejected(
                    order.client_order_id,
                    "update order",
                    f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"new limit px of {price} would have been a TAKER: "
                    f"bid={self.best_bid_price(order.instrument_id)}, "
                    f"ask={self.best_ask_price(order.instrument_id)}",
                )
                return  # Cannot update order
            else:
                self._generate_order_updated(order, qty, price)
                self._passively_fill_order(order, LiquiditySide.TAKER)  # Immediate fill as TAKER
                return  # Filled

        self._generate_order_updated(order, qty, price)

    cdef inline void _update_stop_market_order(
        self,
        StopMarketOrder order,
        Quantity qty,
        Price price,
    ) except *:
        if self._is_stop_marketable(order.instrument_id, order.side, price):
            self._generate_order_update_rejected(
                order.client_order_id,
                "update order",
                f"STOP {OrderSideParser.to_str(order.side)} order "
                f"new stop px of {price} was in the market: "
                f"bid={self.best_bid_price(order.instrument_id)}, "
                f"ask={self.best_ask_price(order.instrument_id)}",
            )
            return  # Cannot update order

        self._generate_order_updated(order, qty, price)

    cdef inline void _update_stop_limit_order(
        self,
        StopLimitOrder order,
        Quantity qty,
        Price price,
    ) except *:
        if not order.is_triggered:
            # Amending stop price
            if self._is_stop_marketable(order.instrument_id, order.side, price):
                self.exec_client.generate_order_update_rejected(
                    order.client_order_id,
                    "update order",
                    f"STOP_LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"new stop px trigger of {price} was in the market: "
                    f"bid={self.best_bid_price(order.instrument_id)}, "
                    f"ask={self.best_ask_price(order.instrument_id)}",
                    self._clock.timestamp_ns(),
                )
                return  # Cannot update order
        else:
            # Amending limit price
            if self._is_limit_marketable(order.instrument_id, order.side, price):
                if order.is_post_only:
                    self.exec_client.generate_order_update_rejected(
                        order.client_order_id,
                        "update order",
                        f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order  "
                        f"new limit px of {price} would have been a TAKER: "
                        f"bid={self.best_bid_price(order.instrument_id)}, "
                        f"ask={self.best_ask_price(order.instrument_id)}",
                        self._clock.timestamp_ns(),
                    )
                    return  # Cannot update order
                else:
                    self._generate_order_updated(order, qty, price)
                    self._passively_fill_order(order, LiquiditySide.TAKER)  # Immediate fill as TAKER
                    return  # Filled

        self._generate_order_updated(order, qty, price)

# -- ORDER MATCHING ENGINE -------------------------------------------------------------------------

    cdef inline void _add_order(self, PassiveOrder order) except *:
        self._working_orders[order.client_order_id] = order
        cdef dict instrument_orders = self._instrument_orders.get(order.instrument_id)
        if instrument_orders is None:
            instrument_orders = {}
            self._instrument_orders[order.instrument_id] = instrument_orders
        instrument_orders[order.client_order_id] = order

    cdef inline void _delete_order(self, Order order) except *:
        self._working_orders.pop(order.client_order_id, None)
        cdef dict instrument_orders = self._instrument_orders.get(order.instrument_id)
        if instrument_orders is not None:
            instrument_orders.pop(order.client_order_id, None)

    cdef inline void _iterate_matching_engine(
        self, InstrumentId instrument_id,
        int64_t timestamp_ns,
    ) except *:
        cdef dict working_orders = self._instrument_orders.get(instrument_id)
        if working_orders is None:
            return  # No orders to iterate

        cdef PassiveOrder order
        for order in working_orders.copy().values():  # Copy dict for safe loop
            if not order.is_working_c():
                continue  # Orders state has changed since the loop started

            # Check for order match
            self._match_order(order)

            # Check for order expiry (if expire time then compare nanoseconds)
            if order.expire_time and timestamp_ns >= order.expire_time_ns:
                self._delete_order(order)
                self._expire_order(order)

    cdef inline void _match_order(self, PassiveOrder order) except *:
        if order.type == OrderType.LIMIT:
            self._match_limit_order(order)
        elif order.type == OrderType.STOP_MARKET:
            self._match_stop_market_order(order)
        elif order.type == OrderType.STOP_LIMIT:
            self._match_stop_limit_order(order)
        else:
            raise RuntimeError("invalid order type")

    cdef inline void _match_limit_order(self, LimitOrder order) except *:
        if self._is_limit_matched(order.instrument_id, order.side, order.price):
            self._passively_fill_order(order, LiquiditySide.MAKER)

    cdef inline void _match_stop_market_order(self, StopMarketOrder order) except *:
        if self._is_stop_triggered(order.instrument_id, order.side, order.price):
            self._aggressively_fill_order(order, LiquiditySide.TAKER)  # Triggered stop places market order

    cdef inline void _match_stop_limit_order(self, StopLimitOrder order) except *:
        if order.is_triggered:
            if self._is_limit_matched(order.instrument_id, order.side, order.price):
                self._passively_fill_order(order, LiquiditySide.MAKER)
        else:  # Order not triggered
            if self._is_stop_triggered(order.instrument_id, order.side, order.trigger):
                self._generate_order_triggered(order)

            # Check for immediate fill
            if not self._is_limit_marketable(order.instrument_id, order.side, order.price):
                return

            if order.is_post_only:  # Would be liquidity taker
                self._delete_order(order)  # Remove order from working orders
                self._reject_order(
                    order,
                    f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"limit px of {order.price} would have been a TAKER: "
                    f"bid={self.best_bid_price(order.instrument_id)}, "
                    f"ask={self.best_ask_price(order.instrument_id)}",
                )
            else:
                self._passively_fill_order(order, LiquiditySide.TAKER)  # Fills as TAKER

    cdef inline bint _is_limit_marketable(self, InstrumentId instrument_id, OrderSide side, Price order_price) except *:
        if side == OrderSide.BUY:
            ask = self.best_ask_price(instrument_id)
            if ask is None:
                return False  # No market
            return order_price >= ask  # Match with LIMIT sells
        else:  # => OrderSide.SELL
            bid = self.best_bid_price(instrument_id)
            if bid is None:  # No market
                return False
            return order_price <= bid  # Match with LIMIT buys

    cdef inline bint _is_limit_matched(self, InstrumentId instrument_id, OrderSide side, Price price) except *:
        if side == OrderSide.BUY:
            ask = self.best_ask_price(instrument_id)
            if ask is None:
                return False  # No market
            return price > ask or (ask == price and self.fill_model.is_limit_filled())
        else:  # => OrderSide.SELL
            bid = self.best_bid_price(instrument_id)
            if bid is None:
                return False  # No market
            return price < bid or (bid == price and self.fill_model.is_limit_filled())

    cdef inline bint _is_stop_marketable(self, InstrumentId instrument_id, OrderSide side, Price price) except *:
        if side == OrderSide.BUY:
            ask = self.best_ask_price(instrument_id)
            if ask is None:
                return False  # No market
            return ask >= price  # Match with LIMIT sells
        else:  # => OrderSide.SELL
            bid = self.best_bid_price(instrument_id)
            if bid is None:
                return False  # No market
            return bid <= price  # Match with LIMIT buys

    cdef inline bint _is_stop_triggered(self, InstrumentId instrument_id, OrderSide side, Price price) except *:
        if side == OrderSide.BUY:
            ask = self.best_ask_price(instrument_id)
            if ask is None:
                return False  # No market
            return ask > price or (ask == price and self.fill_model.is_stop_filled())
        else:  # => OrderSide.SELL
            bid = self.best_bid_price(instrument_id)
            if bid is None:
                return False  # No market
            return bid < price or (bid == price and self.fill_model.is_stop_filled())

    cdef inline list _determine_limit_price_and_volume(self, PassiveOrder order):
        cdef OrderBook book = self.get_book(order.instrument_id)
        cdef OrderBookOrder submit_order = OrderBookOrder(price=order.price, volume=order.quantity, side=order.side)

        if order.side == OrderSide.BUY:
            return book.asks.simulate_order_fills(order=submit_order, depth_type=DepthType.VOLUME)
        else:  # => OrderSide.SELL
            return book.bids.simulate_order_fills(order=submit_order, depth_type=DepthType.VOLUME)

    cdef inline list _determine_market_price_and_volume(self, Order order):
        cdef OrderBook book = self.get_book(order.instrument_id)
        cdef Price price = Price(INT_MAX if order.side == OrderSide.BUY else INT_MIN)
        cdef OrderBookOrder submit_order = OrderBookOrder(price=price, volume=order.quantity, side=order.side)

        if order.side == OrderSide.BUY:
            return book.asks.simulate_order_fills(order=submit_order)
        else:  # => OrderSide.SELL
            return book.bids.simulate_order_fills(order=submit_order)

# --------------------------------------------------------------------------------------------------

    cdef inline void _passively_fill_order(self, PassiveOrder order, LiquiditySide liquidity_side) except *:
        cdef list fills = self._determine_limit_price_and_volume(order)
        if not fills:
            return
        cdef Price fill_px
        cdef Quantity fill_qty
        for fill_px, fill_qty in fills:
            self._fill_order(
                order=order,
                last_px=fill_px,
                last_qty=fill_qty,
                liquidity_side=liquidity_side,
            )

    cdef inline void _aggressively_fill_order(self, Order order, LiquiditySide liquidity_side) except *:
        cdef list fills = self._determine_market_price_and_volume(order)
        if not fills:
            return
        cdef Price fill_px
        cdef Quantity fill_qty
        for fill_px, fill_qty in fills:
            if order.type == OrderType.STOP_MARKET:
                fill_px = order.price  # TODO: Temporary strategy for market moving through price
            if self.exchange_order_book_level == OrderBookLevel.L1 and self.fill_model.is_slipped():
                instrument = self.instruments[order.instrument_id]  # TODO: Pending refactoring
                if order.side == OrderSide.BUY:
                    fill_px = Price(fill_px + instrument.tick_size)
                else:  # => OrderSide.SELL
                    fill_px = Price(fill_px - instrument.tick_size)
            self._fill_order(
                order=order,
                last_px=fill_px,
                last_qty=fill_qty,
                liquidity_side=liquidity_side,
            )

        # TODO: For L1 fill remaining size at next tick price (temporary)
        if self.exchange_order_book_level == OrderBookLevel.L1 and order.is_working_c():
            fill_px = fills[-1][0]
            instrument = self.instruments[order.instrument_id]  # TODO: Pending refactoring
            if order.side == OrderSide.BUY:
                fill_px = Price(fill_px + instrument.tick_size)
            else:  # => OrderSide.SELL
                fill_px = Price(fill_px - instrument.tick_size)
            self._fill_order(
                order=order,
                last_px=fill_px,
                last_qty=Quantity(order.quantity - order.filled_qty),
                liquidity_side=liquidity_side,
            )

    cdef inline void _fill_order(
        self,
        Order order,
        Price last_px,
        Quantity last_qty,
        LiquiditySide liquidity_side,
    ) except *:
        self._delete_order(order)  # Remove order from working orders (if found)

        # Determine position (do not reorder below `generate_order_filled`) as
        # this will change the logic of what position identifier should be.
        cdef PositionId position_id = None
        if self.oms_type == OMSType.NETTING:
            position_id = PositionId.null_c()
        elif self.oms_type == OMSType.HEDGING:
            position_id = self.exec_cache.position_id(order.client_order_id)
            if position_id is None:
                # Set the filled position identifier
                position_id = self._generate_position_id(order.instrument_id)

        cdef Position position = None
        if position_id.not_null():
            position = self.exec_cache.position(position_id)

        # Calculate commission
        cdef Instrument instrument = self.instruments[order.instrument_id]
        cdef Money commission = instrument.calculate_commission(
            last_qty=order.quantity,
            last_px=last_px,
            liquidity_side=liquidity_side,
        )

        # Calculate potential PnL
        cdef Money pnl = None
        if position and position.entry != order.side:
            # Calculate PnL
            pnl = position.calculate_pnl(
                avg_px_open=position.avg_px_open,
                avg_px_close=last_px,
                quantity=order.quantity,
            )

        # Generate event
        self.exec_client.generate_order_filled(
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id if order.venue_order_id is not None else self._generate_venue_order_id(order.instrument_id),
            execution_id=self._generate_execution_id(),
            position_id=position_id,
            instrument_id=order.instrument_id,
            order_side=order.side,
            last_qty=last_qty,
            last_px=last_px,
            quote_currency=instrument.quote_currency,
            is_inverse=instrument.is_inverse,
            commission=commission,
            liquidity_side=liquidity_side,
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._check_oco_order(order.client_order_id)

        # Work any bracket child orders
        if order.client_order_id in self._child_orders:
            for child_order in self._child_orders[order.client_order_id]:
                if not child_order.is_completed:  # The order may already be cancelled or rejected
                    self._process_order(child_order)
            del self._child_orders[order.client_order_id]

        # Cancel any linked OCO orders
        if position and position.is_closed_c():
            oco_orders = self._position_oco_orders.get(position.id)
            if oco_orders:
                for order in self._position_oco_orders[position.id]:
                    if order.is_working_c():
                        self._log.debug(f"Cancelling {order.client_order_id} as linked position closed.")
                        self._cancel_oco_order(order)
                del self._position_oco_orders[position.id]

        # Settle account
        cdef Currency currency  # Settlement currency
        if self.default_currency:  # Single-asset account
            currency = self.default_currency
            if pnl is None:
                pnl = Money(0, currency)

            if commission.currency != currency:
                # Calculate exchange rate to account currency
                xrate: Decimal = self.get_xrate(
                    from_currency=commission.currency,
                    to_currency=currency,
                    price_type=PriceType.BID if order.side is OrderSide.SELL else PriceType.ASK,
                )

                # Convert to account currency
                commission = Money(commission * xrate, currency)
                pnl = Money(pnl * xrate, currency)

            # Final PnL
            pnl = Money(pnl - commission, self.default_currency)
        else:
            currency = instrument.settlement_currency
            if pnl is None:
                pnl = commission

        # Increment total commissions
        total_commissions: Decimal = self.total_commissions.get(currency, Decimal()) + commission
        self.total_commissions[currency] = Money(total_commissions, currency)

        # Finally adjust account
        self.adjust_account(pnl)

    cdef inline void _check_oco_order(self, ClientOrderId client_order_id) except *:
        # Check held OCO orders and remove any paired with the given client_order_id
        cdef ClientOrderId oco_client_order_id = self._oco_orders.pop(client_order_id, None)
        if oco_client_order_id is None:
            return  # No linked order

        del self._oco_orders[oco_client_order_id]
        cdef PassiveOrder oco_order = self._working_orders.pop(oco_client_order_id, None)
        if oco_order is None:
            return  # No linked order

        self._delete_order(oco_order)

        # Reject any latent bracket child orders first
        cdef list child_orders
        cdef PassiveOrder order
        for child_orders in self._child_orders.values():
            for order in child_orders:
                if oco_order == order and not order.is_working_c():
                    self._reject_oco_order(order, client_order_id)

        # Cancel working OCO order
        self._log.debug(f"Cancelling {oco_order.client_order_id} OCO order from {oco_client_order_id}.")
        self._cancel_oco_order(oco_order)

    cdef inline void _clean_up_child_orders(self, ClientOrderId client_order_id) except *:
        # Clean up any residual child orders from the completed order associated
        # with the given identifier.
        self._child_orders.pop(client_order_id, None)

    cdef inline void _reject_oco_order(self, PassiveOrder order, ClientOrderId other_oco) except *:
        # order is the OCO order to reject
        # other_oco is the linked ClientOrderId
        if order.is_completed_c():
            self._log.debug(f"Cannot reject order: state was already {order.state_string_c()}.")
            return

        # Generate event
        self.exec_client.generate_order_rejected(
            order.client_order_id,
            f"OCO order rejected from {other_oco}",
            self._clock.timestamp_ns(),
        )

    cdef inline void _cancel_oco_order(self, PassiveOrder order) except *:
        # order is the OCO order to cancel
        if order.is_completed_c():
            self._log.debug(f"Cannot cancel order: state was already {order.state_string_c()}.")
            return

        # Generate event
        self._generate_order_cancelled(order)
