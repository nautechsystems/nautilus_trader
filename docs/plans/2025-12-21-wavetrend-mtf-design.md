# WaveTrend Multi-Timeframe Strategy Design

**Date**: 2025-12-21
**Strategy**: Multi-timeframe WaveTrend with trend alignment and combination trailing stop
**Instrument**: BTCUSDT-PERP (Binance Futures)
**Position Size**: 0.001 BTC

## Overview

A trend-following strategy using WaveTrend oscillator across three timeframes (5m, 1h, 4h) with majority-rules alignment. Entries occur on 5-minute WaveTrend crosses when at least 2 out of 3 timeframes show alignment. Uses a combination trailing stop that starts with ATR-based distance and switches to percentage-based when in profit.

## Strategy Components

### 1. WaveTrend Indicators

Three WaveTrend oscillators with optimized parameters per timeframe:

- **5m timeframe**: channel_length=10, average_length=21 (standard for entry signals)
- **1h timeframe**: channel_length=9, average_length=18 (slightly faster intermediate)
- **4h timeframe**: channel_length=8, average_length=15 (faster on slower timeframe)

**WaveTrend Calculation** (LazyBear formula):
```
1. HLC3 = (High + Low + Close) / 3
2. ESA = EMA(HLC3, channel_length)
3. D = EMA(abs(HLC3 - ESA), channel_length)
4. CI = (HLC3 - ESA) / (0.015 × D)
5. WT1 = EMA(CI, average_length)
6. WT2 = SMA(WT1, 4)
```

### 2. ATR Indicator

- Timeframe: 5 minutes
- Period: 14
- Used for initial trailing stop distance (3x ATR)

### 3. Alignment Logic

**Majority Rule**: At least 2 out of 3 timeframes must be aligned with entry direction.

**Bullish timeframe**: WT1 > WT2
**Bearish timeframe**: WT1 < WT2

## Entry Conditions

### LONG Entry

1. **5m WaveTrend Crossover**: WT1 crosses above WT2
2. **Alignment Count**:
   - 5m bullish: WT1_5m > WT2_5m ✓
   - 1h bullish: WT1_1h > WT2_1h (check)
   - 4h bullish: WT1_4h > WT2_4h (check)
3. **If aligned_count >= 2**: Submit MARKET BUY order for 0.001 BTC
4. **Position check**: Only enter if no existing position

### SHORT Entry

1. **5m WaveTrend Crossover**: WT1 crosses below WT2
2. **Alignment Count**:
   - 5m bearish: WT1_5m < WT2_5m ✓
   - 1h bearish: WT1_1h < WT2_1h (check)
   - 4h bearish: WT1_4h < WT2_4h (check)
3. **If aligned_count >= 2**: Submit MARKET SELL order for 0.001 BTC
4. **Position check**: Only enter if no existing position

## Exit Logic - Combination Trailing Stop

### Phase 1: ATR-Based Initial Stop

- **Stop distance**: 3 × ATR(14) from entry price
- **Updates**: Recalculated on each new bar as ATR changes
- **Monitoring**: Track unrealized P&L percentage

### Phase 2: Percentage-Based Trail (Active when P&L >= 2%)

- **Trigger**: Switch when unrealized profit reaches 2%
- **Peak tracking**:
  - LONG: Track highest price since entry
  - SHORT: Track lowest price since entry
- **Stop distance**: 1.5% from peak price
  - LONG: stop_price = peak_price × (1 - 0.015)
  - SHORT: stop_price = peak_price × (1 + 0.015)
- **Order type**: STOP_MARKET, updated as peak moves

### Stop Management

- Cancel existing stop when updating
- One position at a time (no pyramiding)
- Stop always active from entry

## Configuration

