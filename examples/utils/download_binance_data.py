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
Download historical OHLCV data from Binance and save to ParquetDataCatalog.

This script downloads historical kline (candlestick) data from Binance's public API
and converts it into NautilusTrader's bar format, saving it to a Parquet catalog
for backtesting.

Usage:
    python examples/utils/download_binance_data.py

Configuration:
    Edit the CONFIGURATION section below to customize:
    - Symbol (e.g., BTCUSDT-PERP)
    - Date range
    - Timeframes
    - Catalog path
"""

import asyncio
import json
import time
from datetime import datetime
from pathlib import Path
from urllib.parse import urlencode
from urllib.request import urlopen

import pandas as pd
from nautilus_trader.adapters.binance import BINANCE_VENUE
from nautilus_trader.adapters.binance import get_cached_binance_http_client
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import BarDataWrangler


# ============================================================================
# CONFIGURATION
# ============================================================================

# Catalog path (where data will be saved)
CATALOG_PATH = Path("~/.nautilus/catalog").expanduser()

# Symbol to download
SYMBOL = "BTCUSDT-PERP"

# Date range (will be updated for multiple years)
START_DATE = "2022-01-01"
END_DATE = "2025-12-22"  # Up to today

# Timeframes to download (Binance intervals)
# Available: 1m, 3m, 5m, 15m, 30m, 1h, 2h, 4h, 6h, 8h, 12h, 1d, 3d, 1w, 1M
TIMEFRAMES = ["5m", "1h", "4h"]

# Binance API endpoint
BINANCE_FUTURES_API = "https://fapi.binance.com/fapi/v1/klines"

# ============================================================================


def download_binance_klines(
    symbol: str,
    interval: str,
    start_time: int,
    end_time: int,
    limit: int = 1500,
) -> pd.DataFrame:
    """
    Download klines from Binance Futures API.

    Parameters
    ----------
    symbol : str
        Trading pair symbol (e.g., BTCUSDT).
    interval : str
        Kline interval (e.g., 1m, 5m, 1h, 4h).
    start_time : int
        Start time in milliseconds.
    end_time : int
        End time in milliseconds.
    limit : int, default 1500
        Number of klines per request (max 1500).

    Returns
    -------
    pd.DataFrame
        DataFrame with columns: timestamp, open, high, low, close, volume.

    """
    # Remove -PERP suffix for Binance API
    api_symbol = symbol.replace("-PERP", "")

    all_data = []
    current_start = start_time

    print(f"Downloading {interval} data for {symbol}...")

    while current_start < end_time:
        params = {
            "symbol": api_symbol,
            "interval": interval,
            "startTime": current_start,
            "endTime": end_time,
            "limit": limit,
        }

        # Build URL with query parameters
        url = f"{BINANCE_FUTURES_API}?{urlencode(params)}"

        # Make request using urllib
        try:
            with urlopen(url, timeout=30) as response:
                data = json.loads(response.read().decode())
        except Exception as e:
            print(f"  Error downloading data: {e}")
            break

        if not data:
            break

        all_data.extend(data)

        # Update start time to last candle's close time + 1ms
        current_start = data[-1][6] + 1  # Close time + 1ms

        print(f"  Downloaded {len(all_data)} bars so far...")

        # Rate limiting
        time.sleep(0.1)

    if not all_data:
        print(f"  No data found for {symbol} {interval}")
        return pd.DataFrame()

    # Convert to DataFrame
    df = pd.DataFrame(
        all_data,
        columns=[
            "open_time",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "close_time",
            "quote_volume",
            "trades",
            "taker_buy_base",
            "taker_buy_quote",
            "ignore",
        ],
    )

    # Convert timestamp to datetime
    df["timestamp"] = pd.to_datetime(df["open_time"], unit="ms")

    # Select and reorder columns
    df = df[["timestamp", "open", "high", "low", "close", "volume"]]

    # Convert prices and volumes to float
    df["open"] = df["open"].astype(float)
    df["high"] = df["high"].astype(float)
    df["low"] = df["low"].astype(float)
    df["close"] = df["close"].astype(float)
    df["volume"] = df["volume"].astype(float)

    # Set timestamp as index
    df = df.set_index("timestamp")

    print(f"  Downloaded {len(df)} bars total for {symbol} {interval}")
    return df


def interval_to_bar_spec(interval: str) -> str:
    """
    Convert Binance interval to NautilusTrader bar spec.

    Parameters
    ----------
    interval : str
        Binance interval (e.g., 1m, 5m, 1h, 4h).

    Returns
    -------
    str
        Bar specification (e.g., 1-MINUTE, 5-MINUTE, 1-HOUR, 4-HOUR).

    """
    mapping = {
        "1m": "1-MINUTE",
        "3m": "3-MINUTE",
        "5m": "5-MINUTE",
        "15m": "15-MINUTE",
        "30m": "30-MINUTE",
        "1h": "1-HOUR",
        "2h": "2-HOUR",
        "4h": "4-HOUR",
        "6h": "6-HOUR",
        "8h": "8-HOUR",
        "12h": "12-HOUR",
        "1d": "1-DAY",
        "3d": "3-DAY",
        "1w": "1-WEEK",
        "1M": "1-MONTH",
    }

    if interval not in mapping:
        raise ValueError(f"Unsupported interval: {interval}")

    return mapping[interval]


async def load_instrument(symbol: str):
    """
    Load instrument definition from Binance.

    Parameters
    ----------
    symbol : str
        Symbol with venue suffix (e.g., BTCUSDT-PERP).

    Returns
    -------
    Instrument
        NautilusTrader instrument object.

    """
    print(f"\nLoading instrument definition for {symbol}...")

    clock = LiveClock()

    client = get_cached_binance_http_client(
        clock=clock,
        account_type=BinanceAccountType.USDT_FUTURES,
        is_testnet=False,  # Use production for instrument definitions
    )

    provider = BinanceFuturesInstrumentProvider(
        client=client,
        clock=clock,
        config=InstrumentProviderConfig(load_all=True, log_warnings=False),
    )

    await provider.load_all_async()

    instrument_id = InstrumentId(
        symbol=Symbol(symbol),
        venue=BINANCE_VENUE,
    )

    instrument = provider.find(instrument_id)

    if instrument is None:
        raise ValueError(f"Instrument {symbol} not found on Binance")

    print(f"  Loaded: {instrument.id}")
    return instrument


async def main():
    """Main execution function."""
    print("=" * 80)
    print("Binance Data Downloader for NautilusTrader")
    print("=" * 80)
    print("\nConfiguration:")
    print(f"  Symbol: {SYMBOL}")
    print(f"  Date range: {START_DATE} to {END_DATE}")
    print(f"  Timeframes: {', '.join(TIMEFRAMES)}")
    print(f"  Catalog path: {CATALOG_PATH}")
    print()

    # Convert dates to milliseconds
    start_ms = int(datetime.strptime(START_DATE, "%Y-%m-%d").timestamp() * 1000)
    end_ms = int(datetime.strptime(END_DATE, "%Y-%m-%d").timestamp() * 1000)

    # Load instrument
    instrument = await load_instrument(SYMBOL)

    # Create catalog
    print(f"\nInitializing catalog at {CATALOG_PATH}...")
    CATALOG_PATH.mkdir(parents=True, exist_ok=True)
    catalog = ParquetDataCatalog(str(CATALOG_PATH))

    # Write instrument to catalog
    catalog.write_data([instrument])
    print(f"  Wrote instrument: {instrument.id}")

    # Download and save each timeframe
    for interval in TIMEFRAMES:
        print(f"\n{'-' * 80}")
        print(f"Processing {interval} bars...")
        print(f"{'-' * 80}")

        # Download data
        df = download_binance_klines(
            symbol=SYMBOL,
            interval=interval,
            start_time=start_ms,
            end_time=end_ms,
        )

        if df.empty:
            print(f"  Skipping {interval} - no data")
            continue

        # Create bar type
        bar_spec = interval_to_bar_spec(interval)
        bar_type = BarType.from_str(f"{instrument.id}-{bar_spec}-LAST-EXTERNAL")

        # Convert to Bar objects
        print("  Converting to NautilusTrader bars...")
        wrangler = BarDataWrangler(bar_type, instrument)
        bars = wrangler.process(df)

        # Write to catalog
        print(f"  Writing {len(bars)} bars to catalog...")
        catalog.write_data(bars)

        print(f"  ✓ Saved {len(bars)} {interval} bars to catalog")

    print("\n" + "=" * 80)
    print("Download complete!")
    print("=" * 80)
    print(f"\nData saved to: {CATALOG_PATH}")
    print("\nYou can now run backtests with this data.")
    print("Update your backtest script with:")
    print(f'  CATALOG_PATH = Path("{CATALOG_PATH}")')
    print(f'  START = "{START_DATE}"')
    print(f'  END = "{END_DATE}"')


if __name__ == "__main__":
    asyncio.run(main())
