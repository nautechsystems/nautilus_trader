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
from datetime import datetime
from datetime import time
from datetime import timezone
from decimal import Decimal
from types import SimpleNamespace
from zoneinfo import ZoneInfo

from nautilus_trader.examples.strategies.sweep_strategy import WIDE_MODE_DURATION_MINUTES
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


class RecordingSweepStrategy(SweepStrategy):
    def __init__(self, config: SweepStrategyConfig) -> None:
        super().__init__(config)
        self.embargo = False
        self.calls = []

    def _is_market_boundary_embargo(self) -> bool:
        return self.embargo

    def cancel_all_orders(self, *args, **kwargs):
        self.calls.append(("cancel_all_orders", args, kwargs))

    def close_all_positions(self, *args, **kwargs):
        self.calls.append(("close_all_positions", args, kwargs))

    def cancel_order(self, *args, **kwargs):
        self.calls.append(("cancel_order", args, kwargs))


class ManualWideModeSweepStrategy(SweepStrategy):
    def __init__(self, config: SweepStrategyConfig) -> None:
        super().__init__(config)
        self.wide_mode = False

    def _is_wide_mode(self) -> bool:
        return self.wide_mode


class FakeInstrument:
    def make_qty(self, value: Decimal) -> Decimal:
        return value


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


def test_config_parse_accepts_order_notional_usd():
    # Arrange
    raw = json.dumps(
        {
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
            "order_notional_usd": 93,
        },
    )

    # Act
    config = SweepStrategyConfig.parse(raw)

    # Assert
    assert config.order_qty is None
    assert config.order_notional_usd == 93


def test_config_parse_rejects_ambiguous_order_sizing():
    # Arrange
    missing_size = json.dumps(
        {
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
        },
    )
    duplicate_size = json.dumps(
        {
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
            "order_qty": "0.001",
            "order_notional_usd": 93,
        },
    )

    # Act, Assert
    try:
        SweepStrategyConfig.parse(missing_size)
    except ValueError as exc:
        assert "Either order_qty or order_notional_usd" in str(exc)
    else:
        raise AssertionError("Expected missing order size to be rejected")

    try:
        SweepStrategyConfig.parse(duplicate_size)
    except ValueError as exc:
        assert "Only one of order_qty or order_notional_usd" in str(exc)
    else:
        raise AssertionError("Expected duplicate order size to be rejected")


def test_order_notional_usd_derives_quantity_from_mid():
    # Arrange
    config = SweepStrategyConfig.parse(
        json.dumps(
            {
                "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
                "order_notional_usd": 93,
            },
        ),
    )
    strategy = SweepStrategy(config=config)
    strategy._instrument = FakeInstrument()

    # Act, Assert
    assert strategy._quote_quantity(Decimal("31")) == Decimal("3")


def test_config_parse_accepts_optional_wide_mode_quote_offset():
    # Arrange
    raw = json.dumps(
        {
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
            "order_qty": "0.001",
            "wide_mode_quote_offset_bps": 25.5,
        },
    )

    # Act
    config = SweepStrategyConfig.parse(raw)
    default_config = _config()

    # Assert
    assert config.wide_mode_quote_offset_bps == 25.5
    assert default_config.wide_mode_quote_offset_bps is None


def test_config_parse_accepts_market_open_embargo_settings():
    # Arrange
    raw = json.dumps(
        {
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
            "order_qty": "0.001",
            "market_open_embargo_minutes": 5,
            "market_open_embargo_pre_open_minutes": 1,
            "market_open_embargo_timezone": "America/New_York",
            "market_open_embargo_start": "09:30:00",
            "market_after_hours_embargo_minutes": 4,
            "market_after_hours_embargo_pre_start_minutes": 1,
            "market_after_hours_embargo_start": "16:00:00",
            "close_positions_on_embargo": False,
            "reduce_only_on_embargo": True,
        },
    )

    # Act
    config = SweepStrategyConfig.parse(raw)

    # Assert
    assert config.market_open_embargo_minutes == 5
    assert config.market_open_embargo_pre_open_minutes == 1
    assert config.market_open_embargo_timezone == "America/New_York"
    assert config.market_open_embargo_start == "09:30:00"
    assert config.market_after_hours_embargo_minutes == 4
    assert config.market_after_hours_embargo_pre_start_minutes == 1
    assert config.market_after_hours_embargo_start == "16:00:00"
    assert not config.close_positions_on_embargo
    assert config.reduce_only_on_embargo


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


