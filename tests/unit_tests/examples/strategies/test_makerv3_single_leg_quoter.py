from decimal import Decimal
from importlib.util import module_from_spec
from importlib.util import spec_from_file_location
from pathlib import Path

import pytest

try:
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        build_ladder_targets,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        build_ladder_place_cancel_levels,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        build_ladder_place_cancel_levels_from_bps,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _bps_to_price_offset,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _clamp_post_only_price,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _decimal_to_json_str,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _did_bot_turn_off,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _parse_bool_text,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _coerce_runtime_param_value,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _should_publish_market_bbo,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _nudge_unique_price,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _round_price_to_tick,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        _serialize_account_payload,
        _serialize_position_payload,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        plan_side_rebalance_actions,
    )
except ModuleNotFoundError:
    module_path = (
        Path(__file__).resolve().parents[4]
        / "nautilus_trader"
        / "examples"
        / "strategies"
        / "makerv3_single_leg_quoter.py"
    )
    spec = spec_from_file_location("makerv3_single_leg_quoter", module_path)
    module = module_from_spec(spec)
    assert spec is not None and spec.loader is not None
    spec.loader.exec_module(module)
    build_ladder_targets = module.build_ladder_targets
    build_ladder_place_cancel_levels = module.build_ladder_place_cancel_levels
    build_ladder_place_cancel_levels_from_bps = module.build_ladder_place_cancel_levels_from_bps
    _bps_to_price_offset = module._bps_to_price_offset
    _round_price_to_tick = module._round_price_to_tick
    _clamp_post_only_price = module._clamp_post_only_price
    _decimal_to_json_str = module._decimal_to_json_str
    _did_bot_turn_off = module._did_bot_turn_off
    _parse_bool_text = module._parse_bool_text
    _coerce_runtime_param_value = module._coerce_runtime_param_value
    _should_publish_market_bbo = module._should_publish_market_bbo
    _nudge_unique_price = module._nudge_unique_price
    _serialize_account_payload = module._serialize_account_payload
    _serialize_position_payload = module._serialize_position_payload
    plan_side_rebalance_actions = module.plan_side_rebalance_actions


def test_build_ladder_targets_three_bands_is_deterministic():
    bid_prices, ask_prices = build_ladder_targets(
        anchor_bid=Decimal("100.0"),
        anchor_ask=Decimal("101.0"),
        bid_edges=(Decimal("0.10"), Decimal("0.30"), Decimal("0.80")),
        ask_edges=(Decimal("0.20"), Decimal("0.50"), Decimal("1.20")),
        distances=(Decimal("0.05"), Decimal("0.10"), Decimal("0.20")),
        n_orders=(2, 1, 3),
    )

    assert bid_prices == [
        Decimal("99.90"),
        Decimal("99.85"),
        Decimal("99.70"),
        Decimal("99.20"),
        Decimal("99.00"),
        Decimal("98.80"),
    ]
    assert ask_prices == [
        Decimal("101.20"),
        Decimal("101.25"),
        Decimal("101.50"),
        Decimal("102.20"),
        Decimal("102.40"),
        Decimal("102.60"),
    ]

    bid_prices2, ask_prices2 = build_ladder_targets(
        anchor_bid=Decimal("100.0"),
        anchor_ask=Decimal("101.0"),
        bid_edges=(Decimal("0.10"), Decimal("0.30"), Decimal("0.80")),
        ask_edges=(Decimal("0.20"), Decimal("0.50"), Decimal("1.20")),
        distances=(Decimal("0.05"), Decimal("0.10"), Decimal("0.20")),
        n_orders=(2, 1, 3),
    )
    assert bid_prices2 == bid_prices
    assert ask_prices2 == ask_prices


def test_build_ladder_targets_skips_empty_bands():
    bid_prices, ask_prices = build_ladder_targets(
        anchor_bid=Decimal("10"),
        anchor_ask=Decimal("10.5"),
        bid_edges=(Decimal("0.1"), Decimal("0.2"), Decimal("0.3")),
        ask_edges=(Decimal("0.1"), Decimal("0.2"), Decimal("0.3")),
        distances=(Decimal("0.01"), Decimal("0.02"), Decimal("0.03")),
        n_orders=(0, 2, 0),
    )

    assert bid_prices == [Decimal("9.8"), Decimal("9.78")]
    assert ask_prices == [Decimal("10.7"), Decimal("10.72")]


