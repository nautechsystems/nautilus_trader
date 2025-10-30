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
- Orderbook History: https://api.domeapi.io/v1/polymarket/orderbooks
- Trades/Prices: https://clob.polymarket.com/prices-history

Note: The DomeAPI orderbook history only has data starting from October 14th, 2025.

"""

import time
from datetime import UTC
from datetime import datetime
from datetime import timedelta
from decimal import Decimal

import pandas as pd

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.parsing import parse_instrument
from nautilus_trader.adapters.polymarket.loaders import PolymarketDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.core.datetime import millis_to_nanos
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


def run_backtest(
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
    # Find the market using the loader
    loader = PolymarketDataLoader()
    market = loader.find_market_by_slug(market_slug)
    condition_id = market["conditionId"]

    print(f"\nMarket found: {market.get('question', 'N/A')}")
    print(f"Slug: {market.get('slug', 'N/A')}")
    print(f"Active: {market.get('active', False)}")

    # Fetch detailed market info
    market_details = loader.fetch_market_details(condition_id)

    # Get token information
    tokens = market_details.get("tokens", [])
    if not tokens:
        raise ValueError(f"No tokens found for market: {condition_id}")

    token = tokens[0]  # Use first token
    token_id = token["token_id"]
    outcome = token["outcome"]

    print(f"Outcome: {outcome}")
    print(f"Condition ID: {condition_id}")
    print(f"Token ID: {token_id}\n")

    # Create instrument
    ts_init = millis_to_nanos(int(datetime.now(tz=UTC).timestamp() * 1000))
    instrument = parse_instrument(
        market_info=market_details,
        token_id=token_id,
        outcome=outcome,
        ts_init=ts_init,
    )

    # Calculate time range for historical data
    end_time = datetime.now(tz=UTC)
    start_time = end_time - timedelta(hours=lookback_hours)

    start_time_ms = int(start_time.timestamp() * 1000)
    end_time_ms = int(end_time.timestamp() * 1000)
    start_time_s = int(start_time.timestamp())
    end_time_s = int(end_time.timestamp())

    print(f"Fetching data from {start_time} to {end_time}")

    # Fetch historical data using the loader
    try:
        print(f"Fetching orderbook history for token_id: {token_id}")
        orderbook_snapshots = loader.fetch_orderbook_history(
            token_id=token_id,
            start_time_ms=start_time_ms,
            end_time_ms=end_time_ms,
        )
        print(f"Fetched {len(orderbook_snapshots)} orderbook snapshots")
    except Exception as e:
        print(f"Warning: Could not fetch orderbook history: {e}")
        orderbook_snapshots = []

    try:
        print(f"Fetching price history for token_id: {token_id}")
        price_history = loader.fetch_price_history(
            token_id=token_id,
            start_ts=start_time_s,
            end_ts=end_time_s,
        )
        print(f"Fetched {len(price_history)} price points")
    except Exception as e:
        print(f"Warning: Could not fetch price history: {e}")
        price_history = []

    if not orderbook_snapshots and not price_history:
        raise ValueError("No historical data available for the specified time range")

    # Parse data using the loader
    book_deltas = []
    if orderbook_snapshots:
        print("Parsing orderbook snapshots to deltas...")
        book_deltas = loader.parse_orderbook_snapshots(orderbook_snapshots, instrument)
        print(f"Created {len(book_deltas)} OrderBookDeltas")

    trades = []
    if price_history:
        print("Parsing price history to trades...")
        trades = loader.parse_price_history(price_history, instrument)
        print(f"Created {len(trades)} TradeTicks")

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
    if book_deltas:
        engine.add_data(book_deltas)
    if trades:
        engine.add_data(trades)

    # Configure strategy
    strategy_config = OrderBookImbalanceConfig(
        instrument_id=instrument.id,
        max_trade_size=Decimal("10"),
        min_seconds_between_triggers=1.0,
    )

    strategy = OrderBookImbalance(config=strategy_config)
    engine.add_strategy(strategy=strategy)

    print("\nStarting backtest...")
    time.sleep(0.1)

    # Run the engine
    engine.run()

    # Display reports
    print("\n" + "=" * 80)
    print("BACKTEST RESULTS")
    print("=" * 80 + "\n")

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
        run_backtest(
            market_slug=MARKET_SLUG,
            lookback_hours=24,  # Fetch last 24 hours of data
        )
    except Exception as e:
        print(f"Error running backtest: {e}")
        raise
