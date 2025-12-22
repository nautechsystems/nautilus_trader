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
WaveTrend Multi-Timeframe Strategy V4.4c Backtest

V4.4c: Daily Trading Strategy (High Frequency):
- Uses 1m bars (primary) and 5m bars (confirmation)
- Three entry strategies: trend continuation, breakout, mean reversion
- Time-based position management: max 2-hour hold, force close EOD
- Expected: ~250+ positions/year (daily trading)

Key V4.4c Features:
1. Fast timeframes: 1m (primary signal) + 5m (confirmation filter)
2. Multiple entry strategies:
   - Trend continuation: Aligned WT signals + momentum
   - Breakout: WT crosses above 53 with confirmation
   - Mean reversion: Oversold bounce from WT < -53
3. Time-based exits:
   - Max hold period: 2 hours (reduces overnight risk)
   - Force close: All positions at EOD (23:50 UTC)
   - No weekend holds
4. Adaptive stops:
   - Initial: 1.5 ATR (tight for intraday)
   - Trailing: 0.5% after 1% profit
5. Volume and volatility filters (blocks low-quality setups)

Expected V4.4c behavior:
- High frequency (1+ trades/day average)
- Fast entries and exits (typical hold: 30m-2h)
- Lower per-trade profit but many opportunities
- Good for active traders wanting daily signals
"""

import sys
from decimal import Decimal
from pathlib import Path

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.wavetrend_mtf_v4_4c import WaveTrendMultiTimeframeV4_4c
from nautilus_trader.examples.strategies.wavetrend_mtf_v4_4c import WaveTrendMultiTimeframeV4_4cConfig
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
TRADE_SIZE = Decimal("0.005")  # Smaller size for intraday trading


def run_backtest(start_date=None, end_date=None):
    """
    Run WaveTrend MTF V4.4c (daily trading) strategy backtest.

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

    # Load bar data for two timeframes (1m + 5m for daily trading)
    print(f"Loading bars for {instrument_id}...")

    # Load 1m bars (primary timeframe)
    bars_1m = catalog.bars(
        bar_types=[f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"],
        instrument_ids=[str(instrument_id)],
        start=START,
        end=END,
    )
    if bars_1m:
        engine.add_data(bars_1m)
        print(f"✓ Loaded {len(bars_1m)} 1m bars (first: {bars_1m[0].ts_init}, last: {bars_1m[-1].ts_init})")
    else:
        print("⚠ No 1m bars loaded!")

    # Load 5m bars (confirmation timeframe)
    bars_5m = catalog.bars(
        bar_types=[f"{instrument_id}-5-MINUTE-LAST-EXTERNAL"],
        instrument_ids=[str(instrument_id)],
        start=START,
        end=END,
    )
    if bars_5m:
        engine.add_data(bars_5m)
        print(f"✓ Loaded {len(bars_5m)} 5m bars (first: {bars_5m[0].ts_init}, last: {bars_5m[-1].ts_init})")
    else:
        print("⚠ No 5m bars loaded!")

    # Configure strategy (V4.4c with daily trading)
    strat_config = WaveTrendMultiTimeframeV4_4cConfig(
        instrument_id=instrument_id,
        trade_size=TRADE_SIZE,
        # WaveTrend parameters for 1m and 5m
        wt_1m_channel_length=8,
        wt_1m_average_length=13,
        wt_5m_channel_length=10,
        wt_5m_average_length=21,
        # Entry strategies
        enable_trend_continuation=True,
        enable_breakout=True,
        enable_mean_reversion=True,
        # Trend continuation params
        tc_profit_pct=1.0,  # 1% profit target
        tc_stop_pct=0.5,  # 0.5% stop loss
        # Breakout params
        bo_range_bars=20,  # 20-bar range lookback
        bo_profit_pct=1.5,  # 1.5% profit target
        bo_stop_pct=0.5,  # 0.5% stop loss
        # Mean reversion params
        mr_extreme_threshold=70.0,  # WT1 > 70 or < -70
        mr_profit_pct=0.8,  # 0.8% profit target
        mr_stop_pct=0.4,  # 0.4% stop loss
        # Time-based management
        max_position_hours=2.0,  # 2 hours max
        force_close_time="19:00:00",  # Close all before 7 PM UTC
        session_start="08:00:00",  # No entries before 8 AM UTC
        session_end="20:00:00",  # No entries after 8 PM UTC
    )

    # Add strategy
    strategy = WaveTrendMultiTimeframeV4_4c(config=strat_config)
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
    # Usage: python backtest_wavetrend_mtf_v4_4c.py [start_date] [end_date]
    # Example: python backtest_wavetrend_mtf_v4_4c.py 2023-01-01 2023-12-31
    start = sys.argv[1] if len(sys.argv) > 1 else None
    end = sys.argv[2] if len(sys.argv) > 2 else None

    run_backtest(start, end)
