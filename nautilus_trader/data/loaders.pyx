# -------------------------------------------------------------------------------------------------
# <copyright file="loaders.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd


cdef class CSVTickDataLoader:
    """
    Provides a means of loading tick data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(file_path: str) -> pd.DataFrame:
        """
        Return the tick pandas.DataFrame loaded from the given csv file.
        
        :param file_path: The absolute path to the CSV file.
        :return: pd.DataFrame.
        """
        return pd.read_csv(file_path,
                           usecols=[1, 2, 3],
                           index_col=0,
                           header=None,
                           parse_dates=True)


cdef class CSVBarDataLoader:
    """
    Provides a means of loading bar data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(file_path: str) -> pd.DataFrame:
        """
        Return the bar pandas.DataFrame loaded from the given csv file.

        :param file_path: The absolute path to the CSV file.
        :return: pd.DataFrame.
        """
        return pd.read_csv(file_path,
                           index_col='Time (UTC)',
                           parse_dates=True)
