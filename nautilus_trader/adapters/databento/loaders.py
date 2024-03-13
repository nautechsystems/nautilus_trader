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

from nautilus_trader.adapters.databento.constants import PUBLISHERS_PATH
from nautilus_trader.adapters.databento.enums import DatabentoSchema
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.data import Data
from nautilus_trader.core.nautilus_pyo3 import drop_cvec_pycapsule
from nautilus_trader.model.data import capsule_to_list
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import instruments_from_pyo3


class DatabentoDataLoader:
    """
    Provides a data loader for Databento Binary Encoding (DBN) format data.

    Supported schemas:
     - MBO -> `OrderBookDelta`
     - MBP_1 -> `QuoteTick` + `TradeTick`
     - MBP_10 -> `OrderBookDepth10`
     - TBBO -> `QuoteTick` + `TradeTick`
     - TRADES -> `TradeTick`
     - OHLCV_1S -> `Bar`
     - OHLCV_1M -> `Bar`
     - OHLCV_1H -> `Bar`
     - OHLCV_1D -> `Bar`
     - DEFINITION -> `Instrument`
     - IMBALANCE -> `DatabentoImbalance`
     - STATISTICS -> `DatabentoStatistics`

    References
    ----------
    https://docs.databento.com/knowledge-base/new-users/dbn-encoding

    """

    def __init__(self) -> None:
        self._pyo3_loader: nautilus_pyo3.DatabentoDataLoader = nautilus_pyo3.DatabentoDataLoader(
            str(PUBLISHERS_PATH),
        )

    def load_publishers(self, path: PathLike[str] | str) -> None:
        """
        Load publisher details from the JSON file at the given path.

        Parameters
        ----------
        path : PathLike[str] | str
            The path for the publishers data to load.

        """
        self._pyo3_loader.load_publishers(str(path))

    def get_publishers(self) -> dict[int, nautilus_pyo3.DatabentoPublisher]:
        """
        Return the internal Databento publishers currently held by the loader.

        Returns
        -------
        dict[int, DatabentoPublisher]

        """
        return self._pyo3_loader.get_publishers()

    def get_dataset_for_venue(self, venue: Venue) -> str:
        """
        Return a dataset for the given `venue`.

        Parameters
        ----------
        venue : Venue
            The venue for the given dataset.

        Returns
        -------
        str

        Raises
        ------
        ValueError
            If `venue` is not in the map of publishers.

        """
        dataset = self._pyo3_loader.get_dataset_for_venue(nautilus_pyo3.Venue(venue.value))
        if dataset is None:
            raise ValueError(f"No Databento dataset for venue '{venue}'")

        return dataset

    def from_dbn_file(  # noqa: C901 (too complex)
        self,
        path: PathLike[str] | str,
        instrument_id: InstrumentId | None = None,
        as_legacy_cython: bool = True,
        include_trades: bool = False,
    ) -> list[Data]:
        """
        Return a list of data objects decoded from the DBN file at the given `path`.

        Parameters
        ----------
        path : PathLike[str] | str
            The path for the DBN data file.
        instrument_id : InstrumentId, optional
            The Nautilus instrument ID for the data. This is a parameter to optimize performance,
            as all records will have their symbology overridden with the given Nautilus identifier.
            This option should only be used if the instrument ID is definitely know (for instance
            if all records in a file are guarantted to be for the same instrument).
        as_legacy_cython : bool, True
            If data should be converted to 'legacy Cython' objects.
            You would typically only set this False if passing the objects
            directly to a data catalog for the data to then be written in Nautilus Parquet format.
            Note: the `imbalance` and `statistics` schemas are only implemented in Rust, and
            so cannot be loaded as legacy Cython objects (so set this to False).
        include_trades : bool, False
            If separate `TradeTick` elements will be included in the data for MBO and MBP-1 schemas
            when applicable (your code will have to handle these two types in the returned list).

        Returns
        -------
        list[Data] | list[pyo3.DatabentoImbalance] | list[pyo3.DatabentoStatistics]

        Raises
        ------
        ValueError
            If there is an error during decoding.
        ValueError
            If `as_legacy_cython` is True when schema is `imbalance` or `statistics`.
        RuntimeError
            If a feature is not currently supported.

        """
        if isinstance(path, Path):
            path = str(path.resolve())

        pyo3_instrument_id: nautilus_pyo3.InstrumentId | None = (
            nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
            if instrument_id is not None
            else None
        )

        schema = self._pyo3_loader.schema_for_file(str(path))
        if schema is None:
            raise RuntimeError("Loading files with mixed schemas not currently supported")

        match schema:
            case DatabentoSchema.DEFINITION.value:
                data = self._pyo3_loader.load_instruments(str(path))
                if as_legacy_cython:
                    data = instruments_from_pyo3(data)
                return data
            case DatabentoSchema.MBO.value:
                if as_legacy_cython:
                    capsule = self._pyo3_loader.load_order_book_deltas_as_pycapsule(
                        path=str(path),
                        instrument_id=pyo3_instrument_id,
                        include_trades=include_trades,
                    )
                    data = capsule_to_list(capsule)
                    # Drop encapsulated `CVec` as data is now transferred
                    drop_cvec_pycapsule(capsule)
                    return data
                else:
                    return self._pyo3_loader.load_order_book_deltas(
                        path=str(path),
                        instrument_id=pyo3_instrument_id,
                        include_trades=include_trades,
                    )
            case DatabentoSchema.MBP_1.value | DatabentoSchema.TBBO.value:
                if as_legacy_cython:
                    capsule = self._pyo3_loader.load_quotes_as_pycapsule(
                        path=str(path),
                        instrument_id=pyo3_instrument_id,
                        include_trades=include_trades,
                    )
                    data = capsule_to_list(capsule)
                    # Drop encapsulated `CVec` as data is now transferred
                    drop_cvec_pycapsule(capsule)
                    return data
                else:
                    return self._pyo3_loader.load_quotes(
                        path=str(path),
                        instrument_id=pyo3_instrument_id,
                        include_trades=include_trades,
                    )
            case DatabentoSchema.MBP_10.value:
                if as_legacy_cython:
                    capsule = self._pyo3_loader.load_order_book_depth10_as_pycapsule(
                        path=str(path),
                        instrument_id=pyo3_instrument_id,
                    )
                    data = capsule_to_list(capsule)
                    # Drop encapsulated `CVec` as data is now transferred
                    drop_cvec_pycapsule(capsule)
                    return data
                else:
                    return self._pyo3_loader.load_order_book_depth10(str(path), pyo3_instrument_id)
            case DatabentoSchema.TRADES.value:
                if as_legacy_cython:
                    capsule = self._pyo3_loader.load_trades_as_pycapsule(
                        path=str(path),
                        instrument_id=pyo3_instrument_id,
                    )
                    data = capsule_to_list(capsule)
                    # Drop encapsulated `CVec` as data is now transferred
                    drop_cvec_pycapsule(capsule)
                    return data
                else:
                    return self._pyo3_loader.load_trades(str(path), pyo3_instrument_id)
            case (
                DatabentoSchema.OHLCV_1S.value
                | DatabentoSchema.OHLCV_1M.value
                | DatabentoSchema.OHLCV_1H.value
                | DatabentoSchema.OHLCV_1D.value
                | DatabentoSchema.OHLCV_EOD
            ):
                if as_legacy_cython:
                    capsule = self._pyo3_loader.load_bars_as_pycapsule(
                        path=str(path),
                        instrument_id=pyo3_instrument_id,
                    )
                    data = capsule_to_list(capsule)
                    # Drop encapsulated `CVec` as data is now transferred
                    drop_cvec_pycapsule(capsule)
                    return data
                else:
                    return self._pyo3_loader.load_bars(str(path), pyo3_instrument_id)
            case DatabentoSchema.IMBALANCE.value:
                if as_legacy_cython:
                    raise ValueError(
                        "Cannot load `DatabentoImbalance` as Cython objects, set `as_legacy_cython` to False",
                    )
                return self._pyo3_loader.load_imbalance(str(path), pyo3_instrument_id)
            case DatabentoSchema.STATISTICS.value:
                if as_legacy_cython:
                    raise ValueError(
                        "Cannot load `DatabentoStatistics` as Cython objects, set `as_legacy_cython` to False",
                    )
                return self._pyo3_loader.load_statistics(str(path), pyo3_instrument_id)
            case _:
                raise RuntimeError(f"Loading schema {schema} not currently supported")
