# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

from pathlib import Path

import pandas as pd


class QuoteTickDataFrameProcessor:
    """
    Provides a means of pre-processing quote tick pandas DataFrames.
    """

    @staticmethod
    def process(df: pd.DataFrame) -> pd.DataFrame:
        """
        Return the pre-processed data from the given dataframe.

        Parameters
        ----------
        df : DataFrame
            The pandas dataframe to pre-process.

        Returns
        -------
        pd.DataFrame

        """
        # Rename column
        df = df.rename(columns={"timestamp": "ts_event"})

        # Multiply by 1e9 and convert to int
        df["bid"] = (df["bid"] * 1e9).astype(pd.Int64Dtype())
        df["ask"] = (df["ask"] * 1e9).astype(pd.Int64Dtype())

        # Create bid_size and ask_size columns
        df["bid_size"] = pd.Series([1_000_000 * 1e9] * len(df), dtype=pd.UInt64Dtype())
        df["ask_size"] = pd.Series([1_000_000 * 1e9] * len(df), dtype=pd.UInt64Dtype())

        df["ts_event"] = (
            pd.to_datetime(df["ts_event"], utc=True, format="mixed")
            .dt.tz_localize(None)
            .view("int64")
            .astype("uint64")
        )
        df["ts_init"] = df["ts_event"]

        # Reorder the columns and drop index column
        df = df[["bid", "ask", "bid_size", "ask_size", "ts_event", "ts_init"]]
        df = df.reset_index(drop=True)

        return df


class TradeTickDataFrameLoader:  # Will become a specific Binance parser (just experimenting)
    """
    Provides a means of loading trade tick data pandas DataFrames from CSV files.
    """

    @staticmethod
    def read_csv(path: str | Path) -> pd.DataFrame:
        """
        Return the tick data read from the CSV file.

        Parameters
        ----------
        path : str | Path
            The path to the CSV file.

        Returns
        -------
        pd.DataFrame

        """
        dtypes = {
            "ts_event": pd.Timestamp,
            "trade_id": str,
        }
        new_columns = ["ts_event", "trade_id", "price", "size", "aggressor_side"]
        df = pd.read_csv(
            path,
            # dtype=dtypes,
            usecols=list(dtypes.keys()) + new_columns[2:],
            parse_dates=["ts_event"],
            names=new_columns,
        )
        df["aggressor_side"] = df["aggressor_side"].map(_map_aggressor_side)
        return df


def _map_aggressor_side(val: bool) -> str:
    return "buyer" if val else "seller"
