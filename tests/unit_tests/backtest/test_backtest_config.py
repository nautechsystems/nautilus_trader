# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec
import pytest
from click.testing import CliRunner

from nautilus_trader.backtest.modules import FXRolloverInterestConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.config.backtest import json_encoder
from nautilus_trader.config.backtest import tokenize_config
from nautilus_trader.config.common import NautilusConfig
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import aud_usd_data_loader
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.config import TestConfigStubs
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


class _TestBacktestConfig:
    def setup(self):
        self.catalog = data_catalog_setup(protocol=self.fs_protocol)
        aud_usd_data_loader(self.catalog)
        self.venue = Venue("SIM")
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=self.venue)
        self.backtest_config = TestConfigStubs.backtest_run_config(catalog=self.catalog)

    def teardown(self):
        # Cleanup
        path = self.catalog.path
        fs = self.catalog.fs
        if fs.exists(path):
            fs.rm(path, recursive=True)

    def test_backtest_config_pickle(self):
        pickle.loads(pickle.dumps(self))  # noqa: S301

    def test_backtest_data_config_load(self):
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        c = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs.protocol,
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
            "use_rust": False,
            "metadata": None,
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
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs.protocol,
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
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs.protocol,
            data_cls=NewsEventData,
            filter_expr="field('currency') == 'CHF'",
            client_id="NewsClient",
        )

        result = c.load()
        assert len(result["data"]) == 2745

    def test_backtest_data_config_status_updates(self):
        process_files(
            glob_path=TEST_DATA_DIR + "/betfair/1.166564490.bz2",
            reader=BetfairTestStubs.betfair_reader(),
            catalog=self.catalog,
        )
        c = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=self.catalog.fs.protocol,
            data_cls=InstrumentStatusUpdate,
        )
        result = c.load()
        assert len(result["data"]) == 2
        assert result["instrument"] is None
        assert result["client_id"] is None

    def test_resolve_cls(self):
        config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            data_cls="nautilus_trader.model.data.tick:QuoteTick",
            catalog_fs_protocol=self.catalog.fs.protocol,
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
    def test_models_to_json(self, model: NautilusConfig):
        raw = model.json()
        assert raw

    def test_backtest_config_to_json(self):
        assert msgspec.json.encode(self.backtest_config)


class TestBacktestConfigFile(_TestBacktestConfig):
    fs_protocol = "file"


class TestBacktestConfigMemory(_TestBacktestConfig):
    fs_protocol = "memory"


class TestBacktestConfigParsing:
    def setup(self):
        self.catalog = data_catalog_setup(protocol="memory", path="/.nautilus/")
        self.venue = Venue("SIM")
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=self.venue)
        self.backtest_config = TestConfigStubs.backtest_run_config(catalog=self.catalog)

    def test_run_config_to_json(self) -> None:
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
        assert result in (882, 886)  # unix, windows sizes

    def test_run_config_parse_obj(self) -> None:
        run_config = TestConfigStubs.backtest_run_config(
            catalog=self.catalog,
            instrument_ids=[self.instrument.id.value],
            venues=[
                BacktestVenueConfig(
                    name="SIM",
                    oms_type="HEDGING",
                    account_type="MARG  IN",
                    starting_balances=["1_000_000 USD"],
                ),
            ],
        )
        raw = msgspec.json.encode(run_config)
        config = msgspec.json.decode(raw, type=BacktestRunConfig)
        assert isinstance(config, BacktestRunConfig)
        node = BacktestNode(configs=[config])
        assert isinstance(node, BacktestNode)
        assert len(raw) in (661, 664)  # unix, windows sizes

    def test_backtest_data_config_to_dict(self) -> None:
        run_config = TestConfigStubs.backtest_run_config(
            catalog=self.catalog,
            instrument_ids=[self.instrument.id.value],
            data_types=(TradeTick, QuoteTick, OrderBookDelta),
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
        assert result in (1738, 1750)  # unix, windows

    def test_backtest_run_config_id(self) -> None:
        token = self.backtest_config.id
        print("token:", token)
        value: bytes = msgspec.json.encode(self.backtest_config.dict(), enc_hook=json_encoder)
        print("token_value:", value.decode())
        assert token in (
            "1bf83318bfeb6359bcacc88d2d5c5e06c1486f9c4c3cc76e203b1ee14e41a5a2",  # unix
            "669d861fcc17dc50210d9bcdad5affa70410a3b3347f393fe17cd59252b3d14b",  # windows
        )

    @pytest.mark.skip(reason="fix after merge")
    @pytest.mark.parametrize(
        ("config_func", "keys", "kw", "expected"),
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
                    "8485d8c61bb15514769412bc4c0fb0a662617b3245d751c40e3627a1b6762ba0",  # unix
                    "d32e5785aad958ec163da39ba501a8fbe654fd973ada46e21907631824369ce4",  # windows
                ),
            ),
            (
                TestConfigStubs.backtest_engine_config,
                ("catalog",),
                {"persist": True},
                (
                    "90f34a9e9474a35a365fa6ffb4bd8586f443a98ff845dec019ed9c857774f6cb",
                    "11048af3175c58d841d1e936e6075d053d8d445d889ab653229208033f60307d",
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
                    "a1d857e553be89e5e6336fa7d1ee2c55032ada5d63193ecc959b216b4afc3f18",
                    "1f1564863058e883768f311e4724fa1f4ddcab0faf717d262a586f734403dc11",
                ),
            ),
        ],
    )
    def test_tokenize_config(self, config_func, keys, kw, expected) -> None:
        config = config_func(**{k: getattr(self, k) for k in keys}, **kw)
        token = tokenize_config(config.dict())
        assert token in expected

    def test_backtest_main_cli(self, mocker) -> None:
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

    def test_simulation_modules(self) -> None:
        # Arrange
        interest_rate_data = TestDataProvider().read_csv("short-term-interest.csv")
        run_config = TestConfigStubs.backtest_run_config(
            catalog=self.catalog,
            instrument_ids=[self.instrument.id.value],
            venues=[
                BacktestVenueConfig(
                    name="SIM",
                    oms_type="HEDGING",
                    account_type="MARGIN",
                    starting_balances=["1_000_000 USD"],
                    modules=[
                        ImportableActorConfig(  # type: ignore
                            actor_path=FXRolloverInterestModule.fully_qualified_name(),
                            config_path=FXRolloverInterestConfig.fully_qualified_name(),
                            config={"rate_data": interest_rate_data},
                        ),
                    ],
                ),
            ],
        )
        node = BacktestNode([run_config])

        # Act
        engine = node._create_engine(
            run_config_id=run_config.id,
            config=run_config.engine,
            venue_configs=run_config.venues or [],
            data_configs=run_config.data or [],
        )

        # Assert
        assert engine
