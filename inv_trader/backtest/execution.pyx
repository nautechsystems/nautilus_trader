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
import uuid

from pandas import DataFrame
from typing import List, Dict

from inv_trader.core.decimal cimport Decimal
from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.brokerage cimport Broker
from inv_trader.enums.currency_code cimport CurrencyCode
from inv_trader.enums.resolution cimport Resolution
from inv_trader.enums.order_type cimport OrderType
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.model.identifiers cimport AccountNumber
from inv_trader.model.objects import Money
from inv_trader.model.objects cimport Symbol, Instrument
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent, AccountEvent, OrderCancelReject
from inv_trader.model.identifiers cimport GUID, OrderId, ExecutionId, ExecutionTicket
from inv_trader.common.clock cimport Clock, TestClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.model.events cimport Event, OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderFilled, OrderPartiallyFilled


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict tick_data: Dict[Symbol, DataFrame],
                 dict bar_data_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict bar_data_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 Decimal starting_capital,
                 int slippage_ticks,
                 TestClock clock,
                 Logger logger):
        """
        Initializes a new instance of the BacktestDataClient class.

        :param instruments: The instruments needed for the backtest.
        :param tick_data: The historical tick market data needed for the backtest.
        :param bar_data_bid: The historical bid market data needed for the backtest.
        :param bar_data_ask: The historical ask market data needed for the backtest.
        :param starting_capital: The starting capital for the backtest account (> 0).
        :param slippage_ticks: The slippage for each order fill in ticks (>= 0).
        :param clock: The clock for the component.
        :param logger: The logger for the component.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.dict_types(tick_data, Symbol, DataFrame, 'tick_data')
        Precondition.dict_types(bar_data_bid, Symbol, dict, 'bar_data_bid')
        Precondition.dict_types(bar_data_ask, Symbol, dict, 'bar_data_ask')
        Precondition.positive(starting_capital, 'starting_capital')
        Precondition.not_negative(slippage_ticks, 'slippage_ticks')
        Precondition.not_none(clock, 'clock')
        Precondition.not_none(logger, 'logger')

        super().__init__(clock, logger)

        # Convert instruments list to dictionary indexed by symbol
        cdef dict instruments_dict = dict()  # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument
        self.instruments = instruments_dict
        self.tick_data = tick_data
        self.bar_data_bid = dict()  # type: Dict[Symbol, DataFrame]
        self.bar_data_ask = dict()  # type: Dict[Symbol, DataFrame]

        # Set minute data index
        first_dataframe = bar_data_bid[next(iter(bar_data_bid))][Resolution.MINUTE]
        self.minute_data_index = list(pd.to_datetime(first_dataframe.index, utc=True))

        # Set bar data to one minute bars
        for symbol, instrument in self.instruments.items():
            self.bar_data_bid[symbol] = bar_data_bid[symbol][Resolution.MINUTE]
            self.bar_data_ask[symbol] = bar_data_ask[symbol][Resolution.MINUTE]

        self.iteration = 0
        self.account_cash_start_day = starting_capital
        self.account_cash_activity_day = Decimal(0, 2)
        self.bids_current = dict()      # type: Dict[Symbol, Decimal]
        self.bids_high = dict()         # type: Dict[Symbol, Decimal]
        self.bids_low = dict()          # type: Dict[Symbol, Decimal]
        self.asks_current = dict()      # type: Dict[Symbol, Decimal]
        self.asks_high = dict()         # type: Dict[Symbol, Decimal]
        self.asks_low = dict()          # type: Dict[Symbol, Decimal]
        self.slippage_index = dict()    # type: Dict[Symbol, Decimal]
        self.working_orders = dict()    # type: Dict[OrderId, Order]

        self._set_market_prices()
        self._set_slippage_index(slippage_ticks)

        cdef AccountEvent initial_starting = AccountEvent(self.account.id,
                                                          Broker.SIMULATED,
                                                          AccountNumber('9999'),
                                                          CurrencyCode.USD,
                                                          starting_capital,
                                                          starting_capital,
                                                          Money.zero(),
                                                          Money.zero(),
                                                          Money.zero(),
                                                          Decimal(0),
                                                          'NONE',
                                                          GUID(uuid.uuid4()),
                                                          self._clock.time_now())

        self.account.apply(initial_starting)

        for symbol, instrument in self.instruments.items():
            self.bids_current[symbol] = Decimal(self.bar_data_bid[symbol].iloc[self.iteration]['Open'], instrument.tick_precision)
            self.asks_current[symbol] = Decimal(self.bar_data_ask[symbol].iloc[self.iteration]['Open'], instrument.tick_precision)

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
        cdef datetime current = self.minute_data_index[0]
        cdef int next_index = 0

        while current < to_time:
            if self.minute_data_index[next_index] == current:
                next_index += 1
                self.iteration += 1
            current += time_step

        self._clock.set_time(current)

    cpdef void iterate(self, datetime time):
        """
        Iterate the data client one time step.
        """
        self._set_market_prices()

        for order_id, order in self.working_orders.items():
            # Check for order fill
            if order.side is OrderSide.BUY:
                if order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                    if self.asks_high[order.symbol] >= order.price:
                        self._fill_order(order, self.asks_high[order.symbol])
                        continue
                elif order.type is OrderType.LIMIT:
                    if self.asks_low[order.symbol] < order.price:
                        self._fill_order(order, self.asks_low[order.symbol])
                        continue
            elif order.side is OrderSide.SELL:
                if order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                    if self.bids_low[order.symbol] <= order.price:
                        self._fill_order(order, self.bids_low[order.symbol])
                        continue
                elif order.type is OrderType.LIMIT:
                    if self.bids_high[order.symbol] > order.price:
                        self._fill_order(order, self.bids_high[order.symbol])
                        continue

            # Check for order expiry
            if order.expire_time is not None and time >= order.expire_time:
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
        Precondition.true(order.id not in self.working_orders, 'order.id not in self.working_orders')

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

        cdef Decimal current_ask = self.asks_current[order.symbol]
        cdef Decimal current_bid = self.bids_current[order.symbol]

        if order.side is OrderSide.BUY:
            if order.type is OrderType.MARKET:
                self._fill_order(order, current_ask)
                return
            elif order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                if order.price < current_ask:
                    self._reject_order(order,  f'Buy stop order price of {order.price} is below the ask {current_ask}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price > current_ask:
                    self._reject_order(order,  f'Buy limit order price of {order.price} is above the ask {current_ask}')
                    return
        elif order.side is OrderSide.SELL:
            if order.type is OrderType.MARKET:
                self._fill_order(order, current_bid)
                return
            elif order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                if order.price > current_bid:
                    self._reject_order(order,  f'Sell stop order price of {order.price} is above the bid {current_bid}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price < current_bid:
                    self._reject_order(order,  f'Sell limit order price of {order.price} is below the bid {current_bid}')
                    return

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
        Precondition.true(order.id in self.working_orders, 'order.id in self.working_orders')

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
        Precondition.true(order.id in self.working_orders, 'order.id in self.working_orders')

        cdef Decimal current_ask = self.asks_current[order.symbol]
        cdef Decimal current_bid = self.bids_current[order.symbol]

        if order.side is OrderSide.BUY:
            if order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
                if order.price < current_ask:
                    self._reject_modify_order(order,  f'Buy stop order price of {order.price} is below the ask {current_ask}')
                    return
            elif order.type is OrderType.LIMIT:
                if order.price > current_ask:
                    self._reject_modify_order(order,  f'Buy limit order price of {order.price} is above the ask {current_ask}')
                    return
        elif order.side is OrderSide.SELL:
            if order.type is OrderType.STOP_MARKET or OrderType.STOP_LIMIT or OrderType.MIT:
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

    cdef void _set_market_prices(self):
        """
        Set the current market prices for the iteration.
        """
        for symbol, instrument in self.instruments.items():
            self.bids_current[symbol] = Decimal(self.bar_data_bid[symbol].iloc[self.iteration]['Close'], instrument.tick_precision)
            self.bids_high[symbol] = Decimal(self.bar_data_bid[symbol].iloc[self.iteration]['High'], instrument.tick_precision)
            self.bids_low[symbol] = Decimal(self.bar_data_bid[symbol].iloc[self.iteration]['Low'], instrument.tick_precision)
            self.asks_current[symbol] = Decimal(self.bar_data_ask[symbol].iloc[self.iteration]['Close'], instrument.tick_precision)
            self.asks_high[symbol] = Decimal(self.bar_data_ask[symbol].iloc[self.iteration]['High'], instrument.tick_precision)
            self.asks_low[symbol] = Decimal(self.bar_data_ask[symbol].iloc[self.iteration]['Low'], instrument.tick_precision)

            # print(f"{symbol}-IDX: {self.iteration}")
            # print(f"{symbol}-ASK: {self.asks_current[symbol]}")
            # print(f"{symbol}-BID: {self.bids_current[symbol]}")

    cdef void _set_slippage_index(self, int slippage_ticks):
        """
        Set the slippage index based on the given integer.
        """
        cdef dict slippage_index = dict()

        for symbol, instrument in self.instruments.items():
            slippage_index[symbol] = Decimal(instrument.tick_size * slippage_ticks)

        self.slippage_index = slippage_index

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
        del self.working_orders[order.id]

    cdef void _fill_order(self, Order order, Decimal market_price):
        """
        Fill the given order at the given price.
        """
        if order.side is OrderSide.BUY:
            slippage = self.slippage_index[order.symbol]
        else:
            slippage = - self.slippage_index[order.symbol]

        cdef OrderFilled filled = OrderFilled(
            order.symbol,
            order.id,
            ExecutionId('E' + str(order.id)),
            ExecutionTicket('ET' + str(order.id)),
            order.side,
            order.quantity,
            market_price + slippage,
            self._clock.time_now(),
            GUID(uuid.uuid4()),
            self._clock.time_now())

        # Remove from working orders if present
        if order.id in self.working_orders:
            del self.working_orders[order.id]

        self._on_event(filled)
