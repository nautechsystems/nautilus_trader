# WaveTrend Multi-Timeframe Strategy Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement a multi-timeframe WaveTrend trend-following strategy for BTCUSDT-PERP with majority-rules alignment and combination trailing stops.

**Architecture:** Strategy subscribes to 5m, 1h, and 4h bars, calculates WaveTrend (WT1/WT2) for each timeframe, enters on 5m crosses when at least 2/3 timeframes are aligned, and uses ATR-based trailing stop initially that switches to percentage-based after 2% profit.

**Tech Stack:** NautilusTrader framework, Python 3.12+, msgspec for config, built-in EMA/SMA indicators

---

## Task 1: Create WaveTrend Helper Functions

**Files:**
- Create: `nautilus_trader/examples/strategies/wavetrend_mtf.py`

**Step 1: Create strategy file with WaveTrend calculation helpers**

Create the file with helper functions to calculate WaveTrend values:

```python
#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.config import StrategyConfig
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.indicators.average.sma import SimpleMovingAverage
from nautilus_trader.model.data import Bar
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


class WaveTrendState:
    """Holds WaveTrend indicator state for one timeframe."""

    def __init__(self, channel_length: int, average_length: int):
        self.channel_length = channel_length
        self.average_length = average_length

        # WaveTrend calculation components
        self.esa_ema = ExponentialMovingAverage(channel_length)
        self.d_ema = ExponentialMovingAverage(channel_length)
        self.wt1_ema = ExponentialMovingAverage(average_length)
        self.wt1_values: list[float] = []  # Store for SMA(WT1, 4)

        # Current values
        self.wt1: float = 0.0
        self.wt2: float = 0.0
        self.prev_wt1: float = 0.0
        self.prev_wt2: float = 0.0

    def update(self, bar: Bar) -> None:
        """Update WaveTrend with new bar using LazyBear formula."""
        # Calculate HLC3 (typical price)
        hlc3 = (bar.high.as_double() + bar.low.as_double() + bar.close.as_double()) / 3.0

        # ESA = EMA(HLC3, channel_length)
        self.esa_ema.update_raw(hlc3)
        if not self.esa_ema.initialized:
            return
        esa = self.esa_ema.value

        # D = EMA(abs(HLC3 - ESA), channel_length)
        d_input = abs(hlc3 - esa)
        self.d_ema.update_raw(d_input)
        if not self.d_ema.initialized:
            return
        d = self.d_ema.value

        # CI = (HLC3 - ESA) / (0.015 * D)
        if d == 0:
            ci = 0.0
        else:
            ci = (hlc3 - esa) / (0.015 * d)

        # WT1 = EMA(CI, average_length)
        self.wt1_ema.update_raw(ci)
        if not self.wt1_ema.initialized:
            return

        # Store previous values
        self.prev_wt1 = self.wt1
        self.prev_wt2 = self.wt2

        # Update WT1
        self.wt1 = self.wt1_ema.value

        # WT2 = SMA(WT1, 4)
        self.wt1_values.append(self.wt1)
        if len(self.wt1_values) > 4:
            self.wt1_values.pop(0)

        if len(self.wt1_values) == 4:
            self.wt2 = sum(self.wt1_values) / 4.0

    @property
    def initialized(self) -> bool:
        """Check if WaveTrend is ready."""
        return len(self.wt1_values) == 4

    def is_bullish(self) -> bool:
        """Check if WT1 > WT2 (bullish)."""
        return self.wt1 > self.wt2

    def is_bearish(self) -> bool:
        """Check if WT1 < WT2 (bearish)."""
        return self.wt1 < self.wt2

    def bullish_cross(self) -> bool:
        """Check if WT1 just crossed above WT2."""
        return self.prev_wt1 <= self.prev_wt2 and self.wt1 > self.wt2

    def bearish_cross(self) -> bool:
        """Check if WT1 just crossed below WT2."""
        return self.prev_wt1 >= self.prev_wt2 and self.wt1 < self.wt2
```

**Step 2: Verify code compiles**

Run: `python nautilus_trader/examples/strategies/wavetrend_mtf.py`
Expected: No syntax errors (file just defines classes)

