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
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_list
from nautilus_trader.model.identifiers import InstrumentId


class TardisCSVDataLoader:
    """
    Provides a means of loading data from CSV files in Tardis format.

    Parameters
    ----------
    price_precision : int
        The price precision for parsing.
        Necessary as implicit precision in the text data may not be consistent.
    size_precision : int
        The size precision for parsing.
        Necessary as implicit precision in the text data may not be consistent.
    instrument_id : InstrumentId, optional
        The instrument ID to override in the data.
        This can be more efficient if the instrument is definitely know (file does not contain
        mixed instruments), or to maintain consistent symbology (such as BTCUSDT-PERP.BINANCE).

    """

    def __init__(
        self,
        price_precision: int,
        size_precision: int,
        instrument_id: InstrumentId | None = None,
    ) -> None:
        self._price_precision = price_precision
        self._size_precision = size_precision
        self._instrument_id = (
            nautilus_pyo3.InstrumentId.from_str(instrument_id.value) if instrument_id else None
        )

    def load_deltas(
        self,
        filepath: PathLike[str] | str,
        as_legacy_cython: bool = True,
        limit: int | None = None,
    ) -> list[OrderBookDelta] | list[nautilus_pyo3.OrderBookDelta]:
        """
        Load order book deltas data from the given `filepath`.

        CSV file must be Tardis incremental book L2 format.

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

        References
        ----------
        https://docs.tardis.dev/downloadable-csv-files#incremental_book_l2

        """
        if isinstance(filepath, Path):
            filepath = str(filepath.resolve())

        if as_legacy_cython:
            capsule = nautilus_pyo3.load_tardis_deltas_as_pycapsule(
                filepath=str(filepath),
                price_precision=self._price_precision,
                size_precision=self._size_precision,
                instrument_id=self._instrument_id,
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
            instrument_id=self._instrument_id,
            limit=limit,
        )

    def load_depth10(
        self,
        filepath: PathLike[str] | str,
        levels: int,
        as_legacy_cython: bool = True,
        limit: int | None = None,
    ) -> list[OrderBookDepth10] | list[nautilus_pyo3.OrderBookDepth10]:
        """
        Load order book depth snapshots from the given `filepath`.

        CSV file must be Tardis book snapshot 5 or snapshot 25 format.

        - For snapshot 5, levels beyond 5 will be filled with null orders.
        - For snapshot 25, levels beyond 10 are discarded.

        Parameters
        ----------
        filepath : PathLike[str] | str
            The path for the CSV data file (must be Tardis trades format).
        levels : int
            The number of levels in the snapshots CSV data (must be either 5 or 25).
        as_legacy_cython : bool, True
            If data should be converted to 'legacy Cython' objects.
            You would typically only set this False if passing the objects
            directly to a data catalog for the data to then be written in Nautilus Parquet format.
        limit : int, optional
            The limit for the number of records to read.

        Returns
        -------
        list[OrderBookDepth10] | list[nautilus_pyo3.OrderBookDepth10]

        Raises
        ------
        ValueError
            If `levels` is not either 5 or 25.

        References
        ----------
        https://docs.tardis.dev/downloadable-csv-files#book_snapshot_5
        https://docs.tardis.dev/downloadable-csv-files#book_snapshot_25

        """
        if isinstance(filepath, Path):
            filepath = str(filepath.resolve())

        match levels:
            case 5:
                if as_legacy_cython:
                    capsule = nautilus_pyo3.load_tardis_depth10_from_snapshot5_as_pycapsule(
                        filepath=str(filepath),
                        price_precision=self._price_precision,
                        size_precision=self._size_precision,
                        instrument_id=self._instrument_id,
                        limit=limit,
                    )
                    data = capsule_to_list(capsule)
                    # Drop encapsulated `CVec` as data is now transferred
                    drop_cvec_pycapsule(capsule)
                    return data

                return nautilus_pyo3.load_tardis_depth10_from_snapshot5(
                    filepath=str(filepath),
                    price_precision=self._price_precision,
                    size_precision=self._size_precision,
                    instrument_id=self._instrument_id,
                    limit=limit,
                )
            case 25:
                if as_legacy_cython:
                    capsule = nautilus_pyo3.load_tardis_depth10_from_snapshot25_as_pycapsule(
                        filepath=str(filepath),
                        price_precision=self._price_precision,
                        size_precision=self._size_precision,
                        instrument_id=self._instrument_id,
                        limit=limit,
                    )
                    data = capsule_to_list(capsule)
                    # Drop encapsulated `CVec` as data is now transferred
                    drop_cvec_pycapsule(capsule)
                    return data

                return nautilus_pyo3.load_tardis_depth10_from_snapshot25(
                    filepath=str(filepath),
                    price_precision=self._price_precision,
                    size_precision=self._size_precision,
                    instrument_id=self._instrument_id,
                    limit=limit,
                )
            case _:
                raise ValueError(
                    "invalid `levels`, use either 5 or 25 corresponding to number of levels in the CSV data",
                )

    def load_quotes(
        self,
        filepath: PathLike[str] | str,
        as_legacy_cython: bool = True,
        limit: int | None = None,
    ) -> list[QuoteTick] | list[nautilus_pyo3.QuoteTick]:
        """
        Load quote tick data from the given `filepath`.

        CSV file must be Tardis quotes format.

        Parameters
        ----------
        filepath : PathLike[str] | str
            The path for the CSV data file.
        as_legacy_cython : bool, True
            If data should be converted to 'legacy Cython' objects.
            You would typically only set this False if passing the objects
            directly to a data catalog for the data to then be written in Nautilus Parquet format.
        limit : int, optional
            The limit for the number of records to read.

        Returns
        -------
        list[QuoteTick] | list[nautilus_pyo3.QuoteTick]

        References
        ----------
        https://docs.tardis.dev/downloadable-csv-files#quotes

        """
        if isinstance(filepath, Path):
            filepath = str(filepath.resolve())

        if as_legacy_cython:
            capsule = nautilus_pyo3.load_tardis_quotes_as_pycapsule(
                filepath=str(filepath),
                price_precision=self._price_precision,
                size_precision=self._size_precision,
                instrument_id=self._instrument_id,
                limit=limit,
            )
            data = capsule_to_list(capsule)
            # Drop encapsulated `CVec` as data is now transferred
            drop_cvec_pycapsule(capsule)
            return data

        return nautilus_pyo3.load_tardis_quotes(
            filepath=str(filepath),
            price_precision=self._price_precision,
            size_precision=self._size_precision,
            instrument_id=self._instrument_id,
            limit=limit,
        )

    def load_trades(
        self,
        filepath: PathLike[str] | str,
        as_legacy_cython: bool = True,
        limit: int | None = None,
    ) -> list[TradeTick] | list[nautilus_pyo3.TradeTick]:
        """
        Load trade tick data from the given `filepath`.

        CSV file must be Tardis trades format.

        Parameters
        ----------
        filepath : PathLike[str] | str
            The path for the CSV data file.
        as_legacy_cython : bool, True
            If data should be converted to 'legacy Cython' objects.
            You would typically only set this False if passing the objects
            directly to a data catalog for the data to then be written in Nautilus Parquet format.
        limit : int, optional
            The limit for the number of records to read.

        Returns
        -------
        list[TradeTick] | list[nautilus_pyo3.TradeTick]

        References
        ----------
        https://docs.tardis.dev/downloadable-csv-files#trades

        """
        if isinstance(filepath, Path):
            filepath = str(filepath.resolve())

        if as_legacy_cython:
            capsule = nautilus_pyo3.load_tardis_trades_as_pycapsule(
                filepath=str(filepath),
                price_precision=self._price_precision,
                size_precision=self._size_precision,
                instrument_id=self._instrument_id,
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
            instrument_id=self._instrument_id,
            limit=limit,
        )
