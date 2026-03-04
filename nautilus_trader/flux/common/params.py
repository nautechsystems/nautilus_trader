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

from __future__ import annotations

from collections.abc import Iterable
from collections.abc import Mapping
from dataclasses import dataclass
import math
from typing import Any
from typing import Final


def _decode_text(value: Any) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    if value is None:
        return ""
    return str(value)


def _parse_bool_text(value: Any) -> bool | None:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        if value in (0, 0.0):
            return False
        if value in (1, 1.0):
            return True
    text = _decode_text(value).strip().lower()
    if text in {"1", "true", "t", "yes", "y", "on", "enabled"}:
        return True
    if text in {"0", "false", "f", "no", "n", "off", "disabled"}:
        return False
    return None


def _coerce_integer(value: Any, *, name: str) -> int:
    if isinstance(value, bool):
        raise ValueError(f"Invalid integer value for {name!r}: {value!r}")
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        if not math.isfinite(value) or not value.is_integer():
            raise ValueError(f"Invalid integer value for {name!r}: {value!r}")
        return int(value)
    text = _decode_text(value).strip()
    if not text:
        raise ValueError(f"Invalid integer value for {name!r}: {value!r}")
    try:
        return int(text)
    except ValueError:
        raise ValueError(f"Invalid integer value for {name!r}: {value!r}") from None


def _coerce_number(value: Any, *, name: str) -> float:
    if isinstance(value, bool):
        raise ValueError(f"Invalid number value for {name!r}: {value!r}")
    if isinstance(value, (int, float)):
        numeric = float(value)
    else:
        text = _decode_text(value).strip()
        if not text:
            raise ValueError(f"Invalid number value for {name!r}: {value!r}")
        try:
            numeric = float(text)
        except ValueError:
            raise ValueError(f"Invalid number value for {name!r}: {value!r}") from None
    if not math.isfinite(numeric):
        raise ValueError(f"Invalid number value for {name!r}: {value!r}")
    return numeric


def _format_summary_value(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, float):
        if value.is_integer():
            return str(int(value))
        return format(value, "g")
    return str(value)


@dataclass(frozen=True, slots=True)
class RuntimeParamSpec:
    """
    Canonical runtime parameter metadata.
    """

    name: str
    schema_type: str
    default: bool | int | float
    description: str
    minimum: int | float | None = None
    maximum: int | float | None = None

    def __post_init__(self) -> None:
        if not isinstance(self.name, str) or not self.name:
            raise ValueError("`name` must be a non-empty string")
        if self.schema_type not in {"boolean", "integer", "number"}:
            raise ValueError(
                f"`schema_type` was invalid for {self.name!r}: {self.schema_type!r}",
            )
        if not isinstance(self.description, str) or not self.description:
            raise ValueError(f"`description` must be a non-empty string for {self.name!r}")

        minimum = self.minimum
        maximum = self.maximum
        default = self.default

        if self.schema_type == "boolean":
            if not isinstance(default, bool):
                raise TypeError(f"`default` must be bool for {self.name!r}")
            if minimum is not None or maximum is not None:
                raise ValueError(f"Boolean parameter {self.name!r} cannot define numeric bounds")
            return

        if isinstance(default, bool):
            raise TypeError(f"`default` must be numeric for {self.name!r}")

        if self.schema_type == "integer":
            if not isinstance(default, int):
                raise TypeError(f"`default` must be int for {self.name!r}")
            if minimum is not None:
                if isinstance(minimum, bool) or not isinstance(minimum, int):
                    raise TypeError(f"`minimum` must be int for {self.name!r}")
            if maximum is not None:
                if isinstance(maximum, bool) or not isinstance(maximum, int):
                    raise TypeError(f"`maximum` must be int for {self.name!r}")
        else:
            if not isinstance(default, (int, float)):
                raise TypeError(f"`default` must be number for {self.name!r}")
            default = float(default)
            if not math.isfinite(default):
                raise ValueError(f"`default` must be finite for {self.name!r}")
            object.__setattr__(self, "default", default)
            if minimum is not None:
                if isinstance(minimum, bool) or not isinstance(minimum, (int, float)):
                    raise TypeError(f"`minimum` must be number for {self.name!r}")
                minimum = float(minimum)
                if not math.isfinite(minimum):
                    raise ValueError(f"`minimum` must be finite for {self.name!r}")
                object.__setattr__(self, "minimum", minimum)
            if maximum is not None:
                if isinstance(maximum, bool) or not isinstance(maximum, (int, float)):
                    raise TypeError(f"`maximum` must be number for {self.name!r}")
                maximum = float(maximum)
                if not math.isfinite(maximum):
                    raise ValueError(f"`maximum` must be finite for {self.name!r}")
                object.__setattr__(self, "maximum", maximum)

        if minimum is not None and maximum is not None and minimum > maximum:
            raise ValueError(f"`minimum` must be <= `maximum` for {self.name!r}")
        if minimum is not None and self.default < minimum:
            raise ValueError(f"`default` must be >= `minimum` for {self.name!r}")
        if maximum is not None and self.default > maximum:
            raise ValueError(f"`default` must be <= `maximum` for {self.name!r}")

    def to_schema(self) -> dict[str, Any]:
        schema = {
            "type": self.schema_type,
            "description": self.description,
        }
        if self.minimum is not None:
            schema["minimum"] = self.minimum
        if self.maximum is not None:
            schema["maximum"] = self.maximum
        return schema


