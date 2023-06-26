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

import polars as pl


class QuoteTickDataFrameLoader:
    """
    Provides a means of loading quote tick data polars DataFrames from CSV files.
    """

    @staticmethod
    def read_csv(path: str | Path) -> pl.DataFrame:
        """
        Return the tick data read from the CSV file.

        Parameters
        ----------
        path : str | Path
            The path to the CSV file.

        Returns
        -------
        pl.DataFrame

        """
        dtypes = {
            "bid": pl.Float64,
            "ask": pl.Float64,
            # "bid_size": pl.Float64,
            # "ask_size": pl.Float64,
            "ts_event": pl.Datetime,
            # "ts_init": pl.Datetime,
        }
        new_columns = ["ts_event", "bid", "ask"]
        df = pl.read_csv(
            path,
            dtypes=dtypes,
            new_columns=new_columns,
        )
        return df


class TradeTickDataFrameLoader:  # Will become a specific Binance parser (just experimenting)
    """
    Provides a means of loading trade tick data polars DataFrames from CSV files.
    """

    @staticmethod
    def read_csv(path: str | Path) -> pl.DataFrame:
        """
        Return the tick data read from the CSV file.

        Parameters
        ----------
        path : str | Path
            The path to the CSV file.

        Returns
        -------
        pl.DataFrame

        """
        dtypes = {
            "ts_event": pl.Datetime,
            "trade_id": pl.Utf8,
        }
        new_columns = ["ts_event", "trade_id", "price", "size", "aggressor_side"]
        df = pl.read_csv(
            path,
            dtypes=dtypes,
            new_columns=new_columns,
        )
        df = df.with_columns(
            pl.col("aggressor_side")
            .apply(_map_aggressor_side, return_dtype=pl.Utf8)
            .alias("aggressor_side"),
        )
        return df


def _map_aggressor_side(val: bool) -> str:
    return "buyer" if val else "seller"


class BarDataFrameLoader:
    """
    Provides a means of loading bar data polars DataFrames from CSV files.
    """

    @staticmethod
    def read_csv(path: str | Path) -> pl.DataFrame:
        """
        Return the bar data read from the CSV file.

        Parameters
        ----------
        path : str | Path
            The path to the CSV file.

        Returns
        -------
        pl.DataFrame

        """
        df = pl.read_csv(path)
        return df