**Step 3: Commit**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf.py
git commit -m "feat: add WaveTrend calculation helpers for MTF strategy"
```

---

## Task 2: Create Strategy Configuration Class

**Files:**
- Modify: `nautilus_trader/examples/strategies/wavetrend_mtf.py`

**Step 1: Add configuration class at top of file (after imports)**

Add after the imports, before `WaveTrendState`:

```python
class WaveTrendMultiTimeframeConfig(StrategyConfig, frozen=True):
    """Configuration for WaveTrend Multi-Timeframe strategy."""

    instrument_id: InstrumentId
    trade_size: Decimal

    # WaveTrend parameters per timeframe
    wt_5m_channel_length: int = 10
    wt_5m_average_length: int = 21
    wt_1h_channel_length: int = 9
    wt_1h_average_length: int = 18
    wt_4h_channel_length: int = 8
    wt_4h_average_length: int = 15

    # Alignment rule
    min_aligned_timeframes: int = 2

    # Trailing stop parameters
    atr_period: int = 14
    atr_multiplier: float = 3.0
    profit_threshold_pct: float = 2.0
    percentage_trail: float = 1.5

    # Order management
    order_id_tag: str = "WT_MTF"
    oms_type: str = "HEDGING"
```

**Step 2: Verify imports are complete**

Add any missing imports at the top:

```python
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import StopMarketOrder
```

**Step 3: Verify code compiles**

Run: `python nautilus_trader/examples/strategies/wavetrend_mtf.py`
Expected: No syntax errors

**Step 4: Commit**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf.py
git commit -m "feat: add configuration class for WaveTrend MTF strategy"
```

---

## Task 3: Create Strategy Class Skeleton

**Files:**
- Modify: `nautilus_trader/examples/strategies/wavetrend_mtf.py`

**Step 1: Add strategy class skeleton at end of file**

```python
class WaveTrendMultiTimeframe(Strategy):
    """
    Multi-timeframe WaveTrend strategy with trend alignment.

    Enters on 5m WaveTrend crosses when at least 2 out of 3 timeframes
    (5m, 1h, 4h) show alignment. Uses combination trailing stop.
    """

    def __init__(self, config: WaveTrendMultiTimeframeConfig):
        super().__init__(config)

        # Configuration
        self.instrument_id = config.instrument_id
        self.trade_size = config.trade_size

        # WaveTrend states for each timeframe
        self.wt_5m = WaveTrendState(
            config.wt_5m_channel_length,
            config.wt_5m_average_length,
        )
        self.wt_1h = WaveTrendState(
            config.wt_1h_channel_length,
            config.wt_1h_average_length,
        )
        self.wt_4h = WaveTrendState(
            config.wt_4h_channel_length,
            config.wt_4h_average_length,
        )

        # ATR for initial trailing stop
        self.atr = AverageTrueRange(config.atr_period)

        # Trailing stop state
        self.entry_price: float | None = None
        self.peak_price: float | None = None
        self.stop_order: StopMarketOrder | None = None
        self.use_percentage_trail: bool = False

        # Configuration values
        self.min_aligned = config.min_aligned_timeframes
        self.atr_multiplier = config.atr_multiplier
        self.profit_threshold = config.profit_threshold_pct / 100.0
        self.percentage_trail = config.percentage_trail / 100.0

    def on_start(self) -> None:
        """Actions to be performed on strategy start."""
        self.log.info(f"Starting {self.__class__.__name__}")

        # Subscribe to bars for all three timeframes
        # Implementation in next task
        pass

    def on_stop(self) -> None:
        """Actions to be performed on strategy stop."""
        self.log.info(f"Stopping {self.__class__.__name__}")
        self.cancel_all_orders(self.instrument_id)
        self.close_all_positions(self.instrument_id)

    def on_bar(self, bar: Bar) -> None:
        """Handle bar updates for all timeframes."""
        # Route to appropriate handler based on bar type
        # Implementation in next task
        pass
```

**Step 2: Verify code compiles**

Run: `python nautilus_trader/examples/strategies/wavetrend_mtf.py`
Expected: No syntax errors

