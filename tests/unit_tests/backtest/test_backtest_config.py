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
import json
import pathlib
import pickle
import sys
from datetime import datetime
from typing import Optional

import dask
import pytest
import pytz
from dask.base import tokenize
from pydantic import BaseModel
from pydantic.json import pydantic_encoder

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.config import Partialable
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.trading.config import ImportableStrategyConfig
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import aud_usd_data_loader
from tests.test_kit.mocks import data_catalog_setup
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACrossConfig


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
    aud_usd_data_loader()


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
                data_cls_path="nautilus_trader.model.data.tick.QuoteTick",
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


# All inputs to dask delayed functions must be deterministically tokenizable


def test_strategies_tokenization(backtest_config: BacktestRunConfig):
    # Arrange, Act
    result = tokenize(backtest_config.strategies)

    # Assert
    assert result == "8c9f081a88f539969f3dff99d6e05e36"


def test_venue_config_tokenization(backtest_config: BacktestRunConfig):
    # Arrange, Act
    venue = backtest_config.venues[0]
    result = tokenize(venue)

    # Assert
    assert result == "04c48e76f89c4ba393caa3f7dc138b00"


def test_data_config_tokenization(backtest_config: BacktestRunConfig):
    # Arrange, Act
    data_config = backtest_config.data[0]

    # Act
    result = tokenize(data_config)

    # Assert
    assert result == "9aa767ed2688e65b681fd7bead9c5d3b"


def test_engine_config_tokenization(backtest_config: BacktestRunConfig):
    # Arrange,
    engine_config = backtest_config.engine

    # Act
    result = tokenize(engine_config)

    # Assert
    assert result == "22d84218139004f8b662d2c6d3dccb4a"


def test_tokenization_config(backtest_config: BacktestRunConfig):
    # Arrange, Act
    result = tokenize(backtest_config)

    # Assert
    assert result == "d6728a094680796c4bd7fdda475acfeb"


def test_backtest_data_config_load(catalog):
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    c = BacktestDataConfig(
        catalog_path="/root/",
        catalog_fs_protocol="memory",
        data_cls_path="nautilus_trader.model.data.tick.QuoteTick",
        instrument_id=instrument.id.value,
        start_time=1580398089820000000,
        end_time=1580504394501000000,
    )
    result = c.query
    assert result == {
        "as_nautilus": True,
        "cls": QuoteTick,
        "instrument_ids": ["AUD/USD.SIM"],
        "start": datetime(2020, 1, 30, 15, 28, 9, 820000, tzinfo=pytz.utc),
        "end": datetime(2020, 1, 31, 20, 59, 54, 501000, tzinfo=pytz.utc),
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
                "/",
                "nautilus_trader.model.data.tick.QuoteTick",
                "memory",
                {},
                "AUD/USD.IDEALPRO",
                1580398089820000,
                1580504394501000,
            )
        ],
    )
    assert config.is_partial()


def test_resolve_cls():
    config = BacktestDataConfig(
        "/",
        "nautilus_trader.model.data.tick.QuoteTick",
        "memory",
        {},
        "AUD/USD.IDEALPRO",
        1580398089820000,
        1580504394501000,
    )
    assert config.data_type == QuoteTick


@pytest.mark.parametrize(
    "model",
    [
        # type ignore due to workaround for kwargs on pydantic data classes
        # https://github.com/python/mypy/issues/6239
        BacktestDataConfig(  # type: ignore
            catalog_path="/",
            data_cls_path="nautilus_trader.model.data.tick.QuoteTick",
            catalog_fs_protocol="memory",
            catalog_fs_storage_options={},
            instrument_id="AUD/USD.IDEALPRO",
            start_time=1580398089820000,
            end_time=1580504394501000,
        ),
    ],
)
def test_models_to_json(model: BaseModel):
    print(json.dumps(model, indent=4, default=pydantic_encoder))
