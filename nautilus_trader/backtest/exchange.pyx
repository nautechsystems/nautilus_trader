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

from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport TestLogger
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.cache cimport ExecutionCache
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderAmended
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderTriggered
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Security
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport PassiveOrder
from nautilus_trader.model.order.limit cimport LimitOrder
from nautilus_trader.model.order.market cimport MarketOrder
from nautilus_trader.model.order.stop_limit cimport StopLimitOrder
from nautilus_trader.model.order.stop_market cimport StopMarketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
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
        bint generate_position_ids,
        bint is_frozen_account,
        list starting_balances not None,
        list instruments not None,
        list modules not None,
        ExecutionCache exec_cache not None,
        FillModel fill_model not None,
        TestClock clock not None,
        TestLogger logger not None,
    ):
        """
        Initialize a new instance of the `SimulatedExchange` class.

        Parameters
        ----------
        venue : Venue
            The venue to simulate for the backtest.
        oms_type : OMSType (Enum)
            The order management system type used by the exchange.
        generate_position_ids : bool
            If the exchange should generate position identifiers.
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
        logger : TestLogger
            The logger for the component.

        """
        Condition.not_empty(instruments, "instruments")
        Condition.list_type(instruments, Instrument, "instruments", "Instrument")
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(starting_balances, Money, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules", "SimulationModule")

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(f"{type(self).__name__}({venue})", logger)

        self.venue = venue
        self.oms_type = oms_type
        self.generate_position_ids = generate_position_ids

        self.exec_cache = exec_cache
        self.exec_client = None  # Initialized when execution client registered

        self.is_frozen_account = is_frozen_account
        self.starting_balances = starting_balances
        # noinspection PyUnresolvedReferences
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

        # Security indexer for order_ids
        self._security_indexer = {}  # type: dict[Security, int]

        # Load instruments
        self.instruments = {}
        for instrument in instruments:
            Condition.equal(instrument.security.venue, self.venue, "instrument.security.venue", "self.venue")
            self.instruments[instrument.security] = instrument
            index = len(self._security_indexer) + 1
            self._security_indexer[instrument.security] = index
            self._log.info(f"Loaded instrument {instrument.security.value}.")

        self._slippages = self._get_tick_sizes()
        self._market_bids = {}          # type: dict[Security, Price]
        self._market_asks = {}          # type: dict[Security, Price]

        self._working_orders = {}       # type: dict[ClientOrderId, Order]
        self._position_index = {}       # type: dict[ClientOrderId, PositionId]
        self._child_orders = {}         # type: dict[ClientOrderId, list[Order]]
        self._oco_orders = {}           # type: dict[ClientOrderId, ClientOrderId]
        self._position_oco_orders = {}  # type: dict[PositionId, list[ClientOrderId]]
        self._symbol_pos_count = {}     # type: dict[Security, int]
        self._symbol_ord_count = {}     # type: dict[Security, int]
        self._executions_count = 0

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.venue})"

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

    cpdef void process_tick(self, Tick tick) except *:
        """
        Process the exchanges market for the given tick.

        Market dynamics are simulated by auctioning working orders.

        Parameters
        ----------
        tick : Tick
            The tick data to process with (`QuoteTick` or `TradeTick`).

        """
        Condition.not_none(tick, "tick")

        self._clock.set_time(tick.timestamp)

        cdef Security security = tick.security

        # Update market bid and ask
        cdef Price bid
        cdef Price ask
        if isinstance(tick, QuoteTick):
            bid = tick.bid
            ask = tick.ask
            self._market_bids[security] = bid
            self._market_asks[security] = ask
        else:  # TradeTick
            if tick.side == OrderSide.SELL:  # TAKER hit the bid
                bid = tick.price
                ask = self._market_asks.get(security)
                if ask is None:
                    ask = bid  # Initialize ask
                self._market_bids[security] = bid
            elif tick.side == OrderSide.BUY:  # TAKER lifted the offer
                ask = tick.price
                bid = self._market_bids.get(security)
                if bid is None:
                    bid = ask  # Initialize bid
                self._market_asks[security] = ask
            # tick.side must be BUY or SELL (condition checked in TradeTick)

        cdef PassiveOrder order
        for order in self._working_orders.copy().values():  # Copy dict for safe loop
            if order.security != tick.security:
                continue  # Order is for a different security
            if not order.is_working_c():
                continue  # Orders state has changed since the loop started

            # Check for order match
            self._match_order(order, bid, ask)

            # Check for order expiry
            if order.expire_time and tick.timestamp >= order.expire_time:
                self._working_orders.pop(order.cl_ord_id, None)
                self._expire_order(order)

    cpdef void process_modules(self, datetime now) except *:
        """
        Process the simulation modules by advancing their time.

        Parameters
        ----------
        now : datetime
            The time to advance to.

        """
        # Iterate through modules
        cdef SimulationModule module
        for module in self.modules:
            module.process(now)

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

        self._market_bids.clear()
        self._market_asks.clear()
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
            self._position_index[command.order.cl_ord_id] = command.position_id

        self._submit_order(command.order)
        self._process_order(command.order)

    cpdef void handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        Condition.not_none(command, "command")

        cdef PositionId position_id = self._generate_position_id(command.bracket_order.entry.security)

        cdef list bracket_orders = [command.bracket_order.stop_loss]
        self._position_oco_orders[position_id] = []
        if command.bracket_order.take_profit is not None:
            bracket_orders.append(command.bracket_order.take_profit)
            self._oco_orders[command.bracket_order.take_profit.cl_ord_id] = command.bracket_order.stop_loss.cl_ord_id
            self._oco_orders[command.bracket_order.stop_loss.cl_ord_id] = command.bracket_order.take_profit.cl_ord_id
            self._position_oco_orders[position_id].append(command.bracket_order.take_profit)

        self._child_orders[command.bracket_order.entry.cl_ord_id] = bracket_orders
        self._position_oco_orders[position_id].append(command.bracket_order.stop_loss)

        self._submit_order(command.bracket_order.entry)
        self._submit_order(command.bracket_order.stop_loss)
        if command.bracket_order.take_profit is not None:
            self._submit_order(command.bracket_order.take_profit)

        self._process_order(command.bracket_order.entry)

    cpdef void handle_cancel_order(self, CancelOrder command) except *:
        Condition.not_none(command, "command")

        self._cancel_order(command.cl_ord_id)

    cpdef void handle_amend_order(self, AmendOrder command) except *:
        Condition.not_none(command, "command")

        self._amend_order(command.cl_ord_id, command.quantity, command.price)