```python
class WaveTrendMultiTimeframeConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    trade_size: Decimal  # 0.001 BTC

    # WaveTrend parameters per timeframe
    wt_5m_channel_length: int = 10
    wt_5m_average_length: int = 21
    wt_1h_channel_length: int = 9
    wt_1h_average_length: int = 18
    wt_4h_channel_length: int = 8
    wt_4h_average_length: int = 15

    # Alignment rule
    min_aligned_timeframes: int = 2  # Majority rule

    # Trailing stop parameters
    atr_period: int = 14
    atr_multiplier: float = 3.0
    profit_threshold_pct: float = 2.0  # Switch to percentage at 2%
    percentage_trail: float = 1.5  # Trail by 1.5%

    # Order management
    order_id_tag: str = "WT_MTF"
    oms_type: str = "HEDGING"
```

## Data Subscription

On strategy start, subscribe to:
- 5m bars: `BTCUSDT-PERP.BINANCE-5-MINUTE-LAST-EXTERNAL`
- 1h bars: `BTCUSDT-PERP.BINANCE-1-HOUR-LAST-EXTERNAL`
- 4h bars: `BTCUSDT-PERP.BINANCE-4-HOUR-LAST-EXTERNAL`
- Quote ticks: For current price monitoring

## Risk Management

### Position Limits
- Max 1 position at a time
- No pyramiding
- No re-entry on same bar

### Stop-Loss Protection
- Stop always active from entry
- ATR-based initially (adapts to volatility)
- Percentage-based when in profit (locks gains)

### Order Validation
- Check sufficient balance before submission
- Validate position state before entry
- Handle order rejections gracefully

## Error Handling

1. **Missing data**: Skip signal if higher timeframe bars unavailable
2. **Indicator warmup**: Require minimum bars before trading (max average_length)
3. **Order rejection**: Log error and continue
4. **Network issues**: Rely on execution engine retry logic

## Logging & Monitoring

Log on each relevant event:
- Timeframe alignment status (5m bars)
- Entry signals with alignment count
- Trailing stop updates (mode switch, price updates)
- P&L at each stop update
- Order fills and rejections

## Implementation Files

```
nautilus_trader/examples/strategies/
└── wavetrend_mtf.py          # Strategy implementation

examples/live/binance/
└── binance_futures_wavetrend_mtf.py  # Live trading script

examples/backtest/
└── backtest_wavetrend_mtf.py  # Backtesting script
```

## Key Methods

1. `__init__()` - Initialize indicators, state variables
2. `on_start()` - Subscribe to multi-timeframe bars
3. `on_bar()` - Route bars to timeframe-specific handlers
4. `_calculate_wavetrend()` - Compute WT1/WT2 from HLC3
5. `_check_alignment()` - Count aligned timeframes
6. `_check_entry_signal()` - Detect crosses + majority alignment
7. `_update_trailing_stop()` - Manage ATR/percentage trailing
8. `on_order_filled()` - Set initial stop when entry fills
9. `on_position_changed()` - Update trailing stop as position moves

## Testing Strategy

### Backtesting
1. **Initial test**: 1 month of historical data
2. **Parameter sweep**: Test different WT parameters
3. **Metrics**: Win rate, profit factor, max drawdown, avg winner/loser
4. **Optimization**: Adjust parameters based on results

### Testnet Validation
1. Run on Binance Futures testnet for 48-72 hours
2. Monitor alignment accuracy
3. Verify trailing stops execute correctly
4. Check edge cases (gaps, volatility spikes)

### Production Deployment
1. Start with minimum position size
2. Monitor for 1 week
3. Gradually increase size if performing as expected

## Success Criteria

- Strategy executes without errors
- Trailing stops update correctly
- Multi-timeframe alignment logic works as designed
- Backtests show positive expectancy
- Testnet validation confirms live execution matches backtest

## Known Risks

1. **Whipsaw in ranging markets**: WaveTrend can generate false signals in low-volatility consolidation
2. **Lag in trend changes**: Waiting for majority alignment may cause late entries
3. **Slippage on market orders**: BTCUSDT-PERP typically has tight spreads, but volatile periods may cause slippage
4. **ATR volatility**: Sudden volatility spikes may widen stops significantly

## Future Enhancements (Post-MVP)

- Add overbought/oversold zone filters (±60 levels)
- Implement volume confirmation
- Add position sizing based on ATR
- Support multiple instruments
- Add performance analytics dashboard