def test_market_open_embargo_window_uses_configured_timezone():
    # Arrange
    tz = ZoneInfo("America/New_York")
    start = time(9, 30)
    minutes = Decimal(5)

    # Act, Assert
    assert SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 14, 29, tzinfo=timezone.utc),
        tz,
        start,
        minutes,
        Decimal(1),
    )
    assert SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 14, 30, tzinfo=timezone.utc),
        tz,
        start,
        minutes,
        Decimal(1),
    )
    assert SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 14, 34, 59, tzinfo=timezone.utc),
        tz,
        start,
        minutes,
        Decimal(1),
    )
    assert not SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 14, 28, 59, tzinfo=timezone.utc),
        tz,
        start,
        minutes,
        Decimal(1),
    )
    assert not SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 14, 35, tzinfo=timezone.utc),
        tz,
        start,
        minutes,
        Decimal(1),
    )
    assert not SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 3, 14, 30, tzinfo=timezone.utc),
        tz,
        start,
        minutes,
        Decimal(1),
    )


def test_market_after_hours_embargo_window_covers_after_hours_boundary():
    # Arrange
    tz = ZoneInfo("America/New_York")
    after_hours_start = time(16, 0)

    # Act, Assert
    assert SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 20, 59, tzinfo=timezone.utc),
        tz,
        after_hours_start,
        Decimal(4),
        Decimal(1),
    )
    assert SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 21, 3, 59, tzinfo=timezone.utc),
        tz,
        after_hours_start,
        Decimal(4),
        Decimal(1),
    )
    assert not SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 20, 58, 59, tzinfo=timezone.utc),
        tz,
        after_hours_start,
        Decimal(4),
        Decimal(1),
    )
    assert not SweepStrategy._datetime_in_market_boundary_embargo(
        datetime(2026, 1, 5, 21, 4, tzinfo=timezone.utc),
        tz,
        after_hours_start,
        Decimal(4),
        Decimal(1),
    )


def test_wide_mode_window_covers_first_half_hour_after_boundaries():
    # Arrange
    tz = ZoneInfo("America/New_York")
    market_open = time(9, 30)
    after_hours_start = time(16, 0)

    # Act, Assert
    assert SweepStrategy._datetime_in_market_boundary_window(
        datetime(2026, 1, 5, 14, 30, tzinfo=timezone.utc),
        tz,
        market_open,
        WIDE_MODE_DURATION_MINUTES,
    )
    assert SweepStrategy._datetime_in_market_boundary_window(
        datetime(2026, 1, 5, 14, 59, 59, tzinfo=timezone.utc),
        tz,
        market_open,
        WIDE_MODE_DURATION_MINUTES,
    )
    assert not SweepStrategy._datetime_in_market_boundary_window(
        datetime(2026, 1, 5, 14, 29, 59, tzinfo=timezone.utc),
        tz,
        market_open,
        WIDE_MODE_DURATION_MINUTES,
    )
    assert not SweepStrategy._datetime_in_market_boundary_window(
        datetime(2026, 1, 5, 15, 0, tzinfo=timezone.utc),
        tz,
        market_open,
        WIDE_MODE_DURATION_MINUTES,
    )
    assert SweepStrategy._datetime_in_market_boundary_window(
        datetime(2026, 1, 5, 21, 0, tzinfo=timezone.utc),
        tz,
        after_hours_start,
        WIDE_MODE_DURATION_MINUTES,
    )
    assert SweepStrategy._datetime_in_market_boundary_window(
        datetime(2026, 1, 5, 21, 29, 59, tzinfo=timezone.utc),
        tz,
        after_hours_start,
        WIDE_MODE_DURATION_MINUTES,
    )
    assert not SweepStrategy._datetime_in_market_boundary_window(
        datetime(2026, 1, 5, 21, 30, tzinfo=timezone.utc),
        tz,
        after_hours_start,
        WIDE_MODE_DURATION_MINUTES,
    )


