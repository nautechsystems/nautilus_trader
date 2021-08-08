import datetime
import pathlib
import sys
from decimal import Decimal

import orjson
import pyarrow.dataset as ds
import pytest

from examples.strategies.orderbook_imbalance import OrderbookImbalance
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
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
from nautilus_trader.persistence.catalog.core import DataCatalog
from nautilus_trader.persistence.catalog.loading import load
from nautilus_trader.persistence.catalog.parsers import TextReader
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.providers import TestInstrumentProvider


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


@pytest.fixture(scope="function")
def load_data():
    instrument_provider = BetfairInstrumentProvider.from_instruments([])
    reader = TextReader(
        line_parser=lambda x, state: on_market_update(
            instrument_provider=instrument_provider, update=orjson.loads(x)
        ),
        instrument_provider_update=historical_instrument_provider_loader,
    )
    load(
        path=TEST_DATA_DIR,
        reader=reader,
        glob_pattern="1.166564490*",
        instrument_provider=instrument_provider,
    )


@pytest.fixture(scope="function")
def catalog():
    catalog = DataCatalog(path="/", fs_protocol="memory")
    try:
        catalog.fs.rm("/", recursive=True)
    except FileNotFoundError:
        pass
    return catalog


@pytest.fixture(scope="function")
def loaded_catalog(catalog, load_data):
    return catalog


def test_data_catalog_instruments_df(loaded_catalog):
    instruments = loaded_catalog.instruments()
    assert len(instruments) == 2


def test_data_catalog_instruments_filtered_df(loaded_catalog):
    instrument_id = (
        "Basketball,,29628709,20191221-001000,ODDS,MATCH_ODDS,1.166564490,237491,0.0.BETFAIR"
    )
    instruments = loaded_catalog.instruments(instrument_ids=[instrument_id])
    assert len(instruments) == 1
    assert instruments["instrument_id"].iloc[0] == instrument_id


def test_data_catalog_instruments_as_nautilus(loaded_catalog):
    instruments = loaded_catalog.instruments(as_nautilus=True)
    assert all(isinstance(ins, BettingInstrument) for ins in instruments)


def test_partition_key_correctly_remapped(catalog):
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
    catalog._write_chunks(chunk=[instrument, tick])
    df = catalog.quote_ticks()
    assert len(df) == 1
    # Ensure we "unmap" the keys that we write the partition filenames as;
    # this instrument_id should be AUD/USD not AUD-USD
    assert df.iloc[0]["instrument_id"] == instrument.id.value


def test_data_catalog_filter(loaded_catalog):
    assert len(loaded_catalog.order_book_deltas()) == 2384
    assert (
        len(loaded_catalog.order_book_deltas(filter_expr=ds.field("delta_type") == "DELETE")) == 351
    )


def test_data_catalog_query_filtered(loaded_catalog):
    ticks = loaded_catalog.trade_ticks()
    assert len(ticks) == 312

    ticks = loaded_catalog.trade_ticks(start="2019-12-20 20:56:18")
    assert len(ticks) == 123

    ticks = loaded_catalog.trade_ticks(start=1576875378384999936)
    assert len(ticks) == 123

    ticks = loaded_catalog.trade_ticks(start=datetime.datetime(2019, 12, 20, 20, 56, 18))
    assert len(ticks) == 123

    deltas = loaded_catalog.order_book_deltas()
    assert len(deltas) == 2384

    filtered_deltas = loaded_catalog.order_book_deltas(
        filter_expr=ds.field("delta_type") == "DELETE"
    )
    assert len(filtered_deltas) == 351


def test_data_catalog_backtest_data_no_filter(loaded_catalog):
    data = loaded_catalog.load_backtest_data()
    assert len(sum(data.values(), [])) == 2323


def test_data_catalog_backtest_data_filtered(loaded_catalog):
    instruments = loaded_catalog.instruments(as_nautilus=True)
    engine = BacktestEngine(bypass_logging=True)
    engine = loaded_catalog.setup_engine(
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
    assert engine.iteration == 600


@pytest.mark.skip(reason="flaky")
def test_data_catalog_backtest_run(loaded_catalog):
    instruments = loaded_catalog.instruments(as_nautilus=True)
    engine = BacktestEngine(bypass_logging=True)
    engine = loaded_catalog.setup_engine(engine=engine, instruments=[instruments[1]])
    engine.add_venue(
        venue=BETFAIR_VENUE,
        venue_type=VenueType.EXCHANGE,
        account_type=AccountType.CASH,
        base_currency=GBP,
        oms_type=OMSType.NETTING,
        starting_balances=[Money(10000, GBP)],
        order_book_level=BookLevel.L2,
    )
    strategy = OrderbookImbalance(
        instrument=instruments[1], max_trade_size=Decimal("50"), order_id_tag="OI"
    )
    engine.run(strategies=[strategy])
    positions = engine.trader.generate_positions_report()
    assert positions["realized_points"].astype(float).sum() == -0.00462297183247178