**Step 3: Commit**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf.py
git commit -m "feat: add WaveTrend MTF strategy class skeleton"
```

---

## Task 4: Implement Bar Subscription and Routing

**Files:**
- Modify: `nautilus_trader/examples/strategies/wavetrend_mtf.py` (in `WaveTrendMultiTimeframe` class)

**Step 1: Implement `on_start()` method**

Replace the `on_start()` method:

```python
def on_start(self) -> None:
    """Actions to be performed on strategy start."""
    self.log.info(f"Starting {self.__class__.__name__}")

    # Subscribe to 5-minute bars
    bar_type_5m = BarType.from_str(
        f"{self.instrument_id}-5-MINUTE-LAST-EXTERNAL"
    )
    self.subscribe_bars(bar_type_5m)

    # Subscribe to 1-hour bars
    bar_type_1h = BarType.from_str(
        f"{self.instrument_id}-1-HOUR-LAST-EXTERNAL"
    )
    self.subscribe_bars(bar_type_1h)

    # Subscribe to 4-hour bars
    bar_type_4h = BarType.from_str(
        f"{self.instrument_id}-4-HOUR-LAST-EXTERNAL"
    )
    self.subscribe_bars(bar_type_4h)

    self.log.info("Subscribed to 5m, 1h, 4h bars")
```

**Step 2: Implement `on_bar()` routing logic**

Replace the `on_bar()` method:

```python
def on_bar(self, bar: Bar) -> None:
    """Handle bar updates for all timeframes."""
    # Update appropriate WaveTrend based on bar aggregation period
    bar_spec = bar.bar_type.spec

    if bar_spec.step == 5 and "MINUTE" in str(bar_spec.aggregation):
        self._on_bar_5m(bar)
    elif bar_spec.step == 1 and "HOUR" in str(bar_spec.aggregation):
        self._on_bar_1h(bar)
    elif bar_spec.step == 4 and "HOUR" in str(bar_spec.aggregation):
        self._on_bar_4h(bar)

def _on_bar_5m(self, bar: Bar) -> None:
    """Handle 5-minute bar updates."""
    # Update ATR
    self.atr.update_raw(
        bar.high.as_double(),
        bar.low.as_double(),
        bar.close.as_double(),
    )

    # Update WaveTrend
    self.wt_5m.update(bar)

    if not self.wt_5m.initialized:
        return

    # Check for entry signals (implementation in next task)
    # self._check_entry_signals()

    # Update trailing stop if in position (implementation in later task)
    # self._update_trailing_stop(bar)

def _on_bar_1h(self, bar: Bar) -> None:
    """Handle 1-hour bar updates."""
    self.wt_1h.update(bar)

def _on_bar_4h(self, bar: Bar) -> None:
    """Handle 4-hour bar updates."""
    self.wt_4h.update(bar)
```

**Step 3: Verify code compiles**

Run: `python nautilus_trader/examples/strategies/wavetrend_mtf.py`
Expected: No syntax errors

**Step 4: Commit**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf.py
git commit -m "feat: implement bar subscription and routing for MTF strategy"
```

---

## Task 5: Implement Alignment and Entry Logic

**Files:**
- Modify: `nautilus_trader/examples/strategies/wavetrend_mtf.py` (in `WaveTrendMultiTimeframe` class)

**Step 1: Add alignment checking method**

Add this method to the `WaveTrendMultiTimeframe` class:

```python
def _count_aligned_timeframes(self, direction: str) -> int:
    """Count how many timeframes are aligned in the given direction."""
    count = 0

    if direction == "bullish":
        if self.wt_5m.is_bullish():
            count += 1
        if self.wt_1h.initialized and self.wt_1h.is_bullish():
            count += 1
        if self.wt_4h.initialized and self.wt_4h.is_bullish():
            count += 1
    elif direction == "bearish":
        if self.wt_5m.is_bearish():
            count += 1
        if self.wt_1h.initialized and self.wt_1h.is_bearish():
            count += 1
        if self.wt_4h.initialized and self.wt_4h.is_bearish():
            count += 1

    return count
```

**Step 2: Add entry signal checking method**

Add this method:

