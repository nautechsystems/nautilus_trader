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

import databento_dbn
import msgspec

from nautilus_trader.adapters.databento.common import check_file_path
from nautilus_trader.adapters.databento.types import DatabentoPublisher
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.data import Data
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import instruments_from_pyo3


class DatabentoDataLoader:
    """
    Provides a data loader for Databento Binary Encoding (DBN) format data.

    Supported schemas:
     - MBO -> `OrderBookDelta`
     - MBP_1 -> `QuoteTick` | `TradeTick`
     - MBP_10 -> `OrderBookDepth10`
     - TBBO -> `QuoteTick` | `TradeTick`
     - TRADES -> `TradeTick`
     - OHLCV_1S -> `Bar`
     - OHLCV_1M -> `Bar`
     - OHLCV_1H -> `Bar`
     - OHLCV_1D -> `Bar`
     - DEFINITION -> `Instrument`
     - IMBALANCE -> `DatabentoImbalance`
     - STATISTICS -> `DatabentoStatistics`

    For the loader to work correctly, you must first either:
     - Load Databento instrument definitions from a DBN file using `load_instruments(...)`
     - Manually add Nautilus instrument objects through `add_instruments(...)`

    Warnings
    --------
    The following Databento instrument classes are not currently supported:
     - ``FUTURE_SPREAD``
     - ``OPTION_SPEAD``
     - ``MIXED_SPREAD``
     - ``FX_SPOT``

    References
    ----------
    https://docs.databento.com/knowledge-base/new-users/dbn-encoding

    """

    def __init__(self) -> None:
        self._publishers: dict[int, DatabentoPublisher] = {}
        self._instruments: dict[InstrumentId, Instrument] = {}

        publishers_path = Path(__file__).resolve().parent / "publishers.json"

        self._pyo3_loader: nautilus_pyo3.DatabentoDataLoader = nautilus_pyo3.DatabentoDataLoader(
            str(publishers_path.resolve()),
        )
        self.load_publishers(path=publishers_path)

    @property
    def publishers(self) -> dict[int, DatabentoPublisher]:
        """
        Return the internal Databento publishers currently held by the loader.

        Returns
        -------
        dict[int, DatabentoPublisher]

        Notes
        -----
        Returns a copy of the internal dictionary.

        """
        return self._publishers

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
        dataset = self._venue_dataset.get(venue)
        if dataset is None:
            raise ValueError(f"No Databento dataset for venue '{venue}'")

        return dataset

    def load_publishers(self, path: PathLike[str] | str) -> None:
        """
        Load publisher details from the JSON file at the given path.

        Parameters
        ----------
        path : PathLike[str] | str
            The path for the publishers data to load.

        """
        path = Path(path)
        check_file_path(path)

        decoder = msgspec.json.Decoder(list[DatabentoPublisher])
        publishers: list[DatabentoPublisher] = decoder.decode(path.read_bytes())

        self._publishers = {p.publisher_id: p for p in publishers}
        self._venue_dataset: dict[Venue, str] = {Venue(p.venue): p.dataset for p in publishers}

    def load_from_file_pyo3(
        self,
        path: PathLike[str] | str,
        instrument_id: InstrumentId | None = None,
        as_legacy_cython: bool = False,
    ) -> list[Data]:
        """
        Return a list of pyo3 data objects decoded from the DBN file at the given
        `path`.

        Parameters
        ----------
        path : PathLike[str] | str
            The path for the DBN data file.
        instrument_id : InstrumentId, optional
            The Nautilus instrument ID for the data. This is a parameter to optimize performance,
            as all records will have their symbology overridden with the given Nautilus identifier.
            This option should only be used if the instrument ID is definitely know (for instance
            if all records in a file are guarantted to be for the same instrument).
        as_legacy_cython : bool, False
            If data should be converted to 'legacy Cython' objects.

        Returns
        -------
        list[Data]

        Raises
        ------
        ValueError
            If there is an error during decoding.
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

        schema = self._pyo3_loader.schema_for_file(path)  # type: ignore
        if schema is None:
            raise RuntimeError("Loading files with mixed schemas not currently supported")

        match schema:
            case databento_dbn.Schema.DEFINITION:
                data = self._pyo3_loader.load_instruments(path)  # type: ignore
                if as_legacy_cython:
                    data = instruments_from_pyo3(data)
                return data
            case databento_dbn.Schema.MBO:
                data = self._pyo3_loader.load_order_book_deltas(path, pyo3_instrument_id)  # type: ignore
                if as_legacy_cython:
                    data = OrderBookDelta.from_pyo3_list(data)
                return data
            case databento_dbn.Schema.MBP_1 | databento_dbn.Schema.TBBO:
                data = self._pyo3_loader.load_quote_ticks(path, pyo3_instrument_id)  # type: ignore
                if as_legacy_cython:
                    data = QuoteTick.from_pyo3_list(data)
                return data
            case databento_dbn.Schema.MBP_10:
                data = self._pyo3_loader.load_order_book_depth10(path)  # type: ignore
                if as_legacy_cython:
                    data = OrderBookDepth10.from_pyo3_list(data)
                return data
            case databento_dbn.Schema.TRADES:
                data = self._pyo3_loader.load_trade_ticks(path, pyo3_instrument_id)  # type: ignore
                if as_legacy_cython:
                    data = TradeTick.from_pyo3_list(data)
                return data
            case (
                databento_dbn.Schema.OHLCV_1S
                | databento_dbn.Schema.OHLCV_1M
                | databento_dbn.Schema.OHLCV_1H
                | databento_dbn.Schema.OHLCV_1D
                | databento_dbn.Schema.OHLCV_EOD
            ):
                data = self._pyo3_loader.load_bars(path, pyo3_instrument_id)  # type: ignore
                if as_legacy_cython:
                    data = Bar.from_pyo3_list(data)
                return data
            case _:
                raise RuntimeError(f"Loading schema {schema} not currently supported")
