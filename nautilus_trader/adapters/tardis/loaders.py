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

from datetime import datetime
from os import PathLike

import pandas as pd


def _ts_parser(time_in_secs: str) -> datetime:
    return datetime.utcfromtimestamp(int(time_in_secs) / 1_000_000.0)


class TardisTradeDataLoader:
    """
    Provides a means of loading trade data pandas DataFrames from Tardis CSV files.
    """

    @staticmethod
    def load(file_path: PathLike[str] | str) -> pd.DataFrame:
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
        df = df.rename(columns={"id": "trade_id", "amount": "quantity"})
        df["side"] = df.side.str.upper()
        df = df[["symbol", "trade_id", "price", "quantity", "side"]]

        return df


class TardisQuoteDataLoader:
    """
    Provides a means of loading quote tick data pandas DataFrames from Tardis CSV files.
    """

    @staticmethod
    def load(file_path: PathLike[str] | str) -> pd.DataFrame:
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
        df = df.rename(
            columns={
                "ask_amount": "ask_size",
                "bid_amount": "bid_size",
            },
        )

        return df[["bid_price", "ask_price", "bid_size", "ask_size"]]
