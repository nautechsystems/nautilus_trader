#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="ema_cross.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from datetime import timedelta

from inv_trader.common.logger import Logger
from inv_trader.enums.order_side import OrderSide
from inv_trader.enums.time_in_force import TimeInForce
from inv_trader.model.objects import Price, Tick, BarType, Bar, Instrument
from inv_trader.model.events import Event
from inv_trader.model.identifiers import Label
from inv_trader.model.events import OrderFilled, OrderExpired, OrderRejected
from inv_trader.strategy import TradeStrategy
from inv_trader.portfolio.sizing import FixedRiskSizer
from inv_indicators.average.ema import ExponentialMovingAverage
from inv_indicators.atr import AverageTrueRange


class EMACrossPy(TradeStrategy):
    """"
    A simple moving average cross example strategy. When the fast EMA crosses
    the slow EMA then a STOP_MARKET atomic order is placed for that direction
    with a trailing stop and profit target at 1R risk.
    """

    def __init__(self,
                 instrument: Instrument,
                 bar_type: BarType,
                 label: str='001',
                 id_tag_trader: str='001',
                 id_tag_strategy: str='001',
                 risk_bp: float=10.0,
                 fast_ema: int=10,
                 slow_ema: int=20,
                 atr_period: int=20,
                 sl_atr_multiple: float=2.0,
                 logger: Logger=None):
        """
        Initializes a new instance of the EMACross class.

        :param instrument: The instrument for the strategy.
        :param bar_type: The bar type for the strategy.
        :param label: The optional unique label for the strategy.
        :param id_tag_trader: The unique order identifier tag for the trader.
        :param id_tag_strategy: The unique order identifier tag for the strategy.
        :param risk_bp: The risk per trade (basis points).
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        :param atr_period: The ATR period.
        :param sl_atr_multiple: The ATR multiple for stop-loss prices.
        :param logger: The logger for the strategy (can be None).
        """
        super().__init__(label=label,
                         id_tag_trader=id_tag_trader,
                         id_tag_strategy=id_tag_strategy,
                         logger=logger)

        self.instrument = instrument
        self.symbol = instrument.symbol
        self.bar_type = bar_type
        self.risk_bp = risk_bp
        self.position_sizer = FixedRiskSizer(self.instrument)
        self.tick_precision = instrument.tick_precision
        self.entry_buffer = instrument.tick_size
        self.SL_atr_multiple = sl_atr_multiple
        self.SL_buffer = instrument.tick_size * 10
        self.spread = Decimal(0)

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema)
        self.slow_ema = ExponentialMovingAverage(slow_ema)
        self.atr = AverageTrueRange(atr_period)

        # Register the indicators for updating
        self.register_indicator(self.bar_type, self.fast_ema, self.fast_ema.update)
        self.register_indicator(self.bar_type, self.slow_ema, self.slow_ema.update)
        self.register_indicator(self.bar_type, self.atr, self.atr.update)

        # Users custom order management logic
        self.entry_order = None
        self.stop_loss_order = None
        self.position_id = None

    def on_start(self):
        """
        This method is called when self.start() is called, and after internal start logic.
        """
        self.historical_bars(self.bar_type)
        self.subscribe_bars(self.bar_type)
        self.subscribe_ticks(self.symbol)

    def on_tick(self, tick: Tick):
        """
        This method is called whenever a Tick is received by the strategy, after
        the Tick has been processed by the base class (update last received Tick
        for the Symbol).
        The received Tick object is then passed into this method.

        :param tick: The received tick.
        """
        self.spread = tick.ask - tick.bid
        self.log.info(f"Received Tick({tick})")  # For demonstration purposes

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        This method is called whenever the strategy receives a Bar, after the
        Bar has been processed by the base class (update indicators etc).
        The received BarType and Bar objects are then passed into this method.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        if not self.fast_ema.initialized or not self.slow_ema.initialized:
            # Wait for indicators to warm up...
            return

        if self.is_flat() and self.entry_order is None:
            atomic_order = None

            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                price_entry = Price(self.last_bar(self.bar_type).high + self.entry_buffer + self.spread)
                price_stop_loss = Price(self.last_bar(self.bar_type).low - (self.atr.value * self.SL_atr_multiple))
                price_profit_target = Price(price_entry + (price_entry - price_stop_loss))

                exchange_rate = self.get_exchange_rate(self.instrument.quote_currency)
                position_size = self.position_sizer.calculate(
                    equity=self.account.free_equity,
                    exchange_rate=exchange_rate,
                    risk_bp=self.risk_bp,
                    price_entry=price_entry,
                    price_stop_loss=price_stop_loss,
                    commission_rate_bp=0.15,
                    hard_limit=20000000,
                    units=1,
                    unit_batch_size=1000)

                if position_size.value > 0:  # Sufficient equity for a position
                    atomic_order = self.order_factory.atomic_stop_market(
                        symbol=self.symbol,
                        order_side=OrderSide.BUY,
                        quantity=position_size,
                        price_entry=price_entry,
                        price_stop_loss=price_stop_loss,
                        price_profit_target=price_profit_target,
                        label=Label('S1'),
                        time_in_force=TimeInForce.GTD,
                        expire_time=self.time_now() + timedelta(minutes=1))

            # SELL LOGIC
            elif self.fast_ema.value < self.slow_ema.value:
                price_entry = Price(self.last_bar(self.bar_type).low - self.entry_buffer)
                price_stop_loss = Price(self.last_bar(self.bar_type).high + (self.atr.value * self.SL_atr_multiple) + self.spread)
                price_profit_target = Price(price_entry - (price_stop_loss - price_entry))

                exchange_rate = self.get_exchange_rate(self.instrument.quote_currency)
                position_size = self.position_sizer.calculate(
                    equity=self.account.free_equity,
                    exchange_rate=exchange_rate,
                    risk_bp=self.risk_bp,
                    price_entry=price_entry,
                    price_stop_loss=price_stop_loss,
                    commission_rate_bp=0.15,
                    hard_limit=20000000,
                    units=1,
                    unit_batch_size=1000)

                if position_size.value > 0:  # Sufficient equity for a position
                    atomic_order = self.order_factory.atomic_stop_market(
                        symbol=self.symbol,
                        order_side=OrderSide.SELL,
                        quantity=position_size,
                        price_entry=price_entry,
                        price_stop_loss=price_stop_loss,
                        price_profit_target=price_profit_target,
                        label=Label('S1'),
                        time_in_force=TimeInForce.GTD,
                        expire_time=self.time_now() + timedelta(minutes=1))

            # ENTRY ORDER SUBMISSION
            if atomic_order is not None:
                self.entry_order = atomic_order.entry
                self.stop_loss_order = atomic_order.stop_loss
                self.position_id = self.generate_position_id(self.symbol)

                self.submit_atomic_order(atomic_order, self.position_id)

        # TRAILING STOP LOGIC
        if self._is_stop_loss_active():
            if self.stop_loss_order.side is OrderSide.SELL:
                temp_price = Price(self.last_bar(self.bar_type).low - (self.atr.value * self.SL_atr_multiple))
                if self.stop_loss_order.price < temp_price:
                    self.modify_order(self.stop_loss_order, temp_price)
            elif self.stop_loss_order.side is OrderSide.BUY:
                temp_price = Price(self.last_bar(self.bar_type).high + (self.atr.value * self.SL_atr_multiple) + self.spread)
                if self.stop_loss_order.price > temp_price:
                    self.modify_order(self.stop_loss_order, temp_price)

    def on_event(self, event: Event):
        """
        This method is called whenever the strategy receives an Event object,
        after the event has been processed by the base class (updating any objects it needs to).
        These events could be AccountEvent, OrderEvent.

        :param event: The received event.
        """
        if isinstance(event, OrderFilled):
            if self.is_flat():
                self._reset_trade()

        elif isinstance(event, (OrderRejected, OrderExpired)):
            if event.order_id.equals(self.entry_order.id):
                self._reset_trade()
            # If a stop-loss order is rejected then flatten the entered position
            elif event.order_id.equals(self.stop_loss_order.id) and not self.is_flat():
                self.flatten_position(self.position_id)
                self._reset_trade()

    def on_stop(self):
        """
        This method is called when self.stop() is called before internal
        stopping logic.
        """
        if not self.is_flat():
            self.flatten_all_positions()

        self.cancel_all_orders("STOPPING STRATEGY")

    def on_reset(self):
        """
        This method is called when self.reset() is called, and after internal
        reset logic such as clearing the internally held bars, ticks and resetting
        all indicators.

        Put custom code to be run on a strategy reset here.
        """
        self._reset_trade()

    def on_dispose(self):
        """
        This method is called when self.dispose() is called. Dispose of any
        resources that had been used by the strategy here.
        """
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_ticks(self.symbol)

    # Custom internal method for this strategy
    def _reset_trade(self):
        """
        Reset the trade by clearing all order and position values.
        """
        self.entry_order = None
        self.stop_loss_order = None
        self.profit_target_order = None
        self.position_id = None

    # Custom internal method for this strategy
    def _is_stop_loss_active(self):
        """
        Return a value indicating whether the stop-loss is active.

        :return: True if active, else False.
        """
        return (self.stop_loss_order is not None
                and self.order_exists(self.stop_loss_order.id)
                and self.order_active(self.stop_loss_order.id))
