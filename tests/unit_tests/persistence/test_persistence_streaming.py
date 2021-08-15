import pathlib
import sys

import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.streaming import FeatherWriter
from nautilus_trader.persistence.streaming import read_feather


pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


def test_feather_writer(loaded_catalog):
    # Arrange
    fs = DataCatalog.from_env().fs
    path = "/root/backtest001"
    instruments = loaded_catalog.instruments(as_nautilus=True)
    engine = BacktestEngine()
    loaded_catalog.setup_engine(engine=engine, instruments=instruments)
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
    for path in fs.ls("/root/backtest001/"):
        name = pathlib.Path(path).name
        persisted = read_feather(fs=fs, path=path)
        if persisted is not None:
            result[name] = persisted.shape
    expected = {
        "InstrumentStatusUpdate.feather": (2, 4),
        "OrderBookData.feather": (2384, 11),
        "TradeTick.feather": (624, 7),
    }
    assert result == expected
