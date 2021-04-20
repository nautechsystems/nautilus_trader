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

from libc.stdint cimport int64_t

from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.oms_type cimport OMSTypeParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelRejected
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderTriggered
from nautilus_trader.model.events cimport OrderUpdateRejected
from nautilus_trader.model.events cimport OrderUpdated
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
from nautilus_trader.model.order.base cimport PassiveOrder
from nautilus_trader.model.order.limit cimport LimitOrder
from nautilus_trader.model.order.market cimport MarketOrder
from nautilus_trader.model.order.stop_limit cimport StopLimitOrder
from nautilus_trader.model.order.stop_market cimport StopMarketOrder
from nautilus_trader.model.orderbook.book cimport L2OrderBook
from nautilus_trader.model.orderbook.book cimport OrderBookDeltas
from nautilus_trader.model.orderbook.book cimport OrderBookSnapshot
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.model.tick cimport TradeTick
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
    ):
        """
        Initialize a new instance of the `SimulatedExchange` class.

        Parameters
        ----------
        venue : Venue
            The venue to simulate for the backtest.
        oms_type : OMSType (Enum)
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

        self.exec_cache = exec_cache
        self.exec_client = None  # Initialized when execution client registered

        self.is_frozen_account = is_frozen_account
        self.starting_balances = starting_balances
        self.default_currency = None if len(starting_balances) > 1 else starting_balances[0].currency
        self.account_balances = {b.currency: b for b in starting_balances}
        self.account_balances_free = {b.currency: b for b in starting_balances}
        self.account_balances_locked = {b.currency: Money(0, b.currency) for b in starting_balances}
        self.total_commissions = {}

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

        self._slippages = self._get_tick_sizes()
        self._books = {}                # type: dict[InstrumentId, L2OrderBook]
        self._market_bids = {}          # type: dict[InstrumentId, Price]
        self._market_asks = {}          # type: dict[InstrumentId, Price]

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

        cdef AccountState initial_event = self._generate_account_event()
        self.exec_client.handle_event(initial_event)

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

    cpdef void initialize_account(self) except *:
        """
        Initialize the account by generating an `AccountState` event.
        """
        self.exec_client.handle_event(self._generate_account_event())

    cpdef void process_order_book(self, OrderBookData data) except *:
        """
        Process the exchanges market for the given snapshot.

        Parameters
        ----------
        data : OrderBookData
            The order book data process.

        """
        Condition.not_none(data, "data")

        self._clock.set_time(data.timestamp_ns)

        cdef InstrumentId instrument_id = data.instrument_id
        cdef Instrument instrument = self.instruments[instrument_id]

        cdef Price bid = None
        cdef Price ask = None
        cdef L2OrderBook order_book = None
        if isinstance(data, OrderBookSnapshot):
            if data.bids:
                bid = Price(data.bids[0], instrument.price_precision)
            if data.asks:
                ask = Price(data.asks[0], instrument.price_precision)
        elif isinstance(data, OrderBookDeltas):
            order_book = self._books.get(instrument_id)
            if order_book is None:
                order_book = L2OrderBook(
                    instrument_id=instrument_id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                )
                self._books[instrument_id] = order_book
            order_book.apply_deltas(data)
            if order_book.best_bid_price():
                bid = Price(order_book.best_bid_price(), instrument.price_precision)
            else:
                bid = None
            if order_book.best_ask_price():
                ask = Price(order_book.best_ask_price(), instrument.price_precision)
            else:
                ask = None

        self._market_bids[instrument_id] = bid
        self._market_asks[instrument_id] = ask
        # bid or ask could be None here

        self._iterate_matching_engine(
            instrument_id,
            bid,
            ask,
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

        cdef InstrumentId instrument_id = tick.instrument_id

        # Update market bid and ask
        cdef Price bid = None
        cdef Price ask = None
        if isinstance(tick, QuoteTick):
            bid = tick.bid
            ask = tick.ask
            self._market_bids[instrument_id] = bid
            self._market_asks[instrument_id] = ask
        elif isinstance(tick, TradeTick):
            if tick.side == OrderSide.SELL:  # TAKER hit the bid
                bid = tick.price
                ask = self._market_asks.get(instrument_id)
                if ask is None:
                    ask = bid  # Initialize ask
                self._market_bids[instrument_id] = bid
            elif tick.side == OrderSide.BUY:  # TAKER lifted the offer
                ask = tick.price
                bid = self._market_bids.get(instrument_id)
                if bid is None:
                    bid = ask  # Initialize bid
                self._market_asks[instrument_id] = ask
            # tick.side must be BUY or SELL (condition checked in TradeTick)
        else:
            raise RuntimeError("not market data")  # Design-time error

        self._iterate_matching_engine(
            tick.instrument_id,
            bid,
            ask,
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
        self._market_bids.clear()
        self._market_asks.clear()
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

        self._submit_order(command.order)
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

        self._submit_order(command.bracket_order.entry)
        self._submit_order(command.bracket_order.stop_loss)
        if command.bracket_order.take_profit is not None:
            self._submit_order(command.bracket_order.take_profit)

        self._process_order(command.bracket_order.entry)

    cpdef void handle_cancel_order(self, CancelOrder command) except *:
        Condition.not_none(command, "command")

        self._cancel_order(command.client_order_id)

    cpdef void handle_update_order(self, UpdateOrder command) except *:
        Condition.not_none(command, "command")

        self._update_order(command.client_order_id, command.quantity, command.price)

# --------------------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment) except *:
        Condition.not_none(adjustment, "adjustment")

        if self.is_frozen_account:
            return  # Nothing to adjust

        balance = self.account_balances[adjustment.currency]
        self.account_balances[adjustment.currency] = Money(balance + adjustment, adjustment.currency)

        # Generate and handle event
        self.exec_client.handle_event(self._generate_account_event())

    cdef inline Price get_current_bid(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        return self._market_bids.get(instrument_id)

    cdef inline Price get_current_ask(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        return self._market_asks.get(instrument_id)

    cdef inline object get_xrate(self, Currency from_currency, Currency to_currency, PriceType price_type):
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")

        return self.xrate_calculator.get_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type,
            bid_quotes=self._build_current_bid_rates(),
            ask_quotes=self._build_current_ask_rates(),
        )

    cdef inline dict _build_current_bid_rates(self):
        return {instrument_id.symbol.value: price.as_decimal() for instrument_id, price in self._market_bids.items()}

    cdef inline dict _build_current_ask_rates(self):
        return {instrument_id.symbol.value: price.as_decimal() for instrument_id, price in self._market_asks.items()}

# -- EVENT HANDLING --------------------------------------------------------------------------------

    cdef inline object _get_tick_sizes(self):
        cdef dict slippage_index = {}  # type: dict[InstrumentId, Decimal]

        for instrument_id, instrument in self.instruments.items():
            # noinspection PyUnresolvedReferences
            slippage_index[instrument_id] = instrument.tick_size

        return slippage_index

    cdef inline PositionId _generate_position_id(self, InstrumentId instrument_id):
        cdef int pos_count = self._symbol_pos_count.get(instrument_id, 0)
        pos_count += 1
        self._symbol_pos_count[instrument_id] = pos_count
        return PositionId(f"{self._instrument_indexer[instrument_id]}-{pos_count:03d}")

    cdef inline VenueOrderId _generate_order_id(self, InstrumentId instrument_id):
        cdef int ord_count = self._symbol_ord_count.get(instrument_id, 0)
        ord_count += 1
        self._symbol_ord_count[instrument_id] = ord_count
        return VenueOrderId(f"{self._instrument_indexer[instrument_id]}-{ord_count:03d}")

    cdef inline ExecutionId _generate_execution_id(self):
        self._executions_count += 1
        return ExecutionId(f"{self._executions_count}")

    cdef inline AccountState _generate_account_event(self):
        cdef dict info
        if self.default_currency is None:
            info = {}
        else:
            info = {"default_currency": self.default_currency.code}
        return AccountState(
            account_id=self.exec_client.account_id,
            balances=list(self.account_balances.values()),
            balances_free=list(self.account_balances_free.values()),
            balances_locked=list(self.account_balances_locked.values()),
            info=info,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

    cdef inline void _submit_order(self, Order order) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            self.exec_client.account_id,
            order.client_order_id,
            self._clock.timestamp_ns(),
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(submitted)

    cdef inline void _accept_order(self, Order order) except *:
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self.exec_client.account_id,
            order.client_order_id,
            self._generate_order_id(order.instrument_id),
            self._clock.timestamp_ns(),
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(accepted)

    cdef inline void _reject_order(self, Order order, str reason) except *:
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.exec_client.account_id,
            order.client_order_id,
            self._clock.timestamp_ns(),
            reason,
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(rejected)
        self._check_oco_order(order.client_order_id)
        self._clean_up_child_orders(order.client_order_id)

    cdef inline void _update_order(self, ClientOrderId client_order_id, Quantity qty, Price price) except *:
        cdef PassiveOrder order = self._working_orders.get(client_order_id)
        if order is None:
            self._reject_update(
                client_order_id,
                "update order",
                f"repr{client_order_id} not found",
            )
            return  # Cannot update order

        if qty <= 0:
            self._reject_update(
                order.client_order_id,
                "update order",
                f"new quantity {qty} invalid",
            )
            return  # Cannot update order

        cdef Price bid = self._market_bids[order.instrument_id]  # Market must exist
        cdef Price ask = self._market_asks[order.instrument_id]  # Market must exist

        if order.type == OrderType.LIMIT:
            self._update_limit_order(order, qty, price, bid, ask)
        elif order.type == OrderType.STOP_MARKET:
            self._update_stop_market_order(order, qty, price, bid, ask)
        elif order.type == OrderType.STOP_LIMIT:
            self._update_stop_limit_order(order, qty, price, bid, ask)
        else:
            raise RuntimeError(f"Invalid order type")

    cdef inline void _cancel_order(self, ClientOrderId client_order_id) except *:
        cdef PassiveOrder order = self._working_orders.pop(client_order_id, None)
        if order is None:
            self._reject_cancel(
                client_order_id,
                "cancel order",
                f"{repr(client_order_id)} not found",
            )
            return  # Rejected the cancel order command

        cdef dict instrument_orders = self._instrument_orders.get(order.instrument_id)
        if instrument_orders is not None:
            instrument_orders.pop(order.client_order_id)

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            order.account_id,
            order.client_order_id,
            order.venue_order_id,
            self._clock.timestamp_ns(),
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(cancelled)
        self._check_oco_order(order.client_order_id)

    cdef inline void _reject_cancel(
        self,
        ClientOrderId client_order_id,
        str response,
        str reason,
    ) except *:
        cdef Order order = self.exec_cache.order(client_order_id)
        if order is not None:
            venue_order_id = order.venue_order_id
        else:
            venue_order_id = VenueOrderId.null_c()

        # Generate event
        cdef OrderCancelRejected cancel_rejected = OrderCancelRejected(
            self.exec_client.account_id,
            client_order_id,
            venue_order_id,
            self._clock.timestamp_ns(),
            response,
            reason,
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(cancel_rejected)

    cdef inline void _reject_update(
        self,
        ClientOrderId client_order_id,
        str response,
        str reason,
    ) except *:
        cdef Order order = self.exec_cache.order(client_order_id)
        if order is not None:
            venue_order_id = order.venue_order_id
        else:
            venue_order_id = VenueOrderId.null_c()

        # Generate event
        cdef OrderUpdateRejected update_rejected = OrderUpdateRejected(
            self.exec_client.account_id,
            client_order_id,
            venue_order_id,
            self._clock.timestamp_ns(),
            response,
            reason,
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(update_rejected)

    cdef inline void _expire_order(self, PassiveOrder order) except *:
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            self.exec_client.account_id,
            order.client_order_id,
            order.venue_order_id,
            order.expire_time_ns,
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(expired)

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

    cdef inline void _trigger_order(self, StopLimitOrder order) except *:
        # Generate event
        cdef OrderTriggered triggered = OrderTriggered(
            self.exec_client.account_id,
            order.client_order_id,
            order.venue_order_id,
            self._clock.timestamp_ns(),
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(triggered)

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

        cdef Price bid = self._market_bids.get(order.instrument_id)
        cdef Price ask = self._market_asks.get(order.instrument_id)

        if order.type == OrderType.MARKET:
            self._process_market_order(order, bid, ask)
        elif order.type == OrderType.LIMIT:
            self._process_limit_order(order, bid, ask)
        elif order.type == OrderType.STOP_MARKET:
            self._process_stop_market_order(order, bid, ask)
        elif order.type == OrderType.STOP_LIMIT:
            self._process_stop_limit_order(order, bid, ask)
        else:
            raise RuntimeError(f"Invalid order type")

    cdef inline void _process_market_order(self, MarketOrder order, Price bid, Price ask) except *:
        # Check market exists
        if order.side == OrderSide.BUY and not ask:
            self._reject_order(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order
        elif order.side == OrderSide.SELL and not bid:
            self._reject_order(order, f"no market for {order.instrument_id}")
            return  # Cannot accept order

        self._accept_order(order)

        # Immediately fill marketable order
        self._fill_order(
            order=order,
            fill_px=self._fill_price_taker(order.instrument_id, order.side, bid, ask),
            liquidity_side=LiquiditySide.TAKER,
        )

    cdef inline void _process_limit_order(self, LimitOrder order, Price bid, Price ask) except *:
        if order.is_post_only:
            if self._is_limit_marketable(order.side, order.price, bid, ask):
                self._reject_order(
                    order,
                    f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"limit px of {order.price} would have been a TAKER: bid={bid}, ask={ask}",
                )
                return  # Invalid price

        # Order is valid and accepted
        self._add_order(order)
        self._accept_order(order)

        # Check for immediate fill
        cdef Price fill_px
        if not order.is_post_only and self._is_limit_marketable(order.side, order.price, bid, ask):
            fill_px = self._fill_price_maker(order.side, bid, ask)
            self._fill_order(
                order=order,
                fill_px=fill_px,
                liquidity_side=LiquiditySide.TAKER,
            )

    cdef inline void _process_stop_market_order(self, StopMarketOrder order, Price bid, Price ask) except *:
        if self._is_stop_marketable(order.side, order.price, bid, ask):
            self._reject_order(
                order,
                f"STOP {OrderSideParser.to_str(order.side)} order "
                f"stop px of {order.price} was in the market: bid={bid}, ask={ask}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._add_order(order)
        self._accept_order(order)

    cdef inline void _process_stop_limit_order(self, StopLimitOrder order, Price bid, Price ask) except *:
        if self._is_stop_marketable(order.side, order.trigger, bid, ask):
            self._reject_order(
                order,
                f"STOP_LIMIT {OrderSideParser.to_str(order.side)} order "
                f"trigger stop px of {order.trigger} was in the market: bid={bid}, ask={ask}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._add_order(order)
        self._accept_order(order)

    cdef inline void _update_limit_order(
        self,
        LimitOrder order,
        Quantity qty,
        Price price,
        Price bid,
        Price ask,
    ) except *:
        cdef Price fill_px
        if self._is_limit_marketable(order.side, price, bid, ask):
            if order.is_post_only:
                self._reject_update(
                    order.client_order_id,
                    "update order",
                    f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"new limit px of {price} would have been a TAKER: bid={bid}, ask={ask}",
                )
                return  # Cannot update order
            else:
                # Immediate fill as TAKER
                self._generate_order_updated(order, qty, price)

                fill_px = self._fill_price_taker(order.instrument_id, order.side, bid, ask)
                self._fill_order(
                    order=order,
                    fill_px=fill_px,
                    liquidity_side=LiquiditySide.TAKER,
                )
                return  # Filled

        self._generate_order_updated(order, qty, price)

    cdef inline void _update_stop_market_order(
        self,
        StopMarketOrder order,
        Quantity qty,
        Price price,
        Price bid,
        Price ask,
    ) except *:
        if self._is_stop_marketable(order.side, price, bid, ask):
            self._reject_update(
                order.client_order_id,
                "update order",
                f"STOP {OrderSideParser.to_str(order.side)} order "
                f"new stop px of {price} was in the market: bid={bid}, ask={ask}",
            )
            return  # Cannot update order

        self._generate_order_updated(order, qty, price)

    cdef inline void _update_stop_limit_order(
        self,
        StopLimitOrder order,
        Quantity qty,
        Price price,
        Price bid,
        Price ask,
    ) except *:
        cdef Price fill_px
        if not order.is_triggered:
            # Amending stop price
            if self._is_stop_marketable(order.side, price, bid, ask):
                self._reject_update(
                    order.client_order_id,
                    "update order",
                    f"STOP_LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"new stop px trigger of {price} was in the market: bid={bid}, ask={ask}",
                )
                return  # Cannot update order

            self._generate_order_updated(order, qty, price)
        else:
            # Amending limit price
            if self._is_limit_marketable(order.side, price, bid, ask):
                if order.is_post_only:
                    self._reject_update(
                        order.client_order_id,
                        "update order",
                        f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order  "
                        f"new limit px of {price} would have been a TAKER: bid={bid}, ask={ask}",
                    )
                    return  # Cannot update order
                else:
                    # Immediate fill as TAKER
                    self._generate_order_updated(order, qty, price)

                    fill_px = self._fill_price_taker(order.instrument_id, order.side, bid, ask)
                    self._fill_order(
                        order=order,
                        fill_px=fill_px,
                        liquidity_side=LiquiditySide.TAKER,
                    )
                    return  # Filled

            self._generate_order_updated(order, qty, price)

    cdef inline void _generate_order_updated(self, PassiveOrder order, Quantity qty, Price price) except *:
        # Generate event
        cdef OrderUpdated updated = OrderUpdated(
            order.account_id,
            order.client_order_id,
            order.venue_order_id,
            qty,
            price,
            self._clock.timestamp_ns(),
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(updated)

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

# -- ORDER MATCHING ENGINE -------------------------------------------------------------------------

    cdef inline void _iterate_matching_engine(
        self, InstrumentId instrument_id,
        Price bid,
        Price ask,
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
            self._match_order(order, bid, ask)

            # Check for order expiry (if expire time then compare nanoseconds)
            if order.expire_time and timestamp_ns >= order.expire_time_ns:
                self._delete_order(order)
                self._expire_order(order)

    cdef inline void _match_order(self, PassiveOrder order, Price bid, Price ask) except *:
        if order.type == OrderType.LIMIT:
            self._match_limit_order(order, bid, ask)
        elif order.type == OrderType.STOP_MARKET:
            self._match_stop_market_order(order, bid, ask)
        elif order.type == OrderType.STOP_LIMIT:
            self._match_stop_limit_order(order, bid, ask)
        else:
            raise RuntimeError("invalid order type")

    cdef inline void _match_limit_order(self, LimitOrder order, Price bid, Price ask) except *:
        if self._is_limit_matched(order.side, order.price, bid, ask):
            self._fill_order(
                order=order,
                fill_px=order.price,  # price 'guaranteed'
                liquidity_side=LiquiditySide.MAKER,
            )

    cdef inline void _match_stop_market_order(self, StopMarketOrder order, Price bid, Price ask) except *:
        if self._is_stop_triggered(order.side, order.price, bid, ask):
            self._fill_order(
                order=order,
                fill_px=self._fill_price_stop(order.instrument_id, order.side, order.price),
                liquidity_side=LiquiditySide.TAKER,  # Triggered stop places market order
            )

    cdef inline void _match_stop_limit_order(self, StopLimitOrder order, Price bid, Price ask) except *:
        if order.is_triggered:
            if self._is_limit_matched(order.side, order.price, bid, ask):
                self._fill_order(
                    order=order,
                    fill_px=order.price,          # Price is 'guaranteed' (negative slippage not currently modeled)
                    liquidity_side=LiquiditySide.MAKER,  # Providing liquidity
                )
        else:  # Order not triggered
            if self._is_stop_triggered(order.side, order.trigger, bid, ask):
                self._trigger_order(order)

                # Check for immediate fill
                if self._is_limit_marketable(order.side, order.price, bid, ask):
                    if order.is_post_only:  # Would be liquidity taker
                        self._delete_order(order)  # Remove order from working orders
                        self._reject_order(
                            order,
                            f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order "
                            f"limit px of {order.price} would have been a TAKER: bid={bid}, ask={ask}",
                        )
                    else:
                        self._fill_order(
                            order=order,
                            fill_px=self._fill_price_taker(order.instrument_id, order.side, bid, ask),
                            liquidity_side=LiquiditySide.TAKER,  # Immediate fill takes liquidity
                        )

    cdef inline bint _is_limit_marketable(self, OrderSide side, Price order_price, Price bid, Price ask) except *:
        if side == OrderSide.BUY:
            if ask is None:
                return False  # No market
            return order_price >= ask  # Match with LIMIT sells
        else:  # => OrderSide.SELL
            if bid is None:  # No market
                return False
            return order_price <= bid  # Match with LIMIT buys

    cdef inline bint _is_limit_matched(self, OrderSide side, Price order_price, Price bid, Price ask) except *:
        if side == OrderSide.BUY:
            if bid is None:
                return False  # No market
            return bid < order_price or (bid == order_price and self.fill_model.is_limit_filled())
        else:  # => OrderSide.SELL
            if ask is None:
                return False  # No market
            return ask > order_price or (ask == order_price and self.fill_model.is_limit_filled())

    cdef inline bint _is_stop_marketable(self, OrderSide side, Price order_price, Price bid, Price ask) except *:
        if side == OrderSide.BUY:
            if ask is None:
                return False  # No market
            return ask >= order_price  # Match with LIMIT sells
        else:  # => OrderSide.SELL
            if bid is None:
                return False  # No market
            return bid <= order_price  # Match with LIMIT buys

    cdef inline bint _is_stop_triggered(self, OrderSide side, Price order_price, Price bid, Price ask) except *:
        if side == OrderSide.BUY:
            if ask is None:
                return False  # No market
            return ask > order_price or (ask == order_price and self.fill_model.is_stop_filled())
        else:  # => OrderSide.SELL
            if bid is None:
                return False  # No market
            return bid < order_price or (bid == order_price and self.fill_model.is_stop_filled())

    cdef inline Price _fill_price_maker(self, OrderSide side, Price bid, Price ask):
        # LIMIT orders will always fill at the top of the book,
        # (currently not simulating market impact).
        if side == OrderSide.BUY:
            return bid
        else:  # => OrderSide.SELL
            return ask

    cdef inline Price _fill_price_taker(self, InstrumentId instrument_id, OrderSide side, Price bid, Price ask):
        # Simulating potential slippage of one tick
        if side == OrderSide.BUY:
            return ask if not self.fill_model.is_slipped() else Price(ask + self._slippages[instrument_id])
        else:  # => OrderSide.SELL
            return bid if not self.fill_model.is_slipped() else Price(bid - self._slippages[instrument_id])

    cdef inline Price _fill_price_stop(self, InstrumentId instrument_id, OrderSide side, Price stop):
        if side == OrderSide.BUY:
            return stop if not self.fill_model.is_slipped() else Price(stop + self._slippages[instrument_id])
        else:  # => OrderSide.SELL
            return stop if not self.fill_model.is_slipped() else Price(stop - self._slippages[instrument_id])

# --------------------------------------------------------------------------------------------------

    cdef inline void _fill_order(
        self,
        Order order,
        Price fill_px,
        LiquiditySide liquidity_side,
    ) except *:
        self._delete_order(order)  # Remove order from working orders (if found)

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
            last_px=fill_px,
            liquidity_side=liquidity_side,
        )

        # Generate event
        cdef OrderFilled fill = OrderFilled(
            account_id=self.exec_client.account_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id if order.venue_order_id is not None else self._generate_order_id(order.instrument_id),
            execution_id=self._generate_execution_id(),
            position_id=position_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            order_side=order.side,
            last_qty=order.quantity,
            last_px=fill_px,
            cum_qty=order.quantity,
            leaves_qty=Quantity(),  # Not modeling partial fills yet
            currency=instrument.quote_currency,
            is_inverse=instrument.is_inverse,
            commission=commission,
            liquidity_side=liquidity_side,
            execution_ns=self._clock.timestamp_ns(),
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        # Calculate potential PnL
        cdef Money pnl = None
        if position and position.entry != order.side:
            # Calculate PnL
            pnl = position.calculate_pnl(
                avg_px_open=position.avg_px_open,
                avg_px_close=fill_px,
                quantity=order.quantity,
            )

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

        # Send event to ExecutionEngine
        self.exec_client.handle_event(fill)
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
        cdef OrderRejected rejected = OrderRejected(
            self.exec_client.account_id,
            order.client_order_id,
            self._clock.timestamp_ns(),
            f"OCO order rejected from {other_oco}",
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(rejected)

    cdef inline void _cancel_oco_order(self, PassiveOrder order) except *:
        # order is the OCO order to cancel
        if order.is_completed_c():
            self._log.debug(f"Cannot cancel order: state was already {order.state_string_c()}.")
            return

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            self.exec_client.account_id,
            order.client_order_id,
            order.venue_order_id,
            self._clock.timestamp_ns(),
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self.exec_client.handle_event(cancelled)
