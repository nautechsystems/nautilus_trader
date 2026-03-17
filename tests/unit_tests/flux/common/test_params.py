from __future__ import annotations

import pytest

from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY
from nautilus_trader.flux.common.params import RuntimeParamSpec
from nautilus_trader.flux.strategies.makerv4 import runtime_params as makerv4_runtime_params
from nautilus_trader.flux.strategies.makerv4.runtime_params import MAKERV4_RUNTIME_PARAM_REGISTRY


def test_makerv3_registry_exposes_schema_defaults_and_hot_path_bounds() -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    schema = registry.schema
    defaults = registry.defaults

    assert registry.param_set == "makerv3"
    assert defaults["qty"] == 1.0
    assert defaults["n_orders1"] == 5
    assert defaults["distance1"] == 2.0
    assert defaults["max_age_ms"] == 10_000

    assert schema["n_orders1"] == {
        "type": "integer",
        "description": "Band 1 order depth per side.",
        "minimum": 0,
        "maximum": 20,
    }
    assert schema["distance1"]["minimum"] == 0.0
    assert schema["distance1"]["maximum"] == 500.0
    assert schema["max_age_ms"]["minimum"] == 1
    assert schema["max_age_ms"]["maximum"] == 60_000
    assert schema["quote_fail_critical_after_count"]["minimum"] == 0
    assert schema["quote_fail_critical_after_count"]["maximum"] == 100
    assert schema["max_cancels_per_side_per_cycle"]["advanced"] is True
    assert schema["max_places_per_side_per_cycle"]["advanced"] is True
    assert schema["max_total_actions_per_cycle"]["advanced"] is True
    assert schema["max_pending_cancels_per_side"]["advanced"] is True
    assert "advanced" not in schema["n_orders1"]


def test_makerv4_registry_exposes_explicit_param_set_with_compatible_schema() -> None:
    registry = MAKERV4_RUNTIME_PARAM_REGISTRY

    assert registry.param_set == "makerv4"
    assert registry.defaults["qty"] == 1.0
    assert registry.defaults["max_qty_global"] == 100.0
    assert registry.defaults["instant_hedge_enabled"] is True
    assert registry.defaults["maker_fee_source"] == "hyperliquid_api"
    assert registry.schema["n_orders1"]["type"] == MAKERV3_RUNTIME_PARAM_REGISTRY.schema["n_orders1"]["type"]
    assert registry.schema["hedge_style"] == {
        "type": "select",
        "description": "Immediate-hedge execution style.",
        "options": [["ioc_through_mid", "IOC Through Mid"]],
    }


@pytest.mark.parametrize(
    "name",
    [
        "bid_edge1",
        "ask_edge1",
        "bid_edge2",
        "ask_edge2",
        "bid_edge3",
        "ask_edge3",
    ],
)
def test_makerv3_registry_allows_signed_bid_and_ask_edges(name: str) -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    assert registry.schema[name]["minimum"] == -100.0
    assert registry.schema[name]["maximum"] == 1_000.0
    assert registry.coerce_updates({name: -100}) == {name: -100.0}


def test_registry_coerce_updates_normalizes_bool_int_and_number_types() -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    coerced = registry.coerce_updates(
        {
            "bot_on": "true",
            "n_orders1": "7",
            "distance1": b"2.25",
            "qty": "1250.5",
        },
    )

    assert coerced == {
        "bot_on": True,
        "n_orders1": 7,
        "distance1": 2.25,
        "qty": 1250.5,
    }


def test_registry_coerce_updates_rejects_unknown_runtime_param() -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    with pytest.raises(ValueError, match="Unsupported runtime param"):
        registry.coerce_updates({"unknown_param": 1})


@pytest.mark.parametrize(
    ("name", "value"),
    [
        ("n_orders1", 21),
        ("distance1", -0.1),
        ("max_age_ms", 0),
        ("quote_fail_critical_after_count", 101),
    ],
)
def test_registry_coerce_updates_rejects_out_of_bounds_values(name: str, value: object) -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    with pytest.raises(ValueError, match=name):
        registry.coerce_updates({name: value})