def test_build_ladder_targets_requires_three_band_params():
    with pytest.raises(ValueError, match="expected three bands"):
        build_ladder_targets(
            anchor_bid=Decimal("1"),
            anchor_ask=Decimal("2"),
            bid_edges=(Decimal("0.1"), Decimal("0.2")),
            ask_edges=(Decimal("0.1"), Decimal("0.2"), Decimal("0.3")),
            distances=(Decimal("0.01"), Decimal("0.02"), Decimal("0.03")),
            n_orders=(1, 1, 1),
        )


def test_build_ladder_place_cancel_levels_uses_place_buffer():
    bid_levels, ask_levels = build_ladder_place_cancel_levels(
        anchor_bid=Decimal("100"),
        anchor_ask=Decimal("101"),
        bid_edges=(Decimal("0.10"), Decimal("0.20"), Decimal("0.30")),
        ask_edges=(Decimal("0.10"), Decimal("0.20"), Decimal("0.30")),
        place_edges=(Decimal("0.02"), Decimal("0.03"), Decimal("0.04")),
        distances=(Decimal("0.05"), Decimal("0.10"), Decimal("0.20")),
        n_orders=(2, 1, 0),
    )

    assert bid_levels == [
        (Decimal("99.88"), Decimal("99.90")),
        (Decimal("99.83"), Decimal("99.85")),
        (Decimal("99.77"), Decimal("99.80")),
    ]
    assert ask_levels == [
        (Decimal("101.12"), Decimal("101.10")),
        (Decimal("101.17"), Decimal("101.15")),
        (Decimal("101.23"), Decimal("101.20")),
    ]


def test_build_ladder_place_cancel_levels_from_bps_matches_reference_anchor_pricing():
    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=Decimal("100"),
        anchor_ask=Decimal("101"),
        bid_edges_bps=(Decimal("10"), Decimal("20"), Decimal("30")),
        ask_edges_bps=(Decimal("10"), Decimal("20"), Decimal("30")),
        place_edges_bps=(Decimal("2"), Decimal("3"), Decimal("4")),
        distances_bps=(Decimal("5"), Decimal("10"), Decimal("20")),
        n_orders=(2, 1, 0),
    )

    assert bid_levels == [
        (Decimal("99.8800"), Decimal("99.900")),
        (Decimal("99.82975"), Decimal("99.84975")),
        (Decimal("99.7700"), Decimal("99.800")),
    ]
    assert ask_levels == [
        (Decimal("101.1212"), Decimal("101.101")),
        (Decimal("101.17145"), Decimal("101.15125")),
        (Decimal("101.2323"), Decimal("101.202")),
    ]


def test_build_ladder_place_cancel_levels_from_bps_applies_min_tick_distance_floor():
    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=Decimal("1"),
        anchor_ask=Decimal("1.01"),
        bid_edges_bps=(Decimal("10"), Decimal("0"), Decimal("0")),
        ask_edges_bps=(Decimal("10"), Decimal("0"), Decimal("0")),
        place_edges_bps=(Decimal("0"), Decimal("0"), Decimal("0")),
        distances_bps=(Decimal("0.1"), Decimal("0"), Decimal("0")),
        n_orders=(3, 0, 0),
        tick=Decimal("0.01"),
    )

    assert bid_levels[0][1] - bid_levels[1][1] == Decimal("0.01")
    assert bid_levels[1][1] - bid_levels[2][1] == Decimal("0.01")
    assert ask_levels[1][1] - ask_levels[0][1] == Decimal("0.01")
    assert ask_levels[2][1] - ask_levels[1][1] == Decimal("0.01")


def test_build_ladder_place_cancel_levels_from_bps_allows_negative_effective_edges():
    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=Decimal("100"),
        anchor_ask=Decimal("101"),
        bid_edges_bps=(Decimal("30"), Decimal("0"), Decimal("0")),
        ask_edges_bps=(Decimal("-10"), Decimal("0"), Decimal("0")),
        place_edges_bps=(Decimal("2"), Decimal("0"), Decimal("0")),
        distances_bps=(Decimal("0"), Decimal("0"), Decimal("0")),
        n_orders=(1, 0, 0),
    )

    assert bid_levels == [(Decimal("99.68"), Decimal("99.70"))]
    assert ask_levels == [(Decimal("100.9192"), Decimal("100.899"))]


def test_bps_to_price_offset_uses_1e4_denominator():
    offset = _bps_to_price_offset(Decimal("0.0094"), Decimal("10"))
    assert offset == Decimal("0.0000094")


