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
from pathlib import Path

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import StrategyId
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
        "RustTestCustomData",
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