class RuntimeParamRegistry:
    """
    Canonical schema/defaults/constraints for a runtime parameter set.
    """

    def __init__(self, *, param_set: str, specs: Iterable[RuntimeParamSpec]) -> None:
        if not isinstance(param_set, str) or not param_set:
            raise ValueError("`param_set` must be a non-empty string")

        ordered_specs: list[RuntimeParamSpec] = []
        spec_by_name: dict[str, RuntimeParamSpec] = {}
        for spec in specs:
            if spec.name in spec_by_name:
                raise ValueError(f"Duplicate runtime param spec for {spec.name!r}")
            ordered_specs.append(spec)
            spec_by_name[spec.name] = spec

        if not ordered_specs:
            raise ValueError("`specs` must not be empty")

        self._param_set = param_set
        self._ordered_specs = tuple(ordered_specs)
        self._spec_by_name = spec_by_name

    @property
    def param_set(self) -> str:
        return self._param_set

    @property
    def names(self) -> tuple[str, ...]:
        return tuple(spec.name for spec in self._ordered_specs)

    @property
    def schema(self) -> dict[str, dict[str, Any]]:
        return {spec.name: spec.to_schema() for spec in self._ordered_specs}

    @property
    def defaults(self) -> dict[str, Any]:
        return {spec.name: spec.default for spec in self._ordered_specs}

    def coerce_value(self, name: str, value: Any) -> Any:
        spec = self._spec_by_name.get(name)
        if spec is None:
            raise ValueError(f"Unsupported runtime param: {name!r}")
        return self._coerce_with_spec(spec, value)

    def coerce_updates(self, updates: Mapping[str, Any]) -> dict[str, Any]:
        coerced: dict[str, Any] = {}
        for name, raw_value in updates.items():
            key = str(name)
            coerced[key] = self.coerce_value(key, raw_value)
        return coerced

    def diff_summary(
        self,
        *,
        before: Mapping[str, Any] | None,
        after: Mapping[str, Any] | None,
        max_changes: int = 10,
    ) -> dict[str, Any]:
        if max_changes < 1:
            raise ValueError("`max_changes` must be >= 1")

        before_typed = self._coerce_known_values(before, context="before")
        after_typed = self._coerce_known_values(after, context="after")

        changes: list[dict[str, Any]] = []
        for name in self.names:
            before_has = name in before_typed
            after_has = name in after_typed
            if not before_has and not after_has:
                continue
            old_value = before_typed.get(name)
            new_value = after_typed.get(name)
            if old_value == new_value:
                continue
            changes.append(
                {
                    "name": name,
                    "before": old_value,
                    "after": new_value,
                },
            )

        summarized_changes = changes[:max_changes]
        summary = "; ".join(
            f"{entry['name']}:{_format_summary_value(entry['before'])}->{_format_summary_value(entry['after'])}"
            for entry in summarized_changes
        )

        return {
            "param_set": self.param_set,
            "changed_count": len(changes),
            "changed_keys": [entry["name"] for entry in changes],
            "changes": summarized_changes,
            "truncated": len(changes) > len(summarized_changes),
            "summary": summary,
        }

    def _coerce_known_values(self, values: Mapping[str, Any] | None, *, context: str) -> dict[str, Any]:
        if not values:
            return {}

        unknown = sorted(str(name) for name in values if str(name) not in self._spec_by_name)
        if len(unknown) == 1:
            raise ValueError(f"Unsupported runtime param in {context}: {unknown[0]!r}")
        if unknown:
            raise ValueError(f"Unsupported runtime params in {context}: {unknown!r}")

        typed: dict[str, Any] = {}
        for name in self.names:
            if name not in values:
                continue
            typed[name] = self.coerce_value(name, values[name])
        return typed

    @staticmethod
    def _coerce_with_spec(spec: RuntimeParamSpec, value: Any) -> Any:
        if spec.schema_type == "boolean":
            parsed = _parse_bool_text(value)
            if parsed is None:
                raise ValueError(f"Invalid boolean value for {spec.name!r}: {value!r}")
            return parsed

        if spec.schema_type == "integer":
            parsed = _coerce_integer(value, name=spec.name)
        else:
            parsed = _coerce_number(value, name=spec.name)

        if spec.minimum is not None and parsed < spec.minimum:
            raise ValueError(f"`{spec.name}` must be >= {spec.minimum}")
        if spec.maximum is not None and parsed > spec.maximum:
            raise ValueError(f"`{spec.name}` must be <= {spec.maximum}")
        return parsed