def test_round_price_to_tick_supports_out_and_in_modes():
    tick = Decimal("0.01")
    price = Decimal("10.034")

    buy_out = _round_price_to_tick(price, tick=tick, is_buy=True, round_in=False)
    buy_in = _round_price_to_tick(price, tick=tick, is_buy=True, round_in=True)
    sell_out = _round_price_to_tick(price, tick=tick, is_buy=False, round_in=False)
    sell_in = _round_price_to_tick(price, tick=tick, is_buy=False, round_in=True)

    assert buy_out == Decimal("10.03")
    assert buy_in == Decimal("10.04")
    assert sell_out == Decimal("10.04")
    assert sell_in == Decimal("10.03")


def test_clamp_post_only_price_uses_tick_and_side():
    tick = Decimal("0.0001")

    bid_clamped = _clamp_post_only_price(
        price=Decimal("0.0094"),
        is_buy=True,
        top_bid=Decimal("0.0093"),
        top_ask=Decimal("0.0094"),
        tick=tick,
    )
    ask_clamped = _clamp_post_only_price(
        price=Decimal("0.0093"),
        is_buy=False,
        top_bid=Decimal("0.0093"),
        top_ask=Decimal("0.0094"),
        tick=tick,
    )

    assert bid_clamped == Decimal("0.0093")
    assert ask_clamped == Decimal("0.0094")


def test_nudge_unique_price_moves_less_aggressive_until_unique():
    buy_nudged = _nudge_unique_price(
        price=Decimal("0.0093"),
        tick=Decimal("0.0001"),
        is_buy=True,
        seen={"0.0093", "0.0092"},
    )
    sell_nudged = _nudge_unique_price(
        price=Decimal("0.0094"),
        tick=Decimal("0.0001"),
        is_buy=False,
        seen={"0.0094", "0.0095"},
    )

    assert buy_nudged == Decimal("0.0091")
    assert sell_nudged == Decimal("0.0096")


def test_decimal_to_json_str_avoids_float_artifacts():
    assert _decimal_to_json_str(Decimal("0.0093575")) == "0.0093575"
    assert _decimal_to_json_str(Decimal("18.701576275712977")) == "18.701576275712977"
    assert _decimal_to_json_str(None) is None


def test_did_bot_turn_off_detects_only_true_to_false_transition():
    assert _did_bot_turn_off(True, False) is True
    assert _did_bot_turn_off(True, True) is False
    assert _did_bot_turn_off(False, False) is False
    assert _did_bot_turn_off(False, True) is False


def test_parse_bool_text_supports_common_bot_flags():
    assert _parse_bool_text("1") is True
    assert _parse_bool_text("true") is True
    assert _parse_bool_text("on") is True
    assert _parse_bool_text("0") is False
    assert _parse_bool_text("false") is False
    assert _parse_bool_text("off") is False
    assert _parse_bool_text("nope") is None


def test_coerce_runtime_param_value_accepts_all_runtime_types():
    assert _coerce_runtime_param_value("bot_on", "1") is True
    assert _coerce_runtime_param_value("n_orders1", "5") == 5
    assert _coerce_runtime_param_value("distance1", "2.5") == Decimal("2.5")


def test_coerce_runtime_param_value_rejects_invalid_decimal():
    with pytest.raises(ValueError):
        _coerce_runtime_param_value("distance1", "not_a_number")


def test_should_publish_market_bbo_on_change_or_heartbeat():
    assert _should_publish_market_bbo(
        bbo_changed=True,
        last_publish_ns=0,
        now_ns=1,
        heartbeat_ms=1_000,
    ) is True
    assert _should_publish_market_bbo(
        bbo_changed=False,
        last_publish_ns=0,
        now_ns=1,
        heartbeat_ms=1_000,
    ) is True
    assert _should_publish_market_bbo(
        bbo_changed=False,
        last_publish_ns=1_000_000_000,
        now_ns=1_000_900_000,
        heartbeat_ms=1_000,
    ) is False
    assert _should_publish_market_bbo(
        bbo_changed=False,
        last_publish_ns=1_000_000_000,
        now_ns=2_000_000_000,
        heartbeat_ms=1_000,
    ) is True


def test_plan_side_rebalance_cancels_least_aggressive_for_missing_more_aggressive():
    cancel_indices, missing = plan_side_rebalance_actions(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99"), Decimal("98")],
        active_stale=[False, False, False],
        desired_levels=[
            (Decimal("101"), Decimal("101.5"), Decimal("0.01")),
            (Decimal("100"), Decimal("100.5"), Decimal("0.01")),
            (Decimal("99"), Decimal("99.5"), Decimal("0.01")),
        ],
        stale_cancel_budget=0,
    )

    assert cancel_indices == [2]
    assert missing == [0]


