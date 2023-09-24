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

from os import PathLike

import pandas as pd


class CSVTickDataLoader:
    """
    Provides a generic tick data CSV file loader.
    """

    @staticmethod
    def load(
        file_path: PathLike[str] | str,
        index_col: str | int = "timestamp",
        format: str = "mixed",
    ) -> pd.DataFrame:
        """
        Return a tick `pandas.DataFrame` loaded from the given CSV `file_path`.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the CSV file.
        index_col : str | int, default 'timestamp'
            The index column.
        format : str, default 'mixed'
            The timestamp column format.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_csv(
            file_path,
            index_col=index_col,
            parse_dates=True,
        )
        df.index = pd.to_datetime(df.index, format=format)
        return df


class CSVBarDataLoader:
    """
    Provides a generic bar data CSV file loader.
    """

    @staticmethod
    def load(file_path: PathLike[str] | str) -> pd.DataFrame:
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
        df = pd.read_csv(
            file_path,
            index_col="timestamp",
            parse_dates=True,
        )
        df.index = pd.to_datetime(df.index, format="mixed")
        return df


class ParquetTickDataLoader:
    """
    Provides a generic tick data Parquet file loader.
    """

    @staticmethod
    def load(
        file_path: PathLike[str] | str,
        timestamp_column: str = "timestamp",
    ) -> pd.DataFrame:
        """
        Return the tick pandas.DataFrame loaded from the given parquet file.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the Parquet file.
        timestamp_column: str
            Name of the timestamp column in the parquet data

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_parquet(file_path)
        df = df.set_index(timestamp_column)
        return df


class ParquetBarDataLoader:
    """
    Provides a generic bar data Parquet file loader.
    """

    @staticmethod
    def load(file_path: PathLike[str] | str) -> pd.DataFrame:
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
        df = df.set_index("timestamp")
        return df
