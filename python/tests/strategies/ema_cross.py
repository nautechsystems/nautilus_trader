# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Simple EMA cross strategy for acceptance testing.

Subscribes to bars, tracks fast/slow EMA, and submits market orders on crossovers.

"""

from __future__ import annotations

from nautilus_trader.core import UUID4
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import OrderSide
from nautilus_trader.model import PositionSide
from nautilus_trader.model import Quantity
from nautilus_trader.model import TimeInForce
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class EMACrossConfig(StrategyConfig):
    """
    Configuration for the EMA cross test strategy.
    """

    def __new__(cls, *args, strategy_id: str | None = None, **kwargs):
        # `StrategyConfig` is a pyo3 @final type whose `__new__` validates
        # `strategy_id` as a `StrategyId`. For tests that need to register
        # multiple instances of the same strategy class, we accept a string
        # `strategy_id` in the subclass `__new__`, strip just that override
        # before delegating (so it doesn't fail base-type validation), and
        # forward every remaining base `StrategyConfig` kwarg
        # (`order_id_tag`, `log_events`, `oms_type`, etc.) so they're applied
        # by the pyo3 base. The override is exposed via a property below.
        kwargs.pop("instrument_id", None)
        kwargs.pop("bar_type", None)
        kwargs.pop("trade_size", None)
        kwargs.pop("fast_ema_period", None)
        kwargs.pop("slow_ema_period", None)
        instance = super().__new__(cls, *args, **kwargs)
        instance._strategy_id_override = strategy_id
        return instance

    def __init__(
        self,
        instrument_id: str,
        bar_type: str,
        trade_size: str,
        fast_ema_period: int = 10,
        slow_ema_period: int = 20,
        strategy_id: str | None = None,
        **kwargs,
    ):
        # The pyo3 base initialises its state in `__new__`, so `__init__`
        # falls through to `object.__init__` which only accepts `self`.
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size
        self.fast_ema_period = fast_ema_period
        self.slow_ema_period = slow_ema_period

    @property
    def strategy_id(self):
        if self._strategy_id_override is not None:
            return self._strategy_id_override
        return super().strategy_id


class EMACross(Strategy):
    """
    Simple EMA cross strategy for acceptance testing.

    Tracks a fast and slow exponential moving average. Enters long when the
    fast EMA crosses above the slow EMA and enters short on the reverse cross.
    Uses netting: flattens the opposite position before entering the new one.

    """

    def __init__(self, config: EMACrossConfig):
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._bar_type = BarType.from_str(config.bar_type)
        self._trade_size = Quantity.from_str(config.trade_size)
        self._fast_period = config.fast_ema_period
        self._slow_period = config.slow_ema_period

        self._fast_ema = 0.0
        self._slow_ema = 0.0
        self._fast_alpha = 2.0 / (self._fast_period + 1)
        self._slow_alpha = 2.0 / (self._slow_period + 1)
        self._bar_count = 0
        self._order_count = 0
        self._position_side = PositionSide.FLAT

    @property
    def bar_count(self) -> int:
        return self._bar_count

    def on_start(self):
        self.subscribe_bars(self._bar_type)

    def on_bar(self, bar: Bar):
        close = float(bar.close)
        self._bar_count += 1

        if self._bar_count == 1:
            self._fast_ema = close
            self._slow_ema = close
            return

        prev_fast = self._fast_ema
        prev_slow = self._slow_ema

        self._fast_ema = self._fast_alpha * close + (1.0 - self._fast_alpha) * prev_fast
        self._slow_ema = self._slow_alpha * close + (1.0 - self._slow_alpha) * prev_slow

        if self._bar_count < self._slow_period:
            return

        fast_above = self._fast_ema > self._slow_ema
        was_above = prev_fast > prev_slow

        if fast_above and not was_above:
            self._enter(OrderSide.BUY)
        elif not fast_above and was_above:
            self._enter(OrderSide.SELL)

    def _enter(self, side: OrderSide):
        if self._position_side == PositionSide.LONG and side == OrderSide.BUY:
            return
        if self._position_side == PositionSide.SHORT and side == OrderSide.SELL:
            return

        if self._position_side != PositionSide.FLAT:
            self._flat()

        self._submit_market(side)

        if side == OrderSide.BUY:
            self._position_side = PositionSide.LONG
        else:
            self._position_side = PositionSide.SHORT

    def _flat(self):
        if self._position_side == PositionSide.LONG:
            self._submit_market(OrderSide.SELL)
        elif self._position_side == PositionSide.SHORT:
            self._submit_market(OrderSide.BUY)
        self._position_side = PositionSide.FLAT

    def _submit_market(self, side: OrderSide):
        self._order_count += 1
        order = MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=self._instrument_id,
            client_order_id=ClientOrderId(f"{self.strategy_id}-{self._order_count}"),
            order_side=side,
            quantity=self._trade_size,
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            reduce_only=False,
            quote_quantity=False,
            contingency_type=ContingencyType.NO_CONTINGENCY,
        )
        self.submit_order(order)

    def on_reset(self):
        self._fast_ema = 0.0
        self._slow_ema = 0.0
        self._bar_count = 0
        self._order_count = 0
        self._position_side = PositionSide.FLAT

    def on_stop(self):
        pass
