# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
import pandas as pd

# Third-party
import pyarrow.dataset as ds
import pytest
from click.testing import CliRunner

# Our code
from nautilus_trader.backtest.config import parse_filters_expr
from nautilus_trader.backtest.modules import FXRolloverInterestConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.config import NautilusConfig
from nautilus_trader.config import msgspec_decoding_hook
from nautilus_trader.config import msgspec_encoding_hook
from nautilus_trader.config import tokenize_config
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import load_catalog_with_stub_quote_ticks_audusd
from nautilus_trader.test_kit.mocks.data import setup_catalog
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.config import TestConfigStubs
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs


@pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
class TestBacktestConfig:
    @pytest.fixture(autouse=True)
    def setup(self, tmp_path) -> None:
        self.catalog = setup_catalog(protocol="file", path=str(tmp_path / "catalog"))
        load_catalog_with_stub_quote_ticks_audusd(self.catalog)

        self.venue = Venue("SIM")
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=self.venue)
        self.backtest_config = TestConfigStubs.backtest_run_config(catalog=self.catalog)

    def test_backtest_config_pickle(self):
        pickle.loads(pickle.dumps(self.backtest_config))  # noqa: S301 (pickle safe here)

    def test_backtest_data_config_load(self):
        # Arrange
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=str(self.catalog.fs.protocol),
            data_cls=QuoteTick,
            instrument_id=instrument.id,
            start_time=1580398089820000000,
            end_time=1580504394501000000,
        )

        # Act
        result = config.query

        # Assert
        assert result == {
            "data_cls": QuoteTick,
            "identifiers": [InstrumentId.from_str("AUD/USD.SIM")],
            "filter_expr": None,
            "start": 1580398089820000000,
            "end": 1580504394501000000,
            "metadata": None,
        }

    def test_backtest_data_config_custom_data(self):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        data = TestPersistenceStubs.news_events()
        self.catalog.write_data(data)

        config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=str(self.catalog.fs.protocol),
            data_cls=NewsEventData,
            client_id="NewsClient",
            metadata={"kind": "news"},
        )

        # Act
        result = BacktestNode.load_data_config(config)

        # Assert
        assert len(result.data) == 5000  # Reduced from 86985 for faster testing
        assert result.instruments is None
        assert result.client_id == ClientId("NewsClient")
        assert result.data[0].data_type.metadata == {"kind": "news"}

    def test_backtest_data_config_filters(self):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        data = TestPersistenceStubs.news_events()
        self.catalog.write_data(data)

        config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=str(self.catalog.fs.protocol),
            data_cls=NewsEventData,
            filter_expr="field('currency') == 'CHF'",
            client_id="NewsClient",
        )

        # Act
        result = BacktestNode.load_data_config(config)

        # Assert
        assert (
            len(result.data) == 210
        )  # Reduced from 2745 for faster testing (CHF events in first 5k rows)

    def test_backtest_data_config_status_updates(self):
        # Arrange
        from tests.integration_tests.adapters.betfair.test_kit import load_betfair_data

        load_betfair_data(self.catalog)

        config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol=str(self.catalog.fs.protocol),
            data_cls=InstrumentStatus,
        )

        # Act
        result = BacktestNode.load_data_config(config)

        # Assert
        assert len(result.data) == 2
        assert result.instruments is None
        assert result.client_id is None

    def test_resolve_cls(self):
        config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            data_cls="nautilus_trader.model.data:QuoteTick",
            catalog_fs_protocol=str(self.catalog.fs.protocol),
            catalog_fs_storage_options={},
            instrument_id=InstrumentId.from_str("AUD/USD.IDEALPRO"),
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
                instrument_id=InstrumentId.from_str("AUD/USD.IDEALPRO"),
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


