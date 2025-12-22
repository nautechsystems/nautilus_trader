# WaveTrend V4 Strategy Improvements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement five improved WaveTrend strategy variants with increasing sophistication and trade frequency

**Architecture:**
- V4.2 Enhanced: Better position sizing (2.0x in NORMAL volatility)
- V4.3 Improved: Hybrid blocking + dynamic exits with faster regime detection
- V4.4a: Moderate frequency (~40-50 trades/year via relaxed filters)
- V4.4b: High frequency (~150-200 trades/year via mean reversion + pyramiding)
- V4.4c: Daily trading (~250+ trades/year via fast timeframes + multiple entries)

**Tech Stack:**
- Python 3.13, NautilusTrader framework
- msgspec for configs (frozen dataclasses)
- WaveTrend indicator, ATR, EMA components

---

## Task 1: Create V4.2 Enhanced Config and Strategy

**Files:**
- Create: `nautilus_trader/examples/strategies/wavetrend_mtf_v4_2_enhanced.py`
- Reference: `nautilus_trader/examples/strategies/wavetrend_mtf_v4_2.py` (existing)

**Step 1: Copy V4.2 base and modify config**

Copy existing V4.2:
```bash
cp nautilus_trader/examples/strategies/wavetrend_mtf_v4_2.py \
   nautilus_trader/examples/strategies/wavetrend_mtf_v4_2_enhanced.py
```

**Step 2: Update config class for enhanced sizing**

Edit `wavetrend_mtf_v4_2_enhanced.py`:

Change class name from `WaveTrendMultiTimeframeV4_2Config` to `WaveTrendMultiTimeframeV4_2EnhancedConfig`

Update docstring:
```python
"""
Configuration for WaveTrend Multi-Timeframe strategy V4.2 Enhanced.

V4.2 Enhanced improvements over original V4.2:
- Larger position sizing multiplier: 2.0x instead of 1.25x (meaningful impact)
- Apply boost in NORMAL volatility (0.9-1.1x) instead of LOW (<0.9x)
- NORMAL occurs ~40-50% of time vs LOW ~10-15%
- Keep V4.1's volatility blocking (blocks HIGH/ELEVATED)

Expected Result:
- Similar trade count to V4.1 (60-70 positions)
- Larger average position size = better returns
- Target: +7-9% over 4 years (vs V4.1's +5.74%)

Risk:
- Larger positions = larger losses when stops hit
- Could underperform V4.1 if win rate drops
"""
```

Update sizing parameters:
```python
# V4.2 Enhanced: Position sizing in NORMAL volatility
normal_vol_size_multiplier: PositiveFloat = 2.0  # Was 1.25x in LOW vol
```

Remove the old `low_vol_size_multiplier` parameter.

Update `order_id_tag`:
```python
order_id_tag: str = "WT_MTF_V4_2E"
```

**Step 3: Update strategy class**

Change class name from `WaveTrendMultiTimeframeV4_2` to `WaveTrendMultiTimeframeV4_2Enhanced`

Update `__init__` to use new config type.

**Step 4: Modify position sizing logic**

In the `_calculate_position_size()` method (around line 450-480), change from:

```python
# OLD V4.2: Boost in LOW volatility
if volatility_regime == "LOW":
    size_multiplier = self.config.low_vol_size_multiplier
```

To:

```python
# V4.2 Enhanced: Boost in NORMAL volatility
if volatility_regime == "NORMAL":
    size_multiplier = self.config.normal_vol_size_multiplier
else:
    size_multiplier = 1.0
```

**Step 5: Test the changes compile**

```bash
cd /home/johan/git/nautilus_trader
make build-debug
```

Expected: Clean build, no errors

**Step 6: Commit V4.2 Enhanced**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf_v4_2_enhanced.py
git commit -m "feat: add V4.2 Enhanced strategy (2.0x sizing in NORMAL vol)

- Increase multiplier from 1.25x to 2.0x
- Apply in NORMAL volatility (0.9-1.1x) instead of LOW (<0.9x)
- More frequent activation, meaningful impact
- Target: +7-9% over 4 years"
```

---

## Task 2: Create V4.2 Enhanced Backtest Script

**Files:**
- Create: `examples/backtest/backtest_wavetrend_mtf_v4_2_enhanced.py`
- Reference: `examples/backtest/backtest_wavetrend_mtf_v4_2.py`

**Step 1: Copy V4.2 backtest and modify**

```bash
cp examples/backtest/backtest_wavetrend_mtf_v4_2.py \
   examples/backtest/backtest_wavetrend_mtf_v4_2_enhanced.py
