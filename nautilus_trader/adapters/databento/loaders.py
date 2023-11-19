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

from os import PathLike
from pathlib import Path

import databento
import msgspec

from nautilus_trader.adapters.databento.common import check_file_path
from nautilus_trader.adapters.databento.parsing import parse_record
from nautilus_trader.adapters.databento.types import DatabentoPublisher
from nautilus_trader.core.data import Data
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument


class DatabentoDataLoader:
    """
    Provides a data loader for Databento Binary Encoding (DBN) format data.

    Supported schemas:
     - MBO
     - MBP_1
     - MBP_10 (top-level only)
     - TBBO
     - TRADES
     - OHLCV_1S
     - OHLCV_1M
     - OHLCV_1H
     - OHLCV_1D
     - DEFINITION

    For the loader to work correctly, you must first either:
     - Load Databento instrument definitions from a DBN file using `load_instruments(...)`
     - Manually add Nautilus instrument objects through `add_instruments(...)`

    References
    ----------
    https://docs.databento.com/knowledge-base/new-users/dbn-encoding

    """

    def __init__(self) -> None:
        self._publishers: dict[int, DatabentoPublisher] = {}
        self._instruments: dict[InstrumentId, Instrument] = {}

        self.load_publishers(path=Path(__file__).resolve().parent / "publishers.json")

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
        return self._publishers.copy()

    def instruments(self) -> dict[InstrumentId, Instrument]:
        """
        Return the internal Nautilus instruments currently held by the loader.

        Returns
        -------
        dict[InstrumentId, Instrument]

        Notes
        -----
        Returns a copy of the internal dictionary.

        """
        return self._instruments.copy()

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

    def load_instruments(self, path: PathLike[str] | str) -> None:
        """
        Load instrument definitions from the DBN file at the given path.

        Parameters
        ----------
        path : PathLike[str] | str
            The path for the instruments data to load.

        """
        path = Path(path)
        check_file_path(path)

        # TODO: Validate actually definitions schema
        instruments = self.from_dbn(path)

        self._instruments = {i.id: i for i in instruments}

    def add_instruments(self, instrument: Instrument | list[Instrument]) -> None:
        """
        Add the given `instrument`(s) for use by the loader.

        Parameters
        ----------
        instrument : Instrument | list[Instrument]
            The Nautilus instrument(s) to add.

        Warnings
        --------
        Will overwrite any existing instrument(s) with the same Nautilus instrument ID(s).

        """
        if not isinstance(instrument, list):
            instruments = [instrument]
        else:
            instruments = instrument

        for inst in instruments:
            self._instruments[inst.id] = inst

    def from_dbn(self, path: PathLike[str] | str) -> list[Data]:
        """
        Return a list of Nautilus objects from the DBN file at the given `path`.

        Parameters
        ----------
        path : PathLike[str] | str
            The path for the data.

        Returns
        -------
        list[Data]

        Raises
        ------
        FileNotFoundError
            If a non-existent file is specified.
        ValueError
            If an empty file is specified.

        """
        store = databento.from_dbn(path)
        instrument_map = databento.common.symbology.InstrumentMap()
        instrument_map.insert_metadata(metadata=store.metadata)

        output: list[Data] = []

        for record in store:
            data = parse_record(record, instrument_map, self._publishers)
            if isinstance(data, tuple):
                output.extend(data)
            else:
                output.append(data)

        return output
