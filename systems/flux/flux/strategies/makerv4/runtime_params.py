"""
Expose the canonical MakerV4 runtime param surface and defaults.
"""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from flux.common.params import MAKERV3_RUNTIME_PARAM_DEFAULTS
from flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY
from flux.common.params import RuntimeParamRegistry
from flux.common.params import RuntimeParamSpec
from flux.params.manager import FluxParamsManager
from flux.strategies.makerv4.constants import MAKERV4_PARAM_SET
from flux.strategies.makerv4.constants import MAKERV4_STRATEGY_FAMILY


_MAKERV4_DEFAULT_OVERRIDES: dict[str, bool | int | float] = {
    "qty": 1.0,
    "max_qty_global": 100.0,
    "max_skew_bps_global": 10.0,
    "n_orders1": 3,
    "bid_edge1": 5.0,
    "ask_edge1": 5.0,
    "place_edge1": 1.0,
    "n_orders2": 0,
    "n_orders3": 0,
}


def _clone_runtime_specs() -> tuple[RuntimeParamSpec, ...]:
    schema = MAKERV3_RUNTIME_PARAM_REGISTRY.schema
    defaults = {
        **MAKERV3_RUNTIME_PARAM_DEFAULTS,
        **_MAKERV4_DEFAULT_OVERRIDES,
    }
    base_specs = [
        RuntimeParamSpec(
            name=name,
            schema_type=str(schema[name]["type"]),
            default=defaults[name],  # type: ignore[arg-type]
            description=str(schema[name]["description"]),
            minimum=schema[name].get("minimum"),
            maximum=schema[name].get("maximum"),
        )
        for name in MAKERV3_RUNTIME_PARAM_REGISTRY.names
    ]
    base_specs.extend(
        [
            RuntimeParamSpec(
                name="instant_hedge_enabled",
                schema_type="boolean",
                default=True,
                description="Submit an immediate hedge order after each maker fill.",
            ),
            RuntimeParamSpec(
                name="execution_mode",
                schema_type="select",
                default="maker_hedge",
                description="MakerV4 execution mode.",
                options=(
                    ("maker_hedge", "Maker Hedge"),
                    ("take_take", "Take-Take"),
                ),
            ),
            RuntimeParamSpec(
                name="hedge_style",
                schema_type="select",
                default="ioc_through_mid",
                description="Immediate-hedge execution style.",
                options=(("ioc_through_mid", "IOC Through Mid"),),
            ),
            RuntimeParamSpec(
                name="hedge_ioc_cross_mid_bps",
                schema_type="number",
                default=2.0,
                description="IOC hedge limit offset through the IBKR midpoint in bps.",
                minimum=0.0,
                maximum=100.0,
            ),
            RuntimeParamSpec(
                name="hedge_ioc_max_cross_bps",
                schema_type="number",
                default=10.0,
                description="Maximum IOC hedge crossing distance from the IBKR midpoint in bps.",
                minimum=0.0,
                maximum=500.0,
            ),
            RuntimeParamSpec(
                name="maker_fee_source",
                schema_type="select",
                default="config",
                description="Maker-fee source used for live quote gross-up.",
                options=(("config", "Configured Assumption"),),
            ),
            RuntimeParamSpec(
                name="hedge_fee_source",
                schema_type="select",
                default="config",
                description="Hedge-fee source used for IBKR hedge gross-up.",
                options=(("config", "Configured Assumption"),),
            ),
            RuntimeParamSpec(
                name="hedge_fee_plan",
                schema_type="select",
                default="ibkr_pro_tiered",
                description="Explicit IBKR fee-plan assumption used for hedge economics.",
                options=(("ibkr_pro_tiered", "IBKR Pro Tiered"),),
            ),
            RuntimeParamSpec(
                name="ibkr_fee_plan",
                schema_type="select",
                default="tiered",
                description="Configured IBKR fee-plan assumption for fee-aware pricing decisions.",
                options=(("fixed", "Fixed"), ("tiered", "Tiered")),
            ),
            RuntimeParamSpec(
                name="ibkr_fee_min_usd",
                schema_type="number",
                default=0.35,
                description="Configured minimum IBKR commission assumption in USD.",
                minimum=0.0,
                maximum=100.0,
            ),
            RuntimeParamSpec(
                name="maker_taker_fee_bps",
                schema_type="number",
                default=4.5,
                description="Configured maker-venue taker-fee assumption in bps.",
                minimum=0.0,
                maximum=100.0,
                aliases=("hl_taker_fee_bps",),
            ),
            RuntimeParamSpec(
                name="maker_maker_fee_bps",
                schema_type="number",
                default=0.25,
                description="Configured maker-venue maker-fee assumption in bps.",
                minimum=0.0,
                maximum=100.0,
                aliases=("hl_maker_fee_bps",),
            ),
            RuntimeParamSpec(
                name="bid_edge_take_bps",
                schema_type="number",
                default=5.0,
                description="Minimum fee-aware buy-side take-take edge threshold in bps.",
                minimum=0.0,
                maximum=500.0,
            ),
            RuntimeParamSpec(
                name="ask_edge_take_bps",
                schema_type="number",
                default=5.0,
                description="Minimum fee-aware sell-side take-take edge threshold in bps.",
                minimum=0.0,
                maximum=500.0,
            ),
            RuntimeParamSpec(
                name="take_cooldown_ms",
                schema_type="number",
                default=1_000,
                description="Cooldown between take-take maker submissions in milliseconds.",
                minimum=0.0,
                maximum=120_000.0,
            ),
            RuntimeParamSpec(
                name="assumed_hedge_fee_bps",
                schema_type="number",
                default=1.0,
                description="Assumed IBKR hedge fee in bps used for quote gross-up.",
                minimum=0.0,
                maximum=100.0,
            ),
        ]
    )
    return tuple(base_specs)