```

**Step 2: Update imports**

Change:
```python
from nautilus_trader.examples.strategies.wavetrend_mtf_v4_2 import WaveTrendMultiTimeframeV4_2, WaveTrendMultiTimeframeV4_2Config
```

To:
```python
from nautilus_trader.examples.strategies.wavetrend_mtf_v4_2_enhanced import WaveTrendMultiTimeframeV4_2Enhanced, WaveTrendMultiTimeframeV4_2EnhancedConfig
```

**Step 3: Update docstring**

Change module docstring to describe V4.2 Enhanced.

**Step 4: Update config instantiation**

Change from `WaveTrendMultiTimeframeV4_2Config` to `WaveTrendMultiTimeframeV4_2EnhancedConfig`.

Change from `WaveTrendMultiTimeframeV4_2` to `WaveTrendMultiTimeframeV4_2Enhanced`.

Update config parameters to remove `low_vol_size_multiplier` and add `normal_vol_size_multiplier=2.0`.

**Step 5: Test backtest runs**

```bash
uv run --active --no-sync python examples/backtest/backtest_wavetrend_mtf_v4_2_enhanced.py 2024-01-01 2024-12-31
```

Expected: Backtest completes successfully, shows results

**Step 6: Commit backtest script**

```bash
git add examples/backtest/backtest_wavetrend_mtf_v4_2_enhanced.py
git commit -m "feat: add V4.2 Enhanced backtest script

- Uses new WaveTrendMultiTimeframeV4_2Enhanced strategy
- 2.0x position sizing in NORMAL volatility
- Test with: python backtest_wavetrend_mtf_v4_2_enhanced.py 2024-01-01 2024-12-31"
```

---

## Task 3: Create V4.3 Improved Config and Strategy

**Files:**
- Create: `nautilus_trader/examples/strategies/wavetrend_mtf_v4_3_improved.py`
- Reference: `nautilus_trader/examples/strategies/wavetrend_mtf_v4_3.py` (existing)

**Step 1: Copy V4.3 base**

```bash
cp nautilus_trader/examples/strategies/wavetrend_mtf_v4_3.py \
   nautilus_trader/examples/strategies/wavetrend_mtf_v4_3_improved.py
```

**Step 2: Update config class**

Change class name to `WaveTrendMultiTimeframeV4_3ImprovedConfig`.

Update docstring:
```python
"""
Configuration for WaveTrend Multi-Timeframe strategy V4.3 Improved.

V4.3 Improved fixes over original V4.3:
1. Add V4.1's volatility BLOCKING (blocks HIGH/ELEVATED volatility)
2. Faster regime detection: 24h vs 7d (was 48h vs 30d) - 4x faster
3. Tighter stops in remaining regimes
4. Simplify to 2 regimes (NORMAL/LOW) since we block HIGH/ELEVATED

Original V4.3 traded through ALL volatility with adjusted stops.
This FAILED in choppy 2022 (-0.04% vs V4.1's +0.55%).

V4.3 Improved BLOCKS bad conditions AND adjusts exits dynamically.

Expected Result:
- Fewer trades than original V4.3 (55-65 vs 73)
- Better 2022 performance (blocking prevents choppy losses)
- Target: +6-7% over 4 years

Risk:
- Faster detection might exit good trends prematurely
"""
```

**Step 3: Add volatility blocking parameters**

Add to config:
```python
# V4.3 Improved: Add V4.1's blocking filter (CRITICAL FIX)
use_volatility_blocking: bool = True  # BLOCK HIGH/ELEVATED volatility
```

**Step 4: Update regime detection to faster windows**

Change:
```python
# V4.3 Improved: Faster regime detection (4x faster)
atr_recent_bars: PositiveInt = 288  # 24 hours at 5m (was 576 = 48h)
atr_baseline_bars: PositiveInt = 2016  # 7 days at 5m (was 8640 = 30d)
```

**Step 5: Simplify to 2 regimes, tighten parameters**

Update dynamic exit parameters:
```python
# V4.3 Improved: Simplified to 2 regimes (we block HIGH/ELEVATED)
# Remove high_vol_* and elevated_vol_* parameters

# NORMAL volatility (0.9-1.1x): Tightened from original
normal_vol_atr_mult: PositiveFloat = 3.5  # Was 4.5 in original V4.3
normal_vol_profit_pct: PositiveFloat = 3.0  # Was 4.0

