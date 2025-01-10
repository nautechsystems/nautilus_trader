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

from __future__ import annotations

import json
from typing import TYPE_CHECKING
from zipfile import ZipFile
from zipfile import is_zipfile

import pandas as pd

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.model.enums import RecordFlag


if TYPE_CHECKING:
    from os import PathLike


class BybitOrderBookDeltaDataLoader:
    """
    Provides a means of loading Bybit order book data.
    """

    @classmethod
    def load(
        cls,
        file_path: PathLike[str] | str,
        nrows: int | None = None,
        product_type: BybitProductType = BybitProductType.LINEAR,
    ) -> pd.DataFrame:
        """
        Return the deltas `pandas.DataFrame` loaded from the given Zip `file_path`.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the Zip file.
        nrows : int, optional
            The maximum number of rows to load.
        product_type : BybitProductType, optional
            The product type to load.

        Returns
        -------
        pd.DataFrame

        """
        assert is_zipfile(file_path), "depth_file must be zip file provided by ByBit"

        orderbook_keys = ["a", "b"]
        rows = []

        with ZipFile(file_path, "r") as zipfile:
            with zipfile.open(zipfile.namelist()[0]) as f:
                for i, row in enumerate(f):
                    if nrows is not None and i >= nrows:
                        break
                    obj = json.loads(row.strip())
                    timestamp_ns = int(float(obj["ts"]) * 1_000_000)
                    timestamp = pd.to_datetime(timestamp_ns, unit="ns", utc=True)

                    data = obj["data"]
                    instrument_id = f"{data['s']}-{product_type.value.upper()}.BYBIT"
                    update_type = obj["type"]
                    sequence = data["seq"]

                    for key in orderbook_keys:
                        if key in data:
                            if update_type == "snapshot":
                                rows.append(
                                    {
                                        "timestamp": timestamp,
                                        "instrument_id": instrument_id,
                                        "action": "CLEAR",
                                        "side": cls.map_sides(key),
                                        "order_id": 0,
                                        "flags": 0,
                                        "price": data[key][-1][0],
                                        "size": 0,
                                        "sequence": sequence,
                                    },
                                )

                            rows.extend(
                                [
                                    {
                                        "timestamp": timestamp,
                                        "instrument_id": instrument_id,
                                        "action": cls.map_actions(update_type, float(qty)),
                                        "side": cls.map_sides(key),
                                        "order_id": 0,
                                        "flags": cls.map_flags(update_type),
                                        "price": px,
                                        "size": qty,
                                        "sequence": sequence,
                                    }
                                    for px, qty in data[key]
                                ],
                            )

        df = pd.DataFrame(rows)

        df = df.set_index("timestamp")

        df = df.astype(
            {
                "size": float,
                "price": float,
                "flags": int,
            },
        )

        return df

    @classmethod
    def map_actions(cls, update_type: str, size: float) -> str:
        if update_type == "snapshot":
            return "ADD"
        elif size == 0:
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
    def map_flags(cls, update_type: str) -> int:
        if update_type == "snapshot":
            return RecordFlag.F_SNAPSHOT.value
        else:
            return 0
