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
"""
Test backtest engine custom data behavior.
"""

import json
from datetime import UTC
from datetime import datetime
from decimal import Decimal
from pathlib import Path

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import GreeksCalculator
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import AssetClass
from nautilus_trader.model import Currency
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import Equity
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OmsType
from nautilus_trader.model import OptionContract
from nautilus_trader.model import OptionKind
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue
from nautilus_trader.model import YieldCurve
from nautilus_trader.model import custom_data_backend_kind
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import RustTestCustomData
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class _CustomDataStrategy(Strategy):
    def on_start(self) -> None:
        """
        On start.
        """
        self.subscribe_data(self.data_type)

    def on_data(self, data: CustomData) -> None:
        """
        On data.
        """
        self.received.append(data)


class _YieldCurveStrategy(_CustomDataStrategy):
    def on_data(self, data: CustomData) -> None:
        """
        On data.
        """
        self.cache.add_yield_curve(data.data)
        super().on_data(data)


class _YieldCurveGreeksStrategy(_YieldCurveStrategy):
    def on_stop(self) -> None:
        """
        On stop.
        """
        calculator = GreeksCalculator(self.cache, self.clock)
        self.greeks = calculator.instrument_greeks(
            self.option_id,
            flat_interest_rate=0.0425,
            ts_event=self.valuation_ns,
        )


def test_catalog_custom_data_reaches_backtest_strategy(tmp_path: Path) -> None:
    """
    Test catalog custom data reaches backtest strategy.
    """
    register_custom_data_class(RustTestCustomData)
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    instrument_id = InstrumentId.from_str("CUSTOM.TEST")
    metadata = {"source": "python", "venue": "TEST"}
    data_type = DataType("RustTestCustomData", metadata, str(instrument_id))
    custom_data = [
        CustomData(
            data_type,
            RustTestCustomData(instrument_id, 1.25, True, 1, 1),
        ),
        CustomData(
            data_type,
            RustTestCustomData(instrument_id, 2.5, False, 2, 2),
        ),
    ]
    catalog.write_custom_data(custom_data)
    loaded = catalog.query_custom_data(
        NautilusDataType.Custom("RustTestCustomData"),
        identifiers=[str(instrument_id)],
    )

    assert [custom_data_backend_kind(item) for item in loaded] == ["native", "native"]
    assert [item.data_type.type_name for item in loaded] == ["RustTestCustomData"] * 2
    assert [item.data_type.metadata for item in loaded] == [metadata] * 2
    assert [item.data_type.identifier for item in loaded] == [str(instrument_id)] * 2
    assert [item.data.instrument_id for item in loaded] == [instrument_id] * 2
    assert [item.data.value for item in loaded] == [1.25, 2.5]
    assert [item.data.flag for item in loaded] == [True, False]
    assert [item.ts_event for item in loaded] == [1, 2]
    assert [item.ts_init for item in loaded] == [1, 2]

    encoded = loaded[0].to_json_bytes()
    envelope = json.loads(encoded)
    restored = CustomData.from_json_bytes(encoded)

    assert set(envelope) == {"type", "data_type", "payload"}
    assert envelope["payload"]["value"] == 1.25
    assert restored.data_type == loaded[0].data_type
    assert restored.data.instrument_id == instrument_id
    assert restored.data.value == 1.25
    assert restored.data.flag is True
    assert restored.ts_event == 1
    assert restored.ts_init == 1

    strategy = _CustomDataStrategy(
        StrategyConfig(
            strategy_id=StrategyId("CUSTOM-DATA-STRATEGY"),
            log_events=False,
            log_commands=False,
        ),
    )
    strategy.data_type = data_type
    strategy.received = []
    engine = BacktestEngine(
        BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        ),
    )
    engine.add_strategy(strategy)

    try:
        engine.add_data(list(reversed(loaded)), validate=True, sort=True)
        engine.run()
        result = engine.get_result()

        assert result.iterations == 2
        assert engine.backtest_start == 1
        assert engine.backtest_end == 2
        assert [item.data.value for item in strategy.received] == [1.25, 2.5]
        assert [item.ts_init for item in strategy.received] == [1, 2]
    finally:
        engine.dispose()


