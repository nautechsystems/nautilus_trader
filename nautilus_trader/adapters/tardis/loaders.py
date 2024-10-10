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

from os import PathLike
from pathlib import Path

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import drop_cvec_pycapsule
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_list


class TardisCSVDataLoader:
    """
    Provides a means of loading data from CSV files in Tardis format.
    """

    def __init__(self, price_precision: int, size_precision: int) -> None:
        self._price_precision = price_precision
        self._size_precision = size_precision

    def load_deltas(
        self,
        filepath: PathLike[str] | str,
        as_legacy_cython: bool = True,
        limit: int | None = None,
    ) -> list[OrderBookDelta] | list[nautilus_pyo3.OrderBookDelta]:
        """
        Load order book deltas data from the given `filepath`.

        Parameters
        ----------
        filepath : PathLike[str] | str
            The path for the CSV data file (must be Tardis trades format).
        as_legacy_cython : bool, True
            If data should be converted to 'legacy Cython' objects.
            You would typically only set this False if passing the objects
            directly to a data catalog for the data to then be written in Nautilus Parquet format.
        limit : int, optional
            The limit for the number of records to read.

        Returns
        -------
        list[OrderBookDelta] | list[nautilus_pyo3.OrderBookDelta]

        """
        if isinstance(filepath, Path):
            filepath = str(filepath.resolve())

        if as_legacy_cython:
            capsule = nautilus_pyo3.load_tardis_deltas_as_pycapsule(
                filepath=str(filepath),
                price_precision=self._price_precision,
                size_precision=self._size_precision,
                limit=limit,
            )
            data = capsule_to_list(capsule)
            # Drop encapsulated `CVec` as data is now transferred
            drop_cvec_pycapsule(capsule)
            return data

        return nautilus_pyo3.load_tardis_deltas(
            filepath=str(filepath),
            price_precision=self._price_precision,
            size_precision=self._size_precision,
            limit=limit,
        )

    def load_trades(
        self,
        filepath: PathLike[str] | str,
        as_legacy_cython: bool = True,
        limit: int | None = None,
    ) -> list[TradeTick] | list[nautilus_pyo3.TradeTick]:
        """
        Load trade ticks data from the given `filepath`.

        Parameters
        ----------
        filepath : PathLike[str] | str
            The path for the CSV data file (must be Tardis trades format).
        as_legacy_cython : bool, True
            If data should be converted to 'legacy Cython' objects.
            You would typically only set this False if passing the objects
            directly to a data catalog for the data to then be written in Nautilus Parquet format.
        limit : int, optional
            The limit for the number of records to read.

        Returns
        -------
        list[TradeTick] | list[nautilus_pyo3.TradeTick]

        """
        if isinstance(filepath, Path):
            filepath = str(filepath.resolve())

        if as_legacy_cython:
            capsule = nautilus_pyo3.load_tardis_trades_as_pycapsule(
                filepath=str(filepath),
                price_precision=self._price_precision,
                size_precision=self._size_precision,
                limit=limit,
            )
            data = capsule_to_list(capsule)
            # Drop encapsulated `CVec` as data is now transferred
            drop_cvec_pycapsule(capsule)
            return data

        return nautilus_pyo3.load_tardis_trades(
            filepath=str(filepath),
            price_precision=self._price_precision,
            size_precision=self._size_precision,
            limit=limit,
        )