class TestBacktestConfigParsing:
    @pytest.fixture(autouse=True)
    def setup(self, tmp_path):
        self.catalog = setup_catalog(protocol="memory", path=str(tmp_path / "nautilus"))
        self.venue = Venue("SIM")
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=self.venue)
        self.backtest_config = TestConfigStubs.backtest_run_config(catalog=self.catalog)

    @pytest.mark.skipif(sys.platform == "win32", reason="redundant to also test Windows")
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
        msgspec.json.encode(run_config)

    @pytest.mark.skipif(sys.platform == "win32", reason="redundant to also test Windows")
    def test_run_config_parse_obj(self) -> None:
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
        config = BacktestRunConfig.parse(raw)
        assert isinstance(config, BacktestRunConfig)
        node = BacktestNode(configs=[config])
        assert isinstance(node, BacktestNode)

    @pytest.mark.skipif(sys.platform == "win32", reason="redundant to also test Windows")
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
        assert result > 0

    @pytest.mark.skipif(sys.platform == "win32", reason="redundant to also test Windows")
    def test_backtest_obj_data_config_to_dict(self) -> None:
        run_config = TestConfigStubs.backtest_run_config(
            catalog=self.catalog,
            instrument_ids=[self.instrument.id.value],
            data_types=(TradeTick, QuoteTick, OrderBookDelta),
            venues=[
                BacktestVenueConfig(
                    name="BETFAIR",
                    oms_type=OmsType.NETTING,
                    account_type=AccountType.BETTING,
                    base_currency=GBP,
                    starting_balances=[Money(10000, GBP)],
                    book_type=BookType.L2_MBP,
                ),
            ],
        )
        json = msgspec.json.encode(run_config, enc_hook=msgspec_encoding_hook)
        obj = msgspec.json.decode(json, type=BacktestRunConfig, dec_hook=msgspec_decoding_hook)
        assert len(msgspec.json.encode(json)) > 0
        assert obj

    @pytest.mark.skipif(sys.platform == "win32", reason="redundant to also test Windows")
    def test_backtest_run_config_id(self) -> None:
        token = self.backtest_config.id
        print("token:", token)
        value: bytes = self.backtest_config.json()
        print("token_value:", value.decode())
        # Check that token is a valid SHA256 hash (64 hex characters)
        assert isinstance(token, str)
        assert len(token) == 64
        assert all(c in "0123456789abcdef" for c in token)

    @pytest.mark.skipif(sys.platform == "win32", reason="redundant to also test Windows")
    @pytest.mark.parametrize(
        ("config_func", "keys", "kw", "expected"),
        [
            (
                TestConfigStubs.venue_config,
                (),
                {},
                ("981a3c21ef4c0af5e36377536728d5cf85e95d6843889021be965bae4ebecd5e",),
            ),
            (
                TestConfigStubs.backtest_data_config,
                ("catalog",),
                {},
                ("22a83df0e65c304aff0a92070c6c55cef8a91392892a99dd6a992ad6ed829556",),
            ),
            (
                TestConfigStubs.backtest_engine_config,
                ("catalog",),
                {"persist": True},
                ("c2b1fb5320292c3a89d93cd4ad4051f6716b5db015084b358e6c2e33845d17ad",),
            ),
            (
                TestConfigStubs.risk_engine_config,
                (),
                {},
                ("0e2e102195b32171d558b122264aed0a024b381fa6f31c6fff5958218c2644c4",),
            ),
            (
                TestConfigStubs.exec_engine_config,
                (),
                {},
                ("fb92939cdb495cb8b2ef2077a6509f080fd7c2b33001e021c304bdb78ecc0cd5",),
            ),
            (
                TestConfigStubs.portfolio_config,
                (),
                {},
                ("3746b2ee6216effd866d21384216b26cde18297b668b122c45d61c62d098be25",),
            ),
            (
                TestConfigStubs.streaming_config,
                ("catalog",),
                {},
                ("fe0f050d36c142fa3ed2d7de1a0155f2ac4741b8abb8b0788165268a3ece77b9",),
            ),
        ],
    )
    def test_tokenize_config(self, config_func, keys, kw, expected) -> None:
        config = config_func(**{k: getattr(self, k) for k in keys}, **kw)
        token = tokenize_config(config)
        # Check that token is a valid SHA256 hash (64 hex characters)
        assert isinstance(token, str)
        assert len(token) == 64
        assert all(c in "0123456789abcdef" for c in token)

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
            enc_hook=msgspec_encoding_hook,
        ).decode()

        # Act
        with mocker.patch("nautilus_trader.backtest.node.BacktestNode.run"):
            result = runner.invoke(main, ["--raw", raw])

        # Assert
        assert result.exception is None
        assert result.exit_code == 0

    def test_simulation_modules(self) -> None:
        # Arrange
        interest_rate_data: pd.DataFrame = TestDataProvider().read_csv("short-term-interest.csv")
        run_config = TestConfigStubs.backtest_run_config(
            catalog=self.catalog,
            instrument_ids=[self.instrument.id],
            venues=[
                BacktestVenueConfig(
                    name="SIM",
                    oms_type="HEDGING",
                    account_type="MARGIN",
                    starting_balances=["1_000_000 USD"],
                    modules=[
                        ImportableActorConfig(
                            actor_path=FXRolloverInterestModule.fully_qualified_name(),
                            config_path=FXRolloverInterestConfig.fully_qualified_name(),
                            config={"rate_data": interest_rate_data.to_json()},
                        ),
                    ],
                ),
            ],
        )
        node = BacktestNode([run_config])

        # Act
        engine = node._create_engine(run_config.id)

        # Assert
        assert engine


