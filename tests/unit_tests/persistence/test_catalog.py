import datetime
import sys
from decimal import Decimal

import pyarrow.dataset as ds
import pytest

from examples.strategies.orderbook_imbalance import OrderbookImbalance
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import RawFile
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import data_catalog_setup
from tests.test_kit.providers import TestInstrumentProvider


TEST_DATA_DIR = PACKAGE_ROOT + "/data"


@pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")
class TestPersistenceCatalog:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()

    def test_data_catalog_instruments_df(self):
        instruments = self.catalog.instruments()
        assert len(instruments) == 2

    def test_data_catalog_instruments_filtered_df(self):
        instrument_id = (
            "Basketball,,29628709,20191221-001000,ODDS,MATCH_ODDS,1.166564490,237491,0.0.BETFAIR"
        )
        instruments = self.catalog.instruments(instrument_ids=[instrument_id])
        assert len(instruments) == 1
        assert instruments["instrument_id"].iloc[0] == instrument_id

    def test_data_catalog_instruments_as_nautilus(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        assert all(isinstance(ins, BettingInstrument) for ins in instruments)

    def test_partition_key_correctly_remapped(self):
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
        assert tick
        rf = RawFile(self.catalog.fs, path="/")
        rf.process()

        df = self.catalog.quote_ticks()
        assert len(df) == 1
        # Ensure we "unmap" the keys that we write the partition filenames as;
        # this instrument_id should be AUD/USD not AUD-USD
        assert df.iloc[0]["instrument_id"] == instrument.id.value

    def test_data_catalog_filter(self):
        # Arrange, Act
        deltas = self.catalog.order_book_deltas()
        filtered_deltas = self.catalog.order_book_deltas(
            filter_expr=ds.field("delta_type") == "DELETE"
        )

        # Assert
        assert len(deltas) == 2384
        assert len(filtered_deltas) == 351

    def test_data_catalog_query_filtered(self):
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

        filtered_deltas = self.catalog.order_book_deltas(
            filter_expr=ds.field("delta_type") == "DELETE"
        )
        assert len(filtered_deltas) == 351

    def test_data_catalog_backtest_data_no_filter(self):
        data = self.catalog.load_backtest_data()
        assert len(sum(data.values(), [])) == 2323

    def test_data_catalog_backtest_data_filtered(self):
        instruments = self.catalog.instruments(as_nautilus=True)
        engine = BacktestEngine(bypass_logging=True)
        engine = self.catalog.setup_engine(
            engine=engine,
            instruments=[instruments[1]],
            start_timestamp=1576869877788000000,
        )
        engine.add_venue(
            venue=BETFAIR_VENUE,
            venue_type=VenueType.EXCHANGE,
            account_type=AccountType.CASH,
            base_currency=GBP,
            oms_type=OMSType.NETTING,
            starting_balances=[Money(10000, GBP)],
            order_book_level=BookLevel.L2,
        )
        engine.run()
        # Total events 1045
        assert engine.iteration in (600, 740)
