# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import copy
import dataclasses
import pathlib
import pickle
import sys
from decimal import Decimal
from functools import partial
from typing import Optional

import dask
import pandas as pd
import pytest
from dask.base import tokenize

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.config import Partialable
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.backtest.results import BacktestRunResults
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.trading.config import ImportableStrategyConfig
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import data_catalog_setup
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACrossConfig
from tests.test_kit.stubs import TestStubs


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


@pytest.fixture(autouse=True, scope="function")
def reset():
    """Cleanup resources before each test run"""
    data_catalog_setup()
    dask.config.set(scheduler="single-threaded")
    yield


@pytest.fixture(scope="function")
def data_loader():
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))

    def parse_csv_tick(df, instrument_id):
        yield instrument
        for r in df.values:
            ts = secs_to_nanos(pd.Timestamp(r[0]).timestamp())
            tick = QuoteTick(
                instrument_id=instrument_id,
                bid=Price.from_str(str(r[1])),
                ask=Price.from_str(str(r[2])),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            )
            yield tick

    catalog = DataCatalog.from_env()
    instrument_provider = InstrumentProvider()
    instrument_provider.add(instrument)
    process_files(
        glob_path=f"{TEST_DATA_DIR}/truefx-audusd-ticks.csv",
        reader=CSVReader(
            block_parser=partial(parse_csv_tick, instrument_id=TestStubs.audusd_id()),
            as_dataframe=True,
        ),
        instrument_provider=instrument_provider,
        catalog=catalog,
    )


@pytest.fixture(scope="function")
def catalog(data_loader):
    catalog = DataCatalog.from_env()
    # assert len(catalog.instruments()) == 1
    assert len(catalog.quote_ticks()) == 100000
    return catalog


@pytest.fixture(scope="function")
def backtest_config(catalog):
    return BacktestRunConfig(
        engine=BacktestEngineConfig(),
        venues=[
            BacktestVenueConfig(
                name="SIM",
                venue_type="ECN",
                oms_type="HEDGING",
                account_type="MARGIN",
                base_currency="USD",
                starting_balances=["1000000 USD"],
                # fill_model=fill_model,  # TODO(cs): Implement next iteration
            )
        ],
        data=[
            BacktestDataConfig(
                catalog_path="/root",
                catalog_fs_protocol="memory",
                data_type=QuoteTick,
                instrument_id="AUD/USD.SIM",
                start_time=1580398089820000000,
                end_time=1580504394501000000,
            )
        ],
        # strategies=[
        #     (
        #         EMACross,
        #         EMACrossConfig(
        #             instrument_id="AUD/USD.SIM",
        #             bar_type="AUD/USD.SIM-15-MINUTE-BID-EXTERNAL",
        #             trade_size=1_000_000,
        #             fast_ema_period=10,
        #             slow_ema_period=20,
        #         ),
        #     ),
        # ],
    )


@pytest.mark.skip(reason="WIP")
@pytest.fixture(scope="function")
def backtest_configs(backtest_config):
    base = copy.copy(backtest_config)
    instrument_id = base.data[0].instrument_id
    base.strategies = None

    shared_params = dict(
        instrument_id=instrument_id,
        bar_type=f"{instrument_id}-15-MINUTE-BID-EXTERNAL",
        trade_size=1_000_000,
    )
    # Create two strategies with different params
    strategies = [
        ImportableStrategyConfig(
            path="tests.test_kit.strategies:EMACross",
            config=EMACrossConfig(**shared_params, **{"fast_ema": x, "slow_ema": y}),
        )
        for x, y in [(10, 20), (20, 30)]
    ]
    # Create a backtest config for each strategy
    return [backtest_config.replace(strategies=[s]) for s in strategies]


@dataclasses.dataclass(repr=False)
class Test(Partialable):
    a: int = None
    b: int = None
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
    test = test.replace(b=1, c=1)
    assert test.check() is None


def test_partialable_optional_check():
    test = Test().replace(a=5)
    with pytest.raises(AssertionError):
        test.check()
    test = test.replace(b=1)
    assert test.check() is None


def test_backtest_config_pickle(backtest_config):
    pickle.loads(pickle.dumps(backtest_config))  # noqa: S301


@pytest.mark.parametrize(
    "key, token",
    [
        ("venues", "820d33524245a874a50b05468e93bd5c"),
        ("data", "20fb687f4136b3f3858bae5529422698"),
        ("engine", "dfbec4bd64a46e522a590ffd1de19607"),
        ("strategies", "8c9f081a88f539969f3dff99d6e05e36"),
    ],
)
def test_tokenization_attributes(backtest_config: BacktestRunConfig, key, token):
    # All inputs to dask delayed functions must be deterministically tokenizable
    # Arrange, Act
    result = tokenize(getattr(backtest_config, key))

    # Assert
    assert result == token


def test_tokenization_config(backtest_config: BacktestRunConfig):
    # Arrange, Act
    result = tokenize(backtest_config)

    # Assert
    assert result == "da51dbca807fed69908173718dafee32"


