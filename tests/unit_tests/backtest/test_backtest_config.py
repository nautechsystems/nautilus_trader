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

import datetime
import json
import pickle
from typing import Optional

import pytest
from pydantic import BaseModel
from pydantic import parse_obj_as
from pydantic.json import pydantic_encoder

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import Partialable
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import aud_usd_data_loader
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.stubs.config import TestConfigStubs
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


class ExamplePartialable(Partialable):
    a: int = None
    b: int = None
    c: Optional[str] = None


class TestBacktestConfig:
    def setup(self):
        self.catalog = data_catalog_setup()
        aud_usd_data_loader()
        self.venue = Venue("SIM")
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=self.venue)
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
                ),
            ],
            data=[
                BacktestDataConfig(
                    catalog_path="/.nautilus/catalog",
                    catalog_fs_protocol="memory",
                    data_cls=QuoteTick,
                    instrument_id="AUD/USD.SIM",
                    start_time=1580398089820000000,
                    end_time=1580504394501000000,
                ),
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
                2020,
                1,
                30,
                15,
                28,
                9,
                820000,
                tzinfo=datetime.timezone.utc,
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
                ),
            ],
        )
        assert config.is_partial()
        config = config.update(
            data=[
                BacktestDataConfig(
                    catalog_path="/",
                    data_cls="nautilus_trader.model.data.tick.QuoteTick",
                    catalog_fs_protocol="memory",
                    catalog_fs_storage_options={},
                    instrument_id="AUD/USD.IDEALPRO",
                    start_time=1580398089820000,
                    end_time=1580504394501000,
                ),
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
            glob_path=TEST_DATA_DIR + "/1.166564490.bz2",
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
            catalog_path="/",
            data_cls="nautilus_trader.model.data.tick:QuoteTick",
            catalog_fs_protocol="memory",
            catalog_fs_storage_options={},
            instrument_id="AUD/USD.IDEALPRO",
            start_time=1580398089820000,
            end_time=1580504394501000,
        )
        assert config.data_type == QuoteTick

    @pytest.mark.parametrize(
        "model",
        [
            BacktestDataConfig(
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

    def test_run_config_parse_obj(self):
        run_config = TestConfigStubs.backtest_run_config(
            catalog=self.catalog,
            instrument_ids=[self.instrument.id.value],
            venues=[
                BacktestVenueConfig(
                    name="SIM",
                    oms_type="HEDGING",
                    account_type="MARGIN",
                    starting_balances=["1_000_000 USD"],
                ),
            ],
        )
        raw = run_config.dict()
        config = parse_obj_as(BacktestRunConfig, raw)
        assert isinstance(config, BacktestRunConfig)
        node = BacktestNode(configs=[config])
        assert isinstance(node, BacktestNode)
        # node.run()
