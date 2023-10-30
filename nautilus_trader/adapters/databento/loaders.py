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

import databento_dbn
import msgspec

from nautilus_trader.adapters.databento.common import check_file_path
from nautilus_trader.adapters.databento.types import DatabentoPublisher
from nautilus_trader.core.data import Data


class DatabentoDataLoader:
    """
    Provides a data loader for Databento format data.

    Supported encodings:
     - Databento Binary Encoding (DBN)

    Supported schemas:
     - MBO
     - MBP_1
     - MBP_10
     - TBBO
     - TRADES
     - OHLCV_1S
     - OHLCV_1M
     - OHLCV_1H
     - OHLCV_1D
     - DEFINITION

    """

    def __init__(self) -> None:
        self._publishers: dict[int, DatabentoPublisher] = {}

        self.load_publishers(path=Path(__file__).resolve().parent / "publishers.json")

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

        """
        path = Path(path)
        check_file_path(path)

        decoder = databento_dbn.DBNDecoder()

        with path.open("rb") as f:
            decoder.write(f.read())
            records = decoder.decode()

        if len(decoder.buffer()) > 0:
            raise RuntimeError("DBN file is truncated or contains an incomplete record")

        output: list[Data] = []

        for record in records:
            if isinstance(record, databento_dbn.Metadata | databento_dbn.SymbolMappingMsg):
                continue  # Unsupported schema

            data = self._parse_record(record)
            output.append(data)

        return output

    def _parse_record(self, record: databento_dbn.Record) -> Data:
        # instrument_id: InstrumentId = nautilus_instrument_id_from_databento(record.symbol)
        # if isinstance(record, databento_dbn.MBOMsg):
        #     return OrderBookDelta(
        #         instrument_id=instrument_id,
        #     )
        # else:
        #     raise ValueError(f"Schema {type(record).__name__} is currently unsupported by Nautilus")
        return None  # WIP