# LOW volatility (<0.9x): Tightened from original
low_vol_atr_mult: PositiveFloat = 5.0  # Was 6.0
low_vol_profit_pct: PositiveFloat = 5.0  # Was 6.0
```

**Step 6: Update order tag**

```python
order_id_tag: str = "WT_MTF_V4_3I"
```

**Step 7: Update strategy class**

Change class name to `WaveTrendMultiTimeframeV4_3Improved`.

**Step 8: Add blocking logic to entry checks**

In the entry signal checking method (around line 500-600), add blocking before entry:

```python
# V4.3 Improved: BLOCK HIGH/ELEVATED volatility (critical fix)
if self.config.use_volatility_blocking:
    volatility_regime = self._detect_volatility_regime()
    if volatility_regime in ("HIGH", "ELEVATED"):
        self._log.info("BLOCKED: HIGH/ELEVATED volatility detected")
        return  # Don't enter trade
```

**Step 9: Update regime detection to only return NORMAL or LOW**

In `_detect_volatility_regime()` method, simplify logic:

```python
def _detect_volatility_regime(self) -> str:
    """Detect volatility regime for V4.3 Improved (NORMAL or LOW only)."""
    # Calculate recent vs baseline ATR ratio
    ratio = self._recent_atr / self._baseline_atr

    # V4.3 Improved: Block HIGH/ELEVATED elsewhere, only classify NORMAL/LOW here
    if ratio > 1.1:
        return "HIGH"  # Will be blocked
    elif ratio > 0.9:
        return "NORMAL"
    else:
        return "LOW"
```

**Step 10: Update dynamic exit parameters method**

Simplify to only handle NORMAL and LOW cases:

```python
def _get_exit_parameters(self, regime: str) -> tuple[float, float]:
    """Get ATR multiplier and profit target % based on regime."""
    if regime == "LOW":
        return (self.config.low_vol_atr_mult, self.config.low_vol_profit_pct)
    else:  # NORMAL
        return (self.config.normal_vol_atr_mult, self.config.normal_vol_profit_pct)
```

**Step 11: Test compilation**

```bash
make build-debug
```

Expected: Clean build

**Step 12: Commit V4.3 Improved**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf_v4_3_improved.py
git commit -m "feat: add V4.3 Improved strategy (hybrid blocking + dynamic exits)

Critical fixes over original V4.3:
- Add V4.1's volatility blocking (blocks HIGH/ELEVATED)
- Faster regime detection: 24h vs 7d (4x faster than original)
- Tightened stop/target parameters
- Simplified to 2 regimes (NORMAL/LOW)

Original V4.3 lost money in choppy 2022 (-0.04%).
V4.3 Improved blocks bad conditions entirely.

Target: +6-7% over 4 years"
```

---

## Task 4: Create V4.3 Improved Backtest Script

**Files:**
- Create: `examples/backtest/backtest_wavetrend_mtf_v4_3_improved.py`
- Reference: `examples/backtest/backtest_wavetrend_mtf_v4_3.py`

**Step 1: Copy and update**

```bash
cp examples/backtest/backtest_wavetrend_mtf_v4_3.py \
   examples/backtest/backtest_wavetrend_mtf_v4_3_improved.py
```

**Step 2: Update imports**

Change to import `WaveTrendMultiTimeframeV4_3Improved` and `WaveTrendMultiTimeframeV4_3ImprovedConfig`.

**Step 3: Update config instantiation**

Use new config class with updated parameters:
- `use_volatility_blocking=True`
- `atr_recent_bars=288`
- `atr_baseline_bars=2016`
- Remove `high_vol_*` and `elevated_vol_*` parameters
- Update `normal_vol_*` and `low_vol_*` to tightened values

**Step 4: Test backtest**

```bash
uv run --active --no-sync python examples/backtest/backtest_wavetrend_mtf_v4_3_improved.py 2024-01-01 2024-12-31
```

Expected: Completes successfully

**Step 5: Commit**

```bash
git add examples/backtest/backtest_wavetrend_mtf_v4_3_improved.py
git commit -m "feat: add V4.3 Improved backtest script

- Uses hybrid blocking + dynamic exits
- Faster regime detection (24h vs 7d)
- Should avoid choppy market losses"
```

---

## Task 5: Create V4.4a Config and Strategy (Moderate Frequency)

**Files:**
- Create: `nautilus_trader/examples/strategies/wavetrend_mtf_v4_4a.py`
- Reference: `nautilus_trader/examples/strategies/wavetrend_mtf_v4_1.py`

**Step 1: Copy V4.1 as base**

```bash
cp nautilus_trader/examples/strategies/wavetrend_mtf_v4_1.py \
   nautilus_trader/examples/strategies/wavetrend_mtf_v4_4a.py
```

**Step 2: Create V4.4a config**

Change class name to `WaveTrendMultiTimeframeV4_4aConfig`.

