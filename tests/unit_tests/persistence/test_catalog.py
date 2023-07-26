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
import fsspec
import pyarrow.dataset as ds

from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.providers import TestInstrumentProvider


class _TestPersistenceCatalog:
    def setup(self) -> None:
        self.catalog = data_catalog_setup(protocol=self.fs_protocol)  # type: ignore
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs

    def teardown(self):
        # Cleanup
        path = self.catalog.path
        fs = self.catalog.fs
        if fs.exists(path):
            fs.rm(path, recursive=True)

    def test_list_data_types(self, data_catalog, load_betfair_data):
        data_types = data_catalog.list_data_types()

        expected = [
            "betfair_ticker",
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
            bid=Price(10, 1),
            ask=Price(11, 1),
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

    def test_data_catalog_query_filtered(self, load_betfair_data):
        # ticks = self.catalog.trade_ticks()
        # assert len(ticks) == 312
        #
        # ticks = self.catalog.trade_ticks(start="2019-12-20 20:56:18")
        # assert len(ticks) == 123
        #
        # ticks = self.catalog.trade_ticks(start=1576875378384999936)
        # assert len(ticks) == 123
        #
        # ticks = self.catalog.trade_ticks(start=datetime.datetime(2019, 12, 20, 20, 56, 18))
        # assert len(ticks) == 123

        deltas = self.catalog.order_book_deltas()
        assert len(deltas) == 2384

        filtered_deltas = self.catalog.order_book_deltas(filter_expr=ds.field("action") == "DELETE")
        assert len(filtered_deltas) == 351

    # def test_data_catalog_trade_ticks_as_nautilus(self):
    #     trade_ticks = self.catalog.trade_ticks(as_nautilus=True)
    #     assert all(isinstance(tick, TradeTick) for tick in trade_ticks)
    #     assert len(trade_ticks) == 312
    #
    # def test_data_catalog_instruments_df(self):
    #     instruments = self.catalog.instruments()
    #     assert len(instruments) == 2
    #
    # def test_writing_instruments_doesnt_overwrite(self, data_catalog):
    #     instruments = self.catalog.instruments(as_nautilus=True)
    #     data_catalog.write_data([instruments[0]])
    #     data_catalog.write_data([instruments[1]])
    #     instruments = self.catalog.instruments(as_nautilus=True)
    #     assert len(instruments) == 2
    #
    # def test_writing_instruments_overwrite(self, data_catalog):
    #     instruments = self.catalog.instruments(as_nautilus=True)
    #     data_catalog.write_data(
    #         catalog=self.catalog,
    #         chunk=[instruments[0]],
    #         merge_existing_data=False,
    #     )
    #     data_catalog.write_data(
    #         catalog=self.catalog,
    #         chunk=[instruments[1]],
    #         merge_existing_data=False,
    #     )
    #     instruments = self.catalog.instruments(as_nautilus=True)
    #     assert len(instruments) == 1
    #
    # def test_data_catalog_instruments_filtered_df(self):
    #     instrument_id = self.catalog.instruments(as_nautilus=True)[0].id.value
    #     instruments = self.catalog.instruments(instrument_ids=[instrument_id])
    #     assert len(instruments) == 1
    #     assert instruments["id"].iloc[0] == instrument_id
    #
    # def test_data_catalog_instruments_as_nautilus(self):
    #     instruments = self.catalog.instruments(as_nautilus=True)
    #     assert all(isinstance(ins, BettingInstrument) for ins in instruments)
    #
    # def test_data_catalog_currency_with_null_max_price_loads(self, data_catalog):
    #     # Arrange
    #     instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
    #     data_catalog.write_data([instrument])
    #
    #     # Act
    #     instrument = self.catalog.instruments(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]
    #
    #     # Assert
    #     assert instrument.max_price is None
    #
    # def test_data_catalog_instrument_ids_correctly_unmapped(self, data_catalog):
    #     # Arrange
    #     instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
    #     trade_tick = TradeTick(
    #         instrument_id=instrument.id,
    #         price=Price.from_str("2.0"),
    #         size=Quantity.from_int(10),
    #         aggressor_side=AggressorSide.NO_AGGRESSOR,
    #         trade_id=TradeId("1"),
    #         ts_event=0,
    #         ts_init=0,
    #     )
    #     data_catalog.write_data([instrument, trade_tick])
    #
    #     # Act
    #     self.catalog.instruments()
    #     instrument = self.catalog.instruments(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]
    #     trade_tick = self.catalog.trade_ticks(instrument_ids=["AUD/USD.SIM"], as_nautilus=True)[0]
    #
    #     # Assert
    #     assert instrument.id.value == "AUD/USD.SIM"
    #     assert trade_tick.instrument_id.value == "AUD/USD.SIM"
    #
    # def test_data_catalog_filter(self):
    #     # Arrange, Act
    #     deltas = self.catalog.order_book_deltas()
    #     filtered_deltas = self.catalog.order_book_deltas(filter_expr=ds.field("action") == "DELETE")
    #
    #     # Assert
    #     assert len(deltas) == 2384
    #     assert len(filtered_deltas) == 351
    #
    # def test_data_catalog_generic_data(self, data_catalog):
    #     # Arrange
    #     TestPersistenceStubs.setup_news_event_persistence()
    #     raise NotImplementedError("Needs new record batch loader")
    #     # process_files(
    #     #     glob_path=f"{TEST_DATA_DIR}/news_events.csv",
    #     #     reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
    #     #     catalog=self.catalog,
    #     # )
    #
    #     # Act
    #     df = self.catalog.generic_data(cls=NewsEventData, filter_expr=ds.field("currency") == "USD")
    #     data = self.catalog.generic_data(
    #         cls=NewsEventData,
    #         filter_expr=ds.field("currency") == "CHF",
    #         as_nautilus=True,
    #     )
    #
    #     # Assert
    #     assert df is not None
    #     assert data is not None
    #     assert len(df) == 22925
    #     assert len(data) == 2745
    #     assert isinstance(data[0], GenericData)
    #
    # def test_data_catalog_bars(self, data_catalog):
    #     # Arrange
    #     bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
    #     instrument = TestInstrumentProvider.adabtc_binance()
    #     wrangler = BarDataWrangler(bar_type, instrument)
    #
    #     def parser(data):
    #         data["timestamp"] = data["timestamp"].astype("datetime64[ms]")
    #         bars = wrangler.process(data.set_index("timestamp"))
    #         return bars
    #
    #     # Act
    #     raise NotImplementedError("Needs new record batch loader")
    #     # reader = CSVReader(block_parser=parser, header=binance_spot_header)
    #     # _ = process_files(
    #     #     glob_path=f"{TEST_DATA_DIR}/ADABTC-1m-2021-11-*.csv",
    #     #     reader=reader,
    #     #     catalog=self.catalog,
    #     # )
    #
    #     # Assert
    #     bars = self.catalog.bars()
    #     assert len(bars) == 21
    #
    # def test_catalog_bar_query_instrument_id(self, data_catalog):
    #     # Arrange
    #     bar = TestDataStubs.bar_5decimal()
    #     data_catalog.write_data([bar])
    #
    #     # Act
    #     objs = self.catalog.bars(instrument_ids=[TestIdStubs.audusd_id().value], as_nautilus=True)
    #     data = self.catalog.bars(instrument_ids=[TestIdStubs.audusd_id().value])
    #
    #     # Assert
    #     assert len(objs) == 1
    #     assert data.shape[0] == 1
    #     assert "instrument_id" in data.columns
    #
    # def test_catalog_projections(self):
    #     projections = {"tid": ds.field("trade_id")}
    #     trades = self.catalog.trade_ticks(projections=projections)
    #     assert "tid" in trades.columns
    #     assert trades["trade_id"].equals(trades["tid"])
    #
    # def test_catalog_persists_equity(self, data_catalog):
    #     # Arrange
    #     instrument = Equity(
    #         instrument_id=InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ")),
    #         raw_symbol=Symbol("AAPL"),
    #         currency=USD,
    #         price_precision=2,
    #         price_increment=Price.from_str("0.01"),
    #         multiplier=Quantity.from_int(1),
    #         lot_size=Quantity.from_int(1),
    #         isin="US0378331005",
    #         ts_event=0,
    #         ts_init=0,
    #         margin_init=Decimal("0.01"),
    #         margin_maint=Decimal("0.005"),
    #         maker_fee=Decimal("0.005"),
    #         taker_fee=Decimal("0.01"),
    #     )
    #
    #     quote_tick = QuoteTick(
    #         instrument_id=instrument.id,
    #         ask=Price.from_str("2.0"),
    #         bid=Price.from_str("2.1"),
    #         bid_size=Quantity.from_int(10),
    #         ask_size=Quantity.from_int(10),
    #         ts_event=0,
    #         ts_init=0,
    #     )
    #
    #     # Act
    #     data_catalog.write_data([instrument, quote_tick])
    #     instrument_from_catalog = self.catalog.instruments(
    #         as_nautilus=True,
    #         instrument_ids=[instrument.id.value],
    #     )[0]
    #
    #     # Assert
    #     assert instrument.taker_fee == instrument_from_catalog.taker_fee
    #     assert instrument.maker_fee == instrument_from_catalog.maker_fee
    #     assert instrument.margin_init == instrument_from_catalog.margin_init
    #     assert instrument.margin_maint == instrument_from_catalog.margin_maint
    #
    # def test_data_catalog_quote_ticks_as_nautilus_use_rust_with_date_range(self):
    #     # Arrange
    #     self._load_quote_ticks_into_catalog_rust()
    #
    #     start_timestamp = 1577898181000000440  # index 44
    #     end_timestamp = 1577898572000000953  # index 99
    #
    #     # Act
    #     quote_ticks = self.catalog.quote_ticks(
    #         as_nautilus=True,
    #         use_rust=True,
    #         instrument_ids=["EUR/USD.SIM"],
    #         start=start_timestamp,
    #         end=end_timestamp,
    #     )
    #
    #     # Assert
    #     assert all(isinstance(tick, QuoteTick) for tick in quote_ticks)
    #     assert len(quote_ticks) == 54
    #     assert quote_ticks[0].ts_init == start_timestamp
    #     assert quote_ticks[-1].ts_init == end_timestamp
    #
    # def test_data_catalog_quote_ticks_as_nautilus_use_rust_with_date_range_with_multiple_instrument_ids(
    #     self,
    # ):
    #     # Arrange
    #     self._load_quote_ticks_into_catalog_rust()
    #
    #     start_timestamp = 1577898181000000440  # EUR/USD.SIM index 44
    #     end_timestamp = 1577898572000000953  # EUR/USD.SIM index 99
    #
    #     # Act
    #     quote_ticks = self.catalog.quote_ticks(
    #         as_nautilus=True,
    #         use_rust=True,
    #         instrument_ids=["EUR/USD.SIM", "USD/JPY.SIM"],
    #         start=start_timestamp,
    #         end=end_timestamp,
    #     )
    #
    #     # Assert
    #     assert all(isinstance(tick, QuoteTick) for tick in quote_ticks)
    #
    #     instrument1_quote_ticks = [t for t in quote_ticks if str(t.instrument_id) == "EUR/USD.SIM"]
    #     assert len(instrument1_quote_ticks) == 54
    #
    #     instrument2_quote_ticks = [t for t in quote_ticks if str(t.instrument_id) == "USD/JPY.SIM"]
    #     assert len(instrument2_quote_ticks) == 54
    #
    #     assert quote_ticks[0].ts_init == start_timestamp
    #     assert quote_ticks[-1].ts_init == end_timestamp


class TestPersistenceCatalogFile(_TestPersistenceCatalog):
    fs_protocol = "file"


class TestPersistenceCatalogMemory(_TestPersistenceCatalog):
    fs_protocol = "memory"