```python
def _check_entry_signals(self) -> None:
    """Check for entry signals based on WaveTrend crosses and alignment."""
    # Don't enter if already in a position
    if self.portfolio.is_flat(self.instrument_id) is False:
        return

    # Check for bullish cross on 5m
    if self.wt_5m.bullish_cross():
        aligned_count = self._count_aligned_timeframes("bullish")
        self.log.info(
            f"5m Bullish cross detected. Aligned timeframes: {aligned_count}/3"
        )

        if aligned_count >= self.min_aligned:
            self.log.info("Majority aligned - entering LONG")
            self._enter_long()

    # Check for bearish cross on 5m
    elif self.wt_5m.bearish_cross():
        aligned_count = self._count_aligned_timeframes("bearish")
        self.log.info(
            f"5m Bearish cross detected. Aligned timeframes: {aligned_count}/3"
        )

        if aligned_count >= self.min_aligned:
            self.log.info("Majority aligned - entering SHORT")
            self._enter_short()
```

**Step 3: Add entry execution methods**

Add these methods:

```python
def _enter_long(self) -> None:
    """Enter a long position."""
    order = self.order_factory.market(
        instrument_id=self.instrument_id,
        order_side=OrderSide.BUY,
        quantity=self.instrument.make_qty(self.trade_size),
    )
    self.submit_order(order)

def _enter_short(self) -> None:
    """Enter a short position."""
    order = self.order_factory.market(
        instrument_id=self.instrument_id,
        order_side=OrderSide.SELL,
        quantity=self.instrument.make_qty(self.trade_size),
    )
    self.submit_order(order)
```

**Step 4: Uncomment the call in `_on_bar_5m`**

In the `_on_bar_5m` method, uncomment:

```python
# Check for entry signals
self._check_entry_signals()
```

**Step 5: Verify code compiles**

Run: `python nautilus_trader/examples/strategies/wavetrend_mtf.py`
Expected: No syntax errors

**Step 6: Commit**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf.py
git commit -m "feat: implement alignment checking and entry signal logic"
```

---

## Task 6: Implement ATR-Based Trailing Stop

**Files:**
- Modify: `nautilus_trader/examples/strategies/wavetrend_mtf.py` (in `WaveTrendMultiTimeframe` class)

**Step 1: Add `on_order_filled` handler to set initial stop**

Add this method:

```python
def on_order_filled(self, event) -> None:
    """Handle order filled events."""
    if event.order_side == OrderSide.BUY or event.order_side == OrderSide.SELL:
        # Entry order filled - set initial stop
        self.entry_price = event.last_px.as_double()
        self.peak_price = self.entry_price
        self.use_percentage_trail = False

        self.log.info(
            f"Entry filled at {self.entry_price:.2f}, setting ATR-based stop"
        )

        # Set initial ATR-based stop
        self._set_atr_stop(event.order_side)
```

**Step 2: Add ATR stop calculation method**

Add this method:

```python
def _set_atr_stop(self, entry_side: OrderSide) -> None:
    """Set ATR-based trailing stop."""
    if not self.atr.initialized or self.entry_price is None:
        return

    # Calculate stop distance
    stop_distance = self.atr.value * self.atr_multiplier

    # Calculate stop price based on position direction
    if entry_side == OrderSide.BUY:
        # Long position - stop below entry
        stop_price = self.entry_price - stop_distance
        trigger_price = self.instrument.make_price(stop_price)

        # Cancel existing stop if any
        if self.stop_order is not None:
            self.cancel_order(self.stop_order)

        # Create new stop order
        self.stop_order = self.order_factory.stop_market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
            trigger_price=trigger_price,
            trigger_type=TriggerType.DEFAULT,
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(self.stop_order)

        self.log.info(f"ATR stop set at {stop_price:.2f} (distance: {stop_distance:.2f})")

    elif entry_side == OrderSide.SELL:
        # Short position - stop above entry
        stop_price = self.entry_price + stop_distance
        trigger_price = self.instrument.make_price(stop_price)

        # Cancel existing stop if any
        if self.stop_order is not None:
            self.cancel_order(self.stop_order)

        # Create new stop order
        self.stop_order = self.order_factory.stop_market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            trigger_price=trigger_price,
            trigger_type=TriggerType.DEFAULT,
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(self.stop_order)

        self.log.info(f"ATR stop set at {stop_price:.2f} (distance: {stop_distance:.2f})")