# --------------------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment) except *:
        Condition.not_none(adjustment, "adjustment")

        if self.is_frozen_account:
            return  # Nothing to adjust

        balance = self.account_balances[adjustment.currency]
        self.account_balances[adjustment.currency] = Money(balance + adjustment, adjustment.currency)

        # Generate and handle event
        self.exec_client.handle_event(self._generate_account_event())

    cdef inline Price get_current_bid(self, Security security):
        Condition.not_none(security, "security")

        return self._market_bids.get(security)

    cdef inline Price get_current_ask(self, Security security):
        Condition.not_none(security, "security")

        return self._market_asks.get(security)

    cdef inline object get_xrate(self, Currency from_currency, Currency to_currency, PriceType price_type):
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")
        Condition.not_equal(price_type, PriceType.UNDEFINED, "price_type", "UNDEFINED")

        return self.xrate_calculator.get_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type,
            bid_quotes=self._build_current_bid_rates(),
            ask_quotes=self._build_current_ask_rates(),
        )

    cdef inline dict _build_current_bid_rates(self):
        cdef Security security
        cdef QuoteTick tick
        return {security.symbol: price.as_decimal() for security, price in self._market_bids.items()}

    cdef inline dict _build_current_ask_rates(self):
        cdef Security security
        cdef QuoteTick tick
        return {security.symbol: price.as_decimal() for security, price in self._market_asks.items()}

