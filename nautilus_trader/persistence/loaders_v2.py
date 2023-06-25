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
            "timestamp": pl.Datetime,
            # Specify other column types here
        }
        columns = ["timestamp", "bid", "ask"]
        df = pl.read_csv(path, dtypes=dtypes, columns=columns)
        return df


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