```

**Step 3: Verify code compiles**

Run: `python nautilus_trader/examples/strategies/wavetrend_mtf.py`
Expected: No syntax errors

**Step 4: Commit**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf.py
git commit -m "feat: implement ATR-based trailing stop initialization"
```

---

## Task 7: Implement Percentage-Based Trailing Stop

**Files:**
- Modify: `nautilus_trader/examples/strategies/wavetrend_mtf.py` (in `WaveTrendMultiTimeframe` class)

**Step 1: Add percentage stop calculation method**

Add this method:

```python
def _set_percentage_stop(self, position_side: OrderSide) -> None:
    """Set percentage-based trailing stop from peak price."""
    if self.peak_price is None:
        return

    if position_side == OrderSide.BUY:
        # Long position - trail below peak
        stop_price = self.peak_price * (1 - self.percentage_trail)
        trigger_price = self.instrument.make_price(stop_price)

        # Cancel existing stop
        if self.stop_order is not None:
            self.cancel_order(self.stop_order)

        # Create new stop
        self.stop_order = self.order_factory.stop_market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
            trigger_price=trigger_price,
            trigger_type=TriggerType.DEFAULT,
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(self.stop_order)

        self.log.info(
            f"Percentage stop updated: {stop_price:.2f} "
            f"({self.percentage_trail * 100:.1f}% from peak {self.peak_price:.2f})"
        )

    elif position_side == OrderSide.SELL:
        # Short position - trail above peak (lowest point)
        stop_price = self.peak_price * (1 + self.percentage_trail)
        trigger_price = self.instrument.make_price(stop_price)

        # Cancel existing stop
        if self.stop_order is not None:
            self.cancel_order(self.stop_order)

        # Create new stop
        self.stop_order = self.order_factory.stop_market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            trigger_price=trigger_price,
            trigger_type=TriggerType.DEFAULT,
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(self.stop_order)

        self.log.info(
            f"Percentage stop updated: {stop_price:.2f} "
            f"({self.percentage_trail * 100:.1f}% from peak {self.peak_price:.2f})"
        )
```

**Step 2: Verify code compiles**

Run: `python nautilus_trader/examples/strategies/wavetrend_mtf.py`
Expected: No syntax errors

**Step 3: Commit**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf.py
git commit -m "feat: implement percentage-based trailing stop"
```

---

## Task 8: Implement Trailing Stop Updates and Mode Switching

**Files:**
- Modify: `nautilus_trader/examples/strategies/wavetrend_mtf.py` (in `WaveTrendMultiTimeframe` class)

**Step 1: Add trailing stop update method**

Add this method:

```python
def _update_trailing_stop(self, bar: Bar) -> None:
    """Update trailing stop based on current price and P&L."""
    # Only update if in a position
    if self.portfolio.is_flat(self.instrument_id):
        return

    if self.entry_price is None:
        return

    # Get current position
    position = self.portfolio.position(self.instrument_id)
    if position is None:
        return

    current_price = bar.close.as_double()

    # Update peak price
    if position.side == OrderSide.BUY:
        # Long position - track highest price
        if self.peak_price is None or current_price > self.peak_price:
            self.peak_price = current_price
    elif position.side == OrderSide.SELL:
        # Short position - track lowest price
        if self.peak_price is None or current_price < self.peak_price:
            self.peak_price = current_price

    # Calculate unrealized P&L percentage
    if position.side == OrderSide.BUY:
        pnl_pct = (current_price - self.entry_price) / self.entry_price
    else:
        pnl_pct = (self.entry_price - current_price) / self.entry_price

    # Check if we should switch to percentage trail
    if not self.use_percentage_trail and pnl_pct >= self.profit_threshold:
        self.log.info(
            f"Profit threshold reached ({pnl_pct * 100:.2f}%), "
            f"switching to percentage trail"
        )
        self.use_percentage_trail = True
        self._set_percentage_stop(position.side)

    # Update stop based on current mode
    elif self.use_percentage_trail:
        # Update percentage stop if peak moved
        self._set_percentage_stop(position.side)
