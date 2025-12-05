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
Backtest example using the OrderFlow Strategy with ETHUSDT trade ticks.

This example demonstrates:
- Loading trade tick data with aggressor_side for order flow analysis
- Using all order flow indicators (Volume Profile, VWAP, Initial Balance, etc.)
- Confluence-based trading decisions
"""

import time
from decimal import Decimal

import pandas as pd

from nautilus_trader.adapters.binance import BINANCE_VENUE
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import ETH
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
    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(
            log_level="INFO",
            log_colors=True,
            use_pyo3=False,
        ),
    )

    # Build the backtest engine
    engine = BacktestEngine(config=config)

    # Add a trading venue
    engine.add_venue(
        venue=BINANCE_VENUE,
        oms_type=OmsType.NETTING,
        book_type=BookType.L1_MBP,
        account_type=AccountType.CASH,
        base_currency=None,
        starting_balances=[Money(100_000.0, USDT), Money(10.0, ETH)],
        trade_execution=True,
    )

    # Add instruments
    ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
    engine.add_instrument(ETHUSDT_BINANCE)

    # Add data - trade ticks with aggressor_side
    print("Loading trade tick data...")
    provider = TestDataProvider()
    wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
    ticks = wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv"))
    engine.add_data(ticks)
    print(f"Loaded {len(ticks)} trade ticks")

    # Get tick size from instrument
    tick_size = float(ETHUSDT_BINANCE.price_increment)

    # Configure the Order Flow Strategy
    strategy_config = OrderFlowStrategyConfig(
        instrument_id=ETHUSDT_BINANCE.id,
        tick_size=tick_size,
        trade_size=Decimal("0.05"),
        max_position_size=Decimal("0.5"),
        delta_threshold=50.0,  # Cumulative delta threshold
        use_ib_levels=True,
        use_vwap_bands=True,
        use_volume_profile=True,
        use_stacked_imbalance=True,
        min_confluence_score=2,  # Need at least 2 confirming signals
    )

    # Instantiate and add the strategy
    strategy = OrderFlowStrategy(config=strategy_config)
    engine.add_strategy(strategy=strategy)

    print("\nOrder Flow Strategy Configuration:")
    print(f"  Instrument: {strategy_config.instrument_id}")
    print(f"  Tick Size: {strategy_config.tick_size}")
    print(f"  Trade Size: {strategy_config.trade_size}")
    print(f"  Max Position: {strategy_config.max_position_size}")
    print(f"  Delta Threshold: {strategy_config.delta_threshold}")
    print(f"  Min Confluence: {strategy_config.min_confluence_score}")
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
            output_path="orderflow_ethusdt_tearsheet.html",
            config=tearsheet_config,
        )
        print("Tearsheet saved to: orderflow_ethusdt_tearsheet.html")
    except ImportError:
        print("\nPlotly not installed. Install with: pip install plotly>=6.3.1")

    # Reset and dispose
    engine.reset()
    engine.dispose()

