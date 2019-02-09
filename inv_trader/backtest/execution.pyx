#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

import pandas as pd

from decimal import Decimal
from cpython.datetime cimport datetime
from functools import partial
from pandas import DataFrame
from typing import List, Dict

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.brokerage cimport Broker
from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.enums.order_type cimport OrderType
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.model.objects cimport Symbol, Price, Money, Instrument
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport OrderEvent, AccountEvent
from inv_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderFilled
from inv_trader.model.identifiers cimport  OrderId, ExecutionId, ExecutionTicket, AccountNumber
from inv_trader.common.account cimport Account
from inv_trader.common.clock cimport TestClock
from inv_trader.common.guid cimport TestGuidFactory
from inv_trader.common.logger cimport Logger
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.commands cimport CollateralInquiry, SubmitOrder, ModifyOrder, CancelOrder
from inv_trader.portfolio.portfolio cimport Portfolio


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
        :param clock: The clock for the component.
        :param clock: The GUID factory for the component.
        :param logger: The logger for the component.
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

        # Convert instruments list to dictionary indexed by symbol
        cdef dict instruments_dict = {}             # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument

        self.instruments = instruments_dict         # type: Dict[Symbol, Instrument]

        # Prepare data
        self.data_ticks = data_ticks                                          # type: Dict[Symbol, List]
        self.data_bars_bid = self._prepare_minute_data(data_bars_bid, 'bid')  # type: Dict[Symbol, List]
        self.data_bars_ask = self._prepare_minute_data(data_bars_ask, 'ask')  # type: Dict[Symbol, List]

        # Set minute data index
        first_dataframe = data_bars_bid[next(iter(data_bars_bid))]
        self.data_minute_index = list(pd.to_datetime(first_dataframe.index, utc=True))  # type: List[datetime]

        assert(isinstance(self.data_minute_index[0], datetime))

        self.iteration = 0
        self.day_number = 0
        self.account_cash_start_day = starting_capital
        self.account_cash_activity_day = Money(0)
        self.slippage_index = {}                    # type: Dict[Symbol, Decimal]
        self.working_orders = {}                    # type: Dict[OrderId, Order]

        self._set_slippage_index(slippage_ticks)

        cdef AccountEvent initial_starting = AccountEvent(
            self._account.id,
            Broker.SIMULATED,
            AccountNumber('9999'),
            CurrencyCode.USD,
            starting_capital,
            starting_capital,
            Money(0),
            Money(0),
            Money(0),
            Decimal(0),
            'NONE',
            self._guid_factory.generate(),
            self._clock.time_now())

        self._account.apply(initial_starting)

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
        # Set account statistics

        cdef datetime time_now = self._clock.time_now()
        if self.day_number is not time_now.day:
            self.day_number = time_now.day
            self.account_cash_start_day = self._account.cash_balance
            self.account_cash_activity_day = Money(0)

            command = CollateralInquiry(
            self._guid_factory.generate(),
            self._clock.time_now())
            self.collateral_inquiry(command)

        # Simulate market dynamics
        cdef Price highest_ask
        cdef Price lowest_bid

        for order_id, order in self.working_orders.copy().items():  # Copy dict to avoid resize during loop
            # Check for order fill
            if order.side is OrderSide.BUY:
                highest_ask = self._get_highest_ask(order.symbol)
                if order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                    if highest_ask >= order.price:
                        del self.working_orders[order.id]
                        self._fill_order(order, Price(order.price + self.slippage_index[order.symbol]))
                        continue
                elif order.type is OrderType.LIMIT:
                    if highest_ask < order.price:
                        del self.working_orders[order.id]
                        self._fill_order(order, Price(order.price + self.slippage_index[order.symbol]))
                        continue
            elif order.side is OrderSide.SELL:
                lowest_bid = self._get_lowest_bid(order.symbol)
                if order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                    if lowest_bid <= order.price:
                        del self.working_orders[order.id]
                        self._fill_order(order, Price(order.price - self.slippage_index[order.symbol]))
                        continue
                elif order.type is OrderType.LIMIT:
                    if lowest_bid > order.price:
                        del self.working_orders[order.id]
                        self._fill_order(order, Price(order.price - self.slippage_index[order.symbol]))
                        continue

            # Check for order expiry
            if order.expire_time is not None and time_now >= order.expire_time:
                del self.working_orders[order.id]
                self._expire_order(order)

        self.iteration += 1

    cpdef void collateral_inquiry(self, CollateralInquiry command):
        """
        Send a collateral inquiry command to the execution service.
        """
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
        self._handle_event(event)

    cpdef void submit_order(self, SubmitOrder command):
        """
        Send a submit order request to the execution service.
        
        :param command: The command to execute.
        """
        Precondition.not_in(command.order.id, self.working_orders, 'order.id', 'working_orders')

        cdef Order order = command.order

        cdef OrderSubmitted submitted = OrderSubmitted(
            order.symbol,
            order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())
        self._handle_event(submitted)

        cdef OrderAccepted accepted = OrderAccepted(
            order.symbol,
            order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())
        self._handle_event(accepted)

        cdef Price closing_ask
        cdef Price closing_bid

        # Check order price is valid or reject
        if order.side is OrderSide.BUY:
            closing_ask = self._get_closing_ask(order.symbol)
            if order.type is OrderType.MARKET:
                self._fill_order(order, Price(closing_ask + self.slippage_index[order.symbol]))
                return
            elif order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                if order.price < closing_ask:
                    self._reject_order(order,  f'Buy stop order price of {order.price} is below the ask {closing_ask}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price > closing_ask:
                    self._reject_order(order,  f'Buy limit order price of {order.price} is above the ask {closing_ask}')
                    return
        elif order.side is OrderSide.SELL:
            closing_bid = self._get_closing_bid(order.symbol)
            if order.type is OrderType.MARKET:
                self._fill_order(order, Price(closing_bid - self.slippage_index[order.symbol]))
                return
            elif order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                if order.price > closing_bid:
                    self._reject_order(order,  f'Sell stop order price of {order.price} is above the bid {closing_bid}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price < closing_bid:
                    self._reject_order(order,  f'Sell limit order price of {order.price} is below the bid {closing_bid}')
                    return

        # Order now becomes working
        self._log.debug(f"{order.id} WORKING at {order.price}.")
        self.working_orders[order.id] = order

        cdef OrderWorking working = OrderWorking(
            order.symbol,
            order.id,
            OrderId('B' + str(order.id)),  # Dummy broker id
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
        self._handle_event(working)

    cpdef void modify_order(self, ModifyOrder command):
        """
        Send a modify order request to the execution service.
        """
        Precondition.is_in(command.order.id, self.working_orders, 'order.id', 'working_orders')

        cdef Order order = command.order
        cdef Price current_ask
        cdef Price current_bid

        if order.side is OrderSide.BUY:
            current_ask = self._get_closing_ask(order.symbol)
            if order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                if order.price < current_ask:
                    self._reject_modify_order(order, f'Buy stop order price of {order.price} is below the ask {current_ask}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price > current_ask:
                    self._reject_modify_order(order, f'Buy limit order price of {order.price} is above the ask {current_ask}')
                    return
        elif order.side is OrderSide.SELL:
            current_bid = self._get_closing_bid(order.symbol)
            if order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                if order.price > current_bid:
                    self._reject_modify_order(order, f'Sell stop order price of {order.price} is above the bid {current_bid}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price < current_bid:
                    self._reject_modify_order(order, f'Sell limit order price of {order.price} is below the bid {current_bid}')
                    return

        cdef OrderModified modified = OrderModified(
            order.symbol,
            order.id,
            order.broker_id,
            command.modified_price,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._handle_event(modified)

    cpdef void cancel_order(self, CancelOrder command):
        """
        Send a cancel order request to the execution service.
        """
        Precondition.is_in(command.order.id, self.working_orders, 'order.id', 'working_orders')

        cdef OrderCancelled cancelled = OrderCancelled(
            command.order.symbol,
            command.order.id,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        del self.working_orders[command.order.id]
        self._handle_event(cancelled)

    cdef dict _prepare_minute_data(self, dict bar_data, str quote_type):
        """
        Prepare the given minute bars data by converting the dataframes of each
        symbol in the dictionary to a list of arrays of Decimal.
        
        :param bar_data: The data to prepare.
        :param quote_type: The quote type of data (bid or ask).
        :return: The Dict[Symbol, List] of prepared data.
        """
        cdef dict minute_data = {}    # type: Dict[Symbol, List]
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
        :return: The array of Decimals.
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
        :return: Return the highest bid price of the current iteration.
        """
        return self.data_bars_bid[symbol][self.iteration][1]

    cdef Price _get_lowest_bid(self, Symbol symbol):
        """
        :return: Return the lowest bid price of the current iteration.
        """
        return self.data_bars_bid[symbol][self.iteration][2]

    cdef Price _get_closing_bid(self, Symbol symbol):
        """
        :return: Return the closing bid price of the current iteration.
        """
        return self.data_bars_bid[symbol][self.iteration][3]

    cdef Price _get_highest_ask(self, Symbol symbol):
        """
        :return: Return the highest ask price of the current iteration.
        """
        return self.data_bars_ask[symbol][self.iteration][1]

    cdef Price _get_lowest_ask(self, Symbol symbol):
        """
        :return: Return the lowest ask price of the current iteration.
        """
        return self.data_bars_ask[symbol][self.iteration][2]

    cdef Price _get_closing_ask(self, Symbol symbol):
        """
        :return: Return the closing ask price of the current iteration.
        """
        return self.data_bars_ask[symbol][self.iteration][3]

    cdef void _reject_order(self, Order order, str reason):
        """
        Reject the given order by sending an OrderRejected event to the on event 
        handler.
        """
        cdef OrderRejected rejected = OrderRejected(
            order.symbol,
            order.id,
            self._clock.time_now(),
            reason,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._handle_event(rejected)

    cdef void _reject_modify_order(self, Order order, str reason):
        """
        Reject the command to modify the given order by sending an 
        OrderCancelReject event to the event handler.
        """
        cdef OrderCancelReject cancel_reject = OrderCancelReject(
            order.symbol,
            order.id,
            self._clock.time_now(),
            'INVALID PRICE',
            reason,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._handle_event(cancel_reject)

    cdef void _expire_order(self, Order order):
        """
        Expire the given order by sending an OrderExpired event to the on event
        handler.
        """
        cdef OrderExpired expired = OrderExpired(
            order.symbol,
            order.id,
            order.expire_time,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._handle_event(expired)


    cdef void _fill_order(self, Order order, Price fill_price):
        """
        Fill the given order at the given price.
        """
        cdef OrderFilled filled = OrderFilled(
            order.symbol,
            order.id,
            ExecutionId('E' + str(order.id)),
            ExecutionTicket('ET' + str(order.id)),
            order.side,
            order.quantity,
            fill_price,
            self._clock.time_now(),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._handle_event(filled)
        self._adjust_account(filled)

    cdef void _adjust_account(self, OrderEvent event):
        """
        Adjust the positions based on the order fill event.
        
        :param event: The order fill event.
        """
        cdef AccountEvent account_event = AccountEvent(
            self._account.id,
            self._account.broker,
            self._account.account_number,
            self._account.currency,
            self._account.cash_balance,
            self.account_cash_start_day,
            self.account_cash_activity_day,
            margin_used_liquidation=Money(0),
            margin_used_maintenance=Money(0),
            margin_ratio=Decimal(0),
            margin_call_status='NONE',
            event_id=self._guid_factory.generate(),
            event_timestamp=self._clock.time_now())

        self._account.apply(account_event)