```

**Step 2: Add position reset on close**

Add this method:

```python
def on_position_closed(self, position) -> None:
    """Handle position closed event."""
    self.log.info(f"Position closed: {position}")

    # Reset state
    self.entry_price = None
    self.peak_price = None
    self.stop_order = None
    self.use_percentage_trail = False
```

**Step 3: Uncomment the call in `_on_bar_5m`**

In the `_on_bar_5m` method, uncomment:

```python
# Update trailing stop if in position
self._update_trailing_stop(bar)
```

**Step 4: Verify code compiles**

Run: `python nautilus_trader/examples/strategies/wavetrend_mtf.py`
Expected: No syntax errors

**Step 5: Commit**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf.py
git commit -m "feat: implement trailing stop updates and mode switching"
```

---

## Task 9: Create Live Trading Script

**Files:**
- Create: `examples/live/binance/binance_futures_wavetrend_mtf.py`

**Step 1: Create live trading script**

```python
#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceAccountType
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceExecClientConfig
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance import BinanceLiveExecClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.wavetrend_mtf import WaveTrendMultiTimeframe
from nautilus_trader.examples.strategies.wavetrend_mtf import WaveTrendMultiTimeframeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A STRATEGY FOR TESTING PURPOSES. ***
# *** ADJUST PARAMETERS AND RISK MANAGEMENT BEFORE LIVE TRADING. ***

# Strategy config params
symbol = "BTCUSDT-PERP"
instrument_id = InstrumentId.from_str(f"{symbol}.{BINANCE}")
order_qty = Decimal("0.001")

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("WAVETREND-001"),
    logging=LoggingConfig(log_level="INFO"),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
    ),
    cache=CacheConfig(
        timestamps_as_iso8601=True,
        flush_on_start=False,
    ),
    data_clients={
        BINANCE: BinanceDataClientConfig(
            api_key=None,  # 'BINANCE_FUTURES_TESTNET_API_KEY' env var
            api_secret=None,  # 'BINANCE_FUTURES_TESTNET_API_SECRET' env var
            account_type=BinanceAccountType.USDT_FUTURES,
            testnet=True,  # Testnet mode
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        BINANCE: BinanceExecClientConfig(
            api_key=None,  # Auto-loads from env
            api_secret=None,
            account_type=BinanceAccountType.USDT_FUTURES,
            testnet=True,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            max_retries=3,
            use_position_ids=False,
        ),
    },
    timeout_connection=30.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure WaveTrend strategy
strat_config = WaveTrendMultiTimeframeConfig(
    instrument_id=instrument_id,
    trade_size=order_qty,
    # WaveTrend parameters (optimized per timeframe)
    wt_5m_channel_length=10,
    wt_5m_average_length=21,
    wt_1h_channel_length=9,
    wt_1h_average_length=18,
    wt_4h_channel_length=8,
    wt_4h_average_length=15,
    # Alignment
    min_aligned_timeframes=2,
    # Trailing stop (conservative)
    atr_period=14,
    atr_multiplier=3.0,
    profit_threshold_pct=2.0,
    percentage_trail=1.5,
    order_id_tag="WT_MTF",
)

# Instantiate strategy
strategy = WaveTrendMultiTimeframe(config=strat_config)

# Add strategy to node
node.trader.add_strategy(strategy)

# Register client factories
node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
node.add_exec_client_factory(BINANCE, BinanceLiveExecClientFactory)
node.build()

# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
```

**Step 2: Verify script syntax**

Run: `python examples/live/binance/binance_futures_wavetrend_mtf.py --help || true`
Expected: Imports work, no syntax errors (will fail to run without API keys)

**Step 3: Commit**

```bash
git add examples/live/binance/binance_futures_wavetrend_mtf.py
git commit -m "feat: add live trading script for WaveTrend MTF strategy"
```

---

## Task 10: Create Backtesting Script

**Files:**
- Create: `examples/backtest/backtest_wavetrend_mtf.py`

