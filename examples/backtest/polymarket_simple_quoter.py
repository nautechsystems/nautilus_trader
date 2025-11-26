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
Example script demonstrating how to fetch and use historical Polymarket data for
backtesting.

This example uses an active Polymarket market for demonstration.
You can find active markets at: https://polymarket.com

Data sources:
- Markets API: https://gamma-api.polymarket.com/markets
- Order book history: https://api.domeapi.io/v1/polymarket/orderbooks
- Trades/Prices: https://clob.polymarket.com/prices-history

Note: The DomeAPI orderbook history only has data starting from October 14th, 2025.

"""

import asyncio
import time
from decimal import Decimal

import pandas as pd

from nautilus_trader.adapters.polymarket import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket import PolymarketDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.model.currencies import USDC_POS
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money


# Market slug to fetch data for
# To find active markets, run:
#   python nautilus_trader/adapters/polymarket/scripts/active_markets.py
# To find BTC/ETH UpDown markets specifically, run:
#   python nautilus_trader/adapters/polymarket/scripts/list_updown_markets.py
MARKET_SLUG = "fed-rate-hike-in-2025"


async def run_backtest(
    market_slug: str,
    lookback_hours: int = 24,
) -> None:
    """
    Run a backtest using historical Polymarket data.

    Parameters
    ----------
    market_slug : str
        The Polymarket market slug.
    lookback_hours : int
        How many hours of historical data to fetch.

    """
    # Create loader by market slug (automatically fetches and parses instrument)
    loader = await PolymarketDataLoader.from_market_slug(market_slug)
    instrument = loader.instrument

    print(f"\nMarket loaded: {instrument.description or market_slug}")
    print(f"Instrument ID: {instrument.id}")
    print(f"Outcome: {instrument.outcome}\n")

    # Calculate time range for historical data
    start = pd.Timestamp("2025-10-30", tz="UTC")
    end = pd.Timestamp("2025-10-31", tz="UTC")

    print(f"Fetching data from {start} to {end}")

    # Load historical data using convenience methods
    print("Loading orderbook snapshots...")

    deltas = await loader.load_orderbook_snapshots(
        start=start,
        end=end,
    )
    print(f"Loaded {len(deltas)} OrderBookDeltas")

    print("Loading trade ticks...")
    trades = await loader.load_trades(
        start=start,
        end=end,
    )
    print(f"Loaded {len(trades)} TradeTicks")

    if not deltas and not trades:
        raise ValueError("No historical data available for the specified time range")

    # Configure backtest engine
    config = BacktestEngineConfig(trader_id=TraderId("BACKTESTER-001"))
    engine = BacktestEngine(config=config)

    # Add venue
    engine.add_venue(
        venue=POLYMARKET_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        base_currency=USDC_POS,
        starting_balances=[Money(10_000, USDC_POS)],
        book_type=BookType.L2_MBP,
    )

    # Add instrument
    engine.add_instrument(instrument)

    # Add data
    if deltas:
        engine.add_data(deltas)
    if trades:
        engine.add_data(trades)

    # Configure strategy
    strategy_config = OrderBookImbalanceConfig(
        instrument_id=instrument.id,
        max_trade_size=Decimal(20),
        min_seconds_between_triggers=1.0,
    )

    strategy = OrderBookImbalance(config=strategy_config)
    engine.add_strategy(strategy=strategy)

    print("\nStarting backtest...")
    time.sleep(0.1)

    # Run backtest
    engine.run()

    with pd.option_context(
        "display.max_rows",
        100,
        "display.max_columns",
        None,
        "display.width",
        300,
    ):
        print(engine.trader.generate_account_report(POLYMARKET_VENUE))
        print("\n")
        print(engine.trader.generate_order_fills_report())
        print("\n")
        print(engine.trader.generate_positions_report())

    # Cleanup
    engine.reset()
    engine.dispose()


if __name__ == "__main__":
    try:
        asyncio.run(
            run_backtest(
                market_slug=MARKET_SLUG,
                lookback_hours=24,  # Fetch last 24 hours of data
            ),
        )
    except Exception as e:
        print(f"Error running backtest: {e}")
        raise
