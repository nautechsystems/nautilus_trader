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

from decimal import Decimal

import pytest

from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.backtest import FXRolloverInterestModule
from nautilus_trader.backtest import InterestRateRecord
from nautilus_trader.common import CacheConfig
from nautilus_trader.common import LoggerConfig
from nautilus_trader.common import MessageBusConfig
from nautilus_trader.core import UUID4
from nautilus_trader.data import DataEngineConfig
from nautilus_trader.execution import BestPriceFillModel
from nautilus_trader.execution import CappedOptionFeeModel
from nautilus_trader.execution import ExecutionEngineConfig
from nautilus_trader.execution import StaticLatencyModel
from nautilus_trader.execution import TieredNotionalOptionFeeModel
from nautilus_trader.live import PortfolioConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BarAggregation
from nautilus_trader.model import BarSpecification
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientId
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType
from nautilus_trader.model import OtoTriggerMode
from nautilus_trader.model import PriceType
from nautilus_trader.model import StandardMarginModel
from nautilus_trader.risk import RiskEngineConfig
from nautilus_trader.trading import ImportableControllerConfig


def test_engine_config_defaults():
    config = BacktestEngineConfig()
    assert config.load_state is False
    assert config.save_state is False
    assert config.shutdown_on_error is False
    assert config.bypass_logging is False
    assert config.run_analysis is True
    assert config.timeout_connection == 60.0


def test_engine_config_with_params():
    instance_id = UUID4()
    logging = LoggerConfig(print_config=True)
    config = BacktestEngineConfig(
        load_state=True,
        save_state=True,
        shutdown_on_error=True,
        bypass_logging=True,
        run_analysis=False,
        logging=logging,
        instance_id=instance_id,
    )
    assert config.load_state is True
    assert config.save_state is True
    assert config.shutdown_on_error is True
    assert config.bypass_logging is True
    assert config.run_analysis is False
    assert config.logging.print_config is True
    assert config.instance_id == instance_id


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


def test_engine_config_accepts_controller_config():
    controller = ImportableControllerConfig(
        controller_path="tests.unit.common.actor:StrategyCreatingController",
        config_path="tests.unit.common.actor:TestControllerConfig",
        config={"actor_id": "Controller-001"},
    )

    config = BacktestEngineConfig(controller=controller)

    assert config.controller is not None
    assert config.controller.controller_path == "tests.unit.common.actor:StrategyCreatingController"


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
    instrument_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    fill_model = BestPriceFillModel(prob_fill_on_limit=0.9, prob_slippage=0.1)
    latency_model = StaticLatencyModel(base_latency_nanos=1_000)
    margin_model = StandardMarginModel()
    module = FXRolloverInterestModule(
        records=[InterestRateRecord(location="USD", time="17:00", value=0.01)],
    )
    config = BacktestVenueConfig(
        name="BINANCE",
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        book_type=BookType.L2_MBP,
        starting_balances=["10 BTC", "100_000 USDT"],
        routing=True,
        frozen_account=True,
        reject_stop_orders=True,
        support_gtd_orders=False,
        support_contingent_orders=False,
        use_position_ids=False,
        use_random_ids=True,
        use_reduce_only=True,
        bar_execution=False,
        bar_adaptive_high_low_ordering=True,
        trade_execution=False,
        use_market_order_acks=True,
        liquidity_consumption=False,
        allow_cash_borrowing=True,
        queue_position=True,
        oto_trigger_mode=OtoTriggerMode.FULL,
        base_currency=Currency.from_str("USDT"),
        default_leverage=Decimal(5),
        leverages={instrument_id: Decimal(3)},
        margin_model=margin_model,
        modules=[module],
        fill_model=fill_model,
        latency_model=latency_model,
        price_protection_points=7,
        settlement_prices={instrument_id: 50_000.0},
    )
    assert config.name == "BINANCE"
    assert config.routing is True
    assert config.frozen_account is True
    assert config.reject_stop_orders is True
    assert config.support_gtd_orders is False
    assert config.support_contingent_orders is False
    assert config.use_position_ids is False
    assert config.use_random_ids is True
    assert config.use_reduce_only is True
    assert config.bar_execution is False
    assert config.bar_adaptive_high_low_ordering is True
    assert config.trade_execution is False
    assert config.use_market_order_acks is True
    assert config.liquidity_consumption is False
    assert config.allow_cash_borrowing is True
    assert config.queue_position is True
    assert config.oto_trigger_mode == OtoTriggerMode.FULL
    assert config.base_currency == Currency.from_str("USDT")
    assert config.default_leverage == Decimal(5)
    assert config.leverages == {instrument_id: Decimal(3)}
    assert isinstance(config.margin_model, StandardMarginModel)
    assert isinstance(config.fill_model, BestPriceFillModel)
    assert isinstance(config.latency_model, StaticLatencyModel)
    assert len(config.modules) == 1
    assert isinstance(config.modules[0], FXRolloverInterestModule)
    assert config.fee_model is None
    assert config.price_protection_points == 7
    assert config.settlement_prices == {instrument_id: 50_000.0}


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


