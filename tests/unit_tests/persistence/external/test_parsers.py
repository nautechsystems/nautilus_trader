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


import msgspec
import pandas as pd
import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.persistence.external.core import make_raw_files
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import process_raw_file
from nautilus_trader.persistence.external.readers import ByteReader
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.external.readers import LinePreprocessor
from nautilus_trader.persistence.external.readers import TextReader
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.mocks.data import MockReader
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.data import TestInstrumentProvider
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


pytestmark = pytest.mark.skip(reason="WIP pending catalog refactor")


class TestPersistenceParsers:
    def setup(self):
        self.catalog = data_catalog_setup(protocol="memory")
        self.reader = MockReader()
        self.line_preprocessor = TestLineProcessor()

    def test_line_preprocessor_preprocess(self):
        line = b'2021-06-29T06:04:11.943000 - {"op":"mcm","id":1,"clk":"AOkiAKEMAL4P","pt":1624946651810}\n'
        line, data = self.line_preprocessor.pre_process(line=line)
        assert line == b'{"op":"mcm","id":1,"clk":"AOkiAKEMAL4P","pt":1624946651810}'
        assert data == {"ts_init": 1624946651943000000}

    def test_line_preprocessor_post_process(self):
        obj = TestDataStubs.trade_tick()
        data = {"ts_init": pd.Timestamp("2021-06-29T06:04:11.943000", tz="UTC").value}
        obj = self.line_preprocessor.post_process(obj=obj, state=data)
        assert obj.ts_init == 1624946651943000000

    def test_byte_reader_parser(self):
        def block_parser(block: bytes):
            for raw in block.split(b"\\n"):
                ts, line = raw.split(b" - ")
                state = {"ts_init": pd.Timestamp(ts.decode(), tz="UTC").value}
                line = line.strip().replace(b"b'", b"")
                msgspec.json.decode(line)
                for obj in BetfairTestStubs.parse_betfair(
                    line,
                ):
                    values = obj.to_dict(obj)
                    values["ts_init"] = state["ts_init"]
                    yield obj.from_dict(values)

        provider = BetfairInstrumentProvider.from_instruments(
            [TestInstrumentProvider.betting_instrument()],
        )
        block = BetfairDataProvider.badly_formatted_log()
        reader = ByteReader(block_parser=block_parser, instrument_provider=provider)

        data = list(reader.parse(block=block))
        result = [pd.Timestamp(d.ts_init).isoformat() for d in data]
        expected = ["2021-06-29T06:03:14.528000"]
        assert result == expected

    def test_text_reader_instrument(self):
        def parser(line):
            from decimal import Decimal

            from nautilus_trader.model.currencies import BTC
            from nautilus_trader.model.currencies import USDT
            from nautilus_trader.model.enums import AssetClass
            from nautilus_trader.model.enums import AssetType
            from nautilus_trader.model.identifiers import InstrumentId
            from nautilus_trader.model.identifiers import Symbol
            from nautilus_trader.model.identifiers import Venue
            from nautilus_trader.model.objects import Price
            from nautilus_trader.model.objects import Quantity

            assert (  # type: ignore  # noqa: F631
                Decimal,
                AssetType,
                AssetClass,
                USDT,
                BTC,
                CurrencyPair,
                InstrumentId,
                Symbol,
                Venue,
                Price,
                Quantity,
            )  # Ensure imports stay

            # Replace str repr with "fully qualified" string we can `eval`
            replacements = {
                b"id=BTCUSDT.BINANCE": b"instrument_id=InstrumentId(Symbol('BTCUSDT'), venue=Venue('BINANCE'))",
                b"native_symbol=BTCUSDT": b"native_symbol=Symbol('BTCUSDT')",
                b"price_increment=0.01": b"price_increment=Price.from_str('0.01')",
                b"size_increment=0.000001": b"size_increment=Quantity.from_str('0.000001')",
                b"margin_init=0": b"margin_init=Decimal(0)",
                b"margin_maint=0": b"margin_maint=Decimal(0)",
                b"maker_fee=0.001": b"maker_fee=Decimal(0.001)",
                b"taker_fee=0.001": b"taker_fee=Decimal(0.001)",
            }
            for k, v in replacements.items():
                line = line.replace(k, v)

            yield eval(line)  # noqa

        reader = TextReader(line_parser=parser)
        raw_file = make_raw_files(glob_path=f"{TEST_DATA_DIR}/binance-btcusdt-instrument.txt")[0]
        result = process_raw_file(catalog=self.catalog, raw_file=raw_file, reader=reader)
        expected = 1
        assert result == expected

    def test_csv_reader_dataframe(self):
        def parser(data):
            if data is None:
                return
            data.loc[:, "timestamp"] = pd.to_datetime(data["timestamp"])
            instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
            wrangler = QuoteTickDataWrangler(instrument)
            ticks = wrangler.process(data.set_index("timestamp"))
            yield from ticks

        reader = CSVReader(block_parser=parser, as_dataframe=True)
        raw_file = make_raw_files(glob_path=f"{TEST_DATA_DIR}/truefx-audusd-ticks.csv")[0]
        result = process_raw_file(catalog=self.catalog, raw_file=raw_file, reader=reader)
        assert result == 100000

    def test_csv_reader_headerless_dataframe(self):
        bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
        instrument = TestInstrumentProvider.adabtc_binance()
        wrangler = BarDataWrangler(bar_type, instrument)

        def parser(data):
            data["timestamp"] = data["timestamp"].astype("datetime64[ms]")
            bars = wrangler.process(data.set_index("timestamp"))
            return bars

        binance_spot_header = [
            "timestamp",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "ts_close",
            "quote_volume",
            "n_trades",
            "taker_buy_base_volume",
            "taker_buy_quote_volume",
            "ignore",
        ]
        reader = CSVReader(block_parser=parser, header=binance_spot_header)
        in_ = process_files(
            glob_path=f"{TEST_DATA_DIR}/ADABTC-1m-2021-11-*.csv",
            reader=reader,
            catalog=self.catalog,
        )
        assert sum(in_.values()) == 21

    def test_csv_reader_dataframe_separator(self):
        bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
        instrument = TestInstrumentProvider.adabtc_binance()
        wrangler = BarDataWrangler(bar_type, instrument)

        def parser(data):
            data["timestamp"] = data["timestamp"].astype("datetime64[ms]")
            bars = wrangler.process(data.set_index("timestamp"))
            return bars

        binance_spot_header = [
            "timestamp",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "ts_close",
            "quote_volume",
            "n_trades",
            "taker_buy_base_volume",
            "taker_buy_quote_volume",
            "ignore",
        ]
        reader = CSVReader(block_parser=parser, header=binance_spot_header, separator="|")
        in_ = process_files(
            glob_path=f"{TEST_DATA_DIR}/ADABTC_pipe_separated-1m-2021-11-*.csv",
            reader=reader,
            catalog=self.catalog,
        )
        assert sum(in_.values()) == 10

    def test_text_reader(self) -> None:
        provider = BetfairInstrumentProvider.from_instruments([])
        reader: TextReader = BetfairTestStubs.betfair_reader(provider)
        raw_file = make_raw_files(glob_path=f"{TEST_DATA_DIR}/betfair/1.166811431.bz2")[0]
        result = process_raw_file(catalog=self.catalog, raw_file=raw_file, reader=reader)
        assert result == 22692

    def test_byte_json_parser(self):
        def parser(block):
            for data in msgspec.json.decode(block):
                obj = CurrencyPair.from_dict(data)
                yield obj

        reader = ByteReader(block_parser=parser)
        raw_file = make_raw_files(glob_path=f"{TEST_DATA_DIR}/crypto*.json")[0]
        result = process_raw_file(catalog=self.catalog, raw_file=raw_file, reader=reader)
        assert result == 6

    # def test_parquet_reader(self):
    #     def parser(data):
    #         if data is None:
    #             return
    #         data.loc[:, "timestamp"] = pd.to_datetime(data.index)
    #         data = data.set_index("timestamp")[["bid", "ask", "bid_size", "ask_size"]]
    #         instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    #         wrangler = QuoteTickDataWrangler(instrument)
    #         ticks = wrangler.process(data)
    #         yield from ticks
    #
    #     reader = ParquetReader(parser=parser)
    #     raw_file = make_raw_files(glob_path=f"{TEST_DATA_DIR}/quote_tick_data.parquet")[0]
    #     result = process_raw_file(catalog=self.catalog, raw_file=raw_file, reader=reader)
    #     assert result == 9500


class TestLineProcessor(LinePreprocessor):
    @staticmethod
    def pre_process(line):
        ts, raw = line.split(b" - ")
        data = {"ts_init": pd.Timestamp(ts.decode(), tz="UTC").value}
        line = raw.strip()
        return line, data

    @staticmethod
    def post_process(obj, state):
        values = obj.to_dict(obj)
        values["ts_init"] = state["ts_init"]
        return obj.from_dict(values)
