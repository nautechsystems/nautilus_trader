#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import pandas as pd

from decimal import Decimal
from cpython.datetime cimport datetime
from collections import deque
from functools import partial
from pandas import DataFrame
from typing import List, Dict

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.brokerage cimport Broker
from inv_trader.enums.quote_type cimport QuoteType
from inv_trader.enums.order_type cimport OrderType
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.market_position cimport MarketPosition, market_position_string
from inv_trader.model.currency cimport CurrencyCalculator
from inv_trader.model.objects cimport ValidString, Symbol, Price, Money, Instrument, Quantity
from inv_trader.model.order cimport Order
from inv_trader.model.position cimport Position
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent
from inv_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderFilled
from inv_trader.model.identifiers cimport OrderId, ExecutionId, ExecutionTicket, AccountNumber
from inv_trader.common.account cimport Account
from inv_trader.common.brokerage cimport CommissionCalculator
from inv_trader.common.clock cimport TestClock
from inv_trader.common.guid cimport TestGuidFactory
from inv_trader.common.logger cimport Logger
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.commands cimport Command, CollateralInquiry
from inv_trader.commands cimport SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from inv_trader.portfolio.portfolio cimport Portfolio

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
                 list instruments: List[Instrument],
                 dict data_ticks: Dict[Symbol, DataFrame],
                 dict data_bars_bid: Dict[Symbol, DataFrame],
                 dict data_bars_ask: Dict[Symbol, DataFrame],
                 Money starting_capital,
                 int slippage_ticks,
                 CommissionCalculator commission_calculator,
                 Account account,
                 Portfolio portfolio,
                 TestClock clock,
                 TestGuidFactory guid_factory,
                 Logger logger):
        """
        Initializes a new instance of the BacktestExecClient class.

        :param instruments: The instruments needed for the backtest.
        :param data_ticks: The historical tick market data needed for the backtest.
        :param data_bars_bid: The historical minute bid bars data needed for the backtest.
        :param data_bars_ask: The historical minute ask bars data needed for the backtest.
        :param starting_capital: The starting capital for the backtest account (> 0).
        :param slippage_ticks: The slippage for each order fill in ticks (>= 0).
        :param commission_calculator: The commission calculator.
        :param clock: The clock for the component.
        :param clock: The GUID factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the instruments list contains a type other than Instrument.
        :raises ValueError: If the data_ticks contains a key other than Symbol or value other than DataFrame.
        :raises ValueError: If the data_bars_bid contains a key other than Symbol or value other than DataFrame.
        :raises ValueError: If the data_bars_ask contains a key other than Symbol or value other than DataFrame.
        :raises ValueError: If the starting capital is not positive (> 0).
        :raises ValueError: If the slippage_ticks is negative (< 0).
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.dict_types(data_ticks, Symbol, DataFrame, 'data_ticks')
        Precondition.dict_types(data_bars_bid, Symbol, DataFrame, 'data_bars_bid')
        Precondition.dict_types(data_bars_ask, Symbol, DataFrame, 'data_bars_ask')
        Precondition.not_negative(slippage_ticks, 'slippage_ticks')

        super().__init__(account,
                         portfolio,
                         clock,
                         guid_factory,
                         logger)
        self._message_bus = deque()

        # Convert instruments list to dictionary indexed by symbol
        cdef dict instruments_dict = {}      # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument

        self.instruments = instruments_dict  # type: Dict[Symbol, Instrument]

        # Prepare data
        self.data_ticks = data_ticks                                          # type: Dict[Symbol, DataFrame]
        self.data_bars_bid = self._prepare_minute_data(data_bars_bid, 'bid')  # type: Dict[Symbol, List]
        self.data_bars_ask = self._prepare_minute_data(data_bars_ask, 'ask')  # type: Dict[Symbol, List]

        # Set minute data index
        first_dataframe = data_bars_bid[next(iter(data_bars_bid))]
        self.data_minute_index = list(pd.to_datetime(first_dataframe.index, utc=True))  # type: List[datetime]

        assert(isinstance(self.data_minute_index[0], datetime))

        self.iteration = 0
        self.day_number = 0
        self.starting_capital = starting_capital
        self.account_capital = starting_capital
        self.account_cash_start_day = self.account_capital
        self.account_cash_activity_day = Money(0)
        self.currency_calculator = CurrencyCalculator()
        self.commission_calculator = commission_calculator
        self.total_commissions = Money(0)
        self.slippage_index = {}       # type: Dict[Symbol, Decimal]
        self.working_orders = {}       # type: Dict[OrderId, Order]
        self.atomic_child_orders = {}  # type: Dict[OrderId, List[Order]]
        self.oco_orders = {}           # type: Dict[OrderId, OrderId]

        self._set_slippage_index(slippage_ticks)
        self.reset_account()

    cpdef void connect(self):
        """
        Connect to the execution service.
        """
        self._log.info("Connected.")

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        self._log.info("Disconnected.")

    cpdef void set_initial_iteration(
            self,
            datetime to_time,
            timedelta time_step):
        """
        Wind the execution clients iteration forwards to the given to_time with 
        the given time_step.

        :param to_time: The time to wind the execution client to.
        :param time_step: The time step to iterate at.
        """
        cdef datetime current = self.data_minute_index[0]
        cdef int next_index = 0

        while current < to_time:
            if self.data_minute_index[next_index] == current:
                next_index += 1
                self.iteration += 1
            current += time_step

        self._clock.set_time(current)

    cpdef void iterate(self):
        """
        Iterate the data client one time step.
        """
        cdef CollateralInquiry command

        cdef datetime time_now = self._clock.time_now()
        if self.day_number is not time_now.day:
            # Set account statistics
            self.day_number = time_now.day
            self.account_cash_start_day = self._account.cash_balance
            self.account_cash_activity_day = Money(0)

            # Generate command
            command = CollateralInquiry(
            self._guid_factory.generate(),
            self._clock.time_now())
            self._collateral_inquiry(command)

        # Simulate market dynamics
        cdef Price highest_ask
        cdef Price lowest_bid

        for order_id, order in self.working_orders.copy().items():  # Copies dict to avoid resize during loop
            # Check for order fill
            if order.side is OrderSide.BUY:
                highest_ask = self._get_highest_ask(order.symbol)
                if order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                    if highest_ask >= order.price:
                        del self.working_orders[order.id]  # Remove from working orders
                        self._fill_order(order, Price(order.price + self.slippage_index[order.symbol]))
                        continue  # To next order
                elif order.type is OrderType.LIMIT:
                    if highest_ask <= order.price:
                        del self.working_orders[order.id]  # Remove from working orders
                        self._fill_order(order, Price(order.price + self.slippage_index[order.symbol]))
                        continue  # To next order
            elif order.side is OrderSide.SELL:
                lowest_bid = self._get_lowest_bid(order.symbol)
                if order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                    if lowest_bid <= order.price:
                        del self.working_orders[order.id]  # Remove from working orders
                        self._fill_order(order, Price(order.price - self.slippage_index[order.symbol]))
                        continue  # To next order
                elif order.type is OrderType.LIMIT:
                    if lowest_bid >= order.price:
                        del self.working_orders[order.id]  # Remove from working orders
                        self._fill_order(order, Price(order.price - self.slippage_index[order.symbol]))
                        continue  # To next order

            # Check for order expiry
            if order.expire_time is not None and time_now >= order.expire_time:
                del self.working_orders[order.id]
                self._expire_order(order)

        self.iteration += 1

    cpdef void execute_command(self, Command command):
        """
        Execute the given command by inserting it into the message bus for processing.
        
        :param command: The command to execute.
        """
        self._message_bus.appendleft(command)

    cpdef void handle_event(self, Event event):
        """
        Handle the given event by inserting it into the message bus for processing.
        
        :param event: The event to handle
        """
        self._message_bus.appendleft(event)

    cpdef void process(self):
        """
        Process the message bus of commands and events.
        """
        while self._message_bus:
            item = self._message_bus.pop()  # Removes from right side of deque
            if isinstance(item, Event):
                self._handle_event(item)
            elif isinstance(item, Command):
                self._execute_command(item)

    cpdef void reset_account(self):
        """
        Resets the account.
        """
        cdef AccountEvent initial_starting = AccountEvent(
            self._account.id,
            Broker.SIMULATED,
            AccountNumber('9999'),
            self._account.currency,
            self.starting_capital,
            self.starting_capital,
            Money(0),
            Money(0),
            Money(0),
            Decimal(0),
            ValidString(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._account.apply(initial_starting)

    cpdef void reset(self):
        """
        Reset the execution client by returning all stateful internal values to their
        initial values, whilst preserving any constructed tick data.
        """
        self._log.info(f"Resetting...")
        self._reset()
        self._message_bus = deque()
        self.iteration = 0
        self.day_number = 0
        self.account_capital = self.starting_capital
        self.account_cash_start_day = self.account_capital
        self.account_cash_activity_day = Money(0)
        self.total_commissions = Money(0)
        self.working_orders = {}       # type: Dict[OrderId, Order]
        self.atomic_child_orders = {}  # type: Dict[OrderId, List[Order]]
        self.oco_orders = {}           # type: Dict[OrderId, OrderId]

        self.reset_account()
        self._log.info("Reset.")

    cdef void _collateral_inquiry(self, CollateralInquiry command):
        """
        Send a collateral inquiry command to the execution service.
        """
        # Generate event
        cdef AccountEvent event = AccountEvent(
            self._account.id,
            self._account.broker,
            self._account.account_number,
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
        self.handle_event(event)

    cdef void _submit_order(self, SubmitOrder command):
        """
        Send a submit order request to the execution service.
        
        :param command: The command to execute.
        """
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            command.order.symbol,
            command.order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())
        self.handle_event(submitted)

        self._accept_order(command.order)

    cdef void _submit_atomic_order(self, SubmitAtomicOrder command):
        """
        Send a submit atomic order command to the mock execution service.
        
        :param command: The command to execute.
        """
        cdef list atomic_orders = [command.atomic_order.stop_loss]
        if command.atomic_order.has_profit_target:
            atomic_orders.append(command.atomic_order.profit_target)
            self.oco_orders[command.atomic_order.profit_target.id] = command.atomic_order.stop_loss.id
            self.oco_orders[command.atomic_order.stop_loss.id] = command.atomic_order.profit_target.id

        self.atomic_child_orders[command.atomic_order.entry.id] = atomic_orders

        # Generate command
        cdef SubmitOrder submit_order = SubmitOrder(
            command.atomic_order.entry,
            command.position_id,
            command.strategy_id,
            command.strategy_name,
            self._guid_factory.generate(),
            self._clock.time_now())
        self._submit_order(submit_order)

    cdef void _modify_order(self, ModifyOrder command):
        """
        Send a modify order request to the execution service.
        
        :param command: The command to execute.
        """
        if command.order.id not in self.working_orders:
            # Generate event
            event = OrderCancelReject(
                command.order.symbol,
                command.order.id,
                self._clock.time_now(),
                ValidString(f'{command.id.value}'),
                ValidString(f'cannot find order with id {command.order.id.value}'),
                self._guid_factory.generate(),
                self._clock.time_now())
            self.handle_event(event)
            return  # Rejected the modify order command

        cdef Order order = command.order
        cdef Price current_ask
        cdef Price current_bid

        if order.side is OrderSide.BUY:
            current_ask = self._get_closing_ask(order.symbol)
            if order.type in STOP_ORDER_TYPES:
                if order.price < current_ask:
                    self._reject_modify_order(order, f'buy stop order price of {order.price} is below the ask {current_ask}')
                    return  # Cannot modify order
            elif order.type is OrderType.LIMIT:
                if order.price > current_ask:
                    self._reject_modify_order(order, f'buy limit order price of {order.price} is above the ask {current_ask}')
                    return  # Cannot modify order
        elif order.side is OrderSide.SELL:
            current_bid = self._get_closing_bid(order.symbol)
            if order.type in STOP_ORDER_TYPES:
                if order.price > current_bid:
                    self._reject_modify_order(order, f'sell stop order price of {order.price} is above the bid {current_bid}')
                    return  # Cannot modify order
            elif order.type is OrderType.LIMIT:
                if order.price < current_bid:
                    self._reject_modify_order(order, f'sell limit order price of {order.price} is below the bid {current_bid}')
                    return  # Cannot modify order

        # Generate event
        cdef OrderModified modified = OrderModified(
            order.symbol,
            order.id,
            order.broker_id,
            command.modified_price,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self.handle_event(modified)

    cdef void _cancel_order(self, CancelOrder command):
        """
        Send a cancel order request to the execution service.
        
        :param command: The command to execute.
        """
        Precondition.is_in(command.order.id, self.working_orders, 'order.id', 'working_orders')

        del self.working_orders[command.order.id]  # Remove from working orders

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            command.order.symbol,
            command.order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self.handle_event(cancelled)
        self._manage_oco_orders(command.order.id)

    cdef dict _prepare_minute_data(self, dict bar_data, str quote_type):
        """
        Prepare the given minute bars data by converting the dataframes of each
        symbol in the dictionary to a list of arrays of Decimal.
        
        :param bar_data: The data to prepare.
        :param quote_type: The quote type of data (bid or ask).
        :return: Dict[Symbol, List[Decimal]].
        """
        cdef dict minute_data = {}  # type: Dict[Symbol, List]
        for symbol, data in bar_data.items():
            start = datetime.utcnow()
            map_func = partial(self._convert_to_prices, precision=self.instruments[symbol].tick_precision)
            minute_data[symbol] = list(map(map_func, data.values))
            self._log.info(f"Prepared minute {quote_type} prices for {symbol} in {round((datetime.utcnow() - start).total_seconds(), 2)}s.")

        return minute_data

    cpdef list _convert_to_prices(self, double[:] values, int precision):
        """
        Convert the given array of double values to an array of Decimals with
        the given precision.

        :param values: The values to convert.
        :return: List[Price].
        """
        return [Price(values[0], precision),
                Price(values[1], precision),
                Price(values[2], precision),
                Price(values[3], precision)]

    cdef void _set_slippage_index(self, int slippage_ticks):
        """
        Set the slippage index based on the given integer.
        """
        cdef dict slippage_index = {}

        for symbol, instrument in self.instruments.items():
            slippage_index[symbol] = instrument.tick_size * slippage_ticks

        self.slippage_index = slippage_index

    cdef Price _get_highest_bid(self, Symbol symbol):
        """
        Return the highest bid price of the current iteration.
        
        :return: Price.
        """
        return self.data_bars_bid[symbol][self.iteration][1]

    cdef Price _get_lowest_bid(self, Symbol symbol):
        """
        Return the lowest bid price of the current iteration.
        
        :return: Price.
        """
        return self.data_bars_bid[symbol][self.iteration][2]

    cdef Price _get_closing_bid(self, Symbol symbol):
        """
        Return the closing bid price of the current iteration.
        
        :return: Price
        """
        return self.data_bars_bid[symbol][self.iteration][3]

    cdef Price _get_highest_ask(self, Symbol symbol):
        """
        Return the highest ask price of the current iteration.
        
        :return: Price.
        """
        return self.data_bars_ask[symbol][self.iteration][1]

    cdef Price _get_lowest_ask(self, Symbol symbol):
        """
        Return the lowest ask price of the current iteration.
        
        :return: Price.
        """
        return self.data_bars_ask[symbol][self.iteration][2]

    cdef Price _get_closing_ask(self, Symbol symbol):
        """
        Return the closing ask price of the current iteration.
        
        :return: Price.
        """
        return self.data_bars_ask[symbol][self.iteration][3]

    cdef void _accept_order(self, Order order):
        """
        Accept the given order and generate an OrderAccepted event.
        
        :param order: The order to accept.
        """
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            order.symbol,
            order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())
        self.handle_event(accepted)

        self._work_order(order)

    cdef void _reject_order(self, Order order, str reason):
        """
        Reject the given order and handle an OrderRejected event.
        
        :param order: The order to reject.
        :param order: The reject reason.
        """
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            order.symbol,
            order.id,
            self._clock.time_now(),
            ValidString(reason),
            self._guid_factory.generate(),
            self._clock.time_now())

        self.handle_event(rejected)
        self._manage_oco_orders(order.id)

    cdef void _reject_modify_order(self, Order order, str reason):
        """
        Reject the command to modify the given order by sending an 
        OrderCancelReject event to the event handler.
        
        :param order: The order the modification reject relates to.
        :param reason: The reason for the modification rejection.
        """
        # Generate event
        cdef OrderCancelReject cancel_reject = OrderCancelReject(
            order.symbol,
            order.id,
            self._clock.time_now(),
            ValidString('INVALID PRICE'),
            ValidString(reason),
            self._guid_factory.generate(),
            self._clock.time_now())

        self.handle_event(cancel_reject)

    cdef void _expire_order(self, Order order):
        """
        Expire the given order by sending an OrderExpired event to the on event
        handler.
        
        :param order: The order to expire.
        """
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            order.symbol,
            order.id,
            order.expire_time,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.handle_event(expired)
        self._manage_oco_orders(order.id)

    cdef void _work_order(self, Order order):
        """
        Work the given order.
        
        :param order: The order to work.
        """
        Precondition.not_in(order.id, self.working_orders, 'order.id', 'working_orders')

        cdef Price closing_ask
        cdef Price closing_bid

        # Check order price is valid or reject
        if order.side is OrderSide.BUY:
            closing_ask = self._get_closing_ask(order.symbol)
            if order.type is OrderType.MARKET:
                # Fill market orders immediately
                self._fill_order(order, Price(closing_ask + self.slippage_index[order.symbol]))
                return  # Order filled - nothing to work
            elif order.type in STOP_ORDER_TYPES:
                if order.price < closing_ask:
                    self._reject_order(order,  f'buy stop order price of {order.price} is below the ask {closing_ask}')
                    return  # Cannot work order
            elif order.type is OrderType.LIMIT:
                if order.price > closing_ask:
                    self._reject_order(order,  f'buy limit order price of {order.price} is above the ask {closing_ask}')
                    return  # Cannot work order
        elif order.side is OrderSide.SELL:
            closing_bid = self._get_closing_bid(order.symbol)
            if order.type is OrderType.MARKET:
                # Fill market orders immediately
                self._fill_order(order, Price(closing_bid - self.slippage_index[order.symbol]))
                return  # Order filled - nothing to work
            elif order.type in STOP_ORDER_TYPES:
                if order.price > closing_bid:
                    self._reject_order(order,  f'sell stop order price of {order.price} is above the bid {closing_bid}')
                    return  # Cannot work order
            elif order.type is OrderType.LIMIT:
                if order.price < closing_bid:
                    self._reject_order(order,  f'sell limit order price of {order.price} is below the bid {closing_bid}')
                    return  # Cannot work order

        # Order now becomes working
        self.working_orders[order.id] = order
        self._log.debug(f"{order.id} WORKING at {order.price}.")

        # Generate event
        cdef OrderWorking working = OrderWorking(
            order.symbol,
            order.id,
            OrderId('B-' + str(order.id.value)),  # Dummy broker id
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
        self.handle_event(working)

    cdef void _fill_order(self, Order order, Price fill_price):
        """
        Fill the given order at the given price.
        
        :param order: The order to fill.
        :param fill_price: The price to fill the order at.
        """
        # Generate event
        cdef OrderFilled filled = OrderFilled(
            order.symbol,
            order.id,
            ExecutionId('E-' + str(order.id.value)),
            ExecutionTicket('ET-' + str(order.id.value)),
            order.side,
            order.quantity,
            fill_price,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        # Adjust account if position exists
        if self._portfolio.order_has_position(order.id):
            self._adjust_account(filled)

        self.handle_event(filled)
        self._manage_oco_orders(order.id)

        # Work any atomic child orders
        if order.id in self.atomic_child_orders:
            for child_order in self.atomic_child_orders[order.id]:
                self._work_order(child_order)
            del self.atomic_child_orders[order.id]

    cdef void _manage_oco_orders(self, OrderId order_id):
        """
        Adjust the list of OCO orders if applicable.
        """
        cdef OrderId oco_order_id
        cdef Order oco_order
        cdef OrderCancelled cancelled

        if order_id in self.oco_orders:
            # Cancel any working OCO orders
            oco_order_id = self.oco_orders[order_id]
            oco_order = self._order_book[oco_order_id]
            del self.oco_orders[order_id]
            del self.oco_orders[oco_order_id]

            if oco_order_id in self.working_orders:
                del self.working_orders[oco_order_id]
                # Generate event
                cancelled = OrderCancelled(
                    oco_order.symbol,
                    oco_order.id,
                    self._clock.time_now(),
                    self._guid_factory.generate(),
                    self._clock.time_now())
                self.handle_event(cancelled)

    cdef void _adjust_account(self, OrderEvent event):
        """
        Adjust the positions based on the order fill event.
        
        :param event: The order fill event.
        """
        cdef Instrument instrument = self.instruments[event.symbol]
        cdef float exchange_rate = self.currency_calculator.exchange_rate(
            from_currency=instrument.quote_currency,
            to_currency=self._account.currency,
            bid_rates=self._build_current_bid_rates(),
            ask_rates=self._build_current_ask_rates(),
            quote_type=QuoteType.BID if event.order_side is OrderSide.SELL else QuoteType.ASK)

        cdef Position position = self._portfolio.get_position_for_order(event.order_id)
        cdef Money pnl = self._calculate_pnl(
            direction=position.market_position,
            entry_price=position.average_entry_price,
            exit_price=event.average_price,
            quantity=event.filled_quantity,
            exchange_rate=exchange_rate)

        cdef Money commission = self.commission_calculator.calculate(
            symbol=event.symbol,
            filled_quantity=event.filled_quantity,
            exchange_rate=exchange_rate)

        self.total_commissions += commission
        pnl -= commission
        self.account_capital += pnl
        self.account_cash_activity_day += pnl

        cdef AccountEvent account_event = AccountEvent(
            self._account.id,
            self._account.broker,
            self._account.account_number,
            self._account.currency,
            self.account_capital,
            self.account_cash_start_day,
            self.account_cash_activity_day,
            margin_used_liquidation=Money(0),
            margin_used_maintenance=Money(0),
            margin_ratio=Decimal(0),
            margin_call_status=ValidString(),
            event_id=self._guid_factory.generate(),
            event_timestamp=self._clock.time_now())

        self.handle_event(account_event)

    cdef dict _build_current_bid_rates(self):
        """
        Return the current currency bid rates in the markets.
        
        :return: Dict[str, float].
        """
        cdef dict bid_rates = {}  # type: Dict[str, float]

        for symbol, prices in self.data_bars_bid.items():
            bid_rates[symbol.code] = prices[self.iteration][3].as_float()

        return bid_rates

    cdef dict _build_current_ask_rates(self):
        """
        Return the current currency ask rates in the markets.
        
        :return: Dict[str, float].
        """
        cdef dict ask_rates = {}  # type: Dict[str, float]

        for symbol, prices in self.data_bars_ask.items():
            ask_rates[symbol.code] = prices[self.iteration][3].as_float()

        return ask_rates

    cdef Money _calculate_pnl(
            self,
            MarketPosition direction,
            Price entry_price,
            Price exit_price,
            Quantity quantity,
            float exchange_rate):
        """
        Return the pnl from the given parameters.
        
        :param direction: The direction of the position affecting pnl.
        :param entry_price: The entry price of the position affecting pnl.
        :param exit_price: The exit price of the position affecting pnl.
        :param quantity: The filled quantity for the position affecting pnl.
        :param exchange_rate: The exchange rate for the transaction.
        :return: Money.
        """
        cdef object difference
        if direction is MarketPosition.LONG:
            difference = exit_price - entry_price
        elif direction is MarketPosition.SHORT:
            difference = entry_price - exit_price
        else:
            raise ValueError(f'Cannot calculate the pnl of a {market_position_string(direction)} direction.')

        return Money(difference * quantity.value * Decimal(exchange_rate))