@pytest.mark.parametrize(
    ("fee_model", "expected_repr"),
    [
        (
            CappedOptionFeeModel(
                maker_rate=Decimal("0.0001"),
                taker_rate=Decimal("0.0003"),
            ),
            "fee_model: Some(CappedOption(",
        ),
        (
            TieredNotionalOptionFeeModel(
                maker_rate=Decimal("0.0002"),
                taker_rate=Decimal("0.0005"),
            ),
            "fee_model: Some(TieredNotionalOption(",
        ),
    ],
)
def test_venue_config_accepts_option_fee_models(fee_model, expected_repr):
    config = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
        fee_model=fee_model,
    )

    assert expected_repr in repr(config)
    assert isinstance(config.fee_model, type(fee_model))


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
    instrument_id = InstrumentId.from_str("EUR/USD.SIM")
    config = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=instrument_id,
    )
    assert config.data_type == "QuoteTick"
    assert config.catalog_path == "/data/catalog"
    assert config.instrument_id == instrument_id


def test_data_config_requires_identifier():
    with pytest.raises(ValueError, match="instrument_id"):
        BacktestDataConfig(
            data_type="QuoteTick",
            catalog_path="/data/catalog",
        )


def test_data_config_with_instrument_id():
    instrument_id = InstrumentId.from_str("EUR/USD.SIM")
    config = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=instrument_id,
    )
    assert config.instrument_id == instrument_id


def test_data_config_readback_redacts_storage_option_values():
    instrument_id = InstrumentId.from_str("EUR/USD.SIM")
    client_id = ClientId("CATALOG")
    bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
    config = BacktestDataConfig(
        data_type="Bar",
        catalog_path="/data/catalog",
        catalog_fs_protocol="s3",
        catalog_fs_storage_options={"access_key": "secret"},
        catalog_fs_rust_storage_options={"region": "ap-southeast-2"},
        instrument_ids=[instrument_id],
        start_time=1,
        end_time=2,
        filter_expr="field('price') > 0",
        client_id=client_id,
        metadata={"source": "historical"},
        bar_spec=bar_spec,
        bar_types=["EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL"],
        optimize_file_loading=True,
    )

    assert config.catalog_fs_protocol == "s3"
    assert config.catalog_fs_storage_option_keys == ["access_key"]
    assert config.catalog_fs_rust_storage_option_keys == ["region"]
    assert not hasattr(config, "catalog_fs_storage_options")
    assert not hasattr(config, "catalog_fs_rust_storage_options")
    assert config.instrument_ids == [instrument_id]
    assert config.start_time == 1
    assert config.end_time == 2
    assert config.filter_expr == "field('price') > 0"
    assert config.client_id == client_id
    assert config.metadata == {"source": "historical"}
    assert config.bar_spec == bar_spec
    assert config.bar_types == ["EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL"]
    assert config.optimize_file_loading is True


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
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
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
    assert len(config.venues) == 1
    assert len(config.data) == 1
    assert config.engine.bypass_logging is True


def test_run_config_options_are_readable():
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
    config = BacktestRunConfig(
        venues=[venue],
        data=[data],
        chunk_size=100,
        raise_exception=True,
        dispose_on_completion=False,
        start=1,
        end=2,
    )

    assert config.chunk_size == 100
    assert config.raise_exception is True
    assert config.dispose_on_completion is False
    assert config.start == 1
    assert config.end == 2


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


def test_run_config_chunk_size_zero_rejected():
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
    with pytest.raises(ValueError, match="chunk_size"):
        BacktestRunConfig(venues=[venue], data=[data], chunk_size=0)
