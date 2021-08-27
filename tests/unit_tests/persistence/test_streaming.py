import pathlib
import sys

import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.streaming import FeatherWriter
from nautilus_trader.persistence.streaming import read_feather
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT


@pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")
class TestPersistenceStreaming:
    def setup(self):
        self.catalog = DataCatalog.from_env()
        self.fs = self.catalog.fs

    def _loaded_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        result = process_files(
            glob_path=PACKAGE_ROOT + "/data/1.166564490*.bz2",
            reader=BetfairTestStubs.betfair_reader(instrument_provider=self.instrument_provider),
            instrument_provider=self.instrument_provider,
            catalog=self.catalog,
        )
        assert result
        data = (
            self.catalog.instruments(as_nautilus=True)
            + self.catalog.instrument_status_updates(as_nautilus=True)
            + self.catalog.trade_ticks(as_nautilus=True)
            + self.catalog.order_book_deltas(as_nautilus=True)
            + self.catalog.ticker(as_nautilus=True)
        )
        return data

    @pytest.mark.skip("Awaiting backtest config refactor")
    def test_feather_writer(self):
        # Arrange
        path = "/root/backtest001"
        instruments = self.catalog.instruments(as_nautilus=True)
        engine = BacktestEngine()
        self.catalog.setup_engine(engine=engine, instruments=instruments)
        engine.add_venue(
            venue=BETFAIR_VENUE,
            venue_type=VenueType.EXCHANGE,
            oms_type=OMSType.NETTING,
            account_type=AccountType.CASH,
            base_currency=GBP,
            starting_balances=[Money(100_000, GBP)],
            order_book_level=BookLevel.L2,
        )

        # Act
        writer = FeatherWriter(path=path, fs_protocol="memory")
        engine.trader.subscribe("*", writer.write)
        engine.run()

        # Assert
        result = {}
        for path in self.fs.ls("/root/backtest001/"):
            name = pathlib.Path(path).name
            persisted = read_feather(fs=self.fs, path=path)
            if persisted is not None:
                result[name] = persisted.shape
        expected = {
            "InstrumentStatusUpdate.feather": (2, 4),
            "OrderBookData.feather": (2384, 11),
            "TradeTick.feather": (624, 7),
        }
        assert result == expected
