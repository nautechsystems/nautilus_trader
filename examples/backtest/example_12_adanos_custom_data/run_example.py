#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import tempfile
from decimal import Decimal
from pathlib import Path

import pandas as pd

from strategy import AdanosSentimentStrategy
from strategy import AdanosSentimentStrategyConfig

from nautilus_trader.adapters.adanos import build_adanos_sentiment_snapshot
from nautilus_trader.adapters.adanos import wrap_adanos_sentiment_snapshot
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def _build_bars():
    instrument = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")
    bar_type = BarType.from_str(f"{instrument.id}-1-DAY-LAST-EXTERNAL")

    df = pd.DataFrame(
        [
            ("2024-03-04 14:30:00+00:00", 178.25, 181.10, 177.80, 180.55, 1_250_000),
            ("2024-03-05 14:30:00+00:00", 180.60, 182.20, 179.50, 181.70, 1_410_000),
            ("2024-03-06 14:30:00+00:00", 181.80, 183.10, 180.90, 182.95, 1_515_000),
            ("2024-03-07 14:30:00+00:00", 183.00, 184.75, 182.10, 184.40, 1_605_000),
        ],
        columns=["timestamp", "open", "high", "low", "close", "volume"],
    )
    df["timestamp"] = pd.to_datetime(df["timestamp"], utc=True)
    df = df.set_index("timestamp")

    wrangler = BarDataWrangler(bar_type, instrument)
    bars: list[Bar] = wrangler.process(df)

    return instrument, bar_type, bars, df.index


def _build_sentiment_data(instrument_id, timestamps):
    sample_rows = [
        {
            "reddit": {"buzz_score": 63.0, "bullish_pct": 71, "mentions": 92, "company_name": "Apple Inc."},
            "x": {"buzz_score": 59.5, "bullish_pct": 61, "mentions": 420},
            "news": {"buzz_score": 51.0, "bullish_pct": 57, "mentions": 14},
            "polymarket": {"buzz_score": 48.0, "bullish_pct": 54, "trade_count": 112, "market_count": 3},
        },
        {
            "reddit": {"buzz_score": 67.0, "bullish_pct": 73, "mentions": 108, "company_name": "Apple Inc."},
            "x": {"buzz_score": 65.0, "bullish_pct": 64, "mentions": 510},
            "news": {"buzz_score": 56.0, "bullish_pct": 60, "mentions": 18},
            "polymarket": {"buzz_score": 52.0, "bullish_pct": 58, "trade_count": 141, "market_count": 4},
        },
        {
            "reddit": {"buzz_score": 49.0, "bullish_pct": 37, "mentions": 85, "company_name": "Apple Inc."},
            "x": {"buzz_score": 54.0, "bullish_pct": 59, "mentions": 455},
            "news": {"buzz_score": 52.5, "bullish_pct": 62, "mentions": 20},
            "polymarket": {"buzz_score": 58.0, "bullish_pct": 66, "trade_count": 170, "market_count": 5},
        },
        {
            "reddit": {"buzz_score": 42.0, "bullish_pct": 34, "mentions": 71, "company_name": "Apple Inc."},
            "x": {"buzz_score": 46.0, "bullish_pct": 39, "mentions": 396},
            "news": {"buzz_score": 49.0, "bullish_pct": 44, "mentions": 17},
            "polymarket": {"buzz_score": 44.5, "bullish_pct": 41, "trade_count": 96, "market_count": 2},
        },
    ]

    custom_data = []
    for timestamp, rows in zip(timestamps, sample_rows, strict=True):
        ts_event = dt_to_unix_nanos(pd.Timestamp(timestamp))
        snapshot = build_adanos_sentiment_snapshot(
            instrument_id,
            ts_event=ts_event,
            reddit=rows["reddit"],
            x=rows["x"],
            news=rows["news"],
            polymarket=rows["polymarket"],
        )
        custom_data.append(wrap_adanos_sentiment_snapshot(snapshot))

    return custom_data


if __name__ == "__main__":
    instrument, bar_type, bars, timestamps = _build_bars()
    sentiment_data = _build_sentiment_data(instrument.id, timestamps)

    catalog_path = Path(tempfile.mkdtemp(prefix="adanos_sentiment_catalog_"))
    catalog = ParquetDataCatalog(str(catalog_path))
    catalog.write_data([instrument])
    catalog.write_data(bars)
    catalog.write_data(sentiment_data)

    print(f"Wrote {len(sentiment_data)} Adanos sentiment snapshots to {catalog_path}")

    engine = BacktestEngine(
        config=BacktestEngineConfig(
            trader_id=TraderId("ADANOS-BACKTEST-001"),
            logging=LoggingConfig(log_level="INFO"),
        ),
    )
    engine.add_venue(
        venue=Venue("XNAS"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[Money(1_000_000, instrument.quote_currency)],
        base_currency=instrument.quote_currency,
        default_leverage=Decimal(1),
    )
    engine.add_instrument(instrument)
    engine.add_data(bars)
    engine.add_data(sentiment_data, ClientId("ADANOS"))
    engine.add_strategy(
        AdanosSentimentStrategy(
            AdanosSentimentStrategyConfig(
                instrument_id=instrument.id,
                bar_type=bar_type,
            ),
        ),
    )
    engine.run()
    engine.dispose()
