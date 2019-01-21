#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

import uuid

from pandas import DataFrame
from typing import List, Dict

from inv_trader.core.decimal cimport Decimal
from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.brokerage cimport Broker
from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.enums.order_type cimport OrderType
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.model.money import money_zero, money
from inv_trader.model.objects cimport Symbol, Bar, BarType, Instrument
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent, OrderCancelReject
from inv_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderFilled, OrderPartiallyFilled
from inv_trader.model.identifiers cimport GUID, OrderId, ExecutionId, ExecutionTicket, AccountNumber
from inv_trader.common.clock cimport Clock, TestClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.common.execution cimport ExecutionClient


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict data_ticks: Dict[Symbol, DataFrame],
                 dict data_bars_bid: Dict[Symbol, List[Bar]],
                 dict data_bars_ask: Dict[Symbol, List[Bar]],
                 list data_minute_index: List[datetime],
                 int starting_capital,
                 int slippage_ticks,
                 TestClock clock,
                 Logger logger):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param instruments: The instruments needed for the backtest.
        :param data_ticks: The historical tick market data needed for the backtest.
        :param data_bars_bid: The historical bid bars data needed for the backtest.
        :param data_bars_ask: The historical ask bars data needed for the backtest.
        :param data_minute_index: The historical minute bars index.
        :param starting_capital: The starting capital for the backtest account (> 0).
        :param slippage_ticks: The slippage for each order fill in ticks (>= 0).
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.dict_types(data_ticks, Symbol, DataFrame, 'data_ticks')
        Precondition.dict_types(data_bars_bid, Symbol, list, 'data_bars_bid')
        Precondition.dict_types(data_bars_ask, Symbol, list, 'data_bars_ask')
        Precondition.positive(starting_capital, 'starting_capital')
        Precondition.not_negative(slippage_ticks, 'slippage_ticks')
        Precondition.not_none(clock, 'clock')
        Precondition.not_none(logger, 'logger')

        super().__init__(clock, logger)

        # Convert instruments list to dictionary indexed by symbol
        cdef dict instruments_dict = {}             # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument

        self.instruments = instruments_dict         # type: Dict[Symbol, Instrument]
        self.data_ticks = data_ticks                # type: Dict[Symbol, DataFrame]
        self.data_bars_bid = data_bars_bid          # type: Dict[Symbol, List[Bar]]
        self.data_bars_ask = data_bars_ask          # type: Dict[Symbol, List[Bar]]
        self.data_minute_index = data_minute_index  # type: List[datetime]
        self.iteration = 0
        self.current_bid_H = dict()                 # type: Dict[Symbol, Decimal]
        self.current_bid_L = dict()                 # type: Dict[Symbol, Decimal]
        self.current_bid_C = dict()                 # type: Dict[Symbol, Decimal]
        self.current_ask_H = dict()                 # type: Dict[Symbol, Decimal]
        self.current_ask_L = dict()                 # type: Dict[Symbol, Decimal]
        self.current_ask_C = dict()                 # type: Dict[Symbol, Decimal]
        self.account_cash_start_day = money(starting_capital)
        self.account_cash_activity_day = money_zero()
        self.slippage_index = dict()                # type: Dict[Symbol, Decimal]
        self.working_orders = dict()                # type: Dict[OrderId, Order]

        self._set_slippage_index(slippage_ticks)
        self._set_market_prices()

        cdef AccountEvent initial_starting = AccountEvent(self.account.id,
                                                          Broker.SIMULATED,
                                                          AccountNumber('9999'),
                                                          CurrencyCode.USD,
                                                          money(starting_capital),
                                                          money(starting_capital),
                                                          money_zero(),
                                                          money_zero(),
                                                          money_zero(),
                                                          Decimal(0),
                                                          'NONE',
                                                          GUID(uuid.uuid4()),
                                                          self._clock.time_now())

        self.account.apply(initial_starting)

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

    cpdef void iterate(self, datetime time):
        """
        Iterate the data client one time step.
        """
        self._set_market_prices()

        for order_id, order in self.working_orders.copy().items():  # Copy dict to avoid resize during loop
            # Check for order fill
            if order.side is OrderSide.BUY:
                if order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                    if self.current_ask_H[order.symbol] >= order.price:
                        del self.working_orders[order.id]
                        self._fill_order(order, order.price + self.slippage_index[order.symbol])
                        continue
                elif order.type is OrderType.LIMIT:
                    if self.current_ask_H[order.symbol] < order.price:
                        del self.working_orders[order.id]
                        self._fill_order(order, order.price + self.slippage_index[order.symbol])
                        continue
            elif order.side is OrderSide.SELL:
                if order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                    if self.current_ask_L[order.symbol] <= order.price:
                        del self.working_orders[order.id]
                        self._fill_order(order, order.price - self.slippage_index[order.symbol])
                        continue
                elif order.type is OrderType.LIMIT:
                    if self.current_ask_L[order.symbol] > order.price:
                        del self.working_orders[order.id]
                        self._fill_order(order, order.price - self.slippage_index[order.symbol])
                        continue

            # Check for order expiry
            if order.expire_time is not None and time >= order.expire_time:
                del self.working_orders[order.id]
                self._expire_order(order)

        self.iteration += 1

    cpdef void collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.
        """
        cdef AccountEvent event = AccountEvent(self.account.id,
                                               self.account.broker,
                                               self.account.account_number,
                                               self.account.currency,
                                               self.account.cash_balance,
                                               self.account_cash_start_day,
                                               self.account_cash_activity_day,
                                               self.account.margin_used_liquidation,
                                               self.account.margin_used_maintenance,
                                               self.account.margin_ratio,
                                               self.account.margin_call_status,
                                               GUID(uuid.uuid4()),
                                               self._clock.time_now())
        self._on_event(event)

    cpdef void submit_order(self, Order order, GUID strategy_id):
        """
        Send a submit order request to the execution service.
        
        :param order: The order to submit.
        :param strategy_id: The strategy identifier to register the order with.
        """
        Precondition.not_in(order.id, self.working_orders, 'order.id', 'working_orders')

        self._register_order(order, strategy_id)

        cdef OrderSubmitted submitted = OrderSubmitted(
            order.symbol,
            order.id,
            self._clock.time_now(),
            GUID(uuid.uuid4()),
            self._clock.time_now())
        self._on_event(submitted)

        cdef OrderAccepted accepted = OrderAccepted(
            order.symbol,
            order.id,
            self._clock.time_now(),
            GUID(uuid.uuid4()),
            self._clock.time_now())
        self._on_event(accepted)

        # Check order price is valid or reject
        if order.side is OrderSide.BUY:
            if order.type is OrderType.MARKET:
                self._fill_order(order, self.current_ask_C[order.symbol])
                return
            elif order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                if order.price < self.current_ask_C[order.symbol]:
                    self._reject_order(order,  f'Buy stop order price of {order.price} is below the ask {self.current_ask_C[order.symbol]}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price > self.current_ask_C[order.symbol]:
                    self._reject_order(order,  f'Buy limit order price of {order.price} is above the ask {self.current_ask_C[order.symbol]}')
                    return
        elif order.side is OrderSide.SELL:
            if order.type is OrderType.MARKET:
                self._fill_order(order, self.current_bid_C[order.symbol])
                return
            elif order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                if order.price > self.current_bid_C[order.symbol]:
                    self._reject_order(order,  f'Sell stop order price of {order.price} is above the bid {self.current_bid_C[order.symbol]}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price < self.current_bid_C[order.symbol]:
                    self._reject_order(order,  f'Sell limit order price of {order.price} is below the bid {self.current_bid_C[order.symbol]}')
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
            GUID(uuid.uuid4()),
            self._clock.time_now(),
            order.expire_time)
        self._on_event(working)

    cpdef void cancel_order(self, Order order, str cancel_reason):
        """
        Send a cancel order request to the execution service.
        """
        Precondition.is_in(order.id, self.working_orders, 'order.id', 'working_orders')

        cdef OrderCancelled cancelled = OrderCancelled(
            order.symbol,
            order.id,
            self._clock.time_now(),
            GUID(uuid.uuid4()),
            self._clock.time_now())

        del self.working_orders[order.id]
        self._on_event(cancelled)

    cpdef void modify_order(self, Order order, Decimal new_price):
        """
        Send a modify order request to the execution service.
        """
        Precondition.is_in(order.id, self.working_orders, 'order.id', 'working_orders')

        if order.side is OrderSide.BUY:
            current_ask = self.current_ask_C[order.symbol]
            if order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                if order.price < current_ask:
                    self._reject_modify_order(order,  f'Buy stop order price of {order.price} is below the ask {current_ask}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price > current_ask:
                    self._reject_modify_order(order,  f'Buy limit order price of {order.price} is above the ask {current_ask}')
                    return
        elif order.side is OrderSide.SELL:
            current_bid = self.current_bid_C[order.symbol]
            if order.type is OrderType.STOP_MARKET or order.type is OrderType.STOP_LIMIT or order.type is OrderType.MIT:
                if order.price > current_bid:
                    self._reject_modify_order(order,  f'Sell stop order price of {order.price} is above the bid {current_bid}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price < current_bid:
                    self._reject_modify_order(order,  f'Sell limit order price of {order.price} is below the bid {current_bid}')
                    return

        cdef OrderModified modified = OrderModified(
            order.symbol,
            order.id,
            order.broker_id,
            new_price,
            self._clock.time_now(),
            GUID(uuid.uuid4()),
            self._clock.time_now())

        self._on_event(modified)

    cdef void _set_slippage_index(self, int slippage_ticks):
        """
        Set the slippage index based on the given integer.
        """
        cdef dict slippage_index = dict()

        for symbol, instrument in self.instruments.items():
            slippage_index[symbol] = Decimal(instrument.tick_size * slippage_ticks)

        self.slippage_index = slippage_index

    cpdef void _set_market_prices(self):
        """
        Set the market prices based on the current iteration.
        """
        for symbol in self.instruments:
            self.current_bid_H[symbol] = self.data_bars_bid[symbol][self.iteration].high
            self.current_bid_L[symbol] = self.data_bars_bid[symbol][self.iteration].low
            self.current_bid_C[symbol] = self.data_bars_bid[symbol][self.iteration].close
            self.current_ask_H[symbol] = self.data_bars_ask[symbol][self.iteration].high
            self.current_ask_L[symbol] = self.data_bars_ask[symbol][self.iteration].low
            self.current_ask_C[symbol] = self.data_bars_ask[symbol][self.iteration].close

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
            GUID(uuid.uuid4()),
            self._clock.time_now())

        self._on_event(rejected)

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
            GUID(uuid.uuid4()),
            self._clock.time_now())

        self._on_event(cancel_reject)

    cdef void _expire_order(self, Order order):
        """
        Expire the given order by sending an OrderExpired event to the on event
        handler.
        """
        cdef OrderExpired expired = OrderExpired(
            order.symbol,
            order.id,
            order.expire_time,
            GUID(uuid.uuid4()),
            self._clock.time_now())

        self._on_event(expired)


    cdef void _fill_order(self, Order order, Decimal fill_price):
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
            GUID(uuid.uuid4()),
            self._clock.time_now())

        self._on_event(filled)
