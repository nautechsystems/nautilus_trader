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
import datetime

import pyarrow.dataset as ds
from _decimal import Decimal

from nautilus_trader.core.rust.model import AggressorSide
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs


class _TestPersistenceCatalog:
    def setup(self) -> None:
        pass
        # self.catalog = data_catalog_setup(protocol=self.fs_protocol)  # type: ignore
        # self.fs: fsspec.AbstractFileSystem = self.catalog.fs

    def test_list_data_types(self, betfair_catalog):
        data_types = betfair_catalog.list_data_types()
        expected = [
            "betfair_ticker",
            "betting_instrument",
            "instrument_status_update",
            "order_book_delta",
            "trade_tick",
        ]
        assert data_types == expected

    def test_list_partitions(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        tick = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price(10, 1),
            ask_price=Price(11, 1),
            bid_size=Quantity(10, 1),
            ask_size=Quantity(10, 1),
            ts_init=0,
            ts_event=0,
        )
        self.catalog.write_data([tick])

        # Act
        self.catalog.list_partitions(QuoteTick)

        # Assert
        # TODO(cs): Assert new HivePartitioning object for catalog v2

    def test_data_catalog_query_filtered(self, betfair_catalog):
        ticks = self.catalog.trade_ticks()
        assert len(ticks) == 312

        ticks = self.catalog.trade_ticks(start="2019-12-20 20:56:18")
        assert len(ticks) == 123

        ticks = self.catalog.trade_ticks(start=1576875378384999936)
        assert len(ticks) == 123

        ticks = self.catalog.trade_ticks(start=datetime.datetime(2019, 12, 20, 20, 56, 18))
        assert len(ticks) == 123

        deltas = self.catalog.order_book_deltas()
        assert len(deltas) == 2384

    def test_data_catalog_query_custom_filtered(self, betfair_catalog):
        filtered_deltas = self.catalog.order_book_deltas(where="action = 'DELETE'")
        assert len(filtered_deltas) == 351

    def test_data_catalog_instruments_df(self, betfair_catalog):
        instruments = self.catalog.instruments()
        assert len(instruments) == 2

    def test_data_catalog_instruments_filtered_df(self, betfair_catalog):
        instrument_id = self.catalog.instruments()[0].id.value
        instruments = self.catalog.instruments(instrument_ids=[instrument_id])
        assert len(instruments) == 1
        assert instruments["id"].iloc[0] == instrument_id

    def test_data_catalog_instruments_as_nautilus(self, betfair_catalog):
        instruments = self.catalog.instruments()
        assert all(isinstance(ins, BettingInstrument) for ins in instruments)

    def test_data_catalog_currency_with_null_max_price_loads(self, betfair_catalog):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        betfair_catalog.write_data([instrument])

        # Act
        instrument = self.catalog.instruments(instrument_ids=["AUD/USD.SIM"])[0]

        # Assert
        assert instrument.max_price is None

    def test_data_catalog_instrument_ids_correctly_unmapped(self, betfair_catalog):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        trade_tick = TradeTick(
            instrument_id=instrument.id,
            price=Price.from_str("2.0"),
            size=Quantity.from_int(10),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("1"),
            ts_event=0,
            ts_init=0,
        )
        betfair_catalog.write_data([instrument, trade_tick])

        # Act
        self.catalog.instruments()
        instrument = self.catalog.instruments(instrument_ids=["AUD/USD.SIM"])[0]
        trade_tick = self.catalog.trade_ticks(instrument_ids=["AUD/USD.SIM"])[0]

        # Assert
        assert instrument.id.value == "AUD/USD.SIM"
        assert trade_tick.instrument_id.value == "AUD/USD.SIM"

    def test_data_catalog_filter(self):
        # Arrange, Act
        deltas = self.catalog.order_book_deltas()
        filtered_deltas = self.catalog.order_book_deltas(filter_expr=ds.field("action") == "DELETE")

        # Assert
        assert len(deltas) == 2384
        assert len(filtered_deltas) == 351

    def test_data_catalog_generic_data(self, betfair_catalog):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        raise NotImplementedError("Needs new record batch loader")
        # process_files(
        #     glob_path=f"{TEST_DATA_DIR}/news_events.csv",
        #     reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
        #     catalog=self.catalog,
        # )

        # Act
        df = self.catalog.generic_data(cls=NewsEventData, filter_expr=ds.field("currency") == "USD")
        data = self.catalog.generic_data(
            cls=NewsEventData,
            filter_expr=ds.field("currency") == "CHF",
        )

        # Assert
        assert df is not None
        assert data is not None
        assert len(df) == 22925
        assert len(data) == 2745
        assert isinstance(data[0], GenericData)

    def test_data_catalog_bars(self, betfair_catalog):
        # Arrange
        bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
        instrument = TestInstrumentProvider.adabtc_binance()
        wrangler = BarDataWrangler(bar_type, instrument)

        def parser(data):
            data["timestamp"] = data["timestamp"].astype("datetime64[ms]")
            bars = wrangler.process(data.set_index("timestamp"))
            return bars

        # Act
        raise NotImplementedError("Needs new record batch loader")
        # reader = CSVReader(block_parser=parser, header=binance_spot_header)
        # _ = process_files(
        #     glob_path=f"{TEST_DATA_DIR}/ADABTC-1m-2021-11-*.csv",
        #     reader=reader,
        #     catalog=self.catalog,
        # )

        # Assert
        bars = self.catalog.bars()
        assert len(bars) == 21

    def test_catalog_bar_query_instrument_id(self, betfair_catalog):
        # Arrange
        bar = TestDataStubs.bar_5decimal()
        betfair_catalog.write_data([bar])

        # Act
        objs = self.catalog.bars(instrument_ids=[TestIdStubs.audusd_id().value])
        data = self.catalog.bars(instrument_ids=[TestIdStubs.audusd_id().value])

        # Assert
        assert len(objs) == 1
        assert data.shape[0] == 1
        assert "instrument_id" in data.columns

    def test_catalog_persists_equity(self, betfair_catalog):
        # Arrange
        instrument = Equity(
            instrument_id=InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ")),
            raw_symbol=Symbol("AAPL"),
            currency=USD,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            isin="US0378331005",
            ts_event=0,
            ts_init=0,
            margin_init=Decimal("0.01"),
            margin_maint=Decimal("0.005"),
            maker_fee=Decimal("0.005"),
            taker_fee=Decimal("0.01"),
        )

        quote_tick = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("2.1"),
            ask_price=Price.from_str("2.0"),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=0,
            ts_init=0,
        )

        # Act
        betfair_catalog.write_data([instrument, quote_tick])
        instrument_from_catalog = self.catalog.instruments(
            instrument_ids=[instrument.id.value],
        )[0]

        # Assert
        assert instrument.taker_fee == instrument_from_catalog.taker_fee
        assert instrument.maker_fee == instrument_from_catalog.maker_fee
        assert instrument.margin_init == instrument_from_catalog.margin_init
        assert instrument.margin_maint == instrument_from_catalog.margin_maint

    def test_data_catalog_quote_ticks_as_nautilus_use_rust_with_date_range(self):
        # Arrange
        self._load_quote_ticks_into_catalog_rust()

        start_timestamp = 1577898181000000440  # index 44
        end_timestamp = 1577898572000000953  # index 99

        # Act
        quote_ticks = self.catalog.quote_ticks(
            use_rust=True,
            instrument_ids=["EUR/USD.SIM"],
            start=start_timestamp,
            end=end_timestamp,
        )

        # Assert
        assert all(isinstance(tick, QuoteTick) for tick in quote_ticks)
        assert len(quote_ticks) == 54
        assert quote_ticks[0].ts_init == start_timestamp
        assert quote_ticks[-1].ts_init == end_timestamp

    def test_data_catalog_quote_ticks_as_nautilus_use_rust_with_date_range_with_multiple_instrument_ids(
        self,
    ):
        # Arrange
        self._load_quote_ticks_into_catalog_rust()

        start_timestamp = 1577898181000000440  # EUR/USD.SIM index 44
        end_timestamp = 1577898572000000953  # EUR/USD.SIM index 99

        # Act
        quote_ticks = self.catalog.quote_ticks(
            use_rust=True,
            instrument_ids=["EUR/USD.SIM", "USD/JPY.SIM"],
            start=start_timestamp,
            end=end_timestamp,
        )

        # Assert
        assert all(isinstance(tick, QuoteTick) for tick in quote_ticks)

        instrument1_quote_ticks = [t for t in quote_ticks if str(t.instrument_id) == "EUR/USD.SIM"]
        assert len(instrument1_quote_ticks) == 54

        instrument2_quote_ticks = [t for t in quote_ticks if str(t.instrument_id) == "USD/JPY.SIM"]
        assert len(instrument2_quote_ticks) == 54

        assert quote_ticks[0].ts_init == start_timestamp
        assert quote_ticks[-1].ts_init == end_timestamp

    def test_list_backtest_runs(self, betfair_catalog):
        # Arrange
        mock_folder = f"{betfair_catalog.path}/backtest/abc"
        betfair_catalog.fs.mkdir(mock_folder)

        # Act
        result = betfair_catalog.list_backtest_runs()

        # Assert
        assert result == ["abc"]

    def test_list_live_runs(self, betfair_catalog):
        # Arrange
        mock_folder = f"{betfair_catalog.path}/live/abc"
        betfair_catalog.fs.mkdir(mock_folder)

        # Act
        result = betfair_catalog.list_live_runs()

        # Assert
        assert result == ["abc"]


class TestPersistenceCatalogFile(_TestPersistenceCatalog):
    fs_protocol = "file"


# class TestPersistenceCatalogMemory(_TestPersistenceCatalog):
#     fs_protocol = "memory"