Update docstring:
```python
"""
Configuration for WaveTrend Multi-Timeframe strategy V4.4a (Moderate Frequency).

Goal: 2-3x more trades than V4.1 (~40-50 positions/year, ~1 per week)

V4.4a changes from V4.1:
1. Relax alignment: 2/3 timeframes instead of 3/3
   - Catches earlier entries when higher timeframes lag
2. Allow ELEVATED volatility with tighter stops
   - Only block HIGH volatility (>1.5x)
   - ELEVATED (1.1-1.5x): 3.5x ATR stop, 3.0% target
3. Keep all other V4.1 filters (ATR min, range, trend)

Expected Result:
- 40-50 positions/year (2.5-3x more than V4.1)
- Slightly lower win rate (trading marginal setups)
- Target: +5-6% over 4 years (similar to V4.1, more activity)

Risk:
- More marginal trades = more commissions
- Could have worse risk/reward on partial alignments
"""
```

**Step 3: Update alignment parameter**

Change:
```python
# V4.4a: Relaxed alignment (2/3 instead of 3/3)
min_aligned_timeframes: PositiveInt = 2
```

**Step 4: Update volatility thresholds**

Change:
```python
# V4.4a: Only block HIGH volatility, allow ELEVATED with tighter stops
use_volatility_filter: bool = True
atr_recent_bars: PositiveInt = 576
atr_baseline_bars: PositiveInt = 8640
high_vol_threshold: PositiveFloat = 1.5  # Only block HIGH (>1.5x)
elevated_vol_threshold: PositiveFloat = 1.1  # Allow but with tighter params
low_vol_threshold: PositiveFloat = 0.9
```

**Step 5: Add ELEVATED volatility exit parameters**

Add new config parameters:
```python
# V4.4a: ELEVATED volatility exit parameters
elevated_vol_atr_mult: PositiveFloat = 3.5  # Tighter stop in ELEVATED
elevated_vol_profit_pct: PositiveFloat = 3.0  # Lower target in ELEVATED
```

**Step 6: Update order tag**

```python
order_id_tag: str = "WT_MTF_V4_4A"
```

**Step 7: Update strategy class**

Change class name to `WaveTrendMultiTimeframeV4_4a`.

**Step 8: Modify blocking logic**

In volatility check, change from blocking ELEVATED to allowing it:

```python
# V4.4a: Only block HIGH volatility
if volatility_regime == "HIGH":
    self._log.info("BLOCKED: HIGH volatility")
    return
# ELEVATED is allowed (will use tighter stops)
```

**Step 9: Add dynamic stops for ELEVATED regime**

In stop/target calculation, add ELEVATED case:

```python
def _get_stop_and_target(self, entry_price: float, side: OrderSide) -> tuple[float, float]:
    """Calculate stop and target based on volatility regime."""
    regime = self._detect_volatility_regime()

    if regime == "ELEVATED":
        atr_mult = self.config.elevated_vol_atr_mult
        profit_pct = self.config.elevated_vol_profit_pct
    elif regime == "LOW":
        atr_mult = self.config.atr_multiplier  # Use base params
        profit_pct = self.config.profit_threshold_pct
    else:  # NORMAL
        atr_mult = self.config.atr_multiplier
        profit_pct = self.config.profit_threshold_pct

    # Calculate stop and target using selected parameters
    # ... rest of logic
```

**Step 10: Test compilation**

```bash
make build-debug
```

Expected: Clean build

**Step 11: Commit V4.4a**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf_v4_4a.py
git commit -m "feat: add V4.4a strategy (moderate frequency: ~40-50/year)

Changes from V4.1:
- 2/3 timeframe alignment (was 3/3)
- Allow ELEVATED volatility with tighter stops
- Only block HIGH volatility

Target: 2-3x more trades with similar returns"
```

---

## Task 6: Create V4.4a Backtest Script

**Files:**
- Create: `examples/backtest/backtest_wavetrend_mtf_v4_4a.py`

**Step 1: Copy V4.1 backtest**

```bash
cp examples/backtest/backtest_wavetrend_mtf_v4_1.py \
   examples/backtest/backtest_wavetrend_mtf_v4_4a.py