def test_plan_side_rebalance_ages_out_least_aggressive_stale_first():
    cancel_indices, missing = plan_side_rebalance_actions(
        side="sell",
        active_prices=[Decimal("100"), Decimal("101"), Decimal("102")],
        active_stale=[False, True, True],
        desired_levels=[
            (Decimal("100"), Decimal("99.5"), Decimal("0.01")),
            (Decimal("101"), Decimal("100.5"), Decimal("0.01")),
            (Decimal("102"), Decimal("101.5"), Decimal("0.01")),
        ],
        stale_cancel_budget=1,
    )

    assert cancel_indices == [2]
    assert missing == [2]


class _FakeMoney:
    def __init__(self, value: str) -> None:
        self._value = Decimal(value)

    def as_decimal(self) -> Decimal:
        return self._value


class _FakeCurrency:
    def __init__(self, code: str) -> None:
        self.code = code

    def __hash__(self) -> int:
        return hash(self.code)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, _FakeCurrency) and self.code == other.code


class _FakeAccount:
    def __init__(self) -> None:
        self.id = "BYBIT-001"
        self._usdt = _FakeCurrency("USDT")
        self._plume = _FakeCurrency("PLUME")

    def to_dict(self) -> dict[str, str]:
        raise AttributeError("simulated margin account serialization failure")

    def balances_total(self) -> dict[_FakeCurrency, _FakeMoney]:
        return {
            self._usdt: _FakeMoney("100.25"),
            self._plume: _FakeMoney("45"),
        }

    def balances_free(self) -> dict[_FakeCurrency, _FakeMoney]:
        return {
            self._usdt: _FakeMoney("95.25"),
            self._plume: _FakeMoney("40"),
        }

    def balances_locked(self) -> dict[_FakeCurrency, _FakeMoney]:
        return {
            self._usdt: _FakeMoney("5"),
            self._plume: _FakeMoney("5"),
        }


def test_serialize_account_payload_falls_back_when_to_dict_breaks():
    payload = _serialize_account_payload(_FakeAccount())

    assert payload["account_id"] == "BYBIT-001"
    assert isinstance(payload["events"], list)
    balances = payload["events"][0]["balances"]
    assert balances == [
        {"currency": "PLUME", "free": "40", "locked": "5", "total": "45"},
        {"currency": "USDT", "free": "95.25", "locked": "5", "total": "100.25"},
    ]


class _FakeAccountUnsafe:
    def __init__(self) -> None:
        self.id = "BYBIT-002"

    def to_dict(self) -> dict[str, object]:
        return {"bad_decimal": Decimal("12.34")}


def test_serialize_account_payload_sanitizes_unsafe_to_dict_output():
    payload = _serialize_account_payload(_FakeAccountUnsafe())

    assert payload["account_id"] == "BYBIT-002"
    assert "bad_decimal" not in payload


class _FakeInstrument:
    def __str__(self) -> str:
        return "PLUMEUSDT-LINEAR.BYBIT"


class _FakePosition:
    def __init__(self) -> None:
        self.position_id = "P-001"
        self.instrument_id = _FakeInstrument()
        self.side = "LONG"
        self.signed_qty = "-15.5"
        self.quantity = "15.5"
        self.avg_px_open = "12.34"
        self.avg_px_close = "12.90"
        self.realized_pnl = "-1.23"

    def to_dict(self) -> dict[str, str]:
        raise AttributeError("simulated position serialization failure")


def test_serialize_position_payload_falls_back_when_to_dict_breaks():
    payload = _serialize_position_payload(_FakePosition())

    assert payload["position_id"] == "P-001"
    assert payload["instrument_id"] == "PLUMEUSDT-LINEAR.BYBIT"
    assert payload["side"] == "LONG"
    assert payload["signed_qty"] == "-15.5"
    assert payload["realized_pnl"] == "-1.23"


class _FakePositionUnsafe:
    def to_dict(self) -> dict[str, object]:
        return {"signed_qty": Decimal("1.2")}


def test_serialize_position_payload_sanitizes_unsafe_to_dict_output():
    payload = _serialize_position_payload(_FakePositionUnsafe())

    assert "signed_qty" not in payload
    assert payload["type"] == "_FakePositionUnsafe"
