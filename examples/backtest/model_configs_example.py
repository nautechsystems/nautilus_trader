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


from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.config import ImportableFeeModelConfig
from nautilus_trader.backtest.config import ImportableFillModelConfig
from nautilus_trader.backtest.config import ImportableLatencyModelConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


if __name__ == "__main__":
    # Example strategy configuration
    strategy_config = ImportableStrategyConfig(
        strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
        config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
        config={
            "instrument_id": "AAPL.NASDAQ",
            "bar_type": "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL",
            "fast_ema_period": 10,
            "slow_ema_period": 20,
            "trade_size": 100,
        },
    )

    # Configure backtest engine
    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(log_level="INFO"),
        strategies=[strategy_config],
    )

    # Create importable fill model configs
    fill_model_config = ImportableFillModelConfig(
        fill_model_path="nautilus_trader.backtest.models:FillModel",
        config_path="nautilus_trader.backtest.config:FillModelConfig",
        config={
            "prob_fill_on_limit": 0.95,  # 95% chance of limit orders filling
            "prob_fill_on_stop": 0.98,  # 98% chance of stop orders filling
            "prob_slippage": 0.05,  # 5% chance of slippage
            "random_seed": 42,  # For reproducibility
        },
    )

    # Create importable latency model configs
    latency_model_config = ImportableLatencyModelConfig(
        latency_model_path="nautilus_trader.backtest.models:LatencyModel",
        config_path="nautilus_trader.backtest.config:LatencyModelConfig",
        config={
            "base_latency_nanos": 5_000_000,  # 5 milliseconds base latency
            "insert_latency_nanos": 2_000_000,  # Additional 2ms for inserts
            "update_latency_nanos": 3_000_000,  # Additional 3ms for updates
            "cancel_latency_nanos": 1_000_000,  # Additional 1ms for cancels
        },
    )

    # Example of different importable fee models
    maker_taker_fee_model_config = ImportableFeeModelConfig(
        fee_model_path="nautilus_trader.backtest.models:MakerTakerFeeModel",
        config_path="nautilus_trader.backtest.config:MakerTakerFeeModelConfig",
        config={},  # Empty config for MakerTakerFeeModel as it doesn't require parameters
    )

    fixed_fee_model_config = ImportableFeeModelConfig(
        fee_model_path="nautilus_trader.backtest.models:FixedFeeModel",
        config_path="nautilus_trader.backtest.config:FixedFeeModelConfig",
        config={
            "commission": "1.50 USD",
            "charge_commission_once": True,
        },
    )

    per_contract_fee_model_config = ImportableFeeModelConfig(
        fee_model_path="nautilus_trader.backtest.models:PerContractFeeModel",
        config_path="nautilus_trader.backtest.config:PerContractFeeModelConfig",
        config={
            "commission": "0.01 USD",
        },
    )

    # Another example with different parameters
    custom_fixed_fee_model_config = ImportableFeeModelConfig(
        fee_model_path="nautilus_trader.backtest.models:FixedFeeModel",
        config_path="nautilus_trader.backtest.config:FixedFeeModelConfig",
        config={
            "commission": "2.00 USD",
            "charge_commission_once": False,
        },
    )

    # Create venue configs with different models
    venue_config1 = BacktestVenueConfig(
        name="NASDAQ",
        oms_type="NETTING",
        account_type="CASH",
        base_currency="USD",
        starting_balances=["1000000 USD"],
        book_type="L1_MBP",
        fill_model=fill_model_config,
        latency_model=latency_model_config,
        fee_model=maker_taker_fee_model_config,
    )

    venue_config2 = BacktestVenueConfig(
        name="NYSE",
        oms_type="NETTING",
        account_type="CASH",
        base_currency="USD",
        starting_balances=["1000000 USD"],
        book_type="L1_MBP",
        fill_model=fill_model_config,
        latency_model=latency_model_config,
        fee_model=fixed_fee_model_config,
    )

    venue_config3 = BacktestVenueConfig(
        name="CME",
        oms_type="NETTING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1000000 USD"],
        book_type="L1_MBP",
        fill_model=fill_model_config,
        latency_model=latency_model_config,
        fee_model=per_contract_fee_model_config,
    )

    # Create venue config with custom fixed fee model
    venue_config4 = BacktestVenueConfig(
        name="BATS",
        oms_type="NETTING",
        account_type="CASH",
        base_currency="USD",
        starting_balances=["1000000 USD"],
        book_type="L1_MBP",
        fill_model=fill_model_config,
        latency_model=latency_model_config,
        fee_model=custom_fixed_fee_model_config,
    )

    # Create data config (this is just a placeholder - you would need actual data)
    data_config = BacktestDataConfig(
        catalog_path="./data",
        data_cls=QuoteTick,
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
    )

    # Create BacktestRunConfig
    run_config = BacktestRunConfig(
        engine=engine_config,
        venues=[venue_config1, venue_config2, venue_config3, venue_config4],
        data=[data_config],
    )

    # Create and run the backtest node
    node = BacktestNode(configs=[run_config])

    # Note: This example won't actually run without proper data
    # results = node.run()

    print("Example of using importable model configs in BacktestVenueConfig")
    print(
        f"Venue 1 uses ImportableFeeModelConfig with MakerTakerFeeModel: {venue_config1.fee_model}",
    )
    print(
        f"Venue 2 uses ImportableFeeModelConfig with FixedFeeModel: {venue_config2.fee_model.config}",
    )
    print(
        f"Venue 3 uses ImportableFeeModelConfig with PerContractFeeModel: {venue_config3.fee_model.config}",
    )
    print(
        f"Venue 4 uses ImportableFeeModelConfig with custom FixedFeeModel: {venue_config4.fee_model.config}",
    )
    print(f"Fill model config: {venue_config1.fill_model.config}")
    print(f"Latency model config: {venue_config1.latency_model.config}")
