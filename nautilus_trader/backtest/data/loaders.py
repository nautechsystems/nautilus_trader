# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime

import pandas as pd


class CSVTickDataLoader:
    """
    Provides a means of loading tick data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(file_path) -> pd.DataFrame:
        """
        Return the tick pandas.DataFrame loaded from the given csv file.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the CSV file.

        Returns
        -------
        pd.DataFrame

        """
        return pd.read_csv(
            file_path,
            index_col="timestamp",
            parse_dates=True,
        )


class CSVBarDataLoader:
    """
    Provides a means of loading bar data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(file_path) -> pd.DataFrame:
        """
        Return the bar pandas.DataFrame loaded from the given csv file.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the CSV file.

        Returns
        -------
        pd.DataFrame

        """
        return pd.read_csv(
            file_path,
            index_col="timestamp",
            parse_dates=True,
        )


def _ts_parser(time_in_secs: str) -> datetime:
    return datetime.utcfromtimestamp(int(time_in_secs) / 1_000_000.0)


class TardisTradeDataLoader:
    """
    Provides a means of loading trade data pandas DataFrames from Tardis CSV files.
    """

    @staticmethod
    def load(file_path) -> pd.DataFrame:
        """
        Return the trade pandas.DataFrame loaded from the given csv file.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the CSV file.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_csv(
            file_path,
            index_col="local_timestamp",
            date_parser=_ts_parser,
            parse_dates=True,
        )
        df.rename(columns={"id": "trade_id", "amount": "quantity"}, inplace=True)
        df["side"] = df.side.str.upper()
        df = df[["symbol", "trade_id", "price", "quantity", "side"]]

        return df


class TardisQuoteDataLoader:
    """
    Provides a means of loading quote data pandas DataFrames from Tardis CSV files.
    """

    @staticmethod
    def load(file_path) -> pd.DataFrame:
        """
        Return the quote pandas.DataFrame loaded from the given csv file.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the CSV file.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_csv(
            file_path,
            index_col="local_timestamp",
            date_parser=_ts_parser,
            parse_dates=True,
        )
        df.rename(
            columns={
                "ask_amount": "ask_size",
                "ask_price": "ask",
                "bid_price": "bid",
                "bid_amount": "bid_size",
            },
            inplace=True,
        )

        return df[["bid", "ask", "bid_size", "ask_size"]]


class ParquetTickDataLoader:
    """
    Provides a means of loading tick data pandas DataFrames from Parquet files.
    """

    @staticmethod
    def load(file_path) -> pd.DataFrame:
        """
        Return the tick pandas.DataFrame loaded from the given parquet file.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the Parquet file.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_parquet(file_path)
        df.set_index("timestamp", inplace=True)
        return df


class ParquetBarDataLoader:
    """
    Provides a means of loading bar data pandas DataFrames from parquet files.
    """

    @staticmethod
    def load(file_path) -> pd.DataFrame:
        """
        Return the bar pandas.DataFrame loaded from the given parquet file.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the parquet file.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_parquet(file_path)
        df.set_index("timestamp", inplace=True)
        return df
