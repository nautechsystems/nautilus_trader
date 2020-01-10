# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import datetime as dt
import pytz

from cpython.datetime cimport datetime, timedelta
from typing import List, Dict

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport ValidString
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.currency cimport Currency, currency_from_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.market_position cimport MarketPosition, market_position_to_string
from nautilus_trader.model.identifiers cimport Symbol, OrderIdBroker
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.model.objects cimport Decimal, Price, Tick, Money, Instrument, Quantity
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.events cimport (
    OrderFillEvent,
    AccountStateEvent,
    OrderSubmitted,
    OrderAccepted,
    OrderRejected,
    OrderWorking,
    OrderExpired,
    OrderModified,
    OrderCancelled,
    OrderCancelReject,
    OrderFilled
)
from nautilus_trader.model.identifiers cimport OrderId, ExecutionId, PositionIdBroker
from nautilus_trader.model.commands cimport (
    AccountInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    ModifyOrder,
    CancelOrder
)
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.brokerage cimport CommissionCalculator, RolloverInterestCalculator
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.guid cimport TestGuidFactory
from nautilus_trader.common.logger cimport Logger
from nautilus_trader.common.execution cimport ExecutionDatabase, ExecutionEngine, ExecutionClient
from nautilus_trader.backtest.config cimport BacktestConfig
from nautilus_trader.backtest.models cimport FillModel