```

**Step 2: Update imports and config**

Change to use `WaveTrendMultiTimeframeV4_4a` and `WaveTrendMultiTimeframeV4_4aConfig`.

Update config parameters:
- `min_aligned_timeframes=2`
- Add `elevated_vol_atr_mult=3.5`
- Add `elevated_vol_profit_pct=3.0`

**Step 3: Test backtest**

```bash
uv run --active --no-sync python examples/backtest/backtest_wavetrend_mtf_v4_4a.py 2024-01-01 2024-12-31
```

Expected: More positions than V4.1

**Step 4: Commit**

```bash
git add examples/backtest/backtest_wavetrend_mtf_v4_4a.py
git commit -m "feat: add V4.4a backtest (moderate frequency)"
```

---

## Task 7: Create V4.4b Strategy (High Frequency with Mean Reversion)

**Files:**
- Create: `nautilus_trader/examples/strategies/wavetrend_mtf_v4_4b.py`

**Step 1: Create new file with base structure**

Create new file with imports and config class.

**Step 2: Define V4.4b config**

```python
class WaveTrendMultiTimeframeV4_4bConfig(StrategyConfig, frozen=True, kw_only=True):
    """
    Configuration for WaveTrend Multi-Timeframe strategy V4.4b (High Frequency).

    Goal: ~150-200 positions/year (~3-4 per week)

    V4.4b combines three sub-strategies:
    1. Trend-following (from V4.1, relaxed to 2/3 alignment): ~50-60/year
    2. Mean reversion (new): ~60-80/year
    3. Pyramiding (new): ~40-60/year

    Mean Reversion Logic:
    - Entry: WT1 oversold (<-60) or overbought (>60) on 5m + 1h
    - Exit: Quick scalp when WT1 returns to -40/+40
    - Stop: Tight 2.0x ATR (mean reversion fails fast)
    - Only in LOW/NORMAL volatility

    Pyramiding Logic:
    - Add 50% of original position when up 2%
    - Maximum 2 pyramids (total 2x position)
    - Trail stop on entire position

    Position Sizing:
    - Base trend: 0.01 BTC
    - Mean reversion: 0.005 BTC (smaller, riskier)
    - Pyramids: 0.005 BTC each

    Expected Result:
    - 150-200 total positions/year
    - More consistent activity
    - Target: +8-12% over 4 years (if mean reversion works)

    Risk:
    - Mean reversion can fail in strong trends
    - Pyramiding increases exposure
    """

    instrument_id: InstrumentId
    base_trade_size: Decimal  # For trend-following
    mean_reversion_size: Decimal  # Smaller for MR trades
    pyramid_size: Decimal  # For adding to positions

    # Trend-following parameters (same as V4.4a)
    # ... (copy from V4.4a)

    # Mean reversion parameters
    enable_mean_reversion: bool = True
    mr_oversold_threshold: PositiveFloat = 60.0  # WT1 < -60 = oversold
    mr_overbought_threshold: PositiveFloat = 60.0  # WT1 > 60 = overbought
    mr_exit_threshold: PositiveFloat = 40.0  # Exit at -40/+40
    mr_atr_multiplier: PositiveFloat = 2.0  # Tight stop for MR

    # Pyramiding parameters
    enable_pyramiding: bool = True
    pyramid_profit_threshold: PositiveFloat = 2.0  # Add when up 2%
    max_pyramids: PositiveInt = 2  # Max 2 additions

    order_id_tag: str = "WT_MTF_V4_4B"
```

**Step 3: Create strategy class skeleton**

```python
class WaveTrendMultiTimeframeV4_4b(Strategy):
    """High-frequency WaveTrend strategy with mean reversion and pyramiding."""

    def __init__(self, config: WaveTrendMultiTimeframeV4_4bConfig):
        super().__init__(config)

        # Track pyramiding state
        self._pyramid_count: dict[str, int] = {}  # position_id -> pyramid count
        self._pyramid_levels: dict[str, list[float]] = {}  # position_id -> entry prices
```

**Step 4: Implement mean reversion signal detection**

Add method:
```python
def _check_mean_reversion_signal(self) -> OrderSide | None:
    """Check for mean reversion setup."""
    if not self.config.enable_mean_reversion:
        return None

    wt_5m = self._wt_5m
    wt_1h = self._wt_1h

    # Both timeframes must be extreme
    if (wt_5m.wt1 < -self.config.mr_oversold_threshold and
        wt_1h.wt1 < -self.config.mr_oversold_threshold):
        # Oversold on both = buy setup
        return OrderSide.BUY

    if (wt_5m.wt1 > self.config.mr_overbought_threshold and
        wt_1h.wt1 > self.config.mr_overbought_threshold):
        # Overbought on both = sell setup
        return OrderSide.SELL

    return None
```

**Step 5: Implement mean reversion exit check**

Add method:
```python
def _check_mean_reversion_exit(self, position) -> bool:
    """Check if mean reversion position should exit."""
    # Check if WT1 has returned from extreme
    wt_5m = self._wt_5m

    if position.side == PositionSide.LONG:
        # Exit long when WT1 rises back above -40
        return wt_5m.wt1 > -self.config.mr_exit_threshold
    else:
        # Exit short when WT1 falls back below +40
        return wt_5m.wt1 < self.config.mr_exit_threshold
