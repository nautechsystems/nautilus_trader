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

"""
WaveTrend Multi-Timeframe Strategy V4 Adaptive Backtest

V4 Adaptive Improvements over V3 (Volatility-Aware Drawdown Scaling):
- 2D position scaling: Drawdown × Volatility regime
- Volatility detection: Recent ATR (48h) vs Baseline ATR (30d)
- Adaptive filters: Trade through chop with reduced size (not blocked)
- Buy the dip: Aggressive scaling when drawdown + volatility declining

Volatility Regimes (Recent ATR / Baseline ATR):
- HIGH (>1.5x): Chop accelerating → Reduce/maintain size
- ELEVATED (1.1-1.5x): Chop continuing → Cautious scaling
- NORMAL (0.9-1.1x): Normal conditions → Standard scaling
- LOW (<0.9x): Chop ending → Aggressive scaling (BUY THE DIP)

Position Scaling Examples:
- < 5% DD + Normal Vol: 1.0x (normal)
- 10-20% DD + Low Vol: 2.0x (buy the dip)
- 20-30% DD + Low Vol: 3.0x (aggressive)
- 10-20% DD + High Vol: 0.75x (protect capital)

Expected V4 Adaptive results:
- 3-5x more trades than V3 (trades through chop with reduced size)
- Drawdown scaling WILL trigger (trading in chop creates DD)
- Tests "buy the dip when volatility declining" hypothesis
- WARNING: VERY AGGRESSIVE - can accelerate losses significantly
"""

import sys
from decimal import Decimal
from pathlib import Path

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.wavetrend_mtf_v4_adaptive import WaveTrendMultiTimeframeV4Adaptive
from nautilus_trader.examples.strategies.wavetrend_mtf_v4_adaptive import WaveTrendMultiTimeframeV4AdaptiveConfig
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
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

# Default backtest period (can be overridden via command line)
DEFAULT_START = "2024-01-01"
DEFAULT_END = "2024-12-31"

# Strategy parameters
TRADE_SIZE = Decimal("0.01")