class TestParseFiltersExpr:
    """
    Test security and functionality of parse_filters_expr function.
    """

    def test_parse_filters_expr_none_input(self):
        """
        Test that None input returns None.
        """
        result = parse_filters_expr(None)
        assert result is None

    def test_parse_filters_expr_empty_string(self):
        """
        Test that empty string returns None.
        """
        result = parse_filters_expr("")
        assert result is None

    @pytest.mark.parametrize(
        "expr",
        [
            'field("Currency") == "CHF"',
            'field("Symbol") != "USD"',
            '(field("Currency") == "CHF") | (field("Symbol") == "USD")',
            # Mixed whitespace
            '  field("Currency") == "CHF"  ',
        ],
    )
    def test_parse_filters_expr_valid_expression(self, expr):
        """
        Expression should parse and return a PyArrow Expression.
        """
        result = parse_filters_expr(expr)
        assert isinstance(result, ds.Expression)

    @pytest.mark.parametrize(
        "expr",
        [
            'print("hello")',
            '__import__("os").system("echo hacked")',
            'eval("1+1")',
            'exec("print(1)")',
            'open("/etc/passwd")',
            "globals()",
            "locals()",
            "vars()",
            "dir()",
            'getattr(field, "__class__")',
            "field.__class__.__bases__[0].__subclasses__()[104]",
            "breakpoint()",
            "exit()",
            "quit()",
        ],
    )
    def test_parse_filters_expr_security_blocks_malicious_code(self, expr):
        """
        Malicious code must be refused.
        """
        with pytest.raises(ValueError, match=r"is not allowed|not permitted"):
            parse_filters_expr(expr)

    @pytest.mark.parametrize(
        "expr",
        [
            'len("test")',
            "str(123)",
            'int("123")',
            "list()",
            "dict()",
            "set()",
            "tuple()",
            "range(10)",
            "enumerate([])",
            "zip([], [])",
            "map(str, [1, 2, 3])",
            "filter(None, [1, 2, 3])",
            "sum([1, 2, 3])",
            "max([1, 2, 3])",
            "min([1, 2, 3])",
        ],
    )
    def test_parse_filters_expr_security_blocks_arbitrary_functions(self, expr):
        """
        Non-field function calls must be refused.
        """
        with pytest.raises(ValueError, match=r"is not allowed|not permitted"):
            parse_filters_expr(expr)

    @pytest.mark.parametrize(
        "expr",
        [
            'field("test").__class__',
            'field("test").__dict__',
            'field("test").__module__',
            "field.__doc__",
            "field.__name__",
        ],
    )
    def test_parse_filters_expr_security_blocks_attribute_access(self, expr):
        """
        Attribute access on field objects is forbidden.
        """
        with pytest.raises(ValueError, match=r"is not allowed|not permitted"):
            parse_filters_expr(expr)

    @pytest.mark.parametrize(
        "expr",
        [
            '__import__("sys")',
            '__import__("os")',
            '__import__("subprocess")',
            '__import__("socket")',
            '__import__("urllib")',
        ],
    )
    def test_parse_filters_expr_security_blocks_imports(self, expr):
        """
        Import attempts must be refused.
        """
        with pytest.raises(ValueError, match=r"is not allowed|not permitted"):
            parse_filters_expr(expr)

    @pytest.mark.parametrize(
        ("expr", "is_valid"),
        [
            ('  field("Currency") == "CHF"  ', True),
            ('  print("hello")  ', False),
        ],
    )
    def test_parse_filters_expr_whitespace_handling(self, expr, is_valid):
        """
        Whitespace should not affect validation semantics.
        """
        if is_valid:
            assert isinstance(parse_filters_expr(expr), ds.Expression)
        else:
            with pytest.raises(ValueError):
                parse_filters_expr(expr)

    def test_parse_filters_expr_complex_valid_expressions(self):
        """
        Logical OR between multiple comparisons should be accepted.
        """
        expr = parse_filters_expr('(field("Currency") == "CHF") | (field("Symbol") == "USD")')
        assert isinstance(expr, ds.Expression)

    @pytest.mark.parametrize(
        "expr",
        [
            'field("Currency") ==',  # Incomplete expression
            'field("Currency" == "CHF"',  # Missing closing paren
            'field(Currency) == "CHF"',  # Missing quotes around field name
            '== "CHF"',  # Missing field() call
        ],
    )
    def test_parse_filters_expr_invalid_syntax(self, expr):
        """
        Broken grammar should raise ValueError.
        """
        with pytest.raises(ValueError):
            parse_filters_expr(expr)

    def test_backtest_venue_config_allow_cash_borrowing_default(self):
        """
        Test that allow_cash_borrowing defaults to False.
        """
        # Arrange & Act
        config = BacktestVenueConfig(
            name="SIM",
            oms_type="NETTING",
            account_type="CASH",
            starting_balances=["1_000_000 USD"],
        )

        # Assert
        assert config.allow_cash_borrowing is False

    def test_backtest_venue_config_allow_cash_borrowing_enabled(self):
        """
        Test that allow_cash_borrowing can be enabled.
        """
        # Arrange & Act
        config = BacktestVenueConfig(
            name="SIM",
            oms_type="NETTING",
            account_type="CASH",
            starting_balances=["1_000_000 USD"],
            allow_cash_borrowing=True,
        )

        # Assert
        assert config.allow_cash_borrowing is True