```

**Step 6: Implement pyramiding logic**

Add method:
```python
def _check_pyramiding_opportunity(self, position) -> bool:
    """Check if we should add to winning position."""
    if not self.config.enable_pyramiding:
        return False

    # Check pyramid count
    pos_id = str(position.id)
    current_pyramids = self._pyramid_count.get(pos_id, 0)
    if current_pyramids >= self.config.max_pyramids:
        return False

    # Check if position is in profit by threshold
    unrealized_pnl_pct = (position.unrealized_pnl(position.last_px) /
                          position.quantity * position.avg_px_open) * 100

    return unrealized_pnl_pct >= self.config.pyramid_profit_threshold
```

**Step 7: Integrate all three strategies in on_bar**

Update `on_bar` to check:
1. Trend-following signals (existing logic, 2/3 alignment)
2. Mean reversion signals (new)
3. Pyramiding opportunities (new)

**Step 8: Test compilation**

```bash
make build-debug
```

Expected: Clean build

**Step 9: Commit V4.4b**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf_v4_4b.py
git commit -m "feat: add V4.4b strategy (high frequency ~150-200/year)

Three combined strategies:
1. Trend-following (relaxed alignment)
2. Mean reversion (oversold/overbought scalping)
3. Pyramiding (add to winners)

Target: +8-12% with more consistent activity"
```

---

## Task 8: Create V4.4b Backtest Script

**Files:**
- Create: `examples/backtest/backtest_wavetrend_mtf_v4_4b.py`

**Step 1: Create backtest script**

Copy V4.4a backtest as template, update to use V4.4b config.

**Step 2: Set position sizes**

```python
BASE_TRADE_SIZE = Decimal("0.01")
MEAN_REVERSION_SIZE = Decimal("0.005")
PYRAMID_SIZE = Decimal("0.005")
```

**Step 3: Configure strategy with all features enabled**

```python
strat_config = WaveTrendMultiTimeframeV4_4bConfig(
    instrument_id=instrument_id,
    base_trade_size=BASE_TRADE_SIZE,
    mean_reversion_size=MEAN_REVERSION_SIZE,
    pyramid_size=PYRAMID_SIZE,
    enable_mean_reversion=True,
    enable_pyramiding=True,
    # ... rest of params
)
```

**Step 4: Test backtest**

```bash
uv run --active --no-sync python examples/backtest/backtest_wavetrend_mtf_v4_4b.py 2024-01-01 2024-12-31
```

Expected: Significantly more positions than V4.1

**Step 5: Commit**

```bash
git add examples/backtest/backtest_wavetrend_mtf_v4_4b.py
git commit -m "feat: add V4.4b backtest (high frequency, 3 strategies)"
```

---

## Task 9: Create V4.4c Strategy (Daily Trading)

**Files:**
- Create: `nautilus_trader/examples/strategies/wavetrend_mtf_v4_4c.py`

**Step 1: Create config for fast timeframes**

```python
class WaveTrendMultiTimeframeV4_4cConfig(StrategyConfig, frozen=True, kw_only=True):
    """
    Configuration for WaveTrend Multi-Timeframe strategy V4.4c (Daily Trading).

    Goal: ~250+ positions/year (multiple per day)

    V4.4c uses fast timeframes for intraday trading:
    - Primary: 1m bars (WT: channel=8, average=13)
    - Confirm: 5m bars (WT: channel=10, average=21)
    - Drop: 1h and 4h (too slow)

    Three entry strategies:
    1. Trend Continuation (40% of trades): 1m+5m aligned, enter pullbacks
    2. Breakout (30% of trades): Break 20-bar range with WT confirmation
    3. Mean Reversion (30% of trades): 1m extreme, 5m not extreme

    Risk Management:
    - All positions: 1-2 hour max hold
    - Force close end of day (no overnight)
    - Small size: 0.005 BTC
    - Tight targets: 0.8-1.5% profit, 0.4-0.5% stop

    Session Filtering:
    - Only trade liquid hours: 8 AM - 8 PM UTC
    - Avoid: 3 AM - 6 AM UTC (low volume)

    Expected Result:
    - 250-300 positions/year (~1 per day)
    - Win rate: 45-55% (tight stops)
    - Target: +15-25% over 4 years

    Risk:
    - Slippage eats profits on fast timeframes
    - Overtrading in ranging markets
    - High transaction costs
    - Requires monitoring
    """

    instrument_id: InstrumentId
    trade_size: Decimal

    # Fast timeframe WaveTrend parameters
    wt_1m_channel_length: PositiveInt = 8
    wt_1m_average_length: PositiveInt = 13
    wt_5m_channel_length: PositiveInt = 10
    wt_5m_average_length: PositiveInt = 21

    # Strategy enables
    enable_trend_continuation: bool = True
    enable_breakout: bool = True
    enable_mean_reversion: bool = True

    # Trend continuation params
    tc_profit_pct: PositiveFloat = 1.0
    tc_stop_pct: PositiveFloat = 0.5

    # Breakout params
    bo_range_bars: PositiveInt = 20  # Look back for high/low
    bo_profit_pct: PositiveFloat = 1.5
    bo_stop_pct: PositiveFloat = 0.5

    # Mean reversion params
    mr_extreme_threshold: PositiveFloat = 70.0  # WT1 < -70 or > 70
    mr_profit_pct: PositiveFloat = 0.8
    mr_stop_pct: PositiveFloat = 0.4

    # Time-based filters
    max_position_hours: PositiveFloat = 2.0  # Force close after 2h
    force_close_time: str = "19:00:00"  # UTC time to close all
    session_start: str = "08:00:00"  # UTC
    session_end: str = "20:00:00"  # UTC

    order_id_tag: str = "WT_MTF_V4_4C"
```