_MAKERV3_RUNTIME_PARAM_SPECS: Final[tuple[RuntimeParamSpec, ...]] = (
    RuntimeParamSpec(
        name="qty",
        schema_type="number",
        default=1_000.0,
        description="Target base quantity per quote/hedge cycle.",
        minimum=0.0,
        maximum=1_000_000.0,
    ),
    RuntimeParamSpec(
        name="des_qty_global",
        schema_type="number",
        default=0.0,
        description="Global desired inventory target in base units.",
        minimum=0.0,
        maximum=1_000_000.0,
    ),
    RuntimeParamSpec(
        name="max_qty_global",
        schema_type="number",
        default=40_000.0,
        description="Global hard inventory cap in base units.",
        minimum=0.0,
        maximum=2_000_000.0,
    ),
    RuntimeParamSpec(
        name="max_skew_bps_global",
        schema_type="number",
        default=20.0,
        description="Global maker/hedge skew cap in bps.",
        minimum=0.0,
        maximum=5_000.0,
    ),
    RuntimeParamSpec(
        name="des_qty_local",
        schema_type="number",
        default=0.0,
        description="Local desired inventory target in base units.",
        minimum=0.0,
        maximum=1_000_000.0,
    ),
    RuntimeParamSpec(
        name="max_qty_local",
        schema_type="number",
        default=0.0,
        description="Local hard inventory cap in base units.",
        minimum=0.0,
        maximum=1_000_000.0,
    ),
    RuntimeParamSpec(
        name="max_skew_bps_local",
        schema_type="number",
        default=0.0,
        description="Local maker skew cap in bps.",
        minimum=0.0,
        maximum=5_000.0,
    ),
    RuntimeParamSpec(
        name="linear_offset_bps",
        schema_type="number",
        default=0.0,
        description="Linear inventory offset in bps.",
        minimum=0.0,
        maximum=5_000.0,
    ),
    RuntimeParamSpec(
        name="n_orders1",
        schema_type="integer",
        default=5,
        description="Band 1 order depth per side.",
        minimum=0,
        maximum=20,
    ),
    RuntimeParamSpec(
        name="distance1",
        schema_type="number",
        default=2.0,
        description="Band 1 spacing increment in bps.",
        minimum=0.0,
        maximum=500.0,
    ),
    RuntimeParamSpec(
        name="bid_edge1",
        schema_type="number",
        default=10.0,
        description="Band 1 bid edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="ask_edge1",
        schema_type="number",
        default=10.0,
        description="Band 1 ask edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="place_edge1",
        schema_type="number",
        default=2.0,
        description="Band 1 placement edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="n_orders2",
        schema_type="integer",
        default=0,
        description="Band 2 order depth per side.",
        minimum=0,
        maximum=20,
    ),
    RuntimeParamSpec(
        name="distance2",
        schema_type="number",
        default=5.0,
        description="Band 2 spacing increment in bps.",
        minimum=0.0,
        maximum=500.0,
    ),
    RuntimeParamSpec(
        name="bid_edge2",
        schema_type="number",
        default=25.0,
        description="Band 2 bid edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="ask_edge2",
        schema_type="number",
        default=25.0,
        description="Band 2 ask edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="place_edge2",
        schema_type="number",
        default=2.0,
        description="Band 2 placement edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="n_orders3",
        schema_type="integer",
        default=0,
        description="Band 3 order depth per side.",
        minimum=0,
        maximum=20,
    ),
    RuntimeParamSpec(
        name="distance3",
        schema_type="number",
        default=5.0,
        description="Band 3 spacing increment in bps.",
        minimum=0.0,
        maximum=500.0,
    ),
    RuntimeParamSpec(
        name="bid_edge3",
        schema_type="number",
        default=50.0,
        description="Band 3 bid edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="ask_edge3",
        schema_type="number",
        default=50.0,
        description="Band 3 ask edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="place_edge3",
        schema_type="number",
        default=2.0,
        description="Band 3 placement edge in bps.",
        minimum=0.0,
        maximum=1_000.0,
    ),
    RuntimeParamSpec(
        name="quote_fail_critical_after_count",
        schema_type="integer",
        default=3,
        description="Escalation count for quote failures.",
        minimum=0,
        maximum=100,
    ),
    RuntimeParamSpec(
        name="quote_fail_critical_after_s",
        schema_type="number",
        default=60.0,
        description="Escalation window for quote failures.",
        minimum=0.0,
        maximum=3_600.0,
    ),
    RuntimeParamSpec(
        name="max_age_ms",
        schema_type="integer",
        default=10_000,
        description="Replace managed orders older than this age.",
        minimum=1,
        maximum=60_000,
    ),
    RuntimeParamSpec(
        name="bot_on",
        schema_type="boolean",
        default=False,
        description="Enable quote publishing and management.",
    ),
)