# -- EVENT HANDLING --------------------------------------------------------------------------------

    cdef inline object _get_tick_sizes(self):
        cdef dict slippage_index = {}  # type: dict[Security, Decimal]

        for security, instrument in self.instruments.items():
            # noinspection PyUnresolvedReferences
            slippage_index[security] = instrument.tick_size

        return slippage_index

    cdef inline PositionId _generate_position_id(self, Security security):
        cdef int pos_count = self._symbol_pos_count.get(security, 0)
        pos_count += 1
        self._symbol_pos_count[security] = pos_count
        return PositionId(f"{self._security_indexer[security]}-{pos_count:03d}")

    cdef inline OrderId _generate_order_id(self, Security security):
        cdef int ord_count = self._symbol_ord_count.get(security, 0)
        ord_count += 1
        self._symbol_ord_count[security] = ord_count
        return OrderId(f"{self._security_indexer[security]}-{ord_count:03d}")

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
            event_timestamp=self._clock.utc_now_c(),
        )

    cdef inline void _submit_order(self, Order order) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            self.exec_client.account_id,
            order.cl_ord_id,
            self._clock.utc_now_c(),
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(submitted)

    cdef inline void _accept_order(self, Order order) except *:
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self.exec_client.account_id,
            order.cl_ord_id,
            self._generate_order_id(order.security),
            self._clock.utc_now_c(),
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(accepted)

    cdef inline void _reject_order(self, Order order, str reason) except *:
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.exec_client.account_id,
            order.cl_ord_id,
            self._clock.utc_now_c(),
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(rejected)
        self._check_oco_order(order.cl_ord_id)
        self._clean_up_child_orders(order.cl_ord_id)

    cdef inline void _amend_order(self, ClientOrderId cl_ord_id, Quantity qty, Price price) except *:
        cdef PassiveOrder order = self._working_orders.get(cl_ord_id)
        if order is None:
            self._cancel_reject(
                cl_ord_id,
                "amend order",
                f"repr{cl_ord_id} not found",
            )
            return  # Cannot amend order

        cdef Instrument instrument = self.instruments[order.security]

        if qty <= 0:
            self._cancel_reject(
                order.cl_ord_id,
                "amend order",
                f"amended quantity {qty} invalid",
            )
            return  # Cannot amend order

        cdef Price bid = self._market_bids[order.security]  # Market must exist
        cdef Price ask = self._market_asks[order.security]  # Market must exist

        if order.type == OrderType.LIMIT:
            self._amend_limit_order(order, qty, price, bid, ask)
        elif order.type == OrderType.STOP_MARKET:
            self._amend_stop_market_order(order, qty, price, bid, ask)
        elif order.type == OrderType.STOP_LIMIT:
            self._amend_stop_limit_order(order, qty, price, bid, ask)
        else:
            raise RuntimeError(f"Invalid order type")

    cdef inline void _cancel_order(self, ClientOrderId cl_ord_id) except *:
        cdef PassiveOrder order = self._working_orders.pop(cl_ord_id, None)
        if order is None:
            self._cancel_reject(
                cl_ord_id,
                "cancel order",
                f"{repr(cl_ord_id)} not found",
            )
            return  # Rejected the cancel order command

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            order.account_id,
            order.cl_ord_id,
            order.id,
            self._clock.utc_now_c(),
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(cancelled)
        self._check_oco_order(order.cl_ord_id)

    cdef inline void _cancel_reject(
        self,
        ClientOrderId cl_ord_id,
        str response,
        str reason,
    ) except *:
        cdef Order order = self.exec_cache.order(cl_ord_id)
        if order is not None:
            order_id = order.id
        else:
            order_id = OrderId.null_c()

        # Generate event
        cdef OrderCancelReject cancel_reject = OrderCancelReject(
            self.exec_client.account_id,
            cl_ord_id,
            order_id,
            self._clock.utc_now_c(),
            response,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(cancel_reject)

    cdef inline void _expire_order(self, PassiveOrder order) except *:
        Condition.true(order.expire_time <= self._clock.utc_now_c(), "order expire time greater than time now")

        # Generate event
        cdef OrderExpired expired = OrderExpired(
            self.exec_client.account_id,
            order.cl_ord_id,
            order.id,
            order.expire_time,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(expired)

        cdef ClientOrderId first_child_order_id
        cdef ClientOrderId other_oco_order_id
        if order.cl_ord_id in self._child_orders:
            # Remove any unprocessed OCO child orders
            first_child_order_id = self._child_orders[order.cl_ord_id][0].cl_ord_id
            if first_child_order_id in self._oco_orders:
                other_oco_order_id = self._oco_orders[first_child_order_id]
                del self._oco_orders[first_child_order_id]
                del self._oco_orders[other_oco_order_id]
        else:
            self._check_oco_order(order.cl_ord_id)
        self._clean_up_child_orders(order.cl_ord_id)

    cdef inline void _trigger_order(self, StopLimitOrder order) except *:
        # Generate event
        cdef OrderTriggered triggered = OrderTriggered(
            self.exec_client.account_id,
            order.cl_ord_id,
            order.id,
            self._clock.utc_now_c(),
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(triggered)

    cdef inline void _process_order(self, Order order) except *:
        Condition.not_in(order.cl_ord_id, self._working_orders, "order.id", "working_orders")

        cdef Instrument instrument = self.instruments[order.security]

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

        cdef Price bid = self._market_bids.get(order.security)
        cdef Price ask = self._market_asks.get(order.security)

        # Check market exists
        if bid is None or ask is None:  # Market not initialized
            self._reject_order(order, f"no market for {order.security}")
            return  # Cannot accept order

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
        self._accept_order(order)

        # Immediately fill marketable order
        self._fill_order(
            order,
            self._fill_price_taker(order.security, order.side, bid, ask),
            LiquiditySide.TAKER,
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
        self._working_orders[order.cl_ord_id] = order
        self._accept_order(order)

        # Check for immediate fill
        cdef Price fill_price
        if not order.is_post_only and self._is_limit_marketable(order.side, order.price, bid, ask):
            fill_price = self._fill_price_maker(order.side, bid, ask)
            self._fill_order(order, fill_price, LiquiditySide.TAKER)

    cdef inline void _process_stop_market_order(self, StopMarketOrder order, Price bid, Price ask) except *:
        if self._is_stop_marketable(order.side, order.price, bid, ask):
            self._reject_order(
                order,
                f"STOP {OrderSideParser.to_str(order.side)} order "
                f"stop px of {order.price} was in the market: bid={bid}, ask={ask}",
            )
            return  # Invalid price

        # Order is valid and accepted
        self._working_orders[order.cl_ord_id] = order
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
        self._working_orders[order.cl_ord_id] = order
        self._accept_order(order)

    cdef inline void _amend_limit_order(
        self,
        LimitOrder order,
        Quantity qty,
        Price price,
        Price bid,
        Price ask,
    ) except *:
        cdef Price fill_price
        if self._is_limit_marketable(order.side, price, bid, ask):
            if order.is_post_only:
                self._cancel_reject(
                    order.cl_ord_id,
                    "amend order",
                    f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"amended limit px of {price} would have been a TAKER: bid={bid}, ask={ask}",
                )
                return  # Cannot amend order
            else:
                # Immediate fill as TAKER
                self._generate_order_amended(order, qty, price)

                fill_price = self._fill_price_taker(order.security, order.side, bid, ask)
                self._fill_order(order, fill_price, LiquiditySide.TAKER)
                return  # Filled

        self._generate_order_amended(order, qty, price)

    cdef inline void _amend_stop_market_order(
        self,
        StopMarketOrder order,
        Quantity qty,
        Price price,
        Price bid,
        Price ask,
    ) except *:
        if self._is_stop_marketable(order.side, price, bid, ask):
            self._cancel_reject(
                order.cl_ord_id,
                "amend order",
                f"STOP {OrderSideParser.to_str(order.side)} order "
                f"amended stop px of {price} was in the market: bid={bid}, ask={ask}",
            )
            return  # Cannot amend order

        self._generate_order_amended(order, qty, price)

    cdef inline void _amend_stop_limit_order(
        self,
        StopLimitOrder order,
        Quantity qty,
        Price price,
        Price bid,
        Price ask,
    ) except *:
        cdef Price fill_price
        if not order.is_triggered:
            # Amending stop price
            if self._is_stop_marketable(order.side, price, bid, ask):
                self._cancel_reject(
                    order.cl_ord_id,
                    "amend order",
                    f"STOP_LIMIT {OrderSideParser.to_str(order.side)} order "
                    f"amended stop px trigger of {price} was in the market: bid={bid}, ask={ask}",
                )
                return  # Cannot amend order

            self._generate_order_amended(order, qty, price)
        else:
            # Amending limit price
            if self._is_limit_marketable(order.side, price, bid, ask):
                if order.is_post_only:
                    self._cancel_reject(
                        order.cl_ord_id,
                        "amend order",
                        f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order  "
                        f"amended limit px of {price} would have been a TAKER: bid={bid}, ask={ask}",
                    )
                    return  # Cannot amend order
                else:
                    # Immediate fill as TAKER
                    self._generate_order_amended(order, qty, price)

                    fill_price = self._fill_price_taker(order.security, order.side, bid, ask)
                    self._fill_order(order, fill_price, LiquiditySide.TAKER)
                    return  # Filled

            self._generate_order_amended(order, qty, price)

    cdef inline void _generate_order_amended(self, PassiveOrder order, Quantity qty, Price price) except *:
        # Generate event
        cdef OrderAmended amended = OrderAmended(
            order.account_id,
            order.cl_ord_id,
            order.id,
            qty,
            price,
            self._clock.utc_now_c(),
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(amended)

# -- ORDER MATCHING ENGINE -------------------------------------------------------------------------

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
                order,
                order.price,  # price 'guaranteed'
                LiquiditySide.MAKER,
            )

    cdef inline void _match_stop_market_order(self, StopMarketOrder order, Price bid, Price ask) except *:
        if self._is_stop_triggered(order.side, order.price, bid, ask):
            self._fill_order(
                order,
                self._fill_price_stop(order.security, order.side, order.price),
                LiquiditySide.TAKER,  # Triggered stop places market order
            )

    cdef inline void _match_stop_limit_order(self, StopLimitOrder order, Price bid, Price ask) except *:
        if order.is_triggered:
            if self._is_limit_matched(order.side, order.price, bid, ask):
                self._fill_order(
                    order,
                    order.price,          # Price is 'guaranteed' (negative slippage not currently modeled)
                    LiquiditySide.MAKER,  # Providing liquidity
                )
        else:  # Order not triggered
            if self._is_stop_triggered(order.side, order.trigger, bid, ask):
                self._trigger_order(order)

                # Check for immediate fill
                if self._is_limit_marketable(order.side, order.price, bid, ask):
                    if order.is_post_only:  # Would be liquidity taker
                        del self._working_orders[order.cl_ord_id]  # Remove order from working orders
                        self._reject_order(
                            order,
                            f"POST_ONLY LIMIT {OrderSideParser.to_str(order.side)} order "
                            f"limit px of {order.price} would have been a TAKER: bid={bid}, ask={ask}",
                        )
                    else:
                        self._fill_order(
                            order,
                            self._fill_price_taker(order.security, order.side, bid, ask),
                            LiquiditySide.TAKER,  # Immediate fill takes liquidity
                        )

    cdef inline bint _is_limit_marketable(self, OrderSide side, Price order_price, Price bid, Price ask) except *:
        if side == OrderSide.BUY:
            return order_price >= ask  # Match with LIMIT sells
        else:  # => OrderSide.SELL
            return order_price <= bid  # Match with LIMIT buys

    cdef inline bint _is_limit_matched(self, OrderSide side, Price order_price, Price bid, Price ask) except *:
        if side == OrderSide.BUY:
            return bid < order_price or (bid == order_price and self.fill_model.is_limit_filled())
        else:  # => OrderSide.SELL
            return ask > order_price or (ask == order_price and self.fill_model.is_limit_filled())

    cdef inline bint _is_stop_marketable(self, OrderSide side, Price order_price, Price bid, Price ask) except *:
        if side == OrderSide.BUY:
            return order_price <= ask  # Match with LIMIT sells
        else:  # => OrderSide.SELL
            return order_price >= bid  # Match with LIMIT buys

    cdef inline bint _is_stop_triggered(self, OrderSide side, Price order_price, Price bid, Price ask) except *:
        if side == OrderSide.BUY:
            return order_price < ask or (order_price == ask and self.fill_model.is_stop_filled())
        else:  # => OrderSide.SELL
            return order_price > bid or (order_price == bid and self.fill_model.is_stop_filled())

    cdef inline Price _fill_price_maker(self, OrderSide side, Price bid, Price ask):
        # LIMIT orders will always fill at the top of the book,
        # (currently not simulating market impact).
        if side == OrderSide.BUY:
            return bid
        else:  # => OrderSide.SELL
            return ask

    cdef inline Price _fill_price_taker(self, Security security, OrderSide side, Price bid, Price ask):
        # Simulating potential slippage of one tick
        if side == OrderSide.BUY:
            return ask if not self.fill_model.is_slipped() else Price(ask + self._slippages[security])
        else:  # => OrderSide.SELL
            return bid if not self.fill_model.is_slipped() else Price(bid - self._slippages[security])

    cdef inline Price _fill_price_stop(self, Security security, OrderSide side, Price stop):
        if side == OrderSide.BUY:
            return stop if not self.fill_model.is_slipped() else Price(stop + self._slippages[security])
        else:  # => OrderSide.SELL
            return stop if not self.fill_model.is_slipped() else Price(stop - self._slippages[security])

# --------------------------------------------------------------------------------------------------

    cdef inline void _fill_order(
        self,
        Order order,
        Price fill_price,
        LiquiditySide liquidity_side,
    ) except *:
        self._working_orders.pop(order.cl_ord_id, None)  # Remove order from working orders if found

        # Query if there is an existing position for this order
        cdef PositionId position_id = self._position_index.get(order.cl_ord_id)
        # *** position_id could be None here ***

        cdef PositionId new_position_id
        cdef Position position = None
        if position_id is None:
            # Generate a new position identifier
            new_position_id = self._generate_position_id(order.security)
            self._position_index[order.cl_ord_id] = new_position_id
            if self.generate_position_ids:
                # Set the filled position identifier
                position_id = new_position_id
            else:
                # Only use the position identifier internally to the exchange
                position_id = PositionId.null_c()
        else:
            position = self.exec_cache.position(position_id)
            position_id = position.id

        # Calculate commission
        cdef Instrument instrument = self.instruments.get(order.security)
        if instrument is None:
            raise RuntimeError(f"Cannot run backtest: no instrument data for {order.security}")

        cdef Money commission = instrument.calculate_commission(
            order.quantity,
            fill_price,
            liquidity_side,
        )

        # Generate event
        cdef OrderFilled filled = OrderFilled(
            self.exec_client.account_id,
            order.cl_ord_id,
            order.id if order.id is not None else self._generate_order_id(order.security),
            self._generate_execution_id(),
            position_id,
            order.strategy_id,
            order.security,
            order.side,
            order.quantity,
            order.quantity,
            Quantity(),  # Not modeling partial fills yet
            fill_price,
            instrument.quote_currency,
            instrument.is_inverse,
            commission,
            liquidity_side,
            self._clock.utc_now_c(),
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        # Calculate potential PnL
        cdef Money pnl = None
        if position is not None and position.entry != order.side:
            # Calculate PnL
            pnl = position.calculate_pnl(
                avg_open=position.avg_open,
                avg_close=fill_price,
                quantity=order.quantity,
            )

        cdef Currency currency  # Settlement currency
        if self.default_currency is not None:  # Single-asset account
            currency = self.default_currency
            if pnl is None:
                pnl = Money(0, currency)

            if commission.currency != currency:
                # Calculate exchange rate to account currency
                xrate = self.get_xrate(
                    from_currency=commission.currency,
                    to_currency=currency,
                    price_type=PriceType.BID if order.side is OrderSide.SELL else PriceType.ASK,
                )

                # Convert to account currency
                commission = Money(commission * xrate, currency)
                pnl = Money(pnl * xrate, currency)

            total_commissions = self.total_commissions.get(currency, Decimal()) + commission
            self.total_commissions[currency] = Money(total_commissions, currency)

            # Final PnL
            pnl = Money(pnl - commission, self.default_currency)
        else:
            currency = instrument.settlement_currency
            if pnl is None:
                pnl = Money(0, currency)

            total_commissions = self.total_commissions.get(currency, Decimal()) + commission
            self.total_commissions[currency] = Money(total_commissions, currency)

        self.exec_client.handle_event(filled)
        self._check_oco_order(order.cl_ord_id)

        # Work any bracket child orders
        if order.cl_ord_id in self._child_orders:
            for child_order in self._child_orders[order.cl_ord_id]:
                if not child_order.is_completed:  # The order may already be cancelled or rejected
                    self._process_order(child_order)
            del self._child_orders[order.cl_ord_id]

        if position and position.is_closed_c():
            oco_orders = self._position_oco_orders.get(position.id)
            if oco_orders:
                for order in self._position_oco_orders[position.id]:
                    if order.is_working_c():
                        self._log.debug(f"Cancelling {order.cl_ord_id} as linked position closed.")
                        self._cancel_oco_order(order)
                del self._position_oco_orders[position.id]

        # Finally adjust account
        self.adjust_account(pnl)

    cdef inline void _check_oco_order(self, ClientOrderId cl_ord_id) except *:
        # Check held OCO orders and remove any paired with the given cl_ord_id
        cdef ClientOrderId oco_cl_ord_id = self._oco_orders.pop(cl_ord_id, None)
        if oco_cl_ord_id is None:
            return  # No linked order

        del self._oco_orders[oco_cl_ord_id]
        cdef PassiveOrder oco_order = self._working_orders.pop(oco_cl_ord_id, None)
        if oco_order is None:
            return  # No linked order

        # Reject any latent bracket child orders first
        cdef ClientOrderId bracket_order_id
        cdef list child_orders
        cdef PassiveOrder order
        for child_orders in self._child_orders.values():
            for order in child_orders:
                if oco_order == order and not order.is_working_c():
                    self._reject_oco_order(order, cl_ord_id)

        # Cancel working OCO order
        self._log.debug(f"Cancelling {oco_order.cl_ord_id} OCO order from {oco_cl_ord_id}.")
        self._cancel_oco_order(oco_order)

    cdef inline void _clean_up_child_orders(self, ClientOrderId cl_ord_id) except *:
        # Clean up any residual child orders from the completed order associated
        # with the given identifier.
        self._child_orders.pop(cl_ord_id, None)

    cdef inline void _reject_oco_order(self, PassiveOrder order, ClientOrderId other_oco) except *:
        # order is the OCO order to reject
        # other_oco is the linked ClientOrderId
        if order.is_completed_c():
            self._log.debug(f"Cannot reject order: state was already {order.state_string_c()}.")
            return

        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.exec_client.account_id,
            order.cl_ord_id,
            self._clock.utc_now_c(),
            f"OCO order rejected from {other_oco}",
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
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
            order.cl_ord_id,
            order.id,
            self._clock.utc_now_c(),
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self.exec_client.handle_event(cancelled)
