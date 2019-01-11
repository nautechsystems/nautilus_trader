#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategies.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from inv_trader.model.enums import Venue, Resolution, QuoteType, OrderSide
from inv_trader.model.objects import Symbol, Tick, BarType, Bar
from inv_trader.model.events import Event
from inv_trader.model.identifiers import Label, OrderId, PositionId
from inv_trader.strategy import TradeStrategy
from inv_indicators.average.ema import ExponentialMovingAverage
from test_kit.objects import ObjectStorer

GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class TestStrategy1(TradeStrategy):
    """"
    A simple strategy for unit testing.
    """

    def __init__(self, bar_type: BarType):
        """
        Initializes a new instance of the TestStrategy1 class.
        """
        super().__init__(label='UnitTests', order_id_tag='TS01')
        self.object_storer = ObjectStorer()
        self.bar_type = bar_type

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.register_indicator(bar_type=self.bar_type,
                                indicator=self.ema1,
                                update_method=self.ema1.update)
        self.register_indicator(bar_type=self.bar_type,
                                indicator=self.ema2,
                                update_method=self.ema2.update)

        self.position_id = None

    def on_start(self):
        self.object_storer.store('custom start logic')

    def on_tick(self, tick: Tick):
        self.object_storer.store(tick)

    def on_bar(
            self,
            bar_type: BarType,
            bar: Bar):

        self.object_storer.store((bar_type, Bar))

        if bar_type == self.bar_type:
            if self.ema1.value > self.ema2.value:
                buy_order = self.order_factory.market(
                    Symbol('GBPUSD', Venue.FXCM),
                    OrderId('O123456'),
                    Label('TestStrategy1_E'),
                    OrderSide.BUY,
                    100000)

                self.submit_order(buy_order, PositionId(str(buy_order.id)))
                self.position_id = buy_order.id

            elif self.ema1.value < self.ema2.value:
                sell_order = self.order_factory.market(
                    Symbol('GBPUSD', Venue.FXCM),
                    OrderId('O123456'),
                    Label('TestStrategy1_E'),
                    OrderSide.SELL,
                    100000)

                self.submit_order(sell_order, PositionId(str(sell_order.id)))
                self.position_id = sell_order.id

    def on_event(self, event: Event):
        self.object_storer.store(event)

    def on_stop(self):
        self.object_storer.store('custom stop logic')

    def on_reset(self):
        self.object_storer.store('custom reset logic')
