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

import pickle
import sys

import msgspec
import pytest
from click.testing import CliRunner

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config.backtest import BacktestEngineConfig
from nautilus_trader.config.backtest import json_encoder
from nautilus_trader.config.backtest import tokenize_config
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import aud_usd_data_loader
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.stubs.config import TestConfigStubs
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.skipif(sys.platform == "win32", reason="failing on Windows")
class TestBacktestConfig:
    def setup(self):
        self.catalog = data_catalog_setup()
        aud_usd_data_loader()
        self.venue = Venue("SIM")
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=self.venue)
        self.backtest_config = TestConfigStubs.backtest_run_config(catalog=self.catalog)

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
            "start": 1580398089820000000,
            "end": 1580504394501000000,
        }

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
                data_cls=QuoteTick.fully_qualified_name(),
                catalog_fs_protocol="memory",
                catalog_fs_storage_options={},
                instrument_id="AUD/USD.IDEALPRO",
                start_time=1580398089820000,
                end_time=1580504394501000,
            ),
        ],
    )
    def test_models_to_json(self, model: msgspec.Struct):
        raw = model.json()
        assert raw

    def test_run_config_to_json(self):
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
        json = msgspec.json.encode(run_config)
        result = len(msgspec.json.encode(json))
        assert result in (774, 702)  # unix, windows sizes

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
        raw = msgspec.json.encode(run_config)
        config = msgspec.json.decode(raw, type=BacktestRunConfig)
        assert isinstance(config, BacktestRunConfig)
        node = BacktestNode(configs=[config])
        assert isinstance(node, BacktestNode)
        assert len(raw) in (579, 628)  # unix, windows sizes

    def test_backtest_config_to_json(self):
        assert msgspec.json.encode(self.backtest_config)

    def test_backtest_data_config_to_dict(self):
        run_config = TestConfigStubs.backtest_run_config(
            catalog=self.catalog,
            instrument_ids=[self.instrument.id.value],
            data_types=(TradeTick, QuoteTick, OrderBookData),
            venues=[
                BacktestVenueConfig(
                    name="BETFAIR",
                    oms_type="NETTING",
                    account_type="BETTING",
                    base_currency="GBP",
                    starting_balances=["10000 GBP"],
                    book_type="L2_MBP",
                ),
            ],
        )
        json = msgspec.json.encode(run_config)
        result = len(msgspec.json.encode(json))
        assert result in (1518, 1370)  # unix, windows

    def test_backtest_run_config_id(self):
        token = self.backtest_config.id
        print("token:", token)
        value: bytes = msgspec.json.encode(self.backtest_config.dict(), enc_hook=json_encoder)
        print("token_value:", value.decode())
        assert token in (
            "0c0862302a77e6e2ee68f11f1d1d019c348e2838124bc4ed6a9204ee170bbfec",  # unix
            "b2379c3eb60a0be2f10a05fc6b953c943bbb482cbaf7787a63606348e2b879c4",  # windows
        )

    @pytest.mark.parametrize(
        "config_func, keys, kw, expected",
        [
            (
                TestConfigStubs.venue_config,
                (),
                {},
                ("7919596b3762fd98d79afa64976a292d408313816d38ec26ebd29e31049b92f9",),
            ),
            (
                TestConfigStubs.backtest_data_config,
                ("catalog",),
                {},
                (
                    "44e5227fb899f348534c0d1f65f5b34176f0faf492b6795879b9ea1a32645e88",  # unix
                    "976eb2b871e6659c646498255dcf6f8bf0af7152f523eb3d55c01e9ad133d99c",  # windows
                ),
            ),
            (
                TestConfigStubs.backtest_engine_config,
                ("catalog",),
                {"persist": True},
                (
                    "58aff849aada8e5a8c789c27b7674ad61443e0b2395f097cab20fcd69488f234",
                    "0ac4b233023aec12464ec119d89c67d31025160858096f193d4c72190074d057",
                ),
            ),
            (
                TestConfigStubs.risk_engine_config,
                (),
                {},
                ("962367da58082b349922801d5fea53526f1c35149a042c84fde2fc69c8fb46cf",),
            ),
            (
                TestConfigStubs.exec_engine_config,
                (),
                {},
                ("a6ca5c188b92707f81a9ba5d45700dcbc8aebe0443c1e7b13b10a86c045c6391",),
            ),
            (
                TestConfigStubs.streaming_config,
                ("catalog",),
                {},
                (
                    "f488bdd4746d00210328b4cee46d9bdf05fab6cdcf6bc00033987f79f245888f",
                    "e9af640c98ccba607aca44aa40113cbf67e70c31afd305ea125e00ff3e326cc5",
                ),
            ),
        ],
    )
    def test_tokenize_config(self, config_func, keys, kw, expected):
        config = config_func(**{k: getattr(self, k) for k in keys}, **kw)
        token = tokenize_config(config.dict())
        assert token in expected

    def test_backtest_main_cli(self, mocker):
        # Arrange
        from nautilus_trader.backtest.__main__ import main

        runner = CliRunner()
        raw = msgspec.json.encode(
            [
                BacktestRunConfig(
                    engine=BacktestEngineConfig(),
                    venues=[
                        BacktestVenueConfig(
                            name="SIM",
                            oms_type="HEDGING",
                            account_type="CASH",
                            starting_balances=["100 USD"],
                        ),
                    ],
                    data=[],
                ),
            ],
        ).decode()

        # Act
        with mocker.patch("nautilus_trader.backtest.node.BacktestNode.run"):
            result = runner.invoke(main, ["--raw", raw])

        # Assert
        assert result.exception is None
        assert result.exit_code == 0