MAKERV3_RUNTIME_PARAM_REGISTRY: Final[RuntimeParamRegistry] = RuntimeParamRegistry(
    param_set="makerv3",
    specs=_MAKERV3_RUNTIME_PARAM_SPECS,
)
MAKERV3_RUNTIME_PARAM_SCHEMA: Final[dict[str, dict[str, Any]]] = MAKERV3_RUNTIME_PARAM_REGISTRY.schema
MAKERV3_RUNTIME_PARAM_DEFAULTS: Final[dict[str, Any]] = MAKERV3_RUNTIME_PARAM_REGISTRY.defaults


def summarize_makerv3_param_diff(
    *,
    before: Mapping[str, Any] | None,
    after: Mapping[str, Any] | None,
    max_changes: int = 10,
) -> dict[str, Any]:
    """
    Build a deterministic, log-friendly summary of runtime parameter changes.
    """
    return MAKERV3_RUNTIME_PARAM_REGISTRY.diff_summary(
        before=before,
        after=after,
        max_changes=max_changes,
    )


__all__ = [
    "MAKERV3_RUNTIME_PARAM_DEFAULTS",
    "MAKERV3_RUNTIME_PARAM_REGISTRY",
    "MAKERV3_RUNTIME_PARAM_SCHEMA",
    "RuntimeParamRegistry",
    "RuntimeParamSpec",
    "summarize_makerv3_param_diff",
]