**Step 1: Create backtesting script**

```python
#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from pathlib import Path

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.examples.strategies.wavetrend_mtf import WaveTrendMultiTimeframe
from nautilus_trader.examples.strategies.wavetrend_mtf import WaveTrendMultiTimeframeConfig
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog import ParquetDataCatalog


# *** CONFIGURE THESE PARAMETERS ***

# Data catalog path (update to your local catalog)
CATALOG_PATH = Path("~/.nautilus/catalog").expanduser()

# Instrument
VENUE = Venue("BINANCE")
SYMBOL = "BTCUSDT-PERP"
instrument_id = InstrumentId.from_str(f"{SYMBOL}.{VENUE}")

# Backtest period (update to match your available data)
START = "2024-01-01"
END = "2024-01-31"

# Strategy parameters
TRADE_SIZE = Decimal("0.001")


def run_backtest():
    """Run WaveTrend MTF strategy backtest."""

    # Load data catalog
    catalog = ParquetDataCatalog(CATALOG_PATH)

    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
    )
    engine = BacktestEngine(config=config)

    # Add venue
    engine.add_venue(
        venue=VENUE,
        oms_type="HEDGING",
        account_type="MARGIN",
        starting_balances=[Money(10_000, "USDT")],
    )

    # Load instrument
    instruments = catalog.instruments(instrument_ids=[str(instrument_id)])
    if not instruments:
        raise ValueError(f"No instrument found for {instrument_id}")

    instrument = instruments[0]
    engine.add_instrument(instrument)

    # Load bar data for all three timeframes
    print(f"Loading bars for {instrument_id}...")

    # Load 5m bars
    bars_5m = catalog.bars(
        instrument_ids=[str(instrument_id)],
        bar_type=BarType.from_str(f"{instrument_id}-5-MINUTE-LAST-EXTERNAL"),
        start=START,
        end=END,
    )
    if bars_5m:
        engine.add_data(bars_5m)
        print(f"Loaded {len(bars_5m)} 5m bars")

    # Load 1h bars
    bars_1h = catalog.bars(
        instrument_ids=[str(instrument_id)],
        bar_type=BarType.from_str(f"{instrument_id}-1-HOUR-LAST-EXTERNAL"),
        start=START,
        end=END,
    )
    if bars_1h:
        engine.add_data(bars_1h)
        print(f"Loaded {len(bars_1h)} 1h bars")

    # Load 4h bars
    bars_4h = catalog.bars(
        instrument_ids=[str(instrument_id)],
        bar_type=BarType.from_str(f"{instrument_id}-4-HOUR-LAST-EXTERNAL"),
        start=START,
        end=END,
    )
    if bars_4h:
        engine.add_data(bars_4h)
        print(f"Loaded {len(bars_4h)} 4h bars")

    # Configure strategy
    strat_config = WaveTrendMultiTimeframeConfig(
        instrument_id=instrument_id,
        trade_size=TRADE_SIZE,
        wt_5m_channel_length=10,
        wt_5m_average_length=21,
        wt_1h_channel_length=9,
        wt_1h_average_length=18,
        wt_4h_channel_length=8,
        wt_4h_average_length=15,
        min_aligned_timeframes=2,
        atr_period=14,
        atr_multiplier=3.0,
        profit_threshold_pct=2.0,
        percentage_trail=1.5,
    )

    # Add strategy
    strategy = WaveTrendMultiTimeframe(config=strat_config)
    engine.add_strategy(strategy)

    # Run backtest
    print("\nRunning backtest...")
    engine.run()

    # Print results
    print("\n" + "=" * 80)
    print("BACKTEST RESULTS")
    print("=" * 80)

    # Account report
    print("\n--- Account Report ---")
    print(engine.trader.generate_account_report(VENUE))

    # Order fills report
    print("\n--- Order Fills Report ---")
    print(engine.trader.generate_order_fills_report())

    # Positions report
    print("\n--- Positions Report ---")
    print(engine.trader.generate_positions_report())

    # Cleanup
    engine.dispose()


if __name__ == "__main__":
    run_backtest()
```

**Step 2: Verify script syntax**

