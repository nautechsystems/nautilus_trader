# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import dataclasses
import datetime
import json
import pathlib
import pickle
import sys
from typing import Optional

import dask
import pytest
from dask.base import tokenize
from pydantic import BaseModel
from pydantic.json import pydantic_encoder

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.config import Partialable
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks.data import NewsEventData
from tests.test_kit.mocks.data import aud_usd_data_loader
from tests.test_kit.mocks.data import data_catalog_setup
from tests.test_kit.stubs.persistence import TestPersistenceStubs


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


@dataclasses.dataclass(repr=False)
class ExamplePartialable(Partialable):
    a: int = None
    b: int = None
    c: Optional[str] = None


class TestBacktestConfig:
    def setup(self):
        data_catalog_setup()
        dask.config.set(scheduler="single-threaded")
        aud_usd_data_loader()
        self.catalog = DataCatalog.from_env()
        self.backtest_config = BacktestRunConfig(
            engine=BacktestEngineConfig(),
            venues=[
                BacktestVenueConfig(
                    name="SIM",
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
                    data_cls=QuoteTick,
                    instrument_id="AUD/USD.SIM",
                    start_time=1580398089820000000,
                    end_time=1580504394501000000,
                )
            ],
        )

    def test_partialable_partial(self):
        test = ExamplePartialable().replace(a=5)
        assert test.is_partial()
        test = test.replace(b=1, c="1")
        assert not test.is_partial()
        test = ExamplePartialable(a=5, b=1, c="1")
        assert not test.is_partial()

    def test_partialable_repr(self):
        test = ExamplePartialable(a=5)
        assert test.__repr__() == "Partial-ExamplePartialable(a=5, b=None, c=None)"
        test = ExamplePartialable(a=5, b=1, c="a")
        assert test.__repr__() == "ExamplePartialable(a=5, b=1, c='a')"

    def test_partialable_is_partial(self):
        test = ExamplePartialable().replace(a=5)
        assert test.is_partial()

    def test_partialable_replace(self):
        test = ExamplePartialable().replace(a=5)
        assert test.is_partial()

        test = test.replace(b=1, a=3, c="a")
        assert test.a == 3
        assert test.b == 1
        assert not test.is_partial()

    def test_partialable_check(self):
        test = ExamplePartialable().replace(a=5)
        with pytest.raises(AssertionError):
            test.check()
        test = test.replace(b=1, c=1)
        assert test.check() is None

    def test_partialable_optional_check(self):
        test = ExamplePartialable().replace(a=5)
        with pytest.raises(AssertionError):
            test.check()
        test = test.replace(b=1)
        assert test.check() is None

    def test_backtest_config_pickle(self):
        pickle.loads(pickle.dumps(self))  # noqa: S301

    def test_strategies_tokenization(self):
        # Arrange, Act
        result = tokenize(self.backtest_config.strategies)

        # Assert
        assert result == "8c9f081a88f539969f3dff99d6e05e36"

    def test_venue_config_tokenization(self):
        # Arrange, Act
        venue = self.backtest_config.venues[0]
        result = tokenize(venue)

        # Assert  # TODO: Investigate partial non-determinism
        assert result == "17a0d2e4c4d55f7382b05d79089bed40" or "1a803a06f1ab329b5e9dd1b52cc134a8"

    def test_data_config_tokenization(self):
        # Arrange, Act
        data_config = self.backtest_config.data[0]

        # Act
        result = tokenize(data_config)

        # Assert  # TODO: Investigate partial non-determinism
        assert result == "d9e2deee8477039142b7d19ca988b752" or "9f9b6cdfb9f645c53e1ca4d85f8007e9"

    def test_engine_config_tokenization(self):
        # Arrange,
        engine_config = self.backtest_config.engine

        # Act
        result = tokenize(engine_config)

        # Assert  # TODO: Investigate partial non-determinism
        assert result == "4e36e7d25fc8e8e98ea5a7127e9cff57" or "22d84218139004f8b662d2c6d3dccb4a"

    def test_tokenization_config(self):
        # Arrange, Act
        result = tokenize(self.backtest_config)

        # Assert  # TODO: Investigate partial non-determinism
        assert result == "83aecc5500d48e6dbcce5f23a7fc56bf" or "881f07f1cbf7628a22eb444d49960be5"

    def test_backtest_data_config_load(self):
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        c = BacktestDataConfig(
            catalog_path="/root/",
            catalog_fs_protocol="memory",
            data_cls=QuoteTick,
            instrument_id=instrument.id.value,
            start_time=1580398089820000000,
            end_time=1580504394501000000,
        )
        result = c.query
        assert result == {
            "as_nautilus": True,
            "cls": QuoteTick,
            "instrument_ids": ["AUD/USD.SIM"],
            "filter_expr": None,
            "start": datetime.datetime(
                2020, 1, 30, 15, 28, 9, 820000, tzinfo=datetime.timezone.utc
            ),
            "end": datetime.datetime(2020, 1, 31, 20, 59, 54, 501000, tzinfo=datetime.timezone.utc),
        }

    def test_backtest_config_partial(self):
        # Arrange
        config = BacktestRunConfig()
        config.update(
            venues=[
                BacktestVenueConfig(
                    name="SIM",
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

    def test_backtest_data_config_generic_data(self):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        process_files(
            glob_path=f"{TEST_DATA_DIR}/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )
        c = BacktestDataConfig(
            catalog_path="/root/",
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            client_id="NewsClient",
        )
        result = c.load()
        assert len(result["data"]) == 86985
        assert result["instrument"] is None
        assert result["client_id"] == ClientId("NewsClient")

    def test_backtest_data_config_filters(self):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        process_files(
            glob_path=f"{TEST_DATA_DIR}/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )
        c = BacktestDataConfig(
            catalog_path="/root/",
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            filter_expr="field('currency') == 'CHF'",
            client_id="NewsClient",
        )
        result = c.load()
        assert len(result["data"]) == 2745

    def test_backtest_data_config_status_updates(self):
        process_files(
            glob_path=PACKAGE_ROOT + "/data/1.166564490.bz2",
            reader=BetfairTestStubs.betfair_reader(),
            catalog=self.catalog,
        )
        c = BacktestDataConfig(
            catalog_path="/root/",
            catalog_fs_protocol="memory",
            data_cls=InstrumentStatusUpdate,
        )
        result = c.load()
        assert len(result["data"]) == 2
        assert result["instrument"] is None
        assert result["client_id"] is None

    def test_resolve_cls(self):
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
                data_cls=QuoteTick,
                catalog_fs_protocol="memory",
                catalog_fs_storage_options={},
                instrument_id="AUD/USD.IDEALPRO",
                start_time=1580398089820000,
                end_time=1580504394501000,
            ),
        ],
    )
    def test_models_to_json(self, model: BaseModel):
        print(json.dumps(model, indent=4, default=pydantic_encoder))