def test_backtest_data_config_load(catalog):
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    c = BacktestDataConfig(
        catalog_path="/root/",
        catalog_fs_protocol="memory",
        data_type=QuoteTick,
        instrument_id=instrument.id.value,
        start_time=1580398089820000000,
        end_time=1580504394501000000,
    )
    result = c.query
    assert result == {
        "as_nautilus": True,
        "cls": QuoteTick,
        "instrument_ids": ["AUD/USD.SIM"],
        "start": 1580398089820000000,
        "end": 1580504394501000000,
    }


def test_backtest_config_partial():
    # Arrange
    config = BacktestRunConfig()
    config.update(
        venues=[
            BacktestVenueConfig(
                name="SIM",
                venue_type="ECN",
                oms_type="HEDGING",
                account_type="MARGIN",
                base_currency="USD",
                starting_balances=["1000000 USD"],
            )
        ],
    )
    assert config.is_partial()
    config = config.update(
        data=[
            BacktestDataConfig(
                catalog_path="/",
                catalog_fs_protocol="memory",
                data_type=QuoteTick,
                instrument_id="AUD/USD.IDEALPRO",
                start_time=1580398089820000,
                end_time=1580504394501000,
            )
        ],
    )
    assert config.is_partial()


def test_build_graph_shared_nodes(backtest_configs):
    # Arrange
    node = BacktestNode()
    graph = node.build_graph(backtest_configs)
    dsk = graph.dask.to_dict()

    # Act - The strategies share the same input data,
    result = sorted([k.split("-")[0] for k in dsk.keys()])

    # Assert
    assert result == [
        "_gather_delayed",
        "_run_delayed",
        "_run_delayed",
        "load",
    ]


def test_backtest_against_example(catalog):
    """Replicate examples/fx_ema_cross_audusd_ticks.py backtest result."""
    # Arrange
    config = BacktestRunConfig(
        engine=BacktestEngineConfig(),
        venues=[
            BacktestVenueConfig(
                name="SIM",
                venue_type="ECN",
                oms_type="HEDGING",  # Venue will generate position_ids
                account_type="MARGIN",
                base_currency="USD",  # Standard single-currency account
                starting_balances=["1000000 USD"],
                # fill_model=FillModel(  # TODO(cs): Implement next iteration
                #     prob_fill_at_limit=0.2,
                #     prob_fill_at_stop=0.95,
                #     prob_slippage=0.5,
                #     random_seed=42,
                # ),
            )
        ],
        data=[
            BacktestDataConfig(
                catalog_path="/root",
                catalog_fs_protocol="memory",
                data_type=QuoteTick,
                instrument_id="AUD/USD.SIM",
                start_time=1580398089820000000,
                end_time=1580504394501000000,
            )
        ],
        strategies=[
            ImportableStrategyConfig(
                path="tests.test_kit.strategies:EMACross",
                config=EMACrossConfig(
                    instrument_id="AUD/USD.SIM",
                    bar_type="AUD/USD.SIM-100-TICK-MID-INTERNAL",
                    fast_ema_period=10,
                    slow_ema_period=20,
                    trade_size=Decimal(1_000_000),
                    order_id_tag="001",
                ),
            )
        ],
    )

    node = BacktestNode()

    # Act
    tasks = node.build_graph([config])
    results: BacktestRunResults = tasks.compute()
    result: BacktestResult = results.results[0]

    # Assert
    assert len(result.account_balances) == 193
    assert len(result.positions) == 48
    assert len(result.fill_report) == 96
    account_result = result.account_balances.iloc[-2].to_dict()
    expected = {
        "account_id": "SIM-001",
        "account_type": "MARGIN",
        "base_currency": "USD",
        "currency": "USD",
        "free": "976269.59",
        "info": b"{}",  # noqa: P103
        "locked": "20096.29",
        "reported": False,
        "total": "996365.88",
        "venue": Venue("SIM"),
    }
    assert account_result == expected


def test_backtest_run_sync(backtest_configs, catalog):
    # Arrange
    node = BacktestNode()

    # Act
    result = node.run_sync(backtest_configs)

    # Assert
    assert len(result.results) == 2


def test_backtest_build_graph(backtest_configs, catalog):
    # Arrange
    node = BacktestNode()
    tasks = node.build_graph(backtest_configs)

    # Act
    result: BacktestRunResults = tasks.compute()

    # Assert
    assert len(result.results) == 2


def test_backtest_run_distributed(backtest_configs, catalog):
    from distributed import Client

    # Arrange
    node = BacktestNode()
    with Client(processes=False):
        tasks = node.build_graph(backtest_configs)

        # Act
        result = tasks.compute()

        # Assert
        assert result


def test_backtest_run_results(backtest_configs, catalog):
    # Arrange
    node = BacktestNode()

    # Act
    result = node.run_sync(backtest_configs)

    # Assert
    assert isinstance(result, BacktestRunResults)
    assert len(result.results) == 2
    assert (
        str(result.results[0])
        == "BacktestResult(backtest-b8a76bdbf6b0b8b8b295d05449fe1393, SIM[USD]=1000000.00)"
    )