def test_registry_diff_summary_is_deterministic_and_log_ready() -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    summary = registry.diff_summary(
        before={
            "n_orders1": "5",
            "distance1": "2.0",
            "bot_on": "0",
        },
        after={
            "n_orders1": "7",
            "distance1": "2.0",
            "bot_on": "1",
        },
    )

    assert summary == {
        "param_set": "makerv3",
        "changed_count": 2,
        "changed_keys": ["n_orders1", "bot_on"],
        "changes": [
            {"name": "n_orders1", "before": 5, "after": 7},
            {"name": "bot_on", "before": False, "after": True},
        ],
        "truncated": False,
        "summary": "n_orders1:5->7; bot_on:false->true",
    }


@pytest.mark.parametrize("default", [float("nan"), float("inf"), float("-inf")])
def test_runtime_param_spec_rejects_non_finite_numeric_default(default: float) -> None:
    with pytest.raises(ValueError, match="`default` must be finite"):
        RuntimeParamSpec(
            name="distance_x",
            schema_type="number",
            default=default,
            description="Distance.",
            minimum=0.0,
            maximum=10.0,
        )


def test_registry_diff_summary_validates_max_changes() -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    with pytest.raises(ValueError, match="max_changes"):
        registry.diff_summary(before={"n_orders1": 1}, after={"n_orders1": 2}, max_changes=0)


def test_registry_diff_summary_applies_truncation_semantics() -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    summary = registry.diff_summary(
        before={
            "n_orders1": "1",
            "n_orders2": "0",
            "n_orders3": "0",
        },
        after={
            "n_orders1": "2",
            "n_orders2": "1",
            "n_orders3": "1",
        },
        max_changes=2,
    )

    assert summary["changed_count"] == 3
    assert summary["changed_keys"] == ["n_orders1", "n_orders2", "n_orders3"]
    assert summary["changes"] == [
        {"name": "n_orders1", "before": 1, "after": 2},
        {"name": "n_orders2", "before": 0, "after": 1},
    ]
    assert summary["truncated"] is True
    assert summary["summary"] == "n_orders1:1->2; n_orders2:0->1"


@pytest.mark.parametrize(
    ("before", "after", "error_fragment"),
    [
        ({"unknown_key": "1"}, {"n_orders1": "1"}, "Unsupported runtime param in before"),
        ({"n_orders1": "1"}, {"unknown_key": "2"}, "Unsupported runtime param in after"),
    ],
)
def test_registry_diff_summary_rejects_unknown_keys(
    before: dict[str, str],
    after: dict[str, str],
    error_fragment: str,
) -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    with pytest.raises(ValueError, match=error_fragment):
        registry.diff_summary(before=before, after=after)


@pytest.mark.parametrize(
    ("updates", "error_fragment"),
    [
        ({"bot_on": "maybe"}, "Invalid boolean value"),
        ({"n_orders1": "1.5"}, "Invalid integer value"),
        ({"n_orders1": True}, "Invalid integer value"),
    ],
)
def test_registry_coerce_updates_rejects_invalid_bool_and_int_values(
    updates: dict[str, object],
    error_fragment: str,
) -> None:
    registry = MAKERV3_RUNTIME_PARAM_REGISTRY

    with pytest.raises(ValueError, match=error_fragment):
        registry.coerce_updates(updates)


def test_makerv4_runtime_params_stub_uses_distinct_param_set() -> None:
    assert makerv4_runtime_params.PARAM_SET == "makerv4"
    assert makerv4_runtime_params.RUNTIME_PARAM_DEFAULTS["qty"] == 1.0
    assert makerv4_runtime_params.RUNTIME_PARAM_DEFAULTS["hedge_style"] == "ioc_through_mid"
