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

from cpython.datetime cimport datetime
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
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderModified
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderWorking
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport LimitOrder
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.model.tick cimport QuoteTick
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
        oms_type : OMSType
            The order management employed by the exchange/broker for this market.
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

        # Load instruments
        self.instruments = {}
        for instrument in instruments:
            Condition.equal(instrument.symbol.venue, self.venue, "instrument.symbol.venue", "self.venue")
            self.instruments[instrument.symbol] = instrument
            self._log.info(f"Loaded instrument {instrument.symbol.value}.")

        self._slippages = self._get_tick_sizes()
        self._market_bids = {}          # type: dict[Symbol, Price]
        self._market_asks = {}          # type: dict[Symbol, Price]

        self._working_orders = {}       # type: dict[ClientOrderId, Order]
        self._position_index = {}       # type: dict[ClientOrderId, PositionId]
        self._child_orders = {}         # type: dict[ClientOrderId, list[Order]]
        self._oco_orders = {}           # type: dict[ClientOrderId, ClientOrderId]
        self._position_oco_orders = {}  # type: dict[PositionId, list[ClientOrderId]]
        self._symbol_pos_count = {}     # type: dict[Symbol, int]
        self._symbol_ord_count = {}     # type: dict[Symbol, int]
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
        Process the exchanges markets with the given tick.

        Market dynamics are simulated against working orders for the ticks symbol.

        Parameters
        ----------
        tick : Tick
            The tick data to process with. Can be `QuoteTick` or `TradeTick`.

        """
        Condition.not_none(tick, "tick")

        self._clock.set_time(tick.timestamp)

        cdef Symbol symbol = tick.symbol

        # Update market bid and ask
        cdef Price bid
        cdef Price ask
        if isinstance(tick, QuoteTick):
            bid = tick.bid
            ask = tick.ask
            self._market_bids[symbol] = bid
            self._market_asks[symbol] = ask
        else:  # TradeTick
            if tick.side == OrderSide.SELL:  # TAKER hit the bid
                bid = tick.price
                ask = self._market_asks.get(symbol)
                if ask is None:
                    ask = bid
                self._market_bids[symbol] = bid
            elif tick.side == OrderSide.BUY:  # TAKER lifted the offer
                ask = tick.price
                bid = self._market_bids.get(symbol)
                if bid is None:
                    bid = ask
                self._market_asks[symbol] = ask
            else:
                raise RuntimeError("invalid trade side")

        cdef datetime now = self._clock.utc_now()

        # Iterate through modules
        cdef SimulationModule module
        for module in self.modules:
            module.process(tick, now)

        cdef PassiveOrder order
        cdef Instrument instrument
        for order in self._working_orders.copy().values():  # Copy dict for safe loop
            if order.symbol != tick.symbol:
                continue  # Order is for a different symbol
            if not order.is_working_c():
                continue  # Orders state has changed since the loop started

            instrument = self.instruments[order.symbol]

            # Check for order fill
            if order.side == OrderSide.BUY:
                self._auction_buy_order(order, ask)
            elif order.side == OrderSide.SELL:
                self._auction_sell_order(order, bid)
            else:
                raise RuntimeError("invalid order side")

            # Check for order expiry
            if order.expire_time and now >= order.expire_time:
                self._working_orders.pop(order.cl_ord_id, None)
                self._expire_order(order)

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

        cdef PositionId position_id = self._generate_position_id(command.bracket_order.entry.symbol)

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

        if command.cl_ord_id not in self._working_orders:
            self._cancel_reject_order(command.cl_ord_id, "cancel order", "order not found")
            return  # Rejected the cancel order command

        cdef Order order = self._working_orders[command.cl_ord_id]

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            command.account_id,
            order.cl_ord_id,
            OrderId(order.cl_ord_id.value.replace('O', 'B', 1)),
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        # Remove from working orders (checked it was in dictionary above)
        del self._working_orders[command.cl_ord_id]

        self.exec_client.handle_event(cancelled)
        self._check_oco_order(command.cl_ord_id)

    cpdef void handle_modify_order(self, ModifyOrder command) except *:
        Condition.not_none(command, "command")

        if command.cl_ord_id not in self._working_orders:
            self._cancel_reject_order(
                command.cl_ord_id,
                "modify order",
                "order not found")
            return  # Rejected the modify order request

        cdef PassiveOrder order = self._working_orders[command.cl_ord_id]
        cdef Instrument instrument = self.instruments[order.symbol]

        if command.quantity == 0:
            self._cancel_reject_order(
                order.cl_ord_id,
                "modify order",
                f"modified quantity {command.quantity} invalid")
            return  # Cannot modify order

        cdef Price market_bid = self._market_bids.get(order.symbol)
        cdef Price market_ask = self._market_asks.get(order.symbol)

        # Check order price is valid and reject or fill
        if order.side == OrderSide.BUY:
            if order.type == OrderType.STOP_MARKET:
                if order.price < market_ask:
                    self._cancel_reject_order(
                        order.cl_ord_id,
                        "modify order",
                        f"BUY STOP order price of {order.price} is too "
                        f"far from the market, ask={market_ask}")
                    return  # Rejected the modify order request
            elif order.type == OrderType.LIMIT:
                if order.price >= market_ask:
                    if order.is_post_only:
                        self._cancel_reject_order(
                            order.cl_ord_id,
                            "modify order",
                            f"BUY LIMIT order price of {order.price} is too "
                            f"far from the market, ask={market_ask}")
                        return  # Rejected the modify order request
                    else:
                        self._fill_order(order, market_ask, LiquiditySide.TAKER)
                    return  # Filled
        elif order.side == OrderSide.SELL:
            if order.type == OrderType.STOP_MARKET:
                if order.price > market_bid:
                    self._cancel_reject_order(
                        order.cl_ord_id,
                        "modify order",
                        f"SELL STOP order price of {order.price} is too "
                        f"far from the market, bid={market_bid}")
                    return  # Rejected the modify order request
            elif order.type == OrderType.LIMIT:
                if order.price <= market_bid:
                    if order.is_post_only:
                        self._cancel_reject_order(
                            order.cl_ord_id,
                            "modify order",
                            f"SELL LIMIT order price of {order.price} is too "
                            f"far from the market, bid={market_bid}")
                        return  # Rejected the modify order request
                    else:
                        self._fill_order(order, market_bid, LiquiditySide.TAKER)
                        return  # Filled

        # Generate event
        cdef OrderModified modified = OrderModified(
            command.account_id,
            order.cl_ord_id,
            order.id,
            command.quantity,
            command.price,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_client.handle_event(modified)

# --------------------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment) except *:
        Condition.not_none(adjustment, "adjustment")

        if self.is_frozen_account:
            return  # Nothing to adjust

        balance = self.account_balances[adjustment.currency]
        self.account_balances[adjustment.currency] = Money(balance + adjustment, adjustment.currency)

        # Generate and handle event
        self.exec_client.handle_event(self._generate_account_event())

    cdef inline Price get_current_bid(self, Symbol symbol):
        Condition.not_none(symbol, "symbol")

        return self._market_bids.get(symbol)

    cdef inline Price get_current_ask(self, Symbol symbol):
        Condition.not_none(symbol, "symbol")

        return self._market_asks.get(symbol)

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
        cdef Symbol symbol
        cdef QuoteTick tick
        return {symbol.code: price.as_decimal() for symbol, price in self._market_bids.items()}

    cdef inline dict _build_current_ask_rates(self):
        cdef Symbol symbol
        cdef QuoteTick tick
        return {symbol.code: price.as_decimal() for symbol, price in self._market_asks.items()}

# -- EVENT HANDLING --------------------------------------------------------------------------------

    cdef inline object _get_tick_sizes(self):
        cdef dict slippage_index = {}  # type: dict[Symbol, Decimal]

        for symbol, instrument in self.instruments.items():
            # noinspection PyUnresolvedReferences
            slippage_index[symbol] = instrument.tick_size

        return slippage_index

    cdef inline PositionId _generate_position_id(self, Symbol symbol):
        cdef int pos_count = self._symbol_pos_count.get(symbol, 0)
        pos_count += 1
        self._symbol_pos_count[symbol] = pos_count
        return PositionId(f"B-{symbol.code}-{pos_count}")

    cdef inline OrderId _generate_order_id(self, Symbol symbol):
        cdef int ord_count = self._symbol_ord_count.get(symbol, 0)
        ord_count += 1
        self._symbol_ord_count[symbol] = ord_count
        return OrderId(f"B-{symbol.code}-{ord_count}")

    cdef inline ExecutionId _generate_execution_id(self):
        self._executions_count += 1
        return ExecutionId(f"E-{self._executions_count}")

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
            event_timestamp=self._clock.utc_now(),
        )

    cdef inline void _submit_order(self, Order order) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            self.exec_client.account_id,
            order.cl_ord_id,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_client.handle_event(submitted)

    cdef inline void _accept_order(self, Order order) except *:
        # Generate event

        cdef OrderAccepted accepted = OrderAccepted(
            self.exec_client.account_id,
            order.cl_ord_id,
            self._generate_order_id(order.symbol),
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_client.handle_event(accepted)

    cdef inline void _reject_order(self, Order order, str reason) except *:
        if order.state_c() != OrderState.SUBMITTED:
            self._log.error(f"Cannot reject order, state was {order.state_string_c()}.")
            return

        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.exec_client.account_id,
            order.cl_ord_id,
            self._clock.utc_now(),
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_client.handle_event(rejected)
        self._check_oco_order(order.cl_ord_id)
        self._clean_up_child_orders(order.cl_ord_id)

    cdef inline void _cancel_reject_order(
            self,
            ClientOrderId order_id,
            str response,
            str reason) except *:
        # Generate event
        cdef OrderCancelReject cancel_reject = OrderCancelReject(
            self.exec_client.account_id,
            order_id,
            self._clock.utc_now(),
            response,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_client.handle_event(cancel_reject)

    cdef inline void _expire_order(self, PassiveOrder order) except *:
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            self.exec_client.account_id,
            order.cl_ord_id,
            order.id,
            order.expire_time,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
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

    cdef inline void _process_order(self, Order order) except *:
        Condition.not_in(order.cl_ord_id, self._working_orders, "order.id", "working_orders")

        cdef Instrument instrument = self.instruments[order.symbol]

        # Check order size is valid or reject
        if instrument.max_quantity and order.quantity > instrument.max_quantity:
            self._reject_order(order, f"order quantity of {order.quantity} exceeds "
                                      f"the maximum trade size of {instrument.max_quantity}")
            return  # Cannot accept order
        if instrument.min_quantity and order.quantity < instrument.min_quantity:
            self._reject_order(order, f"order quantity of {order.quantity} is less than "
                                      f"the minimum trade size of {instrument.min_quantity}")
            return  # Cannot accept order

        cdef Price market_bid = self._market_bids.get(order.symbol)
        cdef Price market_ask = self._market_asks.get(order.symbol)

        # Check market exists
        if market_bid is None or market_ask is None:  # Market not initialized
            self._reject_order(order, f"no market for {order.symbol}")
            return  # Cannot accept order

        # Check if market order and accept and fill immediately
        if order.type == OrderType.MARKET:
            self._process_market_order(order, market_bid, market_ask)
            return  # Market order filled - nothing further to process
        elif order.type == OrderType.LIMIT:
            self._process_limit_order(order, market_bid, market_ask)
        else:
            self._process_passive_order(order, market_bid, market_ask)

    cdef inline void _process_market_order(self, MarketOrder order, Price market_bid, Price market_ask) except *:
        self._accept_order(order)

        if order.side == OrderSide.BUY:
            if self.fill_model.is_slipped():
                self._fill_order(
                    order,
                    Price(market_ask + self._slippages[order.symbol]),
                    LiquiditySide.TAKER,
                )
            else:
                self._fill_order(order, market_ask, LiquiditySide.TAKER)
        elif order.side == OrderSide.SELL:
            if self.fill_model.is_slipped():
                self._fill_order(
                    order,
                    Price(market_bid - self._slippages[order.symbol]),
                    LiquiditySide.TAKER,
                )
            else:
                self._fill_order(order, market_bid, LiquiditySide.TAKER)
        else:
            raise RuntimeError(f"Invalid order side, was {OrderSideParser.to_str(order.side)}")

    cdef inline void _process_limit_order(self, LimitOrder order, Price market_bid, Price market_ask) except *:
        if order.side == OrderSide.BUY:
            if order.price >= market_ask:
                if order.is_post_only:
                    self._reject_order(order, f"BUY LIMIT order price of {order.price} is too "
                                              f"far from the market, ask={market_ask}")
                    return  # Invalid price
            elif order.price >= market_ask:
                self._accept_order(order)
                self._fill_order(order, market_bid, LiquiditySide.TAKER)
                return  # Filled
        elif order.side == OrderSide.SELL:
            if order.price <= market_bid:
                if order.is_post_only:
                    self._reject_order(order, f"SELL LIMIT order price of {order.price} is too "
                                              f"far from the market, bid={market_bid}")
                    return  # Invalid price
            elif order.price <= market_bid:
                self._accept_order(order)
                self._fill_order(order, market_bid, LiquiditySide.TAKER)
                return  # Filled

        # Order is valid and accepted
        self._accept_order(order)
        self._work_order(order)

    cdef inline void _process_passive_order(self, PassiveOrder order, Price market_bid, Price market_ask) except *:
        if order.side == OrderSide.BUY:
            if order.price < market_ask:
                self._reject_order(order, f"BUY STOP order price of {order.price} is too "
                                          f"far from the market, ask={market_ask}")
                return  # Invalid price
        elif order.side == OrderSide.SELL:
            if order.price > market_bid:
                self._reject_order(order, f"SELL STOP order price of {order.price} is too "
                                          f"far from the market, bid={market_bid}")
                return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)
        self._work_order(order)

    cdef inline void _work_order(self, PassiveOrder order) except *:
        # Order now becomes working
        self._working_orders[order.cl_ord_id] = order

        # Generate event
        cdef OrderWorking working = OrderWorking(
            self.exec_client.account_id,
            order.cl_ord_id,
            order.id,
            order.symbol,
            order.side,
            order.type,
            order.quantity,
            order.price,
            order.time_in_force,
            order.expire_time,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_client.handle_event(working)

    cdef inline void _auction_buy_order(self, PassiveOrder order, Price market) except *:
        if order.type == OrderType.STOP_MARKET:
            self._auction_buy_stop_order(order, market)
        elif order.type == OrderType.LIMIT:
            self._auction_buy_limit_order(order, market)
        else:
            raise RuntimeError("invalid order type")

    cdef inline void _auction_buy_stop_order(self, PassiveOrder order, Price market) except *:
        if market > order.price or self._is_marginal_stop_fill(order.price, market):
            del self._working_orders[order.cl_ord_id]  # Remove order from working orders
            if self.fill_model.is_slipped():
                self._fill_order(
                    order,
                    Price(order.price + self._slippages[order.symbol]),
                    LiquiditySide.TAKER,
                )
            else:
                self._fill_order(
                    order,
                    order.price,
                    LiquiditySide.TAKER,
                )

    cdef inline void _auction_buy_limit_order(self, PassiveOrder order, Price market) except *:
        if market < order.price or self._is_marginal_limit_fill(order.price, market):
            del self._working_orders[order.cl_ord_id]  # Remove order from working orders
            self._fill_order(
                order,
                order.price,
                LiquiditySide.MAKER,
            )

    cdef inline void _auction_sell_order(self, PassiveOrder order, Price market) except *:
        if order.type == OrderType.STOP_MARKET:
            self._auction_sell_stop_order(order, market)
        elif order.type == OrderType.LIMIT:
            self._auction_sell_limit_order(order, market)
        else:
            raise RuntimeError("invalid order type")

    cdef inline void _auction_sell_stop_order(self, PassiveOrder order, Price market) except *:
        if market < order.price or self._is_marginal_stop_fill(order.price, market):
            del self._working_orders[order.cl_ord_id]  # Remove order from working orders
            if self.fill_model.is_slipped():
                self._fill_order(
                    order,
                    Price(order.price - self._slippages[order.symbol]),
                    LiquiditySide.TAKER,
                )
            else:
                self._fill_order(
                    order,
                    order.price,
                    LiquiditySide.TAKER,
                )

    cdef inline void _auction_sell_limit_order(self, PassiveOrder order, Price market) except *:
        if market > order.price or self._is_marginal_limit_fill(order.price, market):
            del self._working_orders[order.cl_ord_id]  # Remove order from working orders
            self._fill_order(
                order,
                order.price,
                LiquiditySide.MAKER,
            )

    cdef inline bint _is_marginal_stop_fill(self, Price order_price, Price market) except *:
        return market == order_price and self.fill_model.is_stop_filled()

    cdef inline bint _is_marginal_limit_fill(self, Price order_price, Price market) except *:
        return market == order_price and self.fill_model.is_limit_filled()

    cdef inline void _fill_order(
            self,
            Order order,
            Price fill_price,
            LiquiditySide liquidity_side,
    ) except *:
        # Query if there is an existing position for this order
        cdef PositionId position_id = self._position_index.get(order.cl_ord_id)
        # position_id could be None here

        cdef Position position = None
        if position_id is None:
            position_id = self._generate_position_id(order.symbol)
            self._position_index[order.cl_ord_id] = position_id
        else:
            position = self.exec_cache.position(position_id)
            position_id = position.id

        # Calculate commission
        cdef Instrument instrument = self.instruments.get(order.symbol)
        if instrument is None:
            raise RuntimeError(f"Cannot run backtest, no instrument data for {order.symbol}")

        cdef Money commission = instrument.calculate_commission(
            order.quantity,
            fill_price,
            liquidity_side,
        )

        # Generate event
        cdef OrderFilled filled = OrderFilled(
            self.exec_client.account_id,
            order.cl_ord_id,
            order.id if order.id is not None else self._generate_order_id(order.symbol),
            self._generate_execution_id(),
            position_id,
            order.strategy_id,
            order.symbol,
            order.side,
            order.quantity,
            order.quantity,
            Quantity(),  # Not modeling partial fills yet
            fill_price,
            instrument.quote_currency,
            instrument.is_inverse,
            commission,  # In instrument settlement currency
            liquidity_side,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        # Calculate potential P&L
        cdef Money pnl = None
        if position is not None and position.entry != order.side:
            # Calculate P&L
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

            # Final P&L
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
                        self._cancel_order(order)
                del self._position_oco_orders[position.id]

        # Finally adjust account
        self.adjust_account(pnl)

    cdef inline void _clean_up_child_orders(self, ClientOrderId order_id) except *:
        # Clean up any residual child orders from the completed order associated
        # with the given identifier.
        if order_id in self._child_orders:
            del self._child_orders[order_id]

    cdef inline void _check_oco_order(self, ClientOrderId order_id) except *:
        # Check held OCO orders and remove any paired with the given order_id
        cdef ClientOrderId oco_order_id
        cdef ClientOrderId bracket_order_id
        cdef Order oco_order
        cdef Order order
        cdef list child_orders
        if order_id in self._oco_orders:
            oco_order_id = self._oco_orders[order_id]
            oco_order = self.exec_cache.order(oco_order_id)
            del self._oco_orders[order_id]
            del self._oco_orders[oco_order_id]

            # Reject any latent bracket child orders
            for bracket_order_id, child_orders in self._child_orders.items():
                for order in child_orders:
                    if oco_order == order and order.state_c() != OrderState.WORKING:
                        self._reject_oco_order(order, order_id)

            # Cancel any working OCO orders
            if oco_order_id in self._working_orders:
                self._cancel_oco_order(self._working_orders[oco_order_id], order_id)
                del self._working_orders[oco_order_id]

    cdef inline void _reject_oco_order(self, PassiveOrder order, ClientOrderId oco_order_id) except *:
        # order is the OCO order to reject
        # oco_order_id is the other order_id for this OCO pair
        if order.is_completed_c():
            self._log.debug(f"Cannot reject order, state was already {order.state_string_c()}.")
            return

        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.exec_client.account_id,
            order.cl_ord_id,
            self._clock.utc_now(),
            f"OCO order rejected from {oco_order_id}",
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_client.handle_event(rejected)

    cdef inline void _cancel_oco_order(self, PassiveOrder order, ClientOrderId oco_order_id) except *:
        # order is the OCO order to cancel
        # oco_order_id is the other order_id for this OCO pair
        if order.is_completed_c():
            self._log.debug(f"Cannot cancel order, state was already {order.state_string_c()}.")
            return

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            self.exec_client.account_id,
            order.cl_ord_id,
            order.id,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.debug(f"Cancelling {order.cl_ord_id} OCO order from {oco_order_id}.")
        self.exec_client.handle_event(cancelled)

    cdef inline void _cancel_order(self, PassiveOrder order) except *:
        if order.is_completed_c():
            self._log.debug(f"Cannot cancel order, state was already {order.state_string_c()}.")
            return

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            self.exec_client.account_id,
            order.cl_ord_id,
            order.id,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.debug(f"Cancelling {order.cl_ord_id} as linked position closed.")
        self.exec_client.handle_event(cancelled)
