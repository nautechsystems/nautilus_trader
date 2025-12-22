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
WaveTrend Multi-Timeframe Strategy V3 Backtest

V3 Improvements over V2 (Regime Detection):
1. ADX filter: Only trades when ADX > 25 (trending market)
2. ATR minimum filter: Ensures sufficient volatility (0.5% of price)

V2 Improvements over V1 (still in V3):
1. Stricter alignment: Requires 3/3 timeframes (was 2/3)
2. Wider stops: ATR multiplier 4.5 (was 3.0) - gives trades more room
3. Higher profit target: 4.0% (was 2.0%) - waits for better profits before trailing
4. Tighter trailing: 1.0% (was 1.5%) - locks in profits faster
5. Larger position: 0.01 BTC (was 0.001) - reduces commission impact
6. Trend filter: Only trades with 4h WaveTrend trend

Expected V3 improvements over V2:
- Avoids choppy/ranging markets (2025 failure mode)
- Reduces overtrading in low-trend conditions
- Better risk-adjusted returns across all market regimes
"""

import sys
from decimal import Decimal
from pathlib import Path

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.wavetrend_mtf_v3 import WaveTrendMultiTimeframeV3
from nautilus_trader.examples.strategies.wavetrend_mtf_v3 import WaveTrendMultiTimeframeV3Config
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

# Strategy parameters (V2: 10x larger position for better commission ratio)
TRADE_SIZE = Decimal("0.01")


def run_backtest(start_date=None, end_date=None):
    """
    Run WaveTrend MTF V3 (regime-filtered) strategy backtest.

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

    # Configure strategy (V3 with regime detection filters)
    strat_config = WaveTrendMultiTimeframeV3Config(
        instrument_id=instrument_id,
        trade_size=TRADE_SIZE,
        wt_5m_channel_length=10,
        wt_5m_average_length=21,
        wt_1h_channel_length=9,
        wt_1h_average_length=18,
        wt_4h_channel_length=8,
        wt_4h_average_length=15,
        min_aligned_timeframes=3,  # V2: Require all 3 timeframes (was 2)
        atr_period=14,
        atr_multiplier=4.5,  # V2: Wider stops (was 3.0)
        profit_threshold_pct=4.0,  # V2: Higher profit target (was 2.0)
        percentage_trail=1.0,  # V2: Tighter trailing (was 1.5)
        use_trend_filter=True,  # V2: Enable trend filter
        trend_filter_threshold=20.0,  # V2: Strong trend threshold
        use_atr_min_filter=True,  # V3: Enable ATR minimum filter
        atr_min_multiplier=0.5,  # V3: Minimum ATR as 0.5% of price
        use_range_filter=True,  # V3: Enable range filter
        range_lookback=100,  # V3: Look back 100 bars to check for new highs/lows
    )

    # Add strategy
    strategy = WaveTrendMultiTimeframeV3(config=strat_config)
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
    # Usage: python backtest_wavetrend_mtf_v3.py [start_date] [end_date]
    # Example: python backtest_wavetrend_mtf_v3.py 2023-01-01 2023-12-31
    start = sys.argv[1] if len(sys.argv) > 1 else None
    end = sys.argv[2] if len(sys.argv) > 2 else None

    run_backtest(start, end)