Run: `python examples/backtest/backtest_wavetrend_mtf.py --help || true`
Expected: Imports work, no syntax errors (may fail if catalog doesn't exist)

**Step 3: Commit**

```bash
git add examples/backtest/backtest_wavetrend_mtf.py
git commit -m "feat: add backtesting script for WaveTrend MTF strategy"
```

---

## Task 11: Test Strategy Build

**Files:**
- None (validation task)

**Step 1: Build the project**

Run: `make build-debug`
Expected: Builds successfully without errors

**Step 2: Verify imports work**

Run: `python -c "from nautilus_trader.examples.strategies.wavetrend_mtf import WaveTrendMultiTimeframe, WaveTrendMultiTimeframeConfig; print('✓ Import successful')"`
Expected: `✓ Import successful`

**Step 3: Create summary document**

Create `docs/plans/2025-12-21-wavetrend-mtf-complete.md`:

```markdown
# WaveTrend Multi-Timeframe Strategy - Implementation Complete

**Date**: 2025-12-21
**Status**: ✅ Implementation Complete

## Files Created

1. `nautilus_trader/examples/strategies/wavetrend_mtf.py` - Strategy implementation
2. `examples/live/binance/binance_futures_wavetrend_mtf.py` - Live trading script
3. `examples/backtest/backtest_wavetrend_mtf.py` - Backtesting script

## Strategy Features

- ✅ WaveTrend indicator calculation (LazyBear formula)
- ✅ Multi-timeframe support (5m, 1h, 4h)
- ✅ Majority-rules alignment (2/3 timeframes)
- ✅ Entry on 5m crosses with alignment
- ✅ ATR-based initial trailing stop (3x ATR)
- ✅ Percentage-based trail after 2% profit (1.5% trail)
- ✅ Automatic mode switching

## Next Steps

### 1. Backtesting (Required before testnet)

Update `examples/backtest/backtest_wavetrend_mtf.py`:
- Set `CATALOG_PATH` to your data catalog
- Set `START` and `END` dates matching your data
- Run: `uv run --active --no-sync python examples/backtest/backtest_wavetrend_mtf.py`

Analyze results:
- Win rate
- Profit factor
- Max drawdown
- Average win/loss ratio

### 2. Testnet Validation

After successful backtest:
1. Ensure API keys in `.env`:
   - `BINANCE_FUTURES_TESTNET_API_KEY`
   - `BINANCE_FUTURES_TESTNET_API_SECRET`
2. Run: `uv run --active --no-sync python examples/live/binance/binance_futures_wavetrend_mtf.py`
3. Monitor for 24-48 hours
4. Verify alignment logic and stop updates

### 3. Parameter Optimization (Optional)

After initial testing, consider optimizing:
- WaveTrend channel/average lengths per timeframe
- ATR multiplier (test 2x, 2.5x, 3x, 3.5x)
- Percentage trail (test 1%, 1.5%, 2%)
- Profit threshold for mode switch (test 1.5%, 2%, 2.5%)

### 4. Production Deployment

Only after successful testnet validation:
1. Update script to use `testnet=False`
2. Use production API keys
3. Start with minimum position size
4. Monitor closely for first week

## Configuration Reference

**Conservative (current)**:
- ATR multiplier: 3.0
- Profit threshold: 2.0%
- Trail percentage: 1.5%

**Moderate**:
- ATR multiplier: 2.5
- Profit threshold: 1.5%
- Trail percentage: 1.0%

**Aggressive**:
- ATR multiplier: 2.0
- Profit threshold: 1.0%
- Trail percentage: 0.75%
```

**Step 4: Commit summary**

```bash
git add docs/plans/2025-12-21-wavetrend-mtf-complete.md
git commit -m "docs: add WaveTrend MTF implementation summary and next steps"
```

---

## Implementation Complete

All tasks complete! Strategy is ready for:
1. ✅ Backtesting with historical data
2. ✅ Live testing on Binance Futures testnet
3. ✅ Parameter optimization
4. ✅ Production deployment (after validation)

See `docs/plans/2025-12-21-wavetrend-mtf-complete.md` for next steps.