# Stop order types
cdef set STOP_ORDER_TYPES = {
    OrderType.STOP_MARKET,
    OrderType.STOP_LIMIT,
    OrderType.MIT}


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """

    def __init__(self,
                 ExecutionEngine exec_engine,
                 dict instruments: Dict[Symbol, Instrument],
                 BacktestConfig config,
                 FillModel fill_model,
                 TestClock clock,
                 TestGuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the BacktestExecClient class.

        :param exec_engine: The execution engine for the backtest.
        :param instruments: The instruments needed for the backtest.
        :param config: The backtest configuration.
        :param fill_model: The fill model for the backtest.
        :param clock: The clock for the component.
        :param clock: The GUID factory for the component.
        :param logger: The logger for the component.
        :raises ConditionFailed: If the instruments list contains a type other than Instrument.
        """
        Condition.dict_types(instruments, Symbol, Instrument, 'instruments')

        super().__init__(exec_engine, logger)

        self._clock = clock
        self._guid_factory = guid_factory

        self.instruments = instruments

        self.day_number = 0
        self.rollover_time = None
        self.rollover_applied = False
        self.frozen_account = config.frozen_account
        self.starting_capital = config.starting_capital
        self.account_currency = config.account_currency
        self.account_capital = config.starting_capital
        self.account_cash_start_day = config.starting_capital
        self.account_cash_activity_day = Money.zero()

        self._account = Account(self.reset_account_event())
        self.exec_db = None
        self.exchange_calculator = ExchangeRateCalculator()
        self.commission_calculator = CommissionCalculator(default_rate_bp=config.commission_rate_bp)
        self.rollover_calculator = RolloverInterestCalculator(config.short_term_interest_csv_path)
        self.rollover_spread = 0.0 # Bank + Broker spread markup
        self.total_commissions = Money.zero()
        self.total_rollover = Money.zero()
        self.fill_model = fill_model

        self._market = {}               # type: Dict[Symbol, Tick]
        self._working_orders = {}       # type: Dict[OrderId, Order]
        self._atomic_child_orders = {}  # type: Dict[OrderId, List[Order]]
        self._oco_orders = {}           # type: Dict[OrderId, OrderId]

        self._set_slippage_index()

    cpdef datetime time_now(self):
        """
        Return the current time for the execution client.

        :return: datetime.
        """
        return self._clock.time_now()

    cpdef void register_exec_db(self, ExecutionDatabase exec_db) except *:
        """
        Register the given execution database with the client.
        """
        self.exec_db = exec_db

    cpdef void connect(self) except *:
        """
        Connect to the execution service.
        """
        self._log.info("Connected.")
        # Do nothing else

    cpdef void disconnect(self) except *:
        """
        Disconnect from the execution service.
        """
        self._log.info("Disconnected.")
        # Do nothing else

    cpdef void change_fill_model(self, FillModel fill_model) except *:
        """
        Set the fill model to be the given model.
        
        :param fill_model: The fill model to set.
        """
        self.fill_model = fill_model

    cpdef void process_tick(self, Tick tick) except *:
        """
        Process the execution client with the given tick. Market dynamics are
        simulated against working orders.
        
        :param tick: The tick data to process with.
        """
        self._clock.set_time(tick.timestamp)
        self._market[tick.symbol] = tick

        cdef datetime time_now = self._clock.time_now()

        if self.day_number != time_now.day:
            # Set account statistics for new day
            self.day_number = time_now.day
            self.account_cash_start_day = self._account.cash_balance
            self.account_cash_activity_day = Money.zero()
            self.rollover_applied = False
            self.rollover_time = dt.datetime(
                time_now.year,
                time_now.month,
                time_now.day,
                17,
                0,
                0,
                0,
                tzinfo=pytz.timezone('US/Eastern')).astimezone(tz=pytz.utc) - timedelta(minutes=56) # TODO: Why is this consistently 56 min out?

        if not self.rollover_applied and time_now >= self.rollover_time:
            try:
                self.rollover_applied = True
                self._apply_rollover_interest(time_now, self.rollover_time.isoweekday())
            except RuntimeError as ex:
                # Cannot calculate rollover interest
                self._log.error(str(ex))

        # Simulate market
        cdef OrderId order_id
        cdef Order order
        cdef Instrument instrument
        for order_id, order in self._working_orders.copy().items():  # Copies dict to avoid resize during loop
            if order.symbol != tick.symbol:
                continue  # Order is for a different symbol
            if order.state != OrderState.WORKING:
                continue  # Orders state has changed since the loop commenced

            instrument = self.instruments[order.symbol]
            # Check for order fill
            if order.side == OrderSide.BUY:
                if order.type in STOP_ORDER_TYPES:
                    if tick.ask > order.price or (tick.ask.equals(order.price) and self.fill_model.is_stop_filled()):
                        del self._working_orders[order.id]  # Remove order from working orders
                        if self.fill_model.is_slipped():
                            self._fill_order(order, Price(order.price.value + self._slippage_index[order.symbol], instrument.tick_precision))
                        else:
                            self._fill_order(order, order.price)
                        continue  # Continue loop to next order
                elif order.type == OrderType.LIMIT:
                    if tick.ask < order.price or (tick.ask.equals(order.price) and self.fill_model.is_limit_filled()):
                        del self._working_orders[order.id]  # Remove order from working orders
                        if self.fill_model.is_slipped():
                            self._fill_order(order, Price(order.price.value + self._slippage_index[order.symbol], instrument.tick_precision))
                        else:
                            self._fill_order(order, order.price)
                        continue  # Continue loop to next order
            elif order.side == OrderSide.SELL:
                if order.type in STOP_ORDER_TYPES:
                    if tick.bid < order.price or (tick.bid.equals(order.price) and self.fill_model.is_stop_filled()):
                        del self._working_orders[order.id]  # Remove order from working orders
                        if self.fill_model.is_slipped():
                            self._fill_order(order, Price(order.price.value - self._slippage_index[order.symbol], instrument.tick_precision))
                        else:
                            self._fill_order(order, order.price)
                        continue  # Continue loop to next order
                elif order.type == OrderType.LIMIT:
                    if tick.bid > order.price or (tick.bid.equals(order.price) and self.fill_model.is_limit_filled()):
                        del self._working_orders[order.id]  # Remove order from working orders
                        if self.fill_model.is_slipped():
                            self._fill_order(order, Price(order.price.value - self._slippage_index[order.symbol], instrument.tick_precision))
                        else:
                            self._fill_order(order, order.price)
                        continue  # Continue loop to next order

            # Check for order expiry
            if order.expire_time is not None and time_now >= order.expire_time:
                if order.id in self._working_orders:  # Order may have been removed since loop started
                    del self._working_orders[order.id]
                    self._expire_order(order)

    cpdef void check_residuals(self) except *:
        """
        Check for any residual objects and log warnings if any are found.
        """
        for order_list in self._atomic_child_orders.values():
            for order in order_list:
                self._log.warning(f"Residual child-order {order}")

        for order_id in self._oco_orders.values():
            self._log.warning(f"Residual OCO {order_id}")

    cpdef void reset(self) except *:
        """
        Return the client to its initial state preserving tick data.
        """
        self._log.info(f"Resetting...")
        self._reset()
        self.day_number = 0
        self.account_capital = self.starting_capital
        self.account_cash_start_day = self.account_capital
        self.account_cash_activity_day = Money.zero()
        self._account = Account(self.reset_account_event())
        self.total_commissions = Money.zero()
        self._working_orders = {}       # type: Dict[OrderId, Order]
        self._atomic_child_orders = {}  # type: Dict[OrderId, List[Order]]
        self._oco_orders = {}           # type: Dict[OrderId, OrderId]

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        TBD.
        """
        pass

    cdef AccountStateEvent reset_account_event(self):
        """
        Resets the account.
        """
        return AccountStateEvent(
            self._exec_engine.account_id,
            self.account_currency,
            self.starting_capital,
            self.starting_capital,
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Decimal.zero(),
            ValidString('N'),
            self._guid_factory.generate(),
            self._clock.time_now())

    cdef void _set_slippage_index(self) except *:
        cdef dict slippage_index = {}  # type: Dict[Symbol, float]

        for symbol, instrument in self.instruments.items():
            slippage_index[symbol] = instrument.tick_size.value

        self._slippage_index = slippage_index


# -- COMMAND EXECUTION --------------------------------------------------------------------------- #

    cpdef void account_inquiry(self, AccountInquiry command) except *:
        # Generate event
        cdef AccountStateEvent event = AccountStateEvent(
            self._account.id,
            self._account.currency,
            self._account.cash_balance,
            self.account_cash_start_day,
            self.account_cash_activity_day,
            self._account.margin_used_liquidation,
            self._account.margin_used_maintenance,
            self._account.margin_ratio,
            self._account.margin_call_status,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_engine.handle_event(event)

    cpdef void submit_order(self, SubmitOrder command) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            command.account_id,
            command.order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_engine.handle_event(submitted)
        self._process_order(command.order)

    cpdef void submit_atomic_order(self, SubmitAtomicOrder command) except *:
        cdef list atomic_orders = [command.atomic_order.stop_loss]
        if command.atomic_order.has_take_profit:
            atomic_orders.append(command.atomic_order.take_profit)
            self._oco_orders[command.atomic_order.take_profit.id] = command.atomic_order.stop_loss.id
            self._oco_orders[command.atomic_order.stop_loss.id] = command.atomic_order.take_profit.id

        self._atomic_child_orders[command.atomic_order.entry.id] = atomic_orders

        # Generate command
        cdef SubmitOrder submit_order = SubmitOrder(
            command.trader_id,
            command.account_id,
            command.strategy_id,
            command.position_id,
            command.atomic_order.entry,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.submit_order(submit_order)

    cpdef void cancel_order(self, CancelOrder command) except *:
        if command.order_id not in self._working_orders:
            self._cancel_reject_order(command.order_id, 'cancel order', 'order not found')
            return  # Rejected the cancel order command

        cdef Order order = self._working_orders[command.order_id]

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            command.account_id,
            order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        # Remove from working orders (checked it was in dictionary above)
        del self._working_orders[command.order_id]

        self._exec_engine.handle_event(cancelled)
        self._check_oco_order(command.order_id)

    cpdef void modify_order(self, ModifyOrder command) except *:
        if command.order_id not in self._working_orders:
            self._cancel_reject_order(command.order_id, 'modify order', 'order not found')
            return  # Rejected the modify order command

        cdef Order order = self._working_orders[command.order_id]
        cdef Instrument instrument = self.instruments[order.symbol]

        if command.modified_quantity.value == 0:
            self._cancel_reject_order(order, 'modify order', f'modified quantity {command.modified_quantity} invalid')
            return  # Cannot modify order

        cdef Price current_ask
        cdef Price current_bid
        if order.side == OrderSide.BUY:
            current_ask = self._market[order.symbol].ask
            if order.type in STOP_ORDER_TYPES:
                if order.price.value < current_ask.value + (instrument.min_stop_distance * instrument.tick_size.value):
                    self._cancel_reject_order(order, 'modify order', f'BUY STOP order price of {order.price} is too far from the market, ask={current_ask}')
                    return  # Cannot modify order
            elif order.type == OrderType.LIMIT:
                if order.price.value > current_ask.value + (instrument.min_limit_distance * instrument.tick_size.value):
                    self._cancel_reject_order(order, 'modify order', f'BUY LIMIT order price of {order.price} is too far from the market, ask={current_ask}')
                    return  # Cannot modify order
        elif order.side == OrderSide.SELL:
            current_bid = self._market[order.symbol].bid
            if order.type in STOP_ORDER_TYPES:
                if order.price.value > current_bid.value - (instrument.min_stop_distance * instrument.tick_size.value):
                    self._cancel_reject_order(order, 'modify order', f'SELL STOP order price of {order.price} is too far from the market, bid={current_bid}')
                    return  # Cannot modify order
            elif order.type == OrderType.LIMIT:
                if order.price.value < current_bid.value - (instrument.min_limit_distance * instrument.tick_size.value):
                    self._cancel_reject_order(order, 'modify order', f'SELL LIMIT order price of {order.price} is too far from the market, bid={current_bid}')
                    return  # Cannot modify order

        # Generate event
        cdef OrderModified modified = OrderModified(
            command.account_id,
            order.id,
            order.id_broker,
            command.modified_quantity,
            command.modified_price,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_engine.handle_event(modified)


# -- EVENT HANDLING ------------------------------------------------------------------------------ #

    cdef void _accept_order(self, Order order) except *:
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self._account.id,
            order.id,
            OrderIdBroker('B' + order.id.value),
            order.label,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_engine.handle_event(accepted)

    cdef void _reject_order(self, Order order, str reason) except *:
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self._account.id,
            order.id,
            self._clock.time_now(),
            ValidString(reason),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_engine.handle_event(rejected)
        self._check_oco_order(order.id)
        self._clean_up_child_orders(order.id)

    cdef void _cancel_reject_order(
            self,
            OrderId order_id,
            str response,
            str reason) except *:
        # Generate event
        cdef OrderCancelReject cancel_reject = OrderCancelReject(
            self._account.id,
            order_id,
            self._clock.time_now(),
            ValidString(response),
            ValidString(reason),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_engine.handle_event(cancel_reject)

    cdef void _expire_order(self, Order order) except *:
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            self._account.id,
            order.id,
            order.expire_time,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_engine.handle_event(expired)

        cdef OrderId first_child_order_id
        cdef OrderId other_oco_order_id
        if order.id in self._atomic_child_orders:
            # Remove any unprocessed atomic child order OCO identifiers
            first_child_order_id = self._atomic_child_orders[order.id][0].id
            if first_child_order_id in self._oco_orders:
                other_oco_order_id = self._oco_orders[first_child_order_id]
                del self._oco_orders[first_child_order_id]
                del self._oco_orders[other_oco_order_id]
        else:
            self._check_oco_order(order.id)
        self._clean_up_child_orders(order.id)

    cdef void _process_order(self, Order order) except *:
        """
        Work the given order.
        """
        Condition.not_in(order.id, self._working_orders, 'order.id', 'working_orders')

        cdef Instrument instrument = self.instruments[order.symbol]

        # Check order size is valid or reject
        if order.quantity > instrument.max_trade_size:
            self._reject_order(order,  f'order quantity of {order.quantity} exceeds the maximum trade size of {instrument.max_trade_size}')
            return  # Cannot accept order
        if order.quantity < instrument.min_trade_size:
            self._reject_order(order,  f'order quantity of {order.quantity} is less than the minimum trade size of {instrument.min_trade_size}')
            return  # Cannot accept order

        cdef Tick current_market = self._market.get(order.symbol, None)

        # Check market exists
        if not current_market:  # Market not initialized
            self._reject_order(order,  f'no market for {order.symbol}')
            return  # Cannot accept order

        # Check order price is valid or reject
        if order.side == OrderSide.BUY:
            if order.type == OrderType.MARKET:
                # Accept and fill market orders immediately
                self._accept_order(order)
                if self.fill_model.is_slipped():
                    self._fill_order(order, Price(current_market.ask.value + self._slippage_index[order.symbol], instrument.tick_precision))
                else:
                    self._fill_order(order, current_market.ask)
                return  # Order filled - nothing further to process
            elif order.type in STOP_ORDER_TYPES:
                if order.price.value < current_market.ask.value + (instrument.min_stop_distance_entry * instrument.tick_size.value):
                    self._reject_order(order,  f'BUY STOP order price of {order.price} is too far from the market, ask={current_market.ask}')
                    return  # Cannot accept order
            elif order.type == OrderType.LIMIT:
                if order.price.value > current_market.ask.value + (instrument.min_limit_distance_entry * instrument.tick_size.value):
                    self._reject_order(order,  f'BUY LIMIT order price of {order.price} is too far from the market, ask={current_market.ask}')
                    return  # Cannot accept order
        elif order.side == OrderSide.SELL:
            if order.type == OrderType.MARKET:
                # Accept and fill market orders immediately
                self._accept_order(order)
                if self.fill_model.is_slipped():
                    self._fill_order(order, Price(current_market.bid - self._slippage_index[order.symbol], instrument.tick_precision))
                else:
                    self._fill_order(order, current_market.bid)
                return  # Order filled - nothing further to process
            elif order.type in STOP_ORDER_TYPES:
                if order.price.value > current_market.bid.value - (instrument.min_stop_distance_entry * instrument.tick_size.value):
                    self._reject_order(order,  f'SELL STOP order price of {order.price} is too far from the market, bid={current_market.bid}')
                    return  # Cannot accept order
            elif order.type == OrderType.LIMIT:
                if order.price.value < current_market.bid.value - (instrument.min_limit_distance_entry * instrument.tick_size.value):
                    self._reject_order(order,  f'SELL LIMIT order price of {order.price} is too far from the market, bid={current_market.bid}')
                    return  # Cannot accept order

        # Order is valid and accepted
        self._accept_order(order)

        # Order now becomes working
        self._working_orders[order.id] = order

        # Generate event
        cdef OrderWorking working = OrderWorking(
            self._account.id,
            order.id,
            OrderIdBroker('B' + order.id.value),
            order.symbol,
            order.label,
            order.side,
            order.type,
            order.quantity,
            order.price,
            order.time_in_force,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now(),
            order.expire_time)

        self._exec_engine.handle_event(working)

    cdef void _fill_order(self, Order order, Price fill_price) except *:
        # Generate event
        cdef OrderFilled filled = OrderFilled(
            self._account.id,
            order.id,
            ExecutionId('E-' + order.id.value),
            PositionIdBroker('ET-' + order.id.value),
            order.symbol,
            order.side,
            order.quantity,
            fill_price,
            self.instruments[order.symbol].base_currency,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        # Adjust account if position exists and opposite order side
        cdef Position position = self._exec_engine.database.get_position_for_order(order.id)
        if position is not None and position.entry_direction != order.side:
            self._adjust_account(filled, position)

        self._exec_engine.handle_event(filled)
        self._check_oco_order(order.id)

        # Work any atomic child orders
        if order.id in self._atomic_child_orders:
            for child_order in self._atomic_child_orders[order.id]:
                if not child_order.is_completed:  # The order may already be cancelled or rejected
                    self._process_order(child_order)
            del self._atomic_child_orders[order.id]

    cdef void _clean_up_child_orders(self, OrderId order_id) except *:
        # Clean up any residual child orders from the completed order associated
        # with the given identifier.
        if order_id in self._atomic_child_orders:
            del self._atomic_child_orders[order_id]

    cdef void _check_oco_order(self, OrderId order_id) except *:
        # Check held OCO orders and remove any paired with the given order_id
        cdef OrderId oco_order_id
        cdef Order oco_order

        if order_id in self._oco_orders:
            oco_order_id = self._oco_orders[order_id]
            oco_order = self._exec_engine.database.get_order(oco_order_id)
            del self._oco_orders[order_id]
            del self._oco_orders[oco_order_id]

            # Reject any latent atomic child orders
            for atomic_order_id, child_orders in self._atomic_child_orders.items():
                for order in child_orders:
                    if oco_order.equals(order):
                        self._reject_oco_order(order, order_id)

            # Cancel any working OCO orders
            if oco_order_id in self._working_orders:
                self._cancel_oco_order(self._working_orders[oco_order_id], order_id)
                del self._working_orders[oco_order_id]

    cdef void _reject_oco_order(self, Order order, OrderId oco_order_id) except *:
        # order is the OCO order to reject
        # oco_order_id is the other order_id for this OCO pair

        # Generate event
        cdef OrderRejected event = OrderRejected(
            self._account.id,
            order.id,
            self._clock.time_now(),
            ValidString(f"OCO order rejected from {oco_order_id}"),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_engine.handle_event(event)

    cdef void _cancel_oco_order(self, Order order, OrderId oco_order_id) except *:
        # order is the OCO order to cancel
        # oco_order_id is the other order_id for this OCO pair

        # Generate event
        cdef OrderCancelled event = OrderCancelled(
            self._account.id,
            order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._log.debug(f"OCO order cancelled from {oco_order_id}.")
        self._exec_engine.handle_event(event)

    cdef void _adjust_account(self, OrderFillEvent event, Position position) except *:
        # Calculate commission
        cdef Instrument instrument = self.instruments[event.symbol]
        cdef float exchange_rate = self.exchange_calculator.get_rate(
            from_currency=instrument.base_currency,
            to_currency=self._account.currency,
            price_type=PriceType.BID if event.order_side is OrderSide.SELL else PriceType.ASK,
            bid_rates=self._build_current_bid_rates(),
            ask_rates=self._build_current_ask_rates())

        cdef Money pnl = self._calculate_pnl(
            direction=position.market_position,
            open_price=position.average_open_price,
            close_price=event.average_price.value,
            quantity=event.filled_quantity,
            exchange_rate=exchange_rate)

        cdef Money commission = self.commission_calculator.calculate(
            symbol=event.symbol,
            filled_quantity=event.filled_quantity,
            filled_price=event.average_price,
            exchange_rate=exchange_rate)

        self.total_commissions.subtract(commission)
        pnl.subtract(commission)

        cdef AccountStateEvent account_event
        if not self.frozen_account:
            self.account_capital = self.account_capital.add(pnl)
            self.account_cash_activity_day = self.account_cash_activity_day.add(pnl)

            account_event = AccountStateEvent(
                self._account.id,
                self._account.currency,
                self.account_capital,
                self.account_cash_start_day,
                self.account_cash_activity_day,
                margin_used_liquidation=Money.zero(),
                margin_used_maintenance=Money.zero(),
                margin_ratio=Decimal.zero(),
                margin_call_status=ValidString('N'),
                event_id=self._guid_factory.generate(),
                event_timestamp=self._clock.time_now())

            self._exec_engine.handle_event(account_event)

    cdef void _apply_rollover_interest(self, datetime timestamp, int iso_week_day) except *:
        # Apply rollover interest for all open positions
        if self.exec_db is None:
            self._log.error("Cannot apply rollover interest (no execution database registered).")
            return

        cdef dict open_positions = self.exec_db.get_positions_open()

        cdef Instrument instrument
        cdef Currency base_currency
        cdef float interest_rate
        cdef float exchange_rate
        cdef float rollover
        cdef float rollover_cumulative = 0.0
        cdef float mid_price
        cdef dict mid_prices = {}
        cdef Tick market
        for position in open_positions.values():
            instrument = self.instruments[position.symbol]
            if instrument.security_type == SecurityType.FOREX:
                mid_price = mid_prices.get(instrument.symbol, 0.0)
                if mid_price == 0.0:
                    market = self._market[instrument.symbol]
                    mid_price = (market.ask.as_float() + market.bid.as_float()) / 2.0
                    mid_prices[instrument.symbol] = mid_price
                quote_currency = currency_from_string(position.symbol.code[3:])
                interest_rate = self.rollover_calculator.calc_overnight_rate(position.symbol, timestamp)
                exchange_rate = self.exchange_calculator.get_rate(
                        from_currency=quote_currency,
                        to_currency=self._account.currency,
                        price_type=PriceType.MID,
                        bid_rates=self._build_current_bid_rates(),
                        ask_rates=self._build_current_ask_rates())
                rollover = mid_price * position.quantity.value * interest_rate * exchange_rate
                # Apply any bank and broker spread markup (basis points)
                rollover_cumulative += rollover - (rollover * self.rollover_spread)

        if iso_week_day == 3: # Book triple for Wednesdays
            rollover_cumulative = rollover_cumulative * 3.0
        elif iso_week_day == 5: # Book triple for Fridays (holding over weekend)
            rollover_cumulative = rollover_cumulative * 3.0

        cdef Money rollover_final = Money(rollover_cumulative)
        self.total_rollover = self.total_rollover.add(rollover_final)

        cdef AccountStateEvent account_event
        if not self.frozen_account:
            self.account_capital = self.account_capital.add(rollover_final)
            self.account_cash_activity_day = self.account_cash_activity_day.add(rollover_final)

            account_event = AccountStateEvent(
                self._account.id,
                self._account.currency,
                self.account_capital,
                self.account_cash_start_day,
                self.account_cash_activity_day,
                margin_used_liquidation=Money.zero(),
                margin_used_maintenance=Money.zero(),
                margin_ratio=Decimal.zero(),
                margin_call_status=ValidString('N'),
                event_id=self._guid_factory.generate(),
                event_timestamp=self._clock.time_now())

            self._exec_engine.handle_event(account_event)

    cdef dict _build_current_bid_rates(self):
        """
        Return the current currency bid rates in the markets.
        
        :return: Dict[Symbol, float].
        """
        cdef Symbol symbol
        cdef Tick tick
        return {symbol.code: tick.bid.as_float() for symbol, tick in self._market.items()}

    cdef dict _build_current_ask_rates(self):
        """
        Return the current currency ask rates in the markets.
        
        :return: Dict[Symbol, float].
        """
        cdef Symbol symbol
        cdef Tick tick
        return {symbol.code: tick.ask.as_float() for symbol, tick in self._market.items()}

    cdef Money _calculate_pnl(
            self,
            MarketPosition direction,
            float open_price,
            float close_price,
            Quantity quantity,
            float exchange_rate):
        cdef float difference
        if direction == MarketPosition.LONG:
            difference = close_price - open_price
        elif direction == MarketPosition.SHORT:
            difference = open_price - close_price
        else:
            raise ValueError(f'Cannot calculate the pnl of a {market_position_to_string(direction)} direction.')

        return Money(difference * quantity.value * exchange_rate)
