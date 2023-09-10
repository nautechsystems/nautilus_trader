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
import sys

import fsspec
import pyarrow.dataset as ds
import pytest
from _decimal import Decimal

from nautilus_trader.core.rust.model import AggressorSide
from nautilus_trader.core.rust.model import BookAction
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
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs


class TestPersistenceCatalog:
    fs_protocol = "file"

    def setup(self) -> None:
        self.catalog = data_catalog_setup(protocol=self.fs_protocol)
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs

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
        filtered_deltas = self.catalog.order_book_deltas(
            where=f"action = '{BookAction.DELETE.value}'",
        )
        assert len(filtered_deltas) == 351

    def test_data_catalog_instruments_df(self, betfair_catalog):
        instruments = self.catalog.instruments()
        assert len(instruments) == 2

    def test_data_catalog_instruments_filtered_df(self, betfair_catalog):
        instrument_id = self.catalog.instruments()[0].id.value
        instruments = self.catalog.instruments(instrument_ids=[instrument_id])
        assert len(instruments) == 1
        assert all(isinstance(ins, BettingInstrument) for ins in instruments)
        assert instruments[0].id.value == instrument_id

    @pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
    def test_data_catalog_currency_with_null_max_price_loads(self, betfair_catalog):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
        betfair_catalog.write_data([instrument])

        # Act
        instrument = self.catalog.instruments(instrument_ids=["AUD/USD.SIM"])[0]

        # Assert
        assert instrument.max_price is None

    @pytest.mark.skip(
        reason="pyo3_runtime.PanicException: Failed new_query with error Object Store error",
    )
    def test_data_catalog_instrument_ids_correctly_unmapped(self):
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
        self.catalog.write_data([instrument, trade_tick])

        # Act
        self.catalog.instruments()
        instrument = self.catalog.instruments(instrument_ids=["AUD/USD.SIM"])[0]
        trade_tick = self.catalog.trade_ticks(instrument_ids=["AUD/USD.SIM"])[0]

        # Assert
        assert instrument.id.value == "AUD/USD.SIM"
        assert trade_tick.instrument_id.value == "AUD/USD.SIM"

    def test_data_catalog_filter(self, betfair_catalog):
        # Arrange, Act
        deltas = self.catalog.order_book_deltas()
        filtered_deltas = self.catalog.order_book_deltas(
            where=f"Action = {BookAction.DELETE.value}",
        )

        # Assert
        assert len(deltas) == 2384
        assert len(filtered_deltas) == 351

    def test_data_catalog_generic_data(self, betfair_catalog):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        data = TestPersistenceStubs.news_events()
        self.catalog.write_data(data)

        # Act
        df = self.catalog.generic_data(cls=NewsEventData, filter_expr=ds.field("currency") == "USD")
        data = self.catalog.generic_data(
            cls=NewsEventData,
            filter_expr=ds.field("currency") == "CHF",
        )

        # Assert
        assert df is not None
        assert data is not None
        assert len(df) == 22941
        assert len(data) == 2745
        assert isinstance(data[0], GenericData)

    @pytest.mark.skip(reason="datafusion bar query not working")
    def test_data_catalog_bars(self):
        # Arrange
        bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
        instrument = TestInstrumentProvider.adabtc_binance()
        stub_bars = TestDataStubs.binance_bars_from_csv(
            "ADABTC-1m-2021-11-27.csv",
            bar_type,
            instrument,
        )

        # Act
        self.catalog.write_data(stub_bars)

        # Assert
        bars = self.catalog.bars(bar_types=[str(bar_type)])
        all_bars = self.catalog.bars()
        assert len(all_bars) == 10
        assert len(bars) == len(stub_bars) == 10

    @pytest.mark.skip(reason="datafusion bar query not working")
    def test_catalog_bar_query_instrument_id(self, betfair_catalog):
        # Arrange
        bar = TestDataStubs.bar_5decimal()
        betfair_catalog.write_data([bar])

        # Act
        data = self.catalog.bars(bar_types=[str(bar.bar_type)])

        # Assert
        assert len(data) == 1

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