**Step 2: Create strategy class with 1m/5m WaveTrend**

```python
class WaveTrendMultiTimeframeV4_4c(Strategy):
    """Daily trading WaveTrend strategy using 1m and 5m timeframes."""

    def __init__(self, config: WaveTrendMultiTimeframeV4_4cConfig):
        super().__init__(config)

        # Fast timeframe WaveTrend states
        self._wt_1m: WaveTrendState | None = None
        self._wt_5m: WaveTrendState | None = None

        # Range tracking for breakouts
        self._bar_highs: list[float] = []
        self._bar_lows: list[float] = []

        # Position entry time tracking
        self._position_entry_times: dict[str, int] = {}
```

**Step 3: Subscribe to 1m and 5m bars**

```python
def on_start(self):
    # Create 1m and 5m bar types
    bar_type_1m = BarType.from_str(
        f"{self.config.instrument_id}-1-MINUTE-LAST-EXTERNAL"
    )
    bar_type_5m = BarType.from_str(
        f"{self.config.instrument_id}-5-MINUTE-LAST-EXTERNAL"
    )

    # Subscribe
    self.subscribe_bars(bar_type_1m)
    self.subscribe_bars(bar_type_5m)

    # Initialize WaveTrend
    self._wt_1m = WaveTrendState(
        self.config.wt_1m_channel_length,
        self.config.wt_1m_average_length
    )
    self._wt_5m = WaveTrendState(
        self.config.wt_5m_channel_length,
        self.config.wt_5m_average_length
    )
```

**Step 4: Implement session filter**

```python
def _is_trading_session(self) -> bool:
    """Check if current time is within trading hours."""
    current_time = self.clock.utc_now().time()

    # Parse session times
    from datetime import time
    session_start = time.fromisoformat(self.config.session_start)
    session_end = time.fromisoformat(self.config.session_end)

    return session_start <= current_time <= session_end
```

**Step 5: Implement three entry strategies**

Add methods:
- `_check_trend_continuation()`
- `_check_breakout()`
- `_check_mean_reversion()`

**Step 6: Implement time-based exit checks**

```python
def _check_time_based_exit(self, position) -> bool:
    """Check if position should be closed due to time limits."""
    pos_id = str(position.id)
    entry_time_ns = self._position_entry_times.get(pos_id)

    if entry_time_ns is None:
        return False

    # Check max holding period
    current_time_ns = self.clock.timestamp_ns()
    hours_held = (current_time_ns - entry_time_ns) / 1e9 / 3600

    if hours_held >= self.config.max_position_hours:
        self._log.info(f"Closing {pos_id}: max hold time reached")
        return True

    # Check force close time
    current_time = self.clock.utc_now().time()
    force_close = time.fromisoformat(self.config.force_close_time)

    if current_time >= force_close:
        self._log.info(f"Closing {pos_id}: end of day")
        return True

    return False
```

**Step 7: Test compilation**

```bash
make build-debug
```

Expected: Clean build

**Step 8: Commit V4.4c**

```bash
git add nautilus_trader/examples/strategies/wavetrend_mtf_v4_4c.py
git commit -m "feat: add V4.4c strategy (daily trading ~250+/year)

Fast timeframes (1m + 5m):
- Trend continuation (pullback entries)
- Breakout (range breaks)
- Mean reversion (extreme scalps)

Time-based risk management:
- Max 2h hold
- Force close end of day
- Session filtering

Target: +15-25% with daily activity"
```

---

## Task 10: Create V4.4c Backtest Script

**Files:**
- Create: `examples/backtest/backtest_wavetrend_mtf_v4_4c.py`

**Step 1: Create backtest with 1m and 5m data loading**

