import copy
import dataclasses
from decimal import Decimal
from functools import partial
import os
import pathlib
import pickle
from typing import Optional

from dask.base import tokenize
import fsspec
import pandas as pd
import pytest

from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.config import Partialable
from nautilus_trader.backtest.config import build_graph
from nautilus_trader.backtest.data_loader import CSVParser
from nautilus_trader.backtest.data_loader import DataCatalog
from nautilus_trader.backtest.data_loader import DataLoader
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACross
from tests.test_kit.stubs import TestStubs


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))
CATALOG_DIR = TEST_DATA_DIR + "/catalog"


@pytest.fixture(scope="module")
def catalog_dir():
    # Ensure we have a catalog directory, and its cleaned up after use
    fs = fsspec.filesystem("file")
    catalog = str(pathlib.Path(CATALOG_DIR))
    os.environ.update({"NAUTILUS_BACKTEST_DIR": str(catalog)})
    if fs.exists(catalog):
        fs.rm(catalog, recursive=True)
    fs.mkdir(catalog)
    yield
    fs.rm(catalog, recursive=True)


@pytest.fixture(scope="module")
def data_loader():
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))

    def parse_csv_tick(df, instrument_id, state=None):
        yield instrument
        for r in df.values:
            ts = secs_to_nanos(pd.Timestamp(r[0]).timestamp())
            tick = QuoteTick(
                instrument_id=instrument_id,
                bid=Price.from_str(str(r[1])),
                ask=Price.from_str(str(r[2])),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event_ns=ts,
                ts_recv_ns=ts,
            )
            yield tick

    instrument_provider = InstrumentProvider()
    instrument_provider.add(instrument)
    loader = DataLoader(
        path=TEST_DATA_DIR,
        parser=CSVParser(
            parser=partial(parse_csv_tick, instrument_id=TestStubs.audusd_id())
        ),
        glob_pattern="truefx-audusd-ticks.csv",
        instrument_provider=instrument_provider,
    )
    return loader


@pytest.fixture(scope="module")
def catalog(catalog_dir, data_loader):
    catalog = DataCatalog()
    catalog.import_from_data_loader(loader=data_loader)
    assert len(catalog.instruments()) == 1
    assert len(catalog.quote_ticks()) == 100000
    return catalog


@pytest.fixture(scope="function")
def backtest_config(catalog):
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    # Create a fill model (optional)
    fill_model = FillModel(
        prob_fill_at_limit=0.2,
        prob_fill_at_stop=0.95,
        prob_slippage=0.5,
        random_seed=42,
    )
    return BacktestConfig(
        venues=[
            BacktestVenueConfig(
                name="SIM",
                venue_type="ECN",
                oms_type="HEDGING",
                account_type="MARGIN",
                base_currency=USD,
                starting_balances=[Money(1_000_000, USD)],
                fill_model=fill_model,
            )
        ],
        instruments=[instrument],
        data_config=[
            BacktestDataConfig(
                data_type=QuoteTick,
                instrument_id=instrument.id.value,
                start_time=1580398089820000000,
                end_time=1580504394501000000,
            )
        ],
        strategies=[
            (
                EMACross,
                dict(
                    instrument_id=instrument.id,
                    bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
                    fast_ema_period=10,
                    slow_ema_period=20,
                    trade_size=Decimal(1_000_000),
                ),
            ),
        ],
    )


@pytest.fixture(scope="function")
def backtest_configs(backtest_config):
    base = copy.copy(backtest_config)
    instrument_id = base.strategies[0][1]["instrument_id"]
    base.strategies = None

    shared_params = dict(
        instrument_id=instrument_id,
        bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
        trade_size=Decimal(1_000_000),
    )
    # Create two strategies with different params
    strategies = [
        (EMACross, {**shared_params, **{"fast_ema": x, "slow_ema": y}})
        for x, y in [(10, 20), (20, 30)]
    ]
    # Create a backtest config for each strategy
    return [backtest_config.replace(strategies=[s]) for s in strategies]


@dataclasses.dataclass(repr=False)
class Test(Partialable):
    a: Optional[int] = None
    b: Optional[int] = None
    c: Optional[str] = None


def test_partialable_partial():
    test = Test().replace(a=5)
    assert test.is_partial()
    test = test.replace(b=1, c="1")
    assert not test.is_partial()
    test = Test(a=5, b=1, c="1")
    assert not test.is_partial()


def test_partialable_repr():
    test = Test(a=5)
    assert test.__repr__() == "Partial-Test(a=5, b=None, c=None)"
    test = Test(a=5, b=1, c="a")
    assert test.__repr__() == "Test(a=5, b=1, c='a')"


def test_partialable_is_partial():
    test = Test().replace(a=5)
    assert test.is_partial()


def test_partialable_replace():
    test = Test().replace(a=5)
    assert test.is_partial()

    test = test.replace(b=1, a=3, c="a")
    assert test.a == 3
    assert test.b == 1
    assert not test.is_partial()


