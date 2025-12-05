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
Backtest example using the OrderFlow Strategy with ETHUSDT-PERP Futures.

This example demonstrates:
- FUTURES/PERPETUAL trading with MARGIN account
- 5x leverage for amplified returns
- Long AND Short positions  
- Lower confluence thresholds for more trades
- Detailed logging of indicator signals
"""

import time
from decimal import Decimal

import pandas as pd

from nautilus_trader.adapters.binance import BINANCE_VENUE
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider

from nautilus_trader.examples.strategies.orderflow_strategy import (
    OrderFlowStrategy,
    OrderFlowStrategyConfig,
)


if __name__ == "__main__":
    # Configure backtest engine with verbose logging
    config = BacktestEngineConfig(
        trader_id=TraderId("FUTURES-001"),
        logging=LoggingConfig(
            log_level="INFO",  # INFO to see trade signals
            log_colors=True,
            use_pyo3=False,
        ),
    )

    # Build the backtest engine
    engine = BacktestEngine(config=config)

    # Add a MARGIN venue for futures with 5x leverage
    # Starting with $250 USDT, 5x leverage = $1250 buying power
    engine.add_venue(
        venue=BINANCE_VENUE,
        oms_type=OmsType.NETTING,  # Single position per instrument
        book_type=BookType.L1_MBP,
        account_type=AccountType.MARGIN,  # MARGIN for futures
        base_currency=None,
        starting_balances=[Money(250.0, USDT)],  # $250 initial capital
        default_leverage=Decimal("5"),  # 5x leverage
        trade_execution=True,
    )

    # Add ETHUSDT-PERP perpetual contract
    ETHUSDT_PERP = TestInstrumentProvider.ethusdt_perp_binance()
    engine.add_instrument(ETHUSDT_PERP)

    # Load spot ETHUSDT trade data (same market dynamics)
    # We'll wrangle as spot data, then convert instrument_id to perp
    print("Loading trade tick data...")
    provider = TestDataProvider()

    # First load using spot instrument (which has 5 decimal precision for size)
    ETHUSDT_SPOT = TestInstrumentProvider.ethusdt_binance()
    spot_wrangler = TradeTickDataWrangler(instrument=ETHUSDT_SPOT)
    spot_ticks = spot_wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv"))

    # Create new ticks with the perp instrument_id but keeping original precision
    from nautilus_trader.model.data import TradeTick
    from nautilus_trader.model.objects import Price, Quantity

    perp_ticks = []
    for tick in spot_ticks:
        # Create a new tick with the perp instrument_id
        # We use the perp's precision for price, but keep size that fits
        price_val = float(tick.price)
        size_val = float(tick.size)

        # Skip zero-size ticks (too small for perp precision)
        if size_val < 0.001:
            continue

        perp_tick = TradeTick(
            instrument_id=ETHUSDT_PERP.id,
            price=ETHUSDT_PERP.make_price(price_val),
            size=ETHUSDT_PERP.make_qty(size_val),
            aggressor_side=tick.aggressor_side,
            trade_id=tick.trade_id,
            ts_event=tick.ts_event,
            ts_init=tick.ts_init,
        )
        perp_ticks.append(perp_tick)

    engine.add_data(perp_ticks)
    print(f"Loaded {len(perp_ticks)} trade ticks for {ETHUSDT_PERP.id} (filtered from {len(spot_ticks)} spot ticks)")

    # Get tick size from instrument
    tick_size = float(ETHUSDT_PERP.price_increment)

    # Configure the Order Flow Strategy
    # POI-based trading: only trade at Points of Interest
    # ONE position at a time, dynamic exits and reversals
    # With $250 at 5x = $1250 buying power, and ETH ~$430
    # Use ~3 ETH position = ~$1290 notional = full leverage utilization
    strategy_config = OrderFlowStrategyConfig(
        instrument_id=ETHUSDT_PERP.id,
        tick_size=tick_size,
        trade_size=Decimal("2.9"),         # ~$1250 notional (full 5x leverage on $250)
        poi_tolerance=5.0,                 # Within 5 ticks of POI to trigger
        warmup_ticks=1000,                 # Wait 1000 ticks for indicator warmup
        # Risk Management
        tp_pct=0.30,                       # 0.3% take profit
        sl_pct=0.30,                       # 0.3% stop loss
        trailing_activation_pct=0.25,      # 0.25% to activate trailing stop
        trailing_offset_pct=0.10,          # 0.1% trailing offset
        use_emulated_orders=True,          # Required for backtest
    )

    # Instantiate and add the strategy
    strategy = OrderFlowStrategy(config=strategy_config)
    engine.add_strategy(strategy=strategy)

    print("\n" + "=" * 60)
    print("POI-BASED ORDERFLOW STRATEGY + SL/TP/TRAILING")
    print("=" * 60)
    print(f"  Instrument:      {strategy_config.instrument_id}")
    print(f"  Account Type:    MARGIN (Futures)")
    print(f"  Leverage:        5x")
    print(f"  Starting USDT:   $250")
    print(f"  Buying Power:    $1,250 (5x leverage)")
    print(f"  Trade Size:      {strategy_config.trade_size} ETH (~$1,250)")
    print(f"  POI Tolerance:   {strategy_config.poi_tolerance} ticks")
    print(f"  Warmup:          {strategy_config.warmup_ticks} ticks")
    print(f"  LONG/SHORT:      DYNAMIC (based on POI + orderflow)")
    print("-" * 60)
    print("  RISK MANAGEMENT:")
    print(f"  Take Profit:     {strategy_config.tp_pct}%")
    print(f"  Stop Loss:       {strategy_config.sl_pct}%")
    print(f"  Trailing Start:  {strategy_config.trailing_activation_pct}%")
    print(f"  Trailing Offset: {strategy_config.trailing_offset_pct}%")
    print("=" * 60)
    print()

    time.sleep(0.1)
    input("Press Enter to start backtest...")

    # Run the engine
    engine.run()

    # View reports
    with pd.option_context(
        "display.max_rows",
        100,
        "display.max_columns",
        None,
        "display.width",
        300,
    ):
        print("\n" + "=" * 80)
        print("ACCOUNT REPORT")
        print("=" * 80)
        print(engine.trader.generate_account_report(BINANCE_VENUE))

        print("\n" + "=" * 80)
        print("ORDER FILLS REPORT")
        print("=" * 80)
        print(engine.trader.generate_order_fills_report())

        print("\n" + "=" * 80)
        print("POSITIONS REPORT")
        print("=" * 80)
        print(engine.trader.generate_positions_report())

    # Print final indicator state
    print("\n" + "=" * 80)
    print("FINAL INDICATOR STATE")
    print("=" * 80)
    indicator_state = strategy.get_indicator_state()
    for name, values in indicator_state.items():
        print(f"\n{name}:")
        for key, value in values.items():
            print(f"  {key}: {value}")

    # Generate tearsheet
    try:
        from nautilus_trader.analysis import TearsheetConfig
        from nautilus_trader.analysis.tearsheet import create_tearsheet

        print("\nGenerating tearsheet...")

        tearsheet_config = TearsheetConfig(theme="plotly_white")

        create_tearsheet(
            engine=engine,
            output_path="orderflow_futures_tearsheet.html",
            config=tearsheet_config,
        )
        print("Tearsheet saved to: orderflow_futures_tearsheet.html")
    except ImportError:
        print("\nPlotly not installed. Install with: pip install plotly>=6.3.1")

    # Reset and dispose
    engine.reset()
    engine.dispose()