Create new script similar to others but load 1m and 5m bars instead of 5m/1h/4h.

**Step 2: Update data loading**

```python
# Load 1m bars
bars_1m = catalog.bars(
    bar_types=[f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"],
    instrument_ids=[str(instrument_id)],
    start=START,
    end=END,
)
if bars_1m:
    engine.add_data(bars_1m)

# Load 5m bars
bars_5m = catalog.bars(
    bar_types=[f"{instrument_id}-5-MINUTE-LAST-EXTERNAL"],
    instrument_ids=[str(instrument_id)],
    start=START,
    end=END,
)
if bars_5m:
    engine.add_data(bars_5m)
```

**Step 3: Configure strategy**

```python
strat_config = WaveTrendMultiTimeframeV4_4cConfig(
    instrument_id=instrument_id,
    trade_size=Decimal("0.005"),  # Smaller for intraday
    enable_trend_continuation=True,
    enable_breakout=True,
    enable_mean_reversion=True,
    # ... rest of params
)
```

**Step 4: Test backtest**

```bash
uv run --active --no-sync python examples/backtest/backtest_wavetrend_mtf_v4_4c.py 2024-01-01 2024-12-31
```

Expected: Many positions (250+)

**Step 5: Commit**

```bash
git add examples/backtest/backtest_wavetrend_mtf_v4_4c.py
git commit -m "feat: add V4.4c backtest (daily trading, 1m/5m bars)"
```

---

## Task 11: Run All Backtests and Compare Results

**Files:**
- Create: `/tmp/run_all_v4_improvements.sh`

**Step 1: Create comparison script**

```bash
#!/bin/bash
# Run all improved strategy backtests for 2024

echo "Running all V4 improvement backtests for 2024..."
echo ""

for strategy in v4_2_enhanced v4_3_improved v4_4a v4_4b v4_4c; do
    echo "================================================================"
    echo "RUNNING ${strategy}"
    echo "================================================================"

    uv run --active --no-sync python examples/backtest/backtest_wavetrend_mtf_${strategy}.py 2024-01-01 2024-12-31 \
        | grep -E "(Total|Ending|positions)" | tail -10

    echo ""
    sleep 2
done

echo "================================================================"
echo "ALL BACKTESTS COMPLETE"
echo "================================================================"
```

**Step 2: Make executable and run**

```bash
chmod +x /tmp/run_all_v4_improvements.sh
/tmp/run_all_v4_improvements.sh
```

Expected: See results for all 5 strategies

**Step 3: Create comparison summary**

Create `/tmp/v4_improvements_summary.txt` with results table:

```
Strategy           Total Return    Positions    Avg/Year    Notes
------------------------------------------------------------------
V4.1 (baseline)    +5.74%         61           15.2        Original winner
V4.2 Enhanced      +X.XX%         XX           XX.X        2.0x sizing in NORMAL
V4.3 Improved      +X.XX%         XX           XX.X        Blocking + dynamic exits
V4.4a              +X.XX%         XX           XX.X        Moderate frequency
V4.4b              +X.XX%         XX           XX.X        High frequency
V4.4c              +X.XX%         XX           XX.X        Daily trading
```

**Step 4: Analyze and document findings**

Add to summary:
- Which improvements worked best?
- Did higher frequency improve returns or hurt them?
- Transaction cost impact (more trades = more fees)
- Recommendations for production deployment

**Step 5: Commit summary**

```bash
git add /tmp/run_all_v4_improvements.sh /tmp/v4_improvements_summary.txt
git commit -m "docs: add backtest comparison for all V4 improvements

Ran all 5 improved strategies for 2024.
See summary for performance comparison."
```

---

## Summary

This plan implements 5 improved WaveTrend strategy variants:

1. **V4.2 Enhanced**: Better position sizing (2.0x in NORMAL volatility)
2. **V4.3 Improved**: Hybrid blocking + dynamic exits with faster detection
3. **V4.4a**: Moderate frequency (~40-50/year) via relaxed filters
4. **V4.4b**: High frequency (~150-200/year) via mean reversion + pyramiding
5. **V4.4c**: Daily trading (~250+/year) via fast timeframes

Each variant includes:
- Strategy config class
- Strategy implementation
- Backtest script
- Testing and validation

The plan follows NautilusTrader patterns and ensures all strategies compile and run successfully.

**Total Tasks**: 11 major tasks, ~60 individual steps
**Estimated Time**: 4-6 hours for complete implementation
**Testing**: Each strategy validated via backtest before proceeding

---

**Ready to execute? Choose your approach:**

1. **Subagent-Driven (this session)** - I'll dispatch agents per task, review between tasks
2. **Parallel Session** - Open new session to execute with checkpoints
