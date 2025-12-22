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
WaveTrend Multi-Timeframe Strategy V4.4b Backtest

V4.4b: High Frequency with Mean Reversion & Pyramiding

Combines THREE trading strategies:
1. Trend-following (2/3 timeframe alignment)
2. Mean reversion (oversold/overbought signals)
3. Pyramiding (adding to winning positions)

Expected: ~150-200 positions/year (3-4x more than V4.4a)

Key Features:
- Three position sizes:
  - base_trade_size=0.01 BTC (trend-following)
  - mean_reversion_size=0.005 BTC (MR trades, smaller/riskier)
  - pyramid_size=0.005 BTC (pyramid additions)
- Mean reversion thresholds: oversold=-60, overbought=+60
- Pyramiding: 1 addition per position, requires profit
- Same V4.4a params: 2/3 alignment, ELEVATED volatility trading

Expected V4.4b behavior:
- High frequency: 3-4x more trades than V4.4a
- More opportunities: trend + MR + pyramiding
- Potentially higher churn but more responsive to market conditions
- Good for users wanting active trading approach
"""

import sys
from decimal import Decimal
from pathlib import Path

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.wavetrend_mtf_v4_4b import WaveTrendMultiTimeframeV4_4b
from nautilus_trader.examples.strategies.wavetrend_mtf_v4_4b import WaveTrendMultiTimeframeV4_4bConfig
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

# Strategy parameters - V4.4b uses THREE position sizes
BASE_TRADE_SIZE = Decimal("0.01")       # Trend-following entries
MEAN_REVERSION_SIZE = Decimal("0.005")  # Mean reversion entries (smaller, riskier)
PYRAMID_SIZE = Decimal("0.005")         # Pyramid additions


def run_backtest(start_date=None, end_date=None):
    """
    Run WaveTrend MTF V4.4b (high frequency with MR & pyramiding) strategy backtest.

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
    # CRITICAL: Use NETTING mode so stop orders CLOSE positions instead of opening opposite ones
    engine.add_venue(
        venue=VENUE,
        oms_type=OmsType.NETTING,  # FIXED: Was HEDGING (caused positions to never close!)
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

    # Configure strategy (V4.4b with high frequency: MR + pyramiding)
    strat_config = WaveTrendMultiTimeframeV4_4bConfig(
        instrument_id=instrument_id,
        # V4.4b: THREE position sizes
        base_trade_size=BASE_TRADE_SIZE,
        mean_reversion_size=MEAN_REVERSION_SIZE,
        pyramid_size=PYRAMID_SIZE,
        # WaveTrend parameters (same as V4.4a)
        wt_5m_channel_length=10,
        wt_5m_average_length=21,
        wt_1h_channel_length=9,
        wt_1h_average_length=18,
        wt_4h_channel_length=8,
        wt_4h_average_length=15,
        min_aligned_timeframes=2,  # Relaxed alignment (same as V4.4a)
        # Trailing stop parameters (same as V4.4a)
        atr_period=14,
        atr_multiplier=4.5,
        profit_threshold_pct=4.0,
        percentage_trail=1.0,
        # Regime filters (same as V4.4a)
        use_trend_filter=True,
        trend_filter_threshold=20.0,
        use_atr_min_filter=True,
        atr_min_multiplier=0.5,
        use_range_filter=True,
        range_lookback=100,
        # Volatility filter (same as V4.4a)
        use_volatility_filter=True,
        atr_recent_bars=576,  # 48 hours at 5m
        atr_baseline_bars=8640,  # 30 days at 5m
        high_vol_threshold=1.5,  # Recent/Baseline > 1.5 = HIGH volatility
        elevated_vol_threshold=1.1,  # Recent/Baseline > 1.1 = ELEVATED
        low_vol_threshold=0.9,  # Recent/Baseline < 0.9 = LOW volatility
        # ELEVATED volatility adaptations (same as V4.4a)
        elevated_vol_atr_mult=3.5,  # Tighter stop in ELEVATED (vs 4.5 standard)
        elevated_vol_profit_pct=3.0,  # Lower profit target in ELEVATED (vs 4.0 standard)
        # V4.4b: NEW - Mean reversion parameters
        enable_mean_reversion=True,
        mr_oversold_threshold=60.0,  # WT1 < -60 = oversold
        mr_overbought_threshold=60.0,  # WT1 > 60 = overbought
        mr_exit_threshold=40.0,  # Exit at -40/+40
        mr_atr_multiplier=2.0,  # Tight stop for MR (can fail in trends)
        # V4.4b: NEW - Pyramiding parameters
        enable_pyramiding=True,
        pyramid_profit_threshold=2.0,  # Add when up 2%
        max_pyramids=2,  # Max 2 additions per position
    )

    # Add strategy
    strategy = WaveTrendMultiTimeframeV4_4b(config=strat_config)
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
    # Usage: python backtest_wavetrend_mtf_v4_4b.py [start_date] [end_date]
    # Example: python backtest_wavetrend_mtf_v4_4b.py 2023-01-01 2023-12-31
    start = sys.argv[1] if len(sys.argv) > 1 else None
    end = sys.argv[2] if len(sys.argv) > 2 else None

    run_backtest(start, end)
