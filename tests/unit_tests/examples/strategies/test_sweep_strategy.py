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

import json
from types import SimpleNamespace

from nautilus_trader.examples.strategies.sweep_strategy import SweepStrategy
from nautilus_trader.examples.strategies.sweep_strategy import SweepStrategyConfig
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.objects import Price


def _config(**overrides) -> SweepStrategyConfig:
    values = {
        "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
        "order_qty": "0.001",
    }
    values.update(overrides)
    return SweepStrategyConfig.parse(json.dumps(values))


def test_config_parse_accepts_fractional_bps():
    # Arrange
    raw = json.dumps(
        {
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
            "order_qty": "0.001",
            "quote_offset_bps": 0.2,
            "quote_recenter_threshold_bps": 0.1,
            "unwind_recenter_threshold_bps": 0.1,
        },
    )

    # Act
    config = SweepStrategyConfig.parse(raw)

    # Assert
    assert config.quote_offset_bps == 0.2
    assert config.quote_recenter_threshold_bps == 0.1
    assert config.unwind_recenter_threshold_bps == 0.1


def test_config_parse_accepts_legacy_recenter_threshold_bps():
    # Arrange
    raw = json.dumps(
        {
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
            "order_qty": "0.001",
            "recenter_threshold_bps": 0.1,
        },
    )

    # Act
    config = SweepStrategyConfig.parse(raw)

    # Assert
    assert config.quote_recenter_threshold_bps == 0.1


def test_unwind_recenter_threshold_requires_touch_drift():
    # Arrange
    config = _config(unwind_recenter_threshold_bps=0.5)
    strategy = SweepStrategy(config=config)
    strategy._unwind_order = SimpleNamespace(price=Price.from_str("100.00000"))

    # Act, Assert
    assert not strategy._should_recenter_unwind_order(Price.from_str("100.00400"))
    assert strategy._should_recenter_unwind_order(Price.from_str("100.00600"))


def test_reduce_only_would_increase_rejection_clears_unwind_target():
    # Arrange
    config = _config()
    strategy = SweepStrategy(config=config)
    client_order_id = ClientOrderId("O-20260630-215923-001-SWEEP-95")
    strategy._inventory_to_unwind = strategy.config.order_qty
    strategy._anchor_mid = Price.from_str("100.0").as_decimal()
    strategy._unwind_order = SimpleNamespace(client_order_id=client_order_id)
    event = SimpleNamespace(
        instrument_id=config.instrument_id,
        client_order_id=client_order_id,
        reason="bad request: Order submission rejected: Reduce only order would increase position",
    )

    # Act
    strategy.on_order_rejected(event)

    # Assert
    assert strategy._inventory_to_unwind == 0
    assert strategy._unwind_order is None
    assert strategy._anchor_mid is None


def test_other_unwind_rejection_keeps_unwind_target_for_retry():
    # Arrange
    config = _config()
    strategy = SweepStrategy(config=config)
    client_order_id = ClientOrderId("O-20260630-215923-001-SWEEP-95")
    strategy._inventory_to_unwind = strategy.config.order_qty
    strategy._unwind_order = SimpleNamespace(client_order_id=client_order_id)
    event = SimpleNamespace(
        instrument_id=config.instrument_id,
        client_order_id=client_order_id,
        reason="POST_ONLY_WOULD_EXECUTE",
    )

    # Act
    strategy.on_order_rejected(event)

    # Assert
    assert strategy._inventory_to_unwind == strategy.config.order_qty
    assert strategy._unwind_order is None