def test_catalog_yield_curves_replay_to_caching_strategy(tmp_path: Path) -> None:
    """
    Replay catalog yield curves in time order to a strategy that caches them.
    """
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    data_type = DataType("YieldCurve", {"curve_name": "USD"})
    catalog.write_custom_data(
        [
            CustomData(data_type, YieldCurve("USD", [0.0], [0.04], 2, 3)),
            CustomData(data_type, YieldCurve("USD", [0.0], [0.05], 1, 4)),
        ],
    )
    loaded = catalog.query_custom_data(NautilusDataType.Custom("YieldCurve"))
    strategy = _YieldCurveStrategy(
        StrategyConfig(
            strategy_id=StrategyId("YIELD-CURVE-STRATEGY"),
            log_events=False,
            log_commands=False,
        ),
    )
    strategy.data_type = data_type
    strategy.received = []
    engine = BacktestEngine(
        BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        ),
    )
    engine.add_strategy(strategy)

    try:
        engine.add_data(list(reversed(loaded)), validate=True, sort=True)
        engine.run()

        assert [item.data_type for item in strategy.received] == [data_type] * 2
        assert [item.data.interest_rates for item in strategy.received] == [[0.04], [0.05]]
        assert [item.ts_event for item in strategy.received] == [2, 1]
        assert [item.ts_init for item in strategy.received] == [3, 4]
    finally:
        engine.dispose()


def test_cached_yield_curve_sets_calculator_interest_rate() -> None:
    """
    Use a yield curve a strategy caches in place of the flat interest rate.
    """
    now_ns = int(datetime(2025, 3, 8, 12, tzinfo=UTC).timestamp() * 1_000_000_000)
    expiration_ns = int(datetime(2025, 4, 7, 12, tzinfo=UTC).timestamp() * 1_000_000_000)
    usd = Currency.from_str("USD")
    underlying = Equity(
        instrument_id=InstrumentId.from_str("AAPL.OPRA"),
        raw_symbol=Symbol("AAPL"),
        isin="US0378331005",
        currency=usd,
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    option = OptionContract(
        instrument_id=InstrumentId.from_str("AAPL250407C00149000.OPRA"),
        raw_symbol=Symbol("AAPL250407C00149000"),
        underlying="AAPL",
        asset_class=AssetClass.EQUITY,
        exchange="OPRA",
        currency=usd,
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        option_kind=OptionKind.CALL,
        strike_price=Price.from_str("149.00"),
        activation_ns=0,
        expiration_ns=expiration_ns,
        ts_event=0,
        ts_init=0,
    )
    data_type = DataType("YieldCurve", {"curve_name": "USD"})
    curve = YieldCurve("USD", [0.0], [0.07], now_ns - 1, now_ns - 1)
    quotes = [
        QuoteTick(
            option.id,
            Price.from_str("10.50"),
            Price.from_str("10.60"),
            Quantity.from_int(100),
            Quantity.from_int(100),
            now_ns,
            now_ns,
        ),
        QuoteTick(
            underlying.id,
            Price.from_str("150.00"),
            Price.from_str("150.10"),
            Quantity.from_int(100),
            Quantity.from_int(100),
            now_ns,
            now_ns,
        ),
    ]
    strategy = _YieldCurveGreeksStrategy(
        StrategyConfig(
            strategy_id=StrategyId("YIELD-CURVE-GREEKS-STRATEGY"),
            log_events=False,
            log_commands=False,
        ),
    )
    strategy.data_type = data_type
    strategy.received = []
    strategy.option_id = option.id
    strategy.valuation_ns = now_ns
    engine = BacktestEngine(
        BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
        ),
    )
    engine.add_venue(
        venue=Venue("OPRA"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[Money(1_000_000.0, usd)],
        fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
    )
    engine.add_instrument(underlying)
    engine.add_instrument(option)
    engine.add_strategy(strategy)

    try:
        engine.add_data([CustomData(data_type, curve), *quotes], validate=True, sort=True)
        engine.run()

        assert [item.data.interest_rates for item in strategy.received] == [[0.07]]
        assert strategy.greeks.interest_rate == 0.07
    finally:
        engine.dispose()
