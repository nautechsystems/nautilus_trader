# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import time
from datetime import datetime

import numpy as np
import pandas as pd
import requests


try:
    from scipy.stats import linregress
except ImportError:
    # If scipy is not installed, raise an error with installation instructions
    raise ImportError("scipy is not installed. Please install it using 'pip install scipy'")


def get_binance_historical_bars(
    symbol="BTCUSDT",
    start=datetime(2020, 8, 10),
    end=datetime(2021, 8, 10),
    interval="1h",
    base="fapi",
    version="v1",
):
    """
    Fetch historical Kline data from the Binance API for a specific symbol and return it
    as a DataFrame.

    Args:
        symbol (str): The trading pair symbol.
        start (datetime): The start date as a datetime object.
        end (datetime): The end date as a datetime object.
        interval (str): The interval period for Klines (e.g., '1h').
        base (str): The base endpoint for the Binance API.
        version (str): The API version.

    Returns:
        pd.DataFrame: DataFrame containing Kline data with correctly typed columns.

    """
    klines = []
    start_time = int(start.timestamp()) * 1000
    end_time = min(int(end.timestamp()) * 1000, int(time.time() * 1000))

    interval_map = {"m": 60 * 1000, "h": 3600 * 1000, "d": 86400 * 1000}
    interval_ms = int(interval[:-1]) * interval_map[interval[-1]]

    while start_time < end_time:
        adjusted_end_time = min(start_time + 1000 * interval_ms, end_time)
        url = f"https://{base}.binance.com/{base}/{version}/klines"
        params = {
            "symbol": symbol,
            "interval": interval,
            "startTime": start_time,
            "endTime": adjusted_end_time,
            "limit": 1000,
        }

        try:
            response = requests.get(url, params=params)
            response.raise_for_status()
            data = response.json()
        except requests.exceptions.RequestException as e:
            print(f"Request error for {symbol}: {e}")
            break

        if not data:
            break

        # Add new data to klines and update the start_time to avoid overlap
        klines.extend(data)
        # Update start_time to one millisecond past the last returned kline to avoid duplicates
        start_time = data[-1][0] + interval_ms

        # Sleep to prevent hitting rate limits
        time.sleep(0.1)

    columns = [
        "time",
        "open",
        "high",
        "low",
        "close",
        "volume",
        "end_time",
        "quote_asset_volume",
        "number_of_trades",
        "taker_buy_base_volume",
        "taker_buy_quote_volume",
        "ignore",
    ]

    # Create DataFrame and convert columns to suitable types
    df = pd.DataFrame(klines, columns=columns)

    # Convert columns to appropriate types
    df["symbol"] = symbol
    df["time"] = pd.to_datetime(df["time"], unit="ms")  # Convert time to datetime
    df["open"] = pd.to_numeric(df["open"], errors="coerce")
    df["high"] = pd.to_numeric(df["high"], errors="coerce")
    df["low"] = pd.to_numeric(df["low"], errors="coerce")
    df["close"] = pd.to_numeric(df["close"], errors="coerce")
    df["volume"] = pd.to_numeric(df["volume"], errors="coerce")
    df["quote_asset_volume"] = pd.to_numeric(df["quote_asset_volume"], errors="coerce")
    df["number_of_trades"] = pd.to_numeric(df["number_of_trades"], errors="coerce")
    df["taker_buy_base_volume"] = pd.to_numeric(df["taker_buy_base_volume"], errors="coerce")
    df["taker_buy_quote_volume"] = pd.to_numeric(df["taker_buy_quote_volume"], errors="coerce")

    df = df.set_index("time")

    return df


def compute_bollinger_bands(df, period=20, num_std=2):
    """
    Compute Bollinger Bands for a given DataFrame.
    """
    df["SMA"] = df["close"].rolling(window=period).mean()
    df["STD_DEV"] = df["close"].rolling(window=period).std()
    df["BB_upper"] = df["SMA"] + (df["STD_DEV"] * num_std)
    df["BB_lower"] = df["SMA"] - (df["STD_DEV"] * num_std)
    df["BB_width"] = df["BB_upper"] - df["BB_lower"]
    return df


def compute_atr(df, period=14):
    """
    Compute the ATR for a given DataFrame.
    """
    df["High-Low"] = df["high"] - df["low"]
    df["High-Close"] = np.abs(df["high"] - df["close"].shift())
    df["Low-Close"] = np.abs(df["low"] - df["close"].shift())
    df["TrueRange"] = df[["High-Low", "High-Close", "Low-Close"]].max(axis=1)
    df["ATR"] = df["TrueRange"].rolling(window=period).mean()
    return df["ATR"]


def calculate_slope(df, window=8):
    """
    Calculate slope of the close price using linear regression.
    """
    y = df["close"].tail(window).to_numpy()
    x = np.arange(len(y))
    slope, _, _, _, _ = linregress(x, y)
    return slope


def generate_metrics(df_dict, period_bollinger=20, period_atr=14, slope_window=8):
    """
    Compute metrics for multiple symbols and return them as a DataFrame.

    Args:
        df_dict (dict): Dictionary of DataFrames with symbol as key and DataFrame as value.
        period_bollinger (int): Period for computing Bollinger Bands (default 20).
        period_atr (int): Period for computing ATR (default 14).
        slope_window (int): Window size for calculating slope (default 8).

    Returns:
        pd.DataFrame: DataFrame containing metrics and average score for each symbol.

    """
    results = []

    # Helper function to normalize a series
    def normalize(series):
        return (
            (series - series.min()) / (series.max() - series.min())
            if series.max() != series.min()
            else series * 0
        )

    for symbol, df in df_dict.items():
        # Calculate mean volatility (standard deviation of percentage changes in close prices)
        mean_volatility = df["close"].pct_change().std()

        # Calculate ATR and mean NATR (normalized ATR)
        df["ATR"] = compute_atr(df, period=period_atr)
        mean_natr = (df["ATR"] / df["close"]).mean()

        # Calculate Bollinger Bands width and mean width
        df = compute_bollinger_bands(df, period=period_bollinger)
        mean_bb_width = df["BB_width"].mean()

        # Calculate the latest trend using slope of close prices
        latest_trend = calculate_slope(df, window=slope_window)

        # Calculate average volume per hour (assuming data is in 15-minute intervals)
        avg_volume_per_hour = df["volume"].mean() * 4

        # Store metrics in a dictionary
        metrics = {
            "symbol": symbol,
            "mean_volatility": mean_volatility,
            "mean_natr": mean_natr,
            "mean_bb_width": mean_bb_width,
            "latest_trend": latest_trend,
            "avg_volume_per_hour": avg_volume_per_hour,
        }

        results.append(metrics)

    # Create DataFrame from the metrics list
    df_metrics = pd.DataFrame(results)
    # Normalize the metrics across symbols
    for column in [
        "mean_volatility",
        "mean_natr",
        "mean_bb_width",
        "latest_trend",
        "avg_volume_per_hour",
    ]:
        df_metrics[f"normalized_{column}"] = normalize(df_metrics[column])

    # Compute average score as the mean of all normalized metrics
    normalized_columns = [
        f"normalized_{col}"
        for col in [
            "mean_volatility",
            "mean_natr",
            "mean_bb_width",
            "latest_trend",
            "avg_volume_per_hour",
        ]
    ]
    df_metrics["average_score"] = df_metrics[normalized_columns].mean(axis=1)

    # Sort by average score and return
    df_metrics = df_metrics.sort_values(by="average_score", ascending=False)

    return df_metrics
