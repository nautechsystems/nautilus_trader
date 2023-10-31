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
import pandas as pd
import pytz
from databento.common.symbology import InstrumentMap

from nautilus_trader.adapters.databento.common import check_file_path
from nautilus_trader.adapters.databento.common import nautilus_instrument_id_from_databento
from nautilus_trader.adapters.databento.enums import DatabentoInstrumentClass
from nautilus_trader.adapters.databento.types import DatabentoPublisher
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


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
        self._instruments: dict[InstrumentId, Instrument] = {}

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

        self._instruments = {i.instrument_id: i for i in instruments}

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
        instrument_map = InstrumentMap()
        instrument_map.insert_metadata(metadata=store.metadata)

        output: list[Data] = []

        for record in store:
            data = self._parse_record(record, instrument_map)
            output.append(data)

        return output

    def _parse_record(self, record: databento.DBNRecord, instrument_map: InstrumentMap) -> Data:
        if isinstance(record, databento.InstrumentDefMsg):
            return self._parse_instrument(record)

        record_date = pd.Timestamp(record.ts_event, tz=pytz.utc).date()
        raw_symbol = instrument_map.resolve(record.instrument_id, date=record_date)
        if raw_symbol is None:
            raise ValueError(
                f"Cannot resolve instrument_id {record.instrument_id} on {record_date}",
            )

        publisher = self._publishers[record.publisher_id]
        instrument_id: InstrumentId = nautilus_instrument_id_from_databento(
            raw_symbol=raw_symbol,
            publisher=publisher,
        )

        if isinstance(record, databento.MBOMsg):
            return self._parse_order_book_delta(record, instrument_id)
        else:
            raise ValueError(f"Schema {type(record).__name__} is currently unsupported by Nautilus")

    def _parse_order_side(self, value: str) -> OrderSide:
        match value:
            case "A":
                return OrderSide.BUY
            case "B":
                return OrderSide.SELL
            case _:
                return OrderSide.NO_ORDER_SIDE

    def _parse_book_action(self, value: str) -> BookAction:
        match value:
            case "A":
                return BookAction.ADD
            case "C":
                return BookAction.DELETE
            case "M":
                return BookAction.UPDATE
            case "R":
                return BookAction.CLEAR
            case "T":
                return BookAction.UPDATE
            case "F":
                return BookAction.UPDATE
            case _:
                raise ValueError(f"Invalid `BookAction`, was {value}")

    def _parse_instrument(self, record: databento.InstrumentDefMsg) -> Instrument:
        publisher = self._publishers[record.publisher_id]
        instrument_id: InstrumentId = nautilus_instrument_id_from_databento(
            raw_symbol=record.raw_symbol,
            publisher=publisher,
        )

        match record.instrument_class:
            case DatabentoInstrumentClass.STOCK.value:
                return self._parse_equity(record, instrument_id)
            case DatabentoInstrumentClass.FUTURE.value:
                return self._parse_futures_contract(record, instrument_id)
            case _:
                raise ValueError(f"Invalid `instrument_class`, was {record.instrument_class}")

    def _parse_equity(
        self,
        record: databento.InstrumentDefMsg,
        instrument_id: InstrumentId,
    ) -> Equity:
        return Equity(
            instrument_id=instrument_id,
            raw_symbol=Symbol(record.raw_symbol),
            currency=USD,  # All US equity venues for now
            price_precision=USD.precision,
            price_increment=Price(0.01, USD.precision),
            multiplier=Quantity(1, precision=0),  # TODO: N/A?
            lot_size=Quantity(record.min_lot_size_round_lot, precision=0),
            isin=None,  # TODO
            ts_event=record.ts_event,
            ts_init=record.ts_recv,
        )

    def _parse_futures_contract(
        self,
        record: databento.InstrumentDefMsg,
        instrument_id: InstrumentId,
    ) -> FuturesContract:
        return FuturesContract(
            instrument_id=instrument_id,
            raw_symbol=Symbol(record.raw_symbol),
            asset_class=AssetClass.EQUITY,  # TODO(proper parsing)
            currency=Currency.from_str(record.currency.upper()),
            price_precision=record.price_display_format,
            multiplier=Quantity(record.contract_multiplier, precision=0),
            lot_size=Quantity(record.min_lot_size_round_lot, precision=0),
            underlying=record.underlying,
            expiry_date=unix_nanos_to_dt(record.expiration).date(),  # TODO(should be a timestamp)
            ts_event=record.ts_event,
            ts_init=record.ts_recv,
        )

    def _parse_order_book_delta(
        self,
        record: databento.MBOMsg,
        instrument_id: InstrumentId,
    ) -> OrderBookDelta:
        return OrderBookDelta.from_raw(
            instrument_id=instrument_id,
            book_action=self._parse_book_action(record.action),
            side=self._parse_order_side(record.side),
            price_raw=record.price,
            price_prec=2,  # TODO
            size_raw=record.size,
            size_prec=0,  # TODO
            order_id=record.order_id,
            flags=record.flags,
            sequence=record.sequence,
            ts_event=record.ts_event,
            ts_init=record.ts_recv,
        )
