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
from typing import Optional

import pytest
from pydantic import BaseModel
from pydantic.json import pydantic_encoder

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import Partialable
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks.data import NewsEventData
from tests.test_kit.mocks.data import aud_usd_data_loader
from tests.test_kit.mocks.data import data_catalog_setup
from tests.test_kit.stubs.persistence import TestPersistenceStubs


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))


@dataclasses.dataclass(repr=False)
class ExamplePartialable(Partialable):
    a: int = None
    b: int = None
    c: Optional[str] = None


class TestBacktestConfig:
    def setup(self):
        self.catalog = data_catalog_setup()
        aud_usd_data_loader()
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
                    catalog_path="/.nautilus/catalog",
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

    def test_backtest_data_config_load(self):
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        c = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
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
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            client_id="NewsClient",
            metadata={"kind": "news"},
        )
        result = c.load()
        assert len(result["data"]) == 86985
        assert result["instrument"] is None
        assert result["client_id"] == ClientId("NewsClient")
        assert result["data"][0].data_type.metadata == {"kind": "news"}

    def test_backtest_data_config_filters(self):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        process_files(
            glob_path=f"{TEST_DATA_DIR}/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )
        c = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
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
            catalog_path="/.nautilus/catalog",
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
            "nautilus_trader.model.data.tick:QuoteTick",
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

    def test_backtest_data_start_end_nanos_round_trip_returns_same_value_int(self):
        # Arrange
        start_time_nanos = 1546383605776999936
        end_time_nanos = 1546390125908000000
        config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            start_time=start_time_nanos,
            end_time=end_time_nanos,
        )

        assert start_time_nanos == config.start_time_nanos
        assert end_time_nanos == config.end_time_nanos

    def test_backtest_data_start_end_nanos_round_trip_returns_same_value_datetime(self):
        start_time_nanos = 1546383605776999936
        end_time_nanos = 1546390125908000000
        start_dt = unix_nanos_to_dt(start_time_nanos)
        end_dt = unix_nanos_to_dt(end_time_nanos)

        config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            start_time=start_dt,
            end_time=end_dt,
        )
        assert start_time_nanos == config.start_time_nanos
        assert end_time_nanos == config.end_time_nanos

    def test_backtest_data_start_end_nanos_round_trip_returns_same_value_str(self):
        start_time_nanos = 1546383605776999936
        end_time_nanos = 1546390125908000000

        config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog",
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            start_time=str(start_time_nanos),
            end_time=str(end_time_nanos),
        )
        
        assert start_time_nanos == config.start_time_nanos
        assert end_time_nanos == config.end_time_nanos


TestBacktestConfig().test_backtest_data_start_end_nanos_round_trip_returns_same_value_int()
TestBacktestConfig().test_backtest_data_start_end_nanos_round_trip_returns_same_value_str()
TestBacktestConfig().test_backtest_data_start_end_nanos_round_trip_returns_same_value_datetime()
TestBacktestConfig().test_backtest_data_config_load()