MAKERV4_RUNTIME_PARAM_REGISTRY = RuntimeParamRegistry(
    param_set=MAKERV4_PARAM_SET,
    specs=_clone_runtime_specs(),
)
MAKERV4_RUNTIME_PARAM_SCHEMA: dict[str, dict[str, Any]] = {
    name: dict(spec) for name, spec in MAKERV4_RUNTIME_PARAM_REGISTRY.schema.items()
}
MAKERV4_RUNTIME_PARAM_DEFAULTS: dict[str, Any] = dict(MAKERV4_RUNTIME_PARAM_REGISTRY.defaults)
PARAM_SET = MAKERV4_PARAM_SET
RUNTIME_PARAM_SCHEMA = MAKERV4_RUNTIME_PARAM_SCHEMA
RUNTIME_PARAM_DEFAULTS = MAKERV4_RUNTIME_PARAM_DEFAULTS


def profile_key() -> str:
    return MAKERV4_STRATEGY_FAMILY


def params_manager_factory(
    *,
    redis_client: Any,
    namespace: str = "flux",
    schema_version: str = "v1",
    defaults: Mapping[str, Any] | None = None,
):
    runtime_defaults = dict(MAKERV4_RUNTIME_PARAM_DEFAULTS)
    if defaults:
        runtime_defaults.update(defaults)

    def _factory(strategy: Any) -> FluxParamsManager:
        strategy_runtime_params = getattr(strategy, "_runtime_params", {})
        resolved_defaults = {
            name: strategy_runtime_params.get(name, runtime_defaults[name])
            for name in MAKERV4_RUNTIME_PARAM_REGISTRY.names
        }
        return FluxParamsManager(
            redis_client=redis_client,
            strategy_id=strategy.runtime_strategy_id,
            namespace=namespace,
            schema_version=schema_version,
            schema=MAKERV4_RUNTIME_PARAM_SCHEMA,
            defaults=resolved_defaults,
            param_set=MAKERV4_RUNTIME_PARAM_REGISTRY.param_set,
        )

    return _factory


__all__ = [
    "MAKERV4_RUNTIME_PARAM_DEFAULTS",
    "MAKERV4_RUNTIME_PARAM_REGISTRY",
    "MAKERV4_RUNTIME_PARAM_SCHEMA",
    "PARAM_SET",
    "RUNTIME_PARAM_DEFAULTS",
    "RUNTIME_PARAM_SCHEMA",
    "params_manager_factory",
    "profile_key",
]