def test_partialable_check():
    test = Test().replace(a=5)
    with pytest.raises(AssertionError):
        test.check()
    test = test.replace(b=1)
    with pytest.raises(AssertionError):
        test.check()


def test_backtest_config_pickle(backtest_config):
    pickle.loads(pickle.dumps(backtest_config))  # noqa: S301


def test_tokenization(backtest_config):
    # All inputs to dask delayed functions must be deterministically tokenizable
    required = [
        (backtest_config.instruments, "13ffe6e1ca1d94a1fe33de7e2a053aa4"),
        (backtest_config.venues, "70b12a5d8ef1300bc8db494f8378df77"),
    ]
    for inputs, value in required:
        # Generate many tokens to ensure determinism
        result = tokenize(inputs)
        assert result == value
        # assert all(x == tokens[0] for x in tokens), f"Tokens do not much for {r}"


def test_backtest_data_config_load(catalog):
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    c = BacktestDataConfig(
        data_type=QuoteTick,
        instrument_id=instrument.id.value,
        start_time=1580398089820000000,
        end_time=1580504394501000000,
    )
    result = c.query
    expected = {
        "as_nautilus": True,
        "cls": QuoteTick,
        "instrument_ids": ["AUD/USD.SIM"],
        "start": 1580398089820000000,
        "end": 1580504394501000000,
    }
    assert result == expected


def test_backtest_config_partial():
    config = BacktestConfig()
    config.update(
        venues=[
            BacktestVenueConfig(
                name="SIM",
                venue_type="ECN",
                oms_type="HEDGING",
                account_type="MARGIN",
                base_currency=USD,
                starting_balances=[Money(1_000_000, USD)],
            )
        ],
    )
    assert config.is_partial()
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    config = config.update(
        instruments=[instrument],
        data_config=[
            BacktestDataConfig(
                data_type=QuoteTick,
                instrument_id=instrument.id.value,
                start_time=1580398089820000,
                end_time=1580504394501000,
            )
        ],
    )
    assert config.is_partial()


def test_build_graph_shared_nodes(backtest_configs):
    graph = build_graph(backtest_configs)
    dsk = graph.dask.to_dict()
    result = sorted([k.split("-")[0] for k in dsk.keys()])
    # The strategies share the same input data,
    expected = [
        "gather",
        "load",
        "run_backtest",
        "run_backtest",
    ]
    assert result == expected


def test_backtest_against_example(catalog):
    # Replicate examples/fx_ema_cross_audusd_ticks.py backtest result

    AUDUSD = TestInstrumentProvider.default_fx_ccy("AUD/USD", Venue("SIM"))

    config = BacktestConfig(
        venues=[
            BacktestVenueConfig(
                name="SIM",
                venue_type="ECN",
                oms_type="HEDGING",  # Venue will generate position_ids
                account_type="MARGIN",
                base_currency=USD,  # Standard single-currency account
                starting_balances=[Money(1_000_000, USD)],
                fill_model=FillModel(
                    prob_fill_at_limit=0.2,
                    prob_fill_at_stop=0.95,
                    prob_slippage=0.5,
                    random_seed=42,
                ),
            )
        ],
        data_config=[
            BacktestDataConfig(
                data_type=QuoteTick,
                instrument_id=AUDUSD.id.value,
                start_time=1580398089820000000,
                end_time=1580504394501000000,
            )
        ],
        instruments=[AUDUSD],
        strategies=[
            (
                EMACross,
                dict(
                    instrument_id=AUDUSD.id,
                    bar_spec=BarSpecification(100, BarAggregation.TICK, PriceType.MID),
                    fast_ema=10,
                    slow_ema=20,
                    trade_size=Decimal(1_000_000),
                    extra_id_tag="001",
                ),
            )
        ],
    )

    tasks = build_graph(config)
    results = tasks.compute()
    result = results[list(results)[0]]
    assert len(result["account"]) == 97
    assert len(result["positions"]) == 48
    assert len(result["fills"]) == 96
    expected = '[{"type": "AccountBalance", "currency": "USD", "total": "997652.94", "locked": "0.00", "free": "997652.94"}]'
    account_result = result["account"]["balances"].iloc[-2]
    assert account_result == expected


def test_backtest_run_sync(backtest_configs):
    tasks = build_graph(backtest_configs)
    result = tasks.compute()
    assert len(result) == 2


@pytest.mark.skip
def test_backtest_run_multiprocessing(backtest_configs):
    # TODO (bm) not working; TypeError: no default __reduce__ due to non-trivial __cinit__
    tasks = build_graph(backtest_configs)
    result = tasks.compute(scheduler="processes")
    assert result


def test_backtest_run_distributed(backtest_configs):
    from distributed import Client

    _ = Client()
    tasks = build_graph(backtest_configs)
    result = tasks.compute()
    assert result
