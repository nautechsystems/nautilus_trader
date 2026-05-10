# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.common import CacheConfig
from nautilus_trader.common import MessageBusConfig
from nautilus_trader.data import DataEngineConfig
from nautilus_trader.execution import ExecutionEngineConfig
from nautilus_trader.live import PortfolioConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType
from nautilus_trader.risk import RiskEngineConfig


def test_engine_config_defaults():
    config = BacktestEngineConfig()
    assert config.load_state is False
    assert config.save_state is False
    assert config.bypass_logging is False
    assert config.run_analysis is True


def test_engine_config_with_params():
    config = BacktestEngineConfig(
        load_state=True,
        save_state=True,
        bypass_logging=True,
        run_analysis=False,
    )
    assert config.load_state is True
    assert config.save_state is True
    assert config.bypass_logging is True
    assert config.run_analysis is False


def test_engine_config_repr():
    config = BacktestEngineConfig()
    assert "BacktestEngineConfig" in repr(config)


def test_engine_config_sub_configs_default_to_none():
    config = BacktestEngineConfig()
    assert config.cache is None
    assert config.msgbus is None
    assert config.data_engine is None
    assert config.risk_engine is None
    assert config.exec_engine is None
    assert config.portfolio is None


def test_engine_config_accepts_sub_configs():
    data_engine = DataEngineConfig(debug=True)
    risk_engine = RiskEngineConfig(bypass=True, max_order_submit_rate="250/00:00:05")
    exec_engine = ExecutionEngineConfig(load_cache=False)
    cache = CacheConfig()
    msgbus = MessageBusConfig()
    portfolio = PortfolioConfig()

    config = BacktestEngineConfig(
        data_engine=data_engine,
        risk_engine=risk_engine,
        exec_engine=exec_engine,
        cache=cache,
        msgbus=msgbus,
        portfolio=portfolio,
    )

    assert config.data_engine is not None
    assert config.data_engine.debug is True
    assert config.risk_engine is not None
    assert config.risk_engine.bypass is True
    assert config.risk_engine.max_order_submit_rate == "250/00:00:05"
    assert config.exec_engine is not None
    assert config.exec_engine.load_cache is False
    assert config.cache is not None
    assert config.msgbus is not None
    assert config.portfolio is not None


def test_venue_config_required_params():
    config = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    assert config.name == "SIM"
    assert config.oms_type == OmsType.HEDGING
    assert config.account_type == AccountType.MARGIN
    assert config.book_type == BookType.L1_MBP
    assert config.starting_balances == ["1_000_000 USD"]


def test_venue_config_optional_params():
    config = BacktestVenueConfig(
        name="BINANCE",
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        book_type=BookType.L2_MBP,
        starting_balances=["10 BTC", "100_000 USDT"],
        bar_execution=False,
        trade_execution=False,
    )
    assert config.name == "BINANCE"
    assert config.bar_execution is False
    assert config.trade_execution is False


def test_venue_config_defaults():
    config = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    assert config.bar_execution is True
    assert config.trade_execution is True


def test_venue_config_repr():
    config = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    assert "BacktestVenueConfig" in repr(config)


def test_data_config_minimal():
    config = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
    )
    assert config.data_type == "QuoteTick"
    assert config.catalog_path == "/data/catalog"
    assert config.instrument_id is None


def test_data_config_with_instrument_id():
    instrument_id = InstrumentId.from_str("EUR/USD.SIM")
    config = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=instrument_id,
    )
    assert config.instrument_id == instrument_id


def test_data_config_invalid_data_type():
    with pytest.raises(ValueError, match="Invalid `NautilusDataType`"):
        BacktestDataConfig(
            data_type="InvalidType",
            catalog_path="/data/catalog",
        )


def test_data_config_repr():
    config = BacktestDataConfig(
        data_type="TradeTick",
        catalog_path="/data/catalog",
    )
    assert "BacktestDataConfig" in repr(config)


def test_run_config_auto_id():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    assert len(config.id) > 0


def test_run_config_explicit_id():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data], id="my-run-001")
    assert config.id == "my-run-001"


def test_run_config_with_engine():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    engine = BacktestEngineConfig(bypass_logging=True)
    config = BacktestRunConfig(venues=[venue], data=[data], engine=engine)
    assert len(config.id) > 0


def test_run_config_repr():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    assert "BacktestRunConfig" in repr(config)