def run_backtest(start_date=None, end_date=None):
    """
    Run WaveTrend MTF V4 Adaptive (volatility-aware) strategy backtest.

    Parameters
    ----------
    start_date : str, optional
        Start date in YYYY-MM-DD format (default: DEFAULT_START)
    end_date : str, optional
        End date in YYYY-MM-DD format (default: DEFAULT_END)
    """
    START = start_date or DEFAULT_START
    END = end_date or DEFAULT_END

    print(f"\n{'='*80}")
    print(f"BACKTEST PERIOD: {START} to {END}")
    print(f"{'='*80}\n")

    # Load data catalog
    catalog = ParquetDataCatalog(CATALOG_PATH)

    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(log_level="INFO"),  # INFO level to see strategy decisions only
    )
    engine = BacktestEngine(config=config)

    # Load instrument first to get quote currency
    instruments = catalog.instruments(instrument_ids=[str(instrument_id)])
    if not instruments:
        raise ValueError(f"No instrument found for {instrument_id}")

    instrument = instruments[0]

    # Add venue with correct starting balance currency
    engine.add_venue(
        venue=VENUE,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(10_000, instrument.quote_currency)],
    )

    engine.add_instrument(instrument)

    # Load bar data for all three timeframes
    print(f"Loading bars for {instrument_id}...")

    # Load 5m bars
    bars_5m = catalog.bars(
        bar_types=[f"{instrument_id}-5-MINUTE-LAST-EXTERNAL"],  # Use bar_types (plural)
        instrument_ids=[str(instrument_id)],
        start=START,
        end=END,
    )
    if bars_5m:
        engine.add_data(bars_5m)
        print(f"✓ Loaded {len(bars_5m)} 5m bars (first: {bars_5m[0].ts_init}, last: {bars_5m[-1].ts_init})")
    else:
        print("⚠ No 5m bars loaded!")

    # Load 1h bars
    bars_1h = catalog.bars(
        bar_types=[f"{instrument_id}-1-HOUR-LAST-EXTERNAL"],  # Use bar_types (plural)
        instrument_ids=[str(instrument_id)],
        start=START,
        end=END,
    )
    if bars_1h:
        engine.add_data(bars_1h)
        print(f"✓ Loaded {len(bars_1h)} 1h bars (first: {bars_1h[0].ts_init}, last: {bars_1h[-1].ts_init})")
    else:
        print("⚠ No 1h bars loaded!")

    # Load 4h bars
    bars_4h = catalog.bars(
        bar_types=[f"{instrument_id}-4-HOUR-LAST-EXTERNAL"],  # Use bar_types (plural)
        instrument_ids=[str(instrument_id)],
        start=START,
        end=END,
    )
    if bars_4h:
        engine.add_data(bars_4h)
        print(f"✓ Loaded {len(bars_4h)} 4h bars (first: {bars_4h[0].ts_init}, last: {bars_4h[-1].ts_init})")
    else:
        print("⚠ No 4h bars loaded!")

    # Configure strategy (V4 Adaptive with volatility-aware drawdown scaling)
    strat_config = WaveTrendMultiTimeframeV4AdaptiveConfig(
        instrument_id=instrument_id,
        base_trade_size=TRADE_SIZE,  # V4 Adaptive: Base size (scaled by drawdown × volatility)
        # WaveTrend parameters (same as V3)
        wt_5m_channel_length=10,
        wt_5m_average_length=21,
        wt_1h_channel_length=9,
        wt_1h_average_length=18,
        wt_4h_channel_length=8,
        wt_4h_average_length=15,
        min_aligned_timeframes=3,
        # Trailing stop parameters (same as V3)
        atr_period=14,
        atr_multiplier=4.5,
        profit_threshold_pct=4.0,
        percentage_trail=1.0,
        # Trend filter (same as V3)
        use_trend_filter=True,
        trend_filter_threshold=20.0,
        # V4 Adaptive: Regime filters (ADAPTIVE - reduce size, don't block)
        use_atr_min_filter=True,
        atr_min_multiplier=0.5,  # Minimum ATR as 0.5% of price
        atr_min_size_reduction=0.5,  # Reduce to 50% size if ATR low
        use_range_filter=True,
        range_lookback=100,
        range_size_reduction=0.75,  # Reduce to 75% size if stuck in range
        # V4 Adaptive: Volatility regime detection
        atr_recent_bars=576,  # 48 hours at 5m
        atr_baseline_bars=8640,  # 30 days at 5m
        high_vol_threshold=1.5,  # Recent/Baseline > 1.5 = HIGH volatility
        elevated_vol_threshold=1.1,  # Recent/Baseline > 1.1 = ELEVATED
        low_vol_threshold=0.9,  # Recent/Baseline < 0.9 = LOW volatility
        # V4 Adaptive: 2D Scaling Matrix (defaults shown, can be customized)
        # High Vol scaling (chop accelerating - cautious)
        high_vol_scale_5pct=0.5,
        high_vol_scale_10pct=0.5,
        high_vol_scale_20pct=0.75,
        high_vol_scale_30pct=0.75,
        high_vol_scale_40pct=1.0,
        # Elevated Vol scaling (chop continuing - cautious)
        elevated_vol_scale_5pct=0.75,
        elevated_vol_scale_10pct=0.75,
        elevated_vol_scale_20pct=1.0,
        elevated_vol_scale_30pct=1.25,
        elevated_vol_scale_40pct=2.0,
        # Normal Vol scaling (normal conditions - standard)
        normal_vol_scale_5pct=1.0,
        normal_vol_scale_10pct=1.25,
        normal_vol_scale_20pct=1.5,
        normal_vol_scale_30pct=2.0,
        normal_vol_scale_40pct=3.0,
        # Low Vol scaling (chop ending - AGGRESSIVE BUY THE DIP)
        low_vol_scale_5pct=1.0,
        low_vol_scale_10pct=1.5,
        low_vol_scale_20pct=2.0,
        low_vol_scale_30pct=3.0,
        low_vol_scale_40pct=5.0,
        # Safety cap
        max_position_pct_equity=0.5,
    )

    # Add strategy
    strategy = WaveTrendMultiTimeframeV4Adaptive(config=strat_config)
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
    # Parse command line arguments
    # Usage: python backtest_wavetrend_mtf_v4_adaptive.py [start_date] [end_date]
    # Example: python backtest_wavetrend_mtf_v4_adaptive.py 2023-01-01 2023-12-31
    start = sys.argv[1] if len(sys.argv) > 1 else None
    end = sys.argv[2] if len(sys.argv) > 2 else None

    run_backtest(start, end)