def test_wide_mode_offset_forces_recenter_when_mode_changes():
    # Arrange
    config = _config(
        quote_offset_bps=10,
        wide_mode_quote_offset_bps=25,
        quote_recenter_threshold_bps=5,
    )
    strategy = ManualWideModeSweepStrategy(config=config)
    strategy._anchor_mid = Decimal("100")
    strategy._quote_offset_bps_in_use = Decimal("10")

    # Act, Assert
    assert strategy._active_quote_offset_bps() == Decimal("10")
    assert not strategy._should_recenter(Decimal("100"))

    strategy.wide_mode = True

    assert strategy._active_quote_offset_bps() == Decimal("25")
    assert strategy._should_recenter(Decimal("100"))

    strategy._quote_offset_bps_in_use = Decimal("25")

    assert not strategy._should_recenter(Decimal("100"))


def test_market_open_embargo_cuts_risk_adding_orders_once_and_resumes():
    # Arrange
    config = _config(market_open_embargo_minutes=5)
    strategy = RecordingSweepStrategy(config=config)
    bid_order = SimpleNamespace(is_closed=False, is_pending_cancel=False)
    ask_order = SimpleNamespace(is_closed=False, is_pending_cancel=False)
    unwind_order = SimpleNamespace(is_closed=False, is_pending_cancel=False)
    strategy._bid_order = bid_order
    strategy._ask_order = ask_order
    strategy._unwind_order = unwind_order

    # Act
    strategy.embargo = True
    assert strategy._handle_market_boundary_embargo()
    assert strategy._handle_market_boundary_embargo()

    strategy.embargo = False
    assert not strategy._handle_market_boundary_embargo()

    # Assert
    assert strategy.calls == [
        (
            "cancel_order",
            (bid_order,),
            {"client_id": config.client_id},
        ),
        (
            "cancel_order",
            (ask_order,),
            {"client_id": config.client_id},
        ),
    ]
    assert strategy._bid_order is None
    assert strategy._ask_order is None
    assert strategy._unwind_order is unwind_order
    assert not strategy._embargo_active


def test_market_open_embargo_can_close_positions_when_explicitly_enabled():
    # Arrange
    config = _config(market_open_embargo_minutes=5, close_positions_on_embargo=True)
    strategy = RecordingSweepStrategy(config=config)
    strategy._bid_order = SimpleNamespace(is_closed=False, is_pending_cancel=False)
    strategy._ask_order = SimpleNamespace(is_closed=False, is_pending_cancel=False)
    strategy._unwind_order = SimpleNamespace(is_closed=False, is_pending_cancel=False)
    strategy._inventory_to_unwind = Decimal("1")

    # Act
    strategy.embargo = True
    assert strategy._handle_market_boundary_embargo()

    # Assert
    assert strategy.calls == [
        (
            "cancel_all_orders",
            (config.instrument_id,),
            {"client_id": config.client_id},
        ),
        (
            "close_all_positions",
            (config.instrument_id,),
            {
                "client_id": config.client_id,
                "reduce_only": config.reduce_only_on_embargo,
            },
        ),
    ]
    assert strategy._bid_order is None
    assert strategy._ask_order is None
    assert strategy._unwind_order is None
    assert strategy._inventory_to_unwind == 0


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
