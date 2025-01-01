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

from os import PathLike

import pandas as pd

from nautilus_trader.model.enums import RecordFlag


class BinanceOrderBookDeltaDataLoader:
    """
    Provides a means of loading Binance order book data.
    """

    @classmethod
    def load(
        cls,
        file_path: PathLike[str] | str,
        nrows: int | None = None,
    ) -> pd.DataFrame:
        """
        Return the deltas `pandas.DataFrame` loaded from the given CSV `file_path`.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the CSV file.
        nrows : int, optional
            The maximum number of rows to load.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_csv(file_path, nrows=nrows)

        # Convert the timestamp column from milliseconds to UTC datetime
        df["timestamp"] = pd.to_datetime(df["timestamp"], unit="ms", utc=True)
        df = df.set_index("timestamp")
        df = df.rename(columns={"qty": "size"})

        df["instrument_id"] = df["symbol"] + ".BINANCE"
        df["action"] = df.apply(cls.map_actions, axis=1)
        df["side"] = df["side"].apply(cls.map_sides)
        df["order_id"] = 0  # No order ID for level 2 data
        df["flags"] = df.apply(cls.map_flags, axis=1)
        df["sequence"] = df["last_update_id"]

        # Drop now redundant columns
        df = df.drop(columns=["symbol", "update_type", "first_update_id", "last_update_id"])

        # Reorder columns
        columns = [
            "instrument_id",
            "action",
            "side",
            "price",
            "size",
            "order_id",
            "flags",
            "sequence",
        ]
        df = df[columns]
        assert isinstance(df, pd.DataFrame)

        return df

    @classmethod
    def map_actions(cls, row: pd.Series) -> str:
        if row["update_type"] == "snap":
            return "ADD"
        elif row["size"] == 0:
            return "DELETE"
        else:
            return "UPDATE"

    @classmethod
    def map_sides(cls, side: str) -> str:
        side = side.lower()
        if side == "b":
            return "BUY"
        elif side == "a":
            return "SELL"
        else:
            raise RuntimeError(f"unrecognized side '{side}'")

    @classmethod
    def map_flags(cls, row: pd.Series) -> int:
        if row.update_type == "snap":
            return RecordFlag.F_SNAPSHOT.value
        else:
            return 0
