from __future__ import annotations

from dataclasses import dataclass
from collections.abc import Iterable
from datetime import timedelta
from decimal import ROUND_CEILING
from decimal import ROUND_FLOOR
from decimal import Decimal
import json
from typing import Any

TOPIC_STATE = "flux.strategy.state"
TOPIC_EVENT = "flux.strategy.event"
TOPIC_TRADE = "flux.strategy.trade"
TOPIC_ALERT = "flux.strategy.alert"
TOPIC_MARKET_BBO = "flux.strategy.market_bbo"
TOPIC_FV = "flux.strategy.fv"
TOPIC_BALANCES = "flux.strategy.balances"
json_dumps_compact = None

try:
    from nautilus_trader.serialization import register_serializable_type
except Exception:  # pragma: no cover - fallback test environments
    register_serializable_type = None


if register_serializable_type is not None:

    @dataclass(frozen=True, slots=True)
    class FluxBusPayload:
        topic: str
        payload: str
        ts_event: int = 0
        ts_init: int = 0

        def to_dict(self) -> dict[str, Any]:
            return {
                "type": "FluxBusPayload",
                "topic": self.topic,
                "payload": self.payload,
                "ts_event": self.ts_event,
                "ts_init": self.ts_init,
            }

        @classmethod
        def from_dict(cls, data: dict[str, Any]) -> "FluxBusPayload":
            return cls(
                topic=data.get("topic", ""),
                payload=data.get("payload", ""),
                ts_event=int(data.get("ts_event", 0)),
                ts_init=int(data.get("ts_init", 0)),
            )

    register_serializable_type(FluxBusPayload, FluxBusPayload.to_dict, FluxBusPayload.from_dict)
else:  # pragma: no cover - fallback test environments
    FluxBusPayload = None


def _to_json_safe(payload: Any) -> str:
    if json_dumps_compact is None:
        return json.dumps(payload, sort_keys=True, separators=(",", ":"))
    return json_dumps_compact(payload)


def _parse_bool_text(value: Any) -> bool | None:
    if value is None:
        return None
    text = str(value).strip().lower()
    if text in {"1", "true", "t", "yes", "y", "on", "enabled"}:
        return True
    if text in {"0", "false", "f", "no", "n", "off", "disabled"}:
        return False
    return None

RUNTIME_PARAM_TYPES: dict[str, str] = {
    "qty": "decimal",
    "des_qty_global": "decimal",
    "max_qty_global": "decimal",
    "max_skew_bps_global": "decimal",
    "des_qty_local": "decimal",
    "max_qty_local": "decimal",
    "max_skew_bps_local": "decimal",
    "linear_offset_bps": "decimal",
    "max_age_ms": "int",
    "bid_edge1": "decimal",
    "ask_edge1": "decimal",
    "place_edge1": "decimal",
    "distance1": "decimal",
    "n_orders1": "int",
    "bid_edge2": "decimal",
    "ask_edge2": "decimal",
    "place_edge2": "decimal",
    "distance2": "decimal",
    "n_orders2": "int",
    "bid_edge3": "decimal",
    "ask_edge3": "decimal",
    "place_edge3": "decimal",
    "distance3": "decimal",
    "n_orders3": "int",
    "quote_fail_critical_after_count": "int",
    "quote_fail_critical_after_s": "decimal",
    "bot_on": "bool",
}

RUNTIME_PARAM_SCHEMA: dict[str, dict[str, str]] = {
    name: {
        "type": "boolean" if kind == "bool" else "integer" if kind == "int" else "number",
    }
    for name, kind in RUNTIME_PARAM_TYPES.items()
}


def _coerce_runtime_param_value(name: str, value: Any) -> Any:
    kind = RUNTIME_PARAM_TYPES.get(name)
    if kind is None:
        raise ValueError(f"Unsupported runtime param: {name}")
    if kind == "bool":
        parsed = _parse_bool_text(value)
        if parsed is None:
            raise ValueError(f"Invalid bool value for {name}: {value!r}")
        return parsed
    if kind == "int":
        parsed = int(value)
        return parsed if parsed >= 0 else 0
    if kind == "decimal":
        parsed = _to_decimal_or_none(value)
        if parsed is None:
            raise ValueError(f"Invalid decimal value for {name}: {value!r}")
        return parsed
    raise ValueError(f"Unknown runtime param type for {name}: {kind}")


def _stringify_identifier(value: Any) -> str:
    if value is None:
        return ""
    to_str = getattr(value, "to_str", None)
    if callable(to_str):
        try:
            return str(to_str())
        except Exception:
            pass
    code = getattr(value, "code", None)
    if code is not None:
        return str(code)
    return str(value)


def _money_to_text(value: Any) -> str:
    if value is None:
        return "0"
    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        try:
            return str(as_decimal())
        except Exception:
            pass
    return str(value)


def _json_safe_or_none(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, dict):
        return None
    try:
        return json.loads(_to_json_safe(value))
    except Exception:
        return None


def _account_balances_rows(account: Any) -> list[dict[str, str]]:
    balances_total_fn = getattr(account, "balances_total", None)
    balances_free_fn = getattr(account, "balances_free", None)
    balances_locked_fn = getattr(account, "balances_locked", None)

    if not callable(balances_total_fn):
        return []

    try:
        balances_total = balances_total_fn()
    except Exception:
        return []

    if not isinstance(balances_total, dict):
        return []

    try:
        balances_free = balances_free_fn() if callable(balances_free_fn) else {}
    except Exception:
        balances_free = {}

    try:
        balances_locked = balances_locked_fn() if callable(balances_locked_fn) else {}
    except Exception:
        balances_locked = {}

    rows: list[dict[str, str]] = []
    for currency in sorted(balances_total.keys(), key=_stringify_identifier):
        code = _stringify_identifier(currency).upper()
        rows.append(
            {
                "currency": code,
                "free": _money_to_text(balances_free.get(currency)),
                "locked": _money_to_text(balances_locked.get(currency)),
                "total": _money_to_text(balances_total.get(currency)),
            },
        )

    return rows


def _serialize_account_payload(account: Any) -> dict[str, Any]:
    acct_to_dict = getattr(account, "to_dict", None)
    if callable(acct_to_dict):
        try:
            candidate = _json_safe_or_none(acct_to_dict())
            if candidate is not None:
                return candidate
        except Exception:
            pass

        try:
            candidate = _json_safe_or_none(acct_to_dict(account))
            if candidate is not None:
                return candidate
        except Exception:
            pass

    account_id = _stringify_identifier(getattr(account, "id", None))
    if not account_id:
        account_id = _stringify_identifier(getattr(getattr(account, "last_event", None), "account_id", None))

    balances = _account_balances_rows(account)
    payload: dict[str, Any] = {"type": type(account).__name__}
    if account_id:
        payload["account_id"] = account_id
    if balances:
        payload["events"] = [{"account_id": account_id, "balances": balances}]
    if len(payload) == 1:
        payload["repr"] = repr(account)

    return payload


def _serialize_position_payload(position: Any) -> dict[str, Any]:
    pos_to_dict = getattr(position, "to_dict", None)
    if callable(pos_to_dict):
        try:
            candidate = _json_safe_or_none(pos_to_dict())
            if candidate is not None:
                return candidate
        except Exception:
            pass

        try:
            candidate = _json_safe_or_none(pos_to_dict(position))
            if candidate is not None:
                return candidate
        except Exception:
            pass

    payload: dict[str, Any] = {"type": type(position).__name__}
    for field_name in (
        "position_id",
        "instrument_id",
        "side",
        "signed_qty",
        "quantity",
        "avg_px_open",
        "avg_px_close",
        "realized_pnl",
    ):
        value = getattr(position, field_name, None)
        if value is not None:
            payload[field_name] = _stringify_identifier(value)
    if len(payload) == 1:
        payload["repr"] = repr(position)
    return payload


def _normalize_contract_symbol(raw_symbol: str) -> tuple[str, str]:
    symbol = str(raw_symbol).strip()
    if not symbol:
        return "", ""

    if "/" in symbol:
        base, quote = symbol.split("/", maxsplit=1)
        return base.strip().upper(), quote.strip().upper()

    cleaned = symbol.replace("-", "_").replace("/", "_")
    if "_" in cleaned:
        base, quote = cleaned.split("_", maxsplit=1)
        if base and quote:
            return base.strip().upper(), quote.strip().upper()

    return symbol.upper(), ""


def _to_decimal(value: Decimal | float | str) -> Decimal:
    return value if isinstance(value, Decimal) else Decimal(str(value))


def _to_decimal_or_none(value: Any) -> Decimal | None:
    if value is None:
        return None
    try:
        return _to_decimal(value)
    except Exception:
        pass
    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        try:
            return _to_decimal(as_decimal())
        except Exception:
            return None
    return None


def _decimal_to_json_str(value: Any) -> str | None:
    if value is None:
        return None

    as_decimal = _to_decimal_or_none(value)
    if as_decimal is None:
        return str(value)

    text = format(as_decimal, "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    if text == "-0":
        return "0"
    return text or "0"


def _clamp_decimal(value: Decimal, lower: Decimal, upper: Decimal) -> Decimal:
    return max(lower, min(upper, value))


def _bps_to_price_offset(anchor_price: Decimal, bps: Decimal | float | str) -> Decimal:
    return anchor_price * _to_decimal(bps) / Decimal("10000")


def _price_to_decimal(value: Any) -> Decimal:
    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        try:
            return _to_decimal(as_decimal())
        except Exception:
            pass
    return _to_decimal(value)


def _round_price_to_tick(
    price: Decimal,
    *,
    tick: Decimal,
    is_buy: bool,
    round_in: bool,
) -> Decimal:
    if tick <= 0:
        return price
    if round_in:
        rounding = ROUND_CEILING if is_buy else ROUND_FLOOR
    else:
        rounding = ROUND_FLOOR if is_buy else ROUND_CEILING
    ticks = (price / tick).to_integral_value(rounding=rounding)
    rounded = ticks * tick
    try:
        return rounded.quantize(tick)
    except Exception:
        return rounded


def _clamp_post_only_price(
    *,
    price: Decimal,
    is_buy: bool,
    top_bid: Decimal,
    top_ask: Decimal,
    tick: Decimal,
) -> Decimal:
    if is_buy and top_ask > 0 and price >= top_ask:
        adjusted = max(Decimal("0"), top_ask - tick)
        return _round_price_to_tick(adjusted, tick=tick, is_buy=True, round_in=False)
    if (not is_buy) and top_bid > 0 and price <= top_bid:
        adjusted = top_bid + tick
        return _round_price_to_tick(adjusted, tick=tick, is_buy=False, round_in=False)
    return price


def _nudge_unique_price(
    *,
    price: Decimal,
    tick: Decimal,
    is_buy: bool,
    seen: set[str],
) -> Decimal | None:
    if tick <= 0:
        key = str(price)
        if key in seen:
            return None
        return price

    out = price
    for _ in range(200):
        if out <= 0:
            return None
        key = str(out)
        if key not in seen:
            return out
        out = out - tick if is_buy else out + tick
        if out <= 0:
            return None
        out = _round_price_to_tick(out, tick=tick, is_buy=is_buy, round_in=False)
    return None


def _apply_inventory_skew_to_edges(
    *,
    bid_edge_bps: Decimal,
    ask_edge_bps: Decimal,
    total_skew_bps: Decimal,
) -> tuple[Decimal, Decimal]:
    skew_abs = abs(total_skew_bps)
    if total_skew_bps > 0:
        return bid_edge_bps + skew_abs, ask_edge_bps - skew_abs
    if total_skew_bps < 0:
        return bid_edge_bps - skew_abs, ask_edge_bps + skew_abs
    return bid_edge_bps, ask_edge_bps


def _did_bot_turn_off(previous_bot_on: bool, current_bot_on: bool) -> bool:
    return bool(previous_bot_on) and (not bool(current_bot_on))


def _should_publish_market_bbo(
    *,
    bbo_changed: bool,
    last_publish_ns: int,
    now_ns: int,
    heartbeat_ms: int,
) -> bool:
    if bbo_changed:
        return True
    if last_publish_ns <= 0:
        return True
    interval_ns = max(1, int(heartbeat_ms)) * 1_000_000
    return now_ns - last_publish_ns >= interval_ns


def _validate_three_band_input(values: Iterable[object], name: str) -> tuple[object, object, object]:
    parsed = tuple(values)
    if len(parsed) != 3:
        raise ValueError(f"{name}: expected three bands, got {len(parsed)}")
    return parsed  # type: ignore[return-value]


def build_ladder_targets(
    anchor_bid: Decimal | float | str,
    anchor_ask: Decimal | float | str,
    bid_edges: Iterable[Decimal | float | str],
    ask_edges: Iterable[Decimal | float | str],
    distances: Iterable[Decimal | float | str],
    n_orders: Iterable[int],
) -> tuple[list[Decimal], list[Decimal]]:
    """
    Build 3-band ladder prices from anchor bid/ask and offsets.
    """

    bid_edge_1, bid_edge_2, bid_edge_3 = _validate_three_band_input(bid_edges, "bid_edges")
    ask_edge_1, ask_edge_2, ask_edge_3 = _validate_three_band_input(ask_edges, "ask_edges")
    distance_1, distance_2, distance_3 = _validate_three_band_input(distances, "distances")
    n_1, n_2, n_3 = _validate_three_band_input(n_orders, "n_orders")

    bid_edges = (_to_decimal(bid_edge_1), _to_decimal(bid_edge_2), _to_decimal(bid_edge_3))
    ask_edges = (_to_decimal(ask_edge_1), _to_decimal(ask_edge_2), _to_decimal(ask_edge_3))
    distances = (_to_decimal(distance_1), _to_decimal(distance_2), _to_decimal(distance_3))
    n_orders = (int(n_1), int(n_2), int(n_3))

    if any(v < 0 for v in bid_edges + ask_edges):
        raise ValueError("edges must be non-negative")
    if any(v < 0 for v in distances):
        raise ValueError("distances must be non-negative")
    if any(v < 0 for v in n_orders):
        raise ValueError("n_orders must be non-negative")

    anchor_bid_dec = _to_decimal(anchor_bid)
    anchor_ask_dec = _to_decimal(anchor_ask)

    bid_targets: list[Decimal] = []
    ask_targets: list[Decimal] = []

    for band_idx in range(3):
        for level in range(n_orders[band_idx]):
            step = distances[band_idx] * level
            bid_targets.append(anchor_bid_dec - bid_edges[band_idx] - step)
            ask_targets.append(anchor_ask_dec + ask_edges[band_idx] + step)

    return bid_targets, ask_targets


def build_ladder_place_cancel_levels(
    anchor_bid: Decimal | float | str,
    anchor_ask: Decimal | float | str,
    bid_edges: Iterable[Decimal | float | str],
    ask_edges: Iterable[Decimal | float | str],
    place_edges: Iterable[Decimal | float | str],
    distances: Iterable[Decimal | float | str],
    n_orders: Iterable[int],
) -> tuple[list[tuple[Decimal, Decimal]], list[tuple[Decimal, Decimal]]]:
    """
    Build 3-band ladder with (place_px, cancel_px) pairs per side.
    """

    bid_edge_1, bid_edge_2, bid_edge_3 = _validate_three_band_input(bid_edges, "bid_edges")
    ask_edge_1, ask_edge_2, ask_edge_3 = _validate_three_band_input(ask_edges, "ask_edges")
    place_edge_1, place_edge_2, place_edge_3 = _validate_three_band_input(place_edges, "place_edges")
    distance_1, distance_2, distance_3 = _validate_three_band_input(distances, "distances")
    n_1, n_2, n_3 = _validate_three_band_input(n_orders, "n_orders")

    bid_edges_dec = (_to_decimal(bid_edge_1), _to_decimal(bid_edge_2), _to_decimal(bid_edge_3))
    ask_edges_dec = (_to_decimal(ask_edge_1), _to_decimal(ask_edge_2), _to_decimal(ask_edge_3))
    place_edges_dec = (
        _to_decimal(place_edge_1),
        _to_decimal(place_edge_2),
        _to_decimal(place_edge_3),
    )
    distances_dec = (_to_decimal(distance_1), _to_decimal(distance_2), _to_decimal(distance_3))
    n_orders_int = (int(n_1), int(n_2), int(n_3))

    if any(v < 0 for v in place_edges_dec):
        raise ValueError("place edges must be non-negative")
    if any(v < 0 for v in distances_dec):
        raise ValueError("distances must be non-negative")
    if any(v < 0 for v in n_orders_int):
        raise ValueError("n_orders must be non-negative")

    anchor_bid_dec = _to_decimal(anchor_bid)
    anchor_ask_dec = _to_decimal(anchor_ask)

    bid_levels: list[tuple[Decimal, Decimal]] = []
    ask_levels: list[tuple[Decimal, Decimal]] = []

    for band_idx in range(3):
        for level in range(n_orders_int[band_idx]):
            step = distances_dec[band_idx] * level

            bid_cancel = anchor_bid_dec - bid_edges_dec[band_idx] - step
            bid_place = bid_cancel - place_edges_dec[band_idx]
            bid_levels.append((bid_place, bid_cancel))

            ask_cancel = anchor_ask_dec + ask_edges_dec[band_idx] + step
            ask_place = ask_cancel + place_edges_dec[band_idx]
            ask_levels.append((ask_place, ask_cancel))

    return bid_levels, ask_levels


def build_ladder_place_cancel_levels_from_bps(
    anchor_bid: Decimal | float | str,
    anchor_ask: Decimal | float | str,
    bid_edges_bps: Iterable[Decimal | float | str],
    ask_edges_bps: Iterable[Decimal | float | str],
    place_edges_bps: Iterable[Decimal | float | str],
    distances_bps: Iterable[Decimal | float | str],
    n_orders: Iterable[int],
    tick: Decimal | float | str = Decimal("0"),
) -> tuple[list[tuple[Decimal, Decimal]], list[tuple[Decimal, Decimal]]]:
    """
    Build 3-band ladder from anchor bid/ask and bps inputs.

    This mirrors Chainsaw MakerV3 pricing:
      - cancel prices are edge offsets from anchor bid/ask in bps
      - place prices apply additional place_edge bps (less aggressive)
      - ladder spacing uses anchor mid and distance bps
    """

    bid_edge_1, bid_edge_2, bid_edge_3 = _validate_three_band_input(bid_edges_bps, "bid_edges_bps")
    ask_edge_1, ask_edge_2, ask_edge_3 = _validate_three_band_input(ask_edges_bps, "ask_edges_bps")
    place_edge_1, place_edge_2, place_edge_3 = _validate_three_band_input(place_edges_bps, "place_edges_bps")
    distance_1, distance_2, distance_3 = _validate_three_band_input(distances_bps, "distances_bps")
    n_1, n_2, n_3 = _validate_three_band_input(n_orders, "n_orders")

    bid_edges_dec = (_to_decimal(bid_edge_1), _to_decimal(bid_edge_2), _to_decimal(bid_edge_3))
    ask_edges_dec = (_to_decimal(ask_edge_1), _to_decimal(ask_edge_2), _to_decimal(ask_edge_3))
    place_edges_dec = (_to_decimal(place_edge_1), _to_decimal(place_edge_2), _to_decimal(place_edge_3))
    distances_dec = (_to_decimal(distance_1), _to_decimal(distance_2), _to_decimal(distance_3))
    n_orders_int = (int(n_1), int(n_2), int(n_3))
    tick_dec = _to_decimal(tick)

    if any(v < 0 for v in place_edges_dec):
        raise ValueError("place edges must be non-negative")
    if any(v < 0 for v in distances_dec):
        raise ValueError("distances must be non-negative")
    if any(v < 0 for v in n_orders_int):
        raise ValueError("n_orders must be non-negative")

    anchor_bid_dec = _to_decimal(anchor_bid)
    anchor_ask_dec = _to_decimal(anchor_ask)
    mid_primary = (anchor_bid_dec + anchor_ask_dec) / Decimal("2")
    if mid_primary <= 0:
        return [], []

    bid_levels: list[tuple[Decimal, Decimal]] = []
    ask_levels: list[tuple[Decimal, Decimal]] = []

    for band_idx in range(3):
        bid_edge_frac = bid_edges_dec[band_idx] / Decimal("10000")
        ask_edge_frac = ask_edges_dec[band_idx] / Decimal("10000")
        place_edge_pos = max(Decimal("0"), place_edges_dec[band_idx])
        bid_place_edge_frac = (bid_edges_dec[band_idx] + place_edge_pos) / Decimal("10000")
        ask_place_edge_frac = (ask_edges_dec[band_idx] + place_edge_pos) / Decimal("10000")

        bid_cancel_base = anchor_bid_dec * (Decimal("1") - bid_edge_frac)
        ask_cancel_base = anchor_ask_dec * (Decimal("1") + ask_edge_frac)
        bid_place_base = anchor_bid_dec * (Decimal("1") - bid_place_edge_frac)
        ask_place_base = anchor_ask_dec * (Decimal("1") + ask_place_edge_frac)

        for level in range(n_orders_int[band_idx]):
            offset_px = (mid_primary * distances_dec[band_idx] * Decimal(level)) / Decimal("10000")
            if tick_dec > 0 and level > 0:
                min_offset = tick_dec * Decimal(level)
                if offset_px < min_offset:
                    offset_px = min_offset

            bid_cancel = bid_cancel_base - offset_px
            bid_place = bid_place_base - offset_px
            bid_levels.append((bid_place, bid_cancel))

            ask_cancel = ask_cancel_base + offset_px
            ask_place = ask_place_base + offset_px
            ask_levels.append((ask_place, ask_cancel))

    return bid_levels, ask_levels


def plan_side_rebalance_actions(
    *,
    side: str,
    active_prices: list[Decimal],
    active_stale: list[bool],
    desired_levels: list[tuple[Decimal, Decimal, Decimal]],
    stale_cancel_budget: int = 1,
) -> tuple[list[int], list[int]]:
    """
    Plan side rebalancing decisions.

    Returns:
      - active order indices to cancel
      - desired level indices still missing after planned cancels
    """

    side_norm = str(side).lower()
    if side_norm not in {"buy", "sell"}:
        raise ValueError(f"Unsupported side: {side!r}")
    if len(active_prices) != len(active_stale):
        raise ValueError("active_prices and active_stale length mismatch")

    max_levels = len(desired_levels)
    cancels: set[int] = set()

    # 1) Trim overflow depth from least aggressive tail.
    for index in range(max_levels, len(active_prices)):
        cancels.add(index)

    # 2) Cancel too-aggressive orders by rank.
    for index in range(min(len(active_prices), max_levels)):
        if index in cancels:
            continue
        current_px = active_prices[index]
        _, cancel_px, _ = desired_levels[index]
        too_aggressive = (
            (side_norm == "buy" and current_px > cancel_px)
            or (side_norm == "sell" and current_px < cancel_px)
        )
        if too_aggressive:
            cancels.add(index)

    # 3) Age-out gradually: least aggressive stale survivors first.
    stale_budget = max(0, int(stale_cancel_budget))
    if stale_budget > 0:
        stale_candidates = [
            idx
            for idx, is_stale in enumerate(active_stale)
            if is_stale and idx not in cancels
        ]
        for idx in sorted(stale_candidates, reverse=True)[:stale_budget]:
            cancels.add(idx)

    def survivor_indices() -> list[int]:
        return [idx for idx in range(len(active_prices)) if idx not in cancels]

    def missing_level_indices(survivors: list[int]) -> list[int]:
        survivor_prices = [active_prices[idx] for idx in survivors]
        missing: list[int] = []
        for level_idx, (target_px, _, match_tol) in enumerate(desired_levels):
            if not any(abs(px - target_px) <= match_tol for px in survivor_prices):
                missing.append(level_idx)
        return missing

    survivors = survivor_indices()
    missing = missing_level_indices(survivors)

    # 4) If full but missing more-aggressive levels, cancel least aggressive survivors.
    while True:
        free_slots = max(0, max_levels - len(survivors))
        if len(missing) <= free_slots:
            break
        if not survivors:
            break
        idx_to_cancel = survivors[-1]
        if idx_to_cancel in cancels:
            break
        cancels.add(idx_to_cancel)
        survivors = survivor_indices()
        missing = missing_level_indices(survivors)

    return sorted(cancels), missing


_NAUTILUS_IMPORT_ERROR: ModuleNotFoundError | None = None
try:
    from nautilus_trader.config import NonNegativeFloat
    from nautilus_trader.config import NonNegativeInt
    from nautilus_trader.config import PositiveInt
    from nautilus_trader.config import StrategyConfig
    from nautilus_trader.model.book import OrderBook
    from nautilus_trader.model.data import OrderBookDeltas
    from nautilus_trader.model.enums import BookType
    from nautilus_trader.model.enums import OrderSide
    from nautilus_trader.model.events import OrderFilled
    from nautilus_trader.model.identifiers import InstrumentId
    from nautilus_trader.model.instruments import Instrument
    from nautilus_trader.model.objects import Quantity
    from nautilus_trader.trading.strategy import Strategy
except ModuleNotFoundError as exc:  # pragma: no cover - pure-math test fallback
    _NAUTILUS_IMPORT_ERROR = exc


if _NAUTILUS_IMPORT_ERROR is None:
    class MakerV3SingleLegQuoterConfig(StrategyConfig, frozen=True):
        maker_instrument_id: InstrumentId
        reference_instrument_id: InstrumentId
        order_qty: Decimal
        external_strategy_id: str = "makerv3_single_leg_quoter"
        bot_on: bool = False
        qty: Decimal | None = None
        des_qty_global: NonNegativeFloat = 0.0
        max_qty_global: NonNegativeFloat = 20_000.0
        max_skew_bps_global: NonNegativeFloat = 0.0
        des_qty_local: NonNegativeFloat = 0.0
        max_qty_local: NonNegativeFloat = 0.0
        max_skew_bps_local: NonNegativeFloat = 0.0
        linear_offset_bps: NonNegativeFloat = 0.0
        max_age_ms: PositiveInt = 2_000
        bid_edge1: NonNegativeFloat = 0.05
        ask_edge1: NonNegativeFloat = 0.05
        place_edge1: NonNegativeFloat = 2.0
        distance1: NonNegativeFloat = 0.02
        n_orders1: NonNegativeInt = 1
        bid_edge2: NonNegativeFloat = 0.15
        ask_edge2: NonNegativeFloat = 0.15
        place_edge2: NonNegativeFloat = 2.0
        distance2: NonNegativeFloat = 0.04
        n_orders2: NonNegativeInt = 1
        bid_edge3: NonNegativeFloat = 0.35
        ask_edge3: NonNegativeFloat = 0.35
        place_edge3: NonNegativeFloat = 2.0
        distance3: NonNegativeFloat = 0.08
        n_orders3: NonNegativeInt = 1
        quote_fail_critical_after_count: NonNegativeInt = 3
        quote_fail_critical_after_s: NonNegativeFloat = 60.0

        @property
        def active_order_qty(self) -> Decimal:
            return self.qty if self.qty is not None else self.order_qty


    class MakerV3SingleLegQuoter(Strategy):
        INTERNAL_REQUOTE_THROTTLE_MS = 150
        BALANCES_PUBLISH_INTERVAL_MS = 10_000
        STALE_CANCELS_PER_SIDE_PER_CYCLE = 1
        PARAMS_REFRESH_INTERVAL_MS = 500
        MARKET_BBO_HEARTBEAT_MS = 1_000

        def __init__(self, config: MakerV3SingleLegQuoterConfig) -> None:
            super().__init__(config)
            self._maker_instrument: Instrument | None = None
            self._order_qty: Quantity | None = None
            self._price_precision: int = 8
            self._books: dict[InstrumentId, OrderBook] = {}
            self._last_bbo: dict[InstrumentId, tuple[str, str] | None] = {}
            self._last_bbo_ts_ns: dict[InstrumentId, int] = {}
            self._last_market_bbo_publish_ns: dict[InstrumentId, int] = {}
            self._last_requote_ns = 0
            self._last_fv: Decimal | None = None
            self._last_fv_snapshot_ts_ns = 0
            self._last_state_ns = 0
            self._last_balances_ns = 0
            self._last_pricing_debug: dict[str, Any] = {}
            self._last_bot_on = bool(self.config.bot_on)
            self._external_strategy_id = (
                self.config.external_strategy_id.strip()
                if self.config.external_strategy_id
                else "makerv3_single_leg_quoter"
            )
            self._runtime_params: dict[str, Any] = {
                name: getattr(self.config, name)
                for name in RUNTIME_PARAM_TYPES
                if hasattr(self.config, name)
            }
            if self._runtime_params.get("qty") is None:
                self._runtime_params["qty"] = self.config.active_order_qty
            self._params_manager: Any | None = None
            self._last_params_refresh_ns = 0
            self._params_timer_name = f"maker-v3-params-refresh:{self._external_strategy_id}"
            self._runtime_params_failed = False
            self._instruments: dict[InstrumentId, Instrument] = {}
            self._managed_client_order_ids: set[str] = set()
            self._quote_failures_ns: list[int] = []
            self._quote_failure_circuit_open = False

        def on_start(self) -> None:
            self._last_bot_on = self._runtime_bool("bot_on")
            instrument_id = self.config.maker_instrument_id
            self._maker_instrument = self.cache.instrument(instrument_id)
            if self._maker_instrument is None:
                self._publish_alert(f"Could not find instrument for {instrument_id}")
                self.stop()
                return

            reference_instrument = self.cache.instrument(self.config.reference_instrument_id)
            if reference_instrument is None:
                self._publish_alert(
                    f"Could not find instrument for {self.config.reference_instrument_id}",
                    level="error",
                )
                self.stop()
                return
            self._instruments = {
                self.config.maker_instrument_id: self._maker_instrument,
                self.config.reference_instrument_id: reference_instrument,
            }

            try:
                self._order_qty = self._maker_instrument.make_qty(self.config.active_order_qty)
            except ValueError:
                self._publish_alert(
                    f"Invalid order quantity configured for {instrument_id}",
                    level="error",
                )
                self.stop()
                return
            try:
                self._refresh_runtime_params(force=True)
            except Exception as exc:
                self._fail_fast_runtime_params(context="on_start", exc=exc)
                return
            self._last_bot_on = self._effective_bot_on()
            self.clock.set_timer(
                name=self._params_timer_name,
                interval=timedelta(milliseconds=self.PARAMS_REFRESH_INTERVAL_MS),
                callback=self.on_time_event,
            )
            self._price_precision = self._maker_instrument.price_precision

            self._books = {
                self.config.maker_instrument_id: OrderBook(
                    instrument_id=self.config.maker_instrument_id,
                    book_type=BookType.L2_MBP,
                ),
                self.config.reference_instrument_id: OrderBook(
                    instrument_id=self.config.reference_instrument_id,
                    book_type=BookType.L2_MBP,
                ),
            }
            self._last_bbo = {key: None for key in self._books}
            self._last_bbo_ts_ns = {key: 0 for key in self._books}
            self._last_market_bbo_publish_ns = {key: 0 for key in self._books}

            self.subscribe_order_book_deltas(
                instrument_id=self.config.maker_instrument_id,
                book_type=BookType.L2_MBP,
            )
            self.subscribe_order_book_deltas(
                instrument_id=self.config.reference_instrument_id,
                book_type=BookType.L2_MBP,
            )

            self._publish_event("started")
            self._publish_balances()
            self._publish_state("on_start")
            self.log.info(
                f"MakerV3 strategy started strategy_id={self._external_strategy_id} "
                f"maker={self.config.maker_instrument_id} reference={self.config.reference_instrument_id}",
            )

        def on_stop(self) -> None:
            self._cancel_managed_quotes("on_stop", force=True)
            timer_names: set[str] = set()
            try:
                timer_names = set(self.clock.timer_names)
            except Exception:
                timer_names = set()
            if self._params_timer_name in timer_names:
                self.clock.cancel_timer(self._params_timer_name)
            self.unsubscribe_order_book_deltas(instrument_id=self.config.maker_instrument_id)
            self.unsubscribe_order_book_deltas(instrument_id=self.config.reference_instrument_id)
            self._publish_state("on_stop")
            self.log.info(
                f"MakerV3 strategy stopped strategy_id={self._external_strategy_id}",
            )

        def on_time_event(self, event: Any) -> None:
            if getattr(event, "name", "") != self._params_timer_name:
                return

            if self._runtime_params_failed:
                return

            now_ns = int(self.clock.timestamp_ns())
            try:
                self._refresh_runtime_params(now_ns=now_ns)
            except Exception as exc:
                self._fail_fast_runtime_params(context="on_time_event", exc=exc)
                return
            bot_on_now = self._effective_bot_on()
            if _did_bot_turn_off(self._last_bot_on, bot_on_now):
                self._cancel_managed_quotes("bot_off_flip", force=True)
                self._publish_state("bot_off")
            self._last_bot_on = bot_on_now

        def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
            book = self._books.get(deltas.instrument_id)
            if book is None:
                return

            book.apply_deltas(deltas)
            bid = book.best_bid_price()
            ask = book.best_ask_price()
            if bid is None or ask is None:
                return

            now_ns = int(self.clock.timestamp_ns())
            bid_str = str(bid)
            ask_str = str(ask)
            last = self._last_bbo.get(deltas.instrument_id)
            bbo_changed = last != (bid_str, ask_str)
            if bbo_changed:
                self._last_bbo[deltas.instrument_id] = (bid_str, ask_str)
            self._last_bbo_ts_ns[deltas.instrument_id] = now_ns

            should_publish_bbo = _should_publish_market_bbo(
                bbo_changed=bbo_changed,
                last_publish_ns=self._last_market_bbo_publish_ns.get(deltas.instrument_id, 0),
                now_ns=now_ns,
                heartbeat_ms=self.MARKET_BBO_HEARTBEAT_MS,
            )
            if should_publish_bbo:
                self._last_market_bbo_publish_ns[deltas.instrument_id] = now_ns
                self._publish_market_bbo(
                    instrument_id=deltas.instrument_id,
                    bid=bid,
                    ask=ask,
                    ts_ns=now_ns,
                )
                if bbo_changed or now_ns - self._last_fv_snapshot_ts_ns >= self.MARKET_BBO_HEARTBEAT_MS * 1_000_000:
                    self._recompute_and_publish_fv()
                self._publish_state_if_due()

            self._publish_balances_if_due()

            bot_on_now = self._effective_bot_on()
            if self.config.maker_instrument_id != deltas.instrument_id:
                return

            if not bot_on_now:
                self._cancel_managed_quotes("bot_off")
                self._publish_state("bot_off")
                return

            now_ns = int(self.clock.timestamp_ns())
            if now_ns - self._last_requote_ns < self.INTERNAL_REQUOTE_THROTTLE_MS * 1_000_000:
                return
            if self._quote_failure_circuit_open:
                return
            try:
                self._refresh_quotes(now_ns=now_ns)
                self._quote_failures_ns.clear()
            except Exception as exc:
                self._handle_quote_failure(now_ns=now_ns, exc=exc, context="on_order_book_deltas")

        def _effective_bot_on(self) -> bool:
            return bool(self._runtime_params.get("bot_on", self.config.bot_on))

        def _runtime_decimal(self, name: str) -> Decimal:
            return _to_decimal(self._runtime_params.get(name, getattr(self.config, name)))

        def _runtime_int(self, name: str) -> int:
            value = self._runtime_params.get(name, getattr(self.config, name))
            try:
                return int(value)
            except Exception:
                return int(getattr(self.config, name))

        def _runtime_bool(self, name: str) -> bool:
            value = self._runtime_params.get(name, getattr(self.config, name))
            parsed = _parse_bool_text(value)
            if parsed is None:
                return bool(getattr(self.config, name))
            return parsed

        def set_params_manager(self, manager: Any | None) -> None:
            self._params_manager = manager

        def _apply_runtime_param_updates(self, updates: dict[str, Any]) -> None:
            qty_changed = False
            for name, raw_value in updates.items():
                if name not in RUNTIME_PARAM_TYPES:
                    raise ValueError(f"Unsupported runtime param: {name!r}")
                coerced = _coerce_runtime_param_value(name, raw_value)
                self._runtime_params[name] = coerced
                if name == "qty":
                    qty_changed = True

            if qty_changed and self._maker_instrument is not None:
                qty = self._runtime_decimal("qty")
                if qty > 0:
                    try:
                        self._order_qty = self._maker_instrument.make_qty(qty)
                    except Exception as exc:
                        raise RuntimeError(
                            f"Failed to convert runtime qty to instrument quantity for "
                            f"{self._external_strategy_id}: qty={qty}",
                        ) from exc

        def _refresh_runtime_params(self, *, now_ns: int | None = None, force: bool = False) -> None:
            if now_ns is None:
                now_ns = int(self.clock.timestamp_ns())
            if not force and now_ns - self._last_params_refresh_ns < self.PARAMS_REFRESH_INTERVAL_MS * 1_000_000:
                return
            self._last_params_refresh_ns = now_ns

            if self._params_manager is None:
                return
            updates_fn = getattr(self._params_manager, "load", None)
            if not callable(updates_fn):
                raise RuntimeError("Configured params manager does not provide load()")
            self._apply_runtime_param_updates(updates_fn())

        def _fail_fast_runtime_params(self, *, context: str, exc: Exception) -> None:
            if self._runtime_params_failed:
                return

            self._runtime_params_failed = True
            error_type = type(exc).__name__
            error_message = str(exc)
            event_payload = {
                "context": context,
                "error_type": error_type,
                "error_message": error_message,
            }

            logger = getattr(self, "log", None)
            if logger is not None:
                log_error = getattr(logger, "error", None)
                if callable(log_error):
                    try:
                        log_error(
                            _to_json_safe(
                                {
                                    "event": "runtime_params_failure",
                                    "strategy_id": self._external_strategy_id,
                                    **event_payload,
                                },
                            ),
                        )
                    except Exception:
                        pass

            try:
                self._publish_event("runtime_params_failure", **event_payload)
            except Exception:
                pass

            try:
                self._publish_alert(
                    (
                        f"runtime_params_failure[{context}] "
                        f"{error_type}: {error_message}"
                    ),
                    level="error",
                )
            except Exception:
                pass

            self.stop()

        def _handle_quote_failure(self, *, now_ns: int, exc: Exception, context: str) -> None:
            if not hasattr(self, "_quote_failure_circuit_open"):
                self._quote_failure_circuit_open = False
            if not hasattr(self, "_quote_failures_ns"):
                self._quote_failures_ns = []

            def _safe(effect: Any) -> None:
                try:
                    effect()
                except Exception:
                    pass

            count_threshold = max(0, self._runtime_int("quote_fail_critical_after_count"))
            window_seconds = max(Decimal("0"), self._runtime_decimal("quote_fail_critical_after_s"))
            window_ns = int(window_seconds * Decimal("1_000_000_000"))
            self._quote_failures_ns.append(now_ns)
            if window_ns > 0:
                cutoff_ns = now_ns - window_ns
                self._quote_failures_ns = [ts_ns for ts_ns in self._quote_failures_ns if ts_ns >= cutoff_ns]
            elif count_threshold > 0:
                self._quote_failures_ns = self._quote_failures_ns[-count_threshold:]

            failure_count = len(self._quote_failures_ns)
            _safe(
                lambda: self._publish_event(
                    "quote_refresh_failed",
                    context=context,
                    failure_count=failure_count,
                    threshold=count_threshold,
                    error_type=type(exc).__name__,
                    error_message=str(exc),
                ),
            )
            _safe(
                lambda: self.log.error(
                    f"Quote refresh failure strategy_id={self._external_strategy_id} context={context} "
                    f"count={failure_count} threshold={count_threshold} err={type(exc).__name__}: {exc}",
                ),
            )
            self._last_requote_ns = now_ns
            if count_threshold <= 0 or failure_count < count_threshold:
                return

            self._quote_failure_circuit_open = True
            try:
                _safe(lambda: self._cancel_managed_quotes("quote_fail_circuit_breaker", force=True))
                _safe(lambda: self._publish_state("blocked_quote_failures"))
                _safe(
                    lambda: self._publish_alert(
                        (
                            "quote_fail_circuit_breaker triggered "
                            f"count={failure_count} threshold={count_threshold} window_s={window_seconds}"
                        ),
                        level="error",
                    ),
                )
                _safe(
                    lambda: self._publish_event(
                        "quote_fail_circuit_breaker",
                        failure_count=failure_count,
                        threshold=count_threshold,
                        window_s=str(window_seconds),
                    ),
                )
                _safe(
                    lambda: self.log.error(
                        f"Quote failure circuit breaker triggered strategy_id={self._external_strategy_id}",
                    ),
                )
            finally:
                _safe(self.stop)

        def on_order_filled(self, event: OrderFilled) -> None:
            self._publish_json(
                TOPIC_TRADE,
                {
                    "strategy_id": self._external_strategy_id,
                    "event": "order_filled",
                    "instrument_id": str(event.instrument_id),
                    "client_order_id": str(event.client_order_id),
                    "trade_id": str(event.trade_id),
                    "side": str(event.order_side),
                    "qty": str(event.last_qty),
                    "price": str(event.last_px),
                    "ts_event": int(event.ts_event),
                },
            )
            cached = self.cache.order(event.client_order_id)
            is_closed = bool(getattr(cached, "is_closed", False)) if cached is not None else False
            if is_closed:
                self._reconcile_managed_order(event.client_order_id, lifecycle="filled")

        def on_order_rejected(self, event: Any) -> None:
            self._reconcile_managed_order(getattr(event, "client_order_id", None), lifecycle="rejected")
            self.log.warning(
                f"Order rejected strategy_id={self._external_strategy_id} "
                f"client_order_id={getattr(event, 'client_order_id', None)}",
            )

        def on_order_canceled(self, event: Any) -> None:
            self._reconcile_managed_order(getattr(event, "client_order_id", None), lifecycle="canceled")

        def on_order_expired(self, event: Any) -> None:
            self._reconcile_managed_order(getattr(event, "client_order_id", None), lifecycle="expired")

        def _reconcile_managed_order(self, client_order_id: Any, *, lifecycle: str) -> None:
            client_order_id_str = str(client_order_id or "")
            if not client_order_id_str:
                return

            had_order = client_order_id_str in self._managed_client_order_ids
            self._managed_client_order_ids.discard(client_order_id_str)
            self._publish_event(
                "order_lifecycle",
                lifecycle=lifecycle,
                client_order_id=client_order_id_str,
                tracked_before=had_order,
                tracked_after=len(self._managed_client_order_ids),
            )

        def _position_signed_qty(self) -> Decimal | None:
            positions: list[Any] = []
            try:
                positions.extend(
                    self.cache.positions_open(
                        instrument_id=self.config.maker_instrument_id,
                    ),
                )
            except Exception:
                pass
            if not positions:
                return None

            total = Decimal("0")
            found = False
            for position in positions:
                signed_qty = _to_decimal_or_none(getattr(position, "signed_qty", None))
                if signed_qty is None:
                    qty = _to_decimal_or_none(getattr(position, "quantity", None))
                    side = _stringify_identifier(getattr(position, "side", "")).upper()
                    if qty is not None:
                        signed_qty = -qty if side == "SHORT" else qty
                if signed_qty is None:
                    continue
                total += signed_qty
                found = True
            return total if found else None

        def _spot_balance_total(self, currency_code: str) -> Decimal | None:
            code = str(currency_code).strip().upper()
            if not code:
                return None
            total = Decimal("0")
            found = False
            accounts: list[Any] = []
            try:
                accounts.extend(list(self.cache.accounts()))
            except Exception:
                pass
            if not accounts and hasattr(self, "portfolio"):
                try:
                    maker_venue = getattr(self.config.maker_instrument_id, "venue", None)
                    account = self.portfolio.account(venue=maker_venue) if maker_venue is not None else None
                except Exception:
                    account = None
                if account is not None:
                    accounts.append(account)

            for account in accounts:
                balances_total_fn = getattr(account, "balances_total", None)
                if not callable(balances_total_fn):
                    continue
                try:
                    balances_total = balances_total_fn()
                except Exception:
                    continue
                if not isinstance(balances_total, dict):
                    continue
                for currency, amount in balances_total.items():
                    if _stringify_identifier(currency).upper() != code:
                        continue
                    amount_dec = _to_decimal_or_none(amount)
                    if amount_dec is None:
                        continue
                    total += amount_dec
                    found = True
            return total if found else None

        def _maker_base_currency_code(self) -> str | None:
            instrument = self._maker_instrument
            if instrument is None:
                instrument = self._instruments.get(self.config.maker_instrument_id)
            if instrument is None:
                return None

            direct_code = _stringify_identifier(getattr(instrument, "base_currency", None)).upper()
            if direct_code:
                return direct_code

            parsed_base, _ = _normalize_contract_symbol(str(getattr(instrument, "id", self.config.maker_instrument_id)))
            return parsed_base or None

        def _compute_inventory_skew(self) -> dict[str, Any]:
            position_qty = self._position_signed_qty()
            base_currency = self._maker_base_currency_code()
            spot_qty = self._spot_balance_total(base_currency) if base_currency else None
            if position_qty is not None:
                inventory_qty = position_qty
                inventory_source = "maker_position"
            elif spot_qty is not None:
                inventory_qty = spot_qty
                inventory_source = "maker_spot_balance"
            else:
                inventory_qty = None
                inventory_source = "unavailable"

            des_qty_global = self._runtime_decimal("des_qty_global")
            max_qty_global = self._runtime_decimal("max_qty_global")
            max_skew_bps_global = self._runtime_decimal("max_skew_bps_global")
            des_qty_local = self._runtime_decimal("des_qty_local")
            max_qty_local = self._runtime_decimal("max_qty_local")
            max_skew_bps_local = self._runtime_decimal("max_skew_bps_local")
            linear_offset_bps = self._runtime_decimal("linear_offset_bps")

            global_ratio: Decimal | None = None
            global_skew_bps: Decimal | None = None
            if inventory_qty is not None and max_qty_global > 0:
                global_ratio = _clamp_decimal((inventory_qty - des_qty_global) / max_qty_global, Decimal("-1"), Decimal("1"))
                global_skew_bps = global_ratio * max(Decimal("0"), max_skew_bps_global)

            local_ratio: Decimal | None = None
            local_skew_bps: Decimal | None = None
            if inventory_qty is not None and max_qty_local > 0:
                local_ratio = _clamp_decimal((inventory_qty - des_qty_local) / max_qty_local, Decimal("-1"), Decimal("1"))
                local_skew_bps = local_ratio * max(Decimal("0"), max_skew_bps_local)

            total_skew_bps = linear_offset_bps
            if global_skew_bps is not None:
                total_skew_bps += global_skew_bps
            if local_skew_bps is not None:
                total_skew_bps += local_skew_bps

            return {
                "inventory_qty": inventory_qty,
                "inventory_source": inventory_source,
                "base_currency": base_currency,
                "position_qty": position_qty,
                "spot_qty": spot_qty,
                "des_qty_global": des_qty_global,
                "max_qty_global": max_qty_global,
                "max_skew_bps_global": max_skew_bps_global,
                "des_qty_local": des_qty_local,
                "max_qty_local": max_qty_local,
                "max_skew_bps_local": max_skew_bps_local,
                "linear_offset_bps": linear_offset_bps,
                "global_ratio": global_ratio,
                "global_skew_bps": global_skew_bps,
                "local_ratio": local_ratio,
                "local_skew_bps": local_skew_bps,
                "total_skew_bps": total_skew_bps,
            }

        def _refresh_quotes(self, now_ns: int) -> None:
            if self._maker_instrument is None or self._order_qty is None:
                return

            maker_bbo = self._best_bid_ask(self.config.maker_instrument_id)
            if maker_bbo is None:
                self._cancel_managed_quotes("maker_md_stale")
                self._publish_state("blocked_maker_md")
                self.log.warning(
                    f"Quoting blocked (maker book unavailable) strategy_id={self._external_strategy_id}",
                )
                return
            best_bid_px, best_ask_px = maker_bbo
            maker_mid = (best_bid_px + best_ask_px) / Decimal("2")

            maker_age_ms = None
            if self._last_bbo_ts_ns.get(self.config.maker_instrument_id, 0) > 0:
                maker_age_ms = int((now_ns - self._last_bbo_ts_ns[self.config.maker_instrument_id]) / 1_000_000)
            reference_age_ms = None
            if self._last_bbo_ts_ns.get(self.config.reference_instrument_id, 0) > 0:
                reference_age_ms = int((now_ns - self._last_bbo_ts_ns[self.config.reference_instrument_id]) / 1_000_000)
            max_age_ms = self._runtime_int("max_age_ms")
            maker_fresh = bool(maker_age_ms is not None and maker_age_ms < max_age_ms)
            reference_fresh = bool(reference_age_ms is not None and reference_age_ms < max_age_ms)
            if not maker_fresh:
                self._cancel_managed_quotes("maker_md_stale")
                self._publish_state("blocked_maker_md")
                self.log.warning(
                    f"Quoting blocked (maker data stale) strategy_id={self._external_strategy_id} "
                    f"age_ms={maker_age_ms} max_age_ms={max_age_ms}",
                )
                return

            ref_bbo = self._best_bid_ask(self.config.reference_instrument_id)
            if ref_bbo is None or not reference_fresh:
                self._cancel_managed_quotes("reference_md_stale")
                self._publish_state("blocked_reference_md")
                self.log.warning(
                    f"Quoting blocked (reference data stale) strategy_id={self._external_strategy_id} "
                    f"age_ms={reference_age_ms} max_age_ms={max_age_ms}",
                )
                return

            ref_bid, ref_ask = ref_bbo
            anchor_bid = ref_bid
            anchor_ask = ref_ask
            anchor_source = "reference_leg"

            reference_mid = (ref_bid + ref_ask) / Decimal("2") if ref_bid is not None and ref_ask is not None else None
            if reference_mid is not None:
                fair_value = (maker_mid + reference_mid) / Decimal("2")
            else:
                fair_value = maker_mid

            bps_anchor = (anchor_bid + anchor_ask) / Decimal("2")
            if bps_anchor <= 0:
                return

            skew_ctx = self._compute_inventory_skew()
            total_skew_bps = _to_decimal(skew_ctx.get("total_skew_bps", Decimal("0")))

            bid_edge1_eff_bps, ask_edge1_eff_bps = _apply_inventory_skew_to_edges(
                bid_edge_bps=self._runtime_decimal("bid_edge1"),
                ask_edge_bps=self._runtime_decimal("ask_edge1"),
                total_skew_bps=total_skew_bps,
            )
            bid_edge2_eff_bps, ask_edge2_eff_bps = _apply_inventory_skew_to_edges(
                bid_edge_bps=self._runtime_decimal("bid_edge2"),
                ask_edge_bps=self._runtime_decimal("ask_edge2"),
                total_skew_bps=total_skew_bps,
            )
            bid_edge3_eff_bps, ask_edge3_eff_bps = _apply_inventory_skew_to_edges(
                bid_edge_bps=self._runtime_decimal("bid_edge3"),
                ask_edge_bps=self._runtime_decimal("ask_edge3"),
                total_skew_bps=total_skew_bps,
            )

            tick = self._maker_instrument.price_increment.as_decimal()

            bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
                anchor_bid=anchor_bid,
                anchor_ask=anchor_ask,
                bid_edges_bps=(bid_edge1_eff_bps, bid_edge2_eff_bps, bid_edge3_eff_bps),
                ask_edges_bps=(ask_edge1_eff_bps, ask_edge2_eff_bps, ask_edge3_eff_bps),
                place_edges_bps=(
                    self._runtime_decimal("place_edge1"),
                    self._runtime_decimal("place_edge2"),
                    self._runtime_decimal("place_edge3"),
                ),
                distances_bps=(
                    self._runtime_decimal("distance1"),
                    self._runtime_decimal("distance2"),
                    self._runtime_decimal("distance3"),
                ),
                n_orders=(self._runtime_int("n_orders1"), self._runtime_int("n_orders2"), self._runtime_int("n_orders3")),
                tick=tick,
            )
            match_tol = tick / Decimal("2") if tick > 0 else Decimal("0")

            desired_buys: list[tuple[Any, Decimal, Decimal]] = []
            desired_sells: list[tuple[Any, Decimal, Decimal]] = []
            seen_buy_prices: set[str] = set()
            seen_sell_prices: set[str] = set()
            for bid_place, bid_cancel in bid_levels:
                bid_place_rounded = _round_price_to_tick(
                    bid_place,
                    tick=tick,
                    is_buy=True,
                    round_in=False,
                )
                bid_cancel_rounded = _round_price_to_tick(
                    bid_cancel,
                    tick=tick,
                    is_buy=True,
                    round_in=False,
                )
                bid_place_rounded = _clamp_post_only_price(
                    price=bid_place_rounded,
                    is_buy=True,
                    top_bid=best_bid_px,
                    top_ask=best_ask_px,
                    tick=tick,
                )
                bid_place_rounded = _nudge_unique_price(
                    price=bid_place_rounded,
                    tick=tick,
                    is_buy=True,
                    seen=seen_buy_prices,
                )
                if bid_place_rounded is None:
                    continue
                seen_buy_prices.add(str(bid_place_rounded))
                if bid_place_rounded > 0 and bid_cancel_rounded > 0:
                    desired_buys.append(
                        (
                            self._maker_instrument.make_price(bid_place_rounded),
                            bid_cancel_rounded,
                            match_tol,
                        ),
                    )
            for ask_place, ask_cancel in ask_levels:
                ask_place_rounded = _round_price_to_tick(
                    ask_place,
                    tick=tick,
                    is_buy=False,
                    round_in=False,
                )
                ask_cancel_rounded = _round_price_to_tick(
                    ask_cancel,
                    tick=tick,
                    is_buy=False,
                    round_in=False,
                )
                ask_place_rounded = _clamp_post_only_price(
                    price=ask_place_rounded,
                    is_buy=False,
                    top_bid=best_bid_px,
                    top_ask=best_ask_px,
                    tick=tick,
                )
                ask_place_rounded = _nudge_unique_price(
                    price=ask_place_rounded,
                    tick=tick,
                    is_buy=False,
                    seen=seen_sell_prices,
                )
                if ask_place_rounded is None:
                    continue
                seen_sell_prices.add(str(ask_place_rounded))
                if ask_place_rounded > 0 and ask_cancel_rounded > 0:
                    desired_sells.append(
                        (
                            self._maker_instrument.make_price(ask_place_rounded),
                            ask_cancel_rounded,
                            match_tol,
                        ),
                    )

            self._last_pricing_debug = {
                "pricing": {
                    "anchor_source": anchor_source,
                    "fv": _decimal_to_json_str(fair_value),
                    "anchor_bid": _decimal_to_json_str(anchor_bid),
                    "anchor_ask": _decimal_to_json_str(anchor_ask),
                    "ref_bid": _decimal_to_json_str(ref_bid),
                    "ref_ask": _decimal_to_json_str(ref_ask),
                    "ref_mid": _decimal_to_json_str(reference_mid),
                    "maker_top_bid": _decimal_to_json_str(best_bid_px),
                    "maker_top_ask": _decimal_to_json_str(best_ask_px),
                    "maker_mid": _decimal_to_json_str(maker_mid),
                    "reference_mid": _decimal_to_json_str(reference_mid),
                    "anchor_spread_bps": _decimal_to_json_str(
                        ((anchor_ask - anchor_bid) / bps_anchor) * Decimal("10000")
                        if bps_anchor > 0
                        else None,
                    ),
                    "bid_edge1_cfg_bps": _decimal_to_json_str(self._runtime_decimal("bid_edge1")),
                    "ask_edge1_cfg_bps": _decimal_to_json_str(self._runtime_decimal("ask_edge1")),
                    "bid_edge1_eff_bps": _decimal_to_json_str(bid_edge1_eff_bps),
                    "ask_edge1_eff_bps": _decimal_to_json_str(ask_edge1_eff_bps),
                    "effective_skew_bps": _decimal_to_json_str(total_skew_bps),
                    "total_skew_bps": _decimal_to_json_str(total_skew_bps),
                },
                "skew": {
                    "inventory_qty": _decimal_to_json_str(skew_ctx["inventory_qty"]),
                    "inventory_source": skew_ctx["inventory_source"],
                    "position_qty": _decimal_to_json_str(skew_ctx["position_qty"]),
                    "spot_base_total": _decimal_to_json_str(skew_ctx["spot_qty"]),
                    "base_currency": skew_ctx["base_currency"],
                    "des_qty_global": _decimal_to_json_str(skew_ctx["des_qty_global"]),
                    "max_qty_global": _decimal_to_json_str(skew_ctx["max_qty_global"]),
                    "max_skew_bps_global": _decimal_to_json_str(skew_ctx["max_skew_bps_global"]),
                    "des_qty_local": _decimal_to_json_str(skew_ctx["des_qty_local"]),
                    "max_qty_local": _decimal_to_json_str(skew_ctx["max_qty_local"]),
                    "max_skew_bps_local": _decimal_to_json_str(skew_ctx["max_skew_bps_local"]),
                    "linear_offset_bps": _decimal_to_json_str(skew_ctx["linear_offset_bps"]),
                    "global_ratio": _decimal_to_json_str(skew_ctx["global_ratio"]),
                    "global_skew_bps": _decimal_to_json_str(skew_ctx["global_skew_bps"]),
                    "local_ratio": _decimal_to_json_str(skew_ctx["local_ratio"]),
                    "local_skew_bps": _decimal_to_json_str(skew_ctx["local_skew_bps"]),
                    "total_skew_bps": _decimal_to_json_str(skew_ctx["total_skew_bps"]),
                },
                "md_health": {
                    "maker_age_ms": maker_age_ms,
                    "reference_age_ms": reference_age_ms,
                    "maker_fresh": maker_fresh,
                    "reference_fresh": reference_fresh,
                },
            }

            if not desired_buys and not desired_sells:
                self._cancel_managed_quotes("no_targets")
                self._last_requote_ns = now_ns
                return

            active_orders = self._managed_orders()
            active_buys = sorted(
                [order for order in active_orders if order.side == OrderSide.BUY],
                key=lambda order: _price_to_decimal(order.price),
                reverse=True,
            )
            active_sells = sorted(
                [order for order in active_orders if order.side == OrderSide.SELL],
                key=lambda order: _price_to_decimal(order.price),
            )

            cancels = 0
            places = 0
            cancels += self._rebalance_side(
                side=OrderSide.BUY,
                active_orders=active_buys,
                desired_levels=desired_buys,
                now_ns=now_ns,
            )
            cancels += self._rebalance_side(
                side=OrderSide.SELL,
                active_orders=active_sells,
                desired_levels=desired_sells,
                now_ns=now_ns,
            )
            places += self._place_missing_levels(
                side=OrderSide.BUY,
                active_orders=active_buys,
                desired_levels=desired_buys,
                best_bid_px=best_bid_px,
                best_ask_px=best_ask_px,
            )
            places += self._place_missing_levels(
                side=OrderSide.SELL,
                active_orders=active_sells,
                desired_levels=desired_sells,
                best_bid_px=best_bid_px,
                best_ask_px=best_ask_px,
            )

            self._last_requote_ns = now_ns
            if cancels or places:
                self._publish_event(
                    "quotes_rebalanced",
                    bid_levels=len(desired_buys),
                    ask_levels=len(desired_sells),
                    cancels=cancels,
                    places=places,
                )
                self._publish_state("quotes_replaced")

        def _publish_state_if_due(self) -> None:
            now_ns = int(self.clock.timestamp_ns())
            if now_ns - self._last_state_ns < 250_000_000:
                return
            self._publish_state("running")

        def _publish_balances_if_due(self) -> None:
            now_ns = int(self.clock.timestamp_ns())
            if now_ns - self._last_balances_ns < self.BALANCES_PUBLISH_INTERVAL_MS * 1_000_000:
                return
            self._publish_balances()

        def _is_stale_order(self, order: Any, now_ns: int) -> bool:
            max_age_ns = self._runtime_int("max_age_ms") * 1_000_000
            ts_init = int(getattr(order, "ts_init", 0))
            return ts_init > 0 and now_ns - ts_init >= max_age_ns

        def _rebalance_side(
            self,
            *,
            side: OrderSide,
            active_orders: list[Any],
            desired_levels: list[tuple[Any, Decimal, Decimal]],
            now_ns: int,
        ) -> int:
            side_name = "buy" if side == OrderSide.BUY else "sell"
            active_prices = [_price_to_decimal(order.price) for order in active_orders]
            active_stale = [self._is_stale_order(order, now_ns) for order in active_orders]
            desired_dec = [
                (_price_to_decimal(target_price), cancel_px, match_tol)
                for target_price, cancel_px, match_tol in desired_levels
            ]

            cancel_indices, _ = plan_side_rebalance_actions(
                side=side_name,
                active_prices=active_prices,
                active_stale=active_stale,
                desired_levels=desired_dec,
                stale_cancel_budget=self.STALE_CANCELS_PER_SIDE_PER_CYCLE,
            )

            for index in cancel_indices:
                self.cancel_order(active_orders[index])

            if cancel_indices:
                cancel_index_set = set(cancel_indices)
                active_orders[:] = [
                    order
                    for index, order in enumerate(active_orders)
                    if index not in cancel_index_set
                ]

            return len(cancel_indices)

        def _place_missing_levels(
            self,
            *,
            side: OrderSide,
            active_orders: list[Any],
            desired_levels: list[tuple[Any, Decimal, Decimal]],
            best_bid_px: Decimal,
            best_ask_px: Decimal,
        ) -> int:
            places = 0
            active_prices = [
                _price_to_decimal(order.price)
                for order in active_orders
            ]
            for target_price, _, match_tol in desired_levels:
                target_px = _price_to_decimal(target_price)
                if side == OrderSide.BUY and target_px >= best_ask_px:
                    continue
                if side == OrderSide.SELL and target_px <= best_bid_px:
                    continue
                if any(abs(existing_px - target_px) <= match_tol for existing_px in active_prices):
                    continue
                order = self.order_factory.limit(
                    instrument_id=self.config.maker_instrument_id,
                    order_side=side,
                    quantity=self._order_qty,
                    price=target_price,
                    post_only=True,
                )
                self.submit_order(order)
                self._register_managed_order(order)
                places += 1
                active_prices.append(target_px)
            return places

        def _register_managed_order(self, order: Any) -> None:
            client_order_id = str(getattr(order, "client_order_id", "") or "")
            if not client_order_id:
                return
            self._managed_client_order_ids.add(client_order_id)

        def _managed_orders(self) -> list[Any]:
            orders: list[Any] = []
            seen_order_keys: set[tuple[Any, ...]] = set()
            sources: list[list[Any]] = []
            for fetch_name in ("orders_open", "orders_inflight"):
                fetch = getattr(self.cache, fetch_name, None)
                if not callable(fetch):
                    continue
                try:
                    rows = fetch(
                        instrument_id=self.config.maker_instrument_id,
                        strategy_id=self.id,
                    )
                except Exception:
                    rows = []
                sources.append(list(rows))

            for source in sources:
                for order in source:
                    is_closed = getattr(order, "is_closed", False)
                    if callable(is_closed):
                        try:
                            is_closed = bool(is_closed())
                        except Exception:
                            is_closed = False
                    if bool(is_closed):
                        continue

                    client_order_id = str(getattr(order, "client_order_id", "") or "")
                    venue_order_id = str(getattr(order, "venue_order_id", "") or "")
                    if client_order_id:
                        dedupe_key: tuple[Any, ...] = ("client", client_order_id)
                    elif venue_order_id:
                        dedupe_key = ("venue", venue_order_id)
                    else:
                        dedupe_key = (
                            "shape",
                            str(getattr(order, "side", "")),
                            str(getattr(order, "price", "")),
                            str(getattr(order, "quantity", "")),
                            int(getattr(order, "ts_init", 0) or 0),
                        )

                    if dedupe_key in seen_order_keys:
                        continue
                    seen_order_keys.add(dedupe_key)
                    orders.append(order)
            return orders

        def _cancel_managed_quotes(self, reason: str, force: bool = False) -> None:
            managed_orders = self._managed_orders()
            tracked_ids = getattr(self, "_managed_client_order_ids", set())
            tracked_count = len(tracked_ids)
            should_cancel = bool(managed_orders or tracked_count > 0)
            if not should_cancel:
                return

            for order in managed_orders:
                try:
                    self.cancel_order(order)
                except Exception:
                    pass
            self.cancel_all_orders(self.config.maker_instrument_id)
            self._publish_event(
                "quotes_canceled",
                reason=reason,
                force=force,
                cache_count=len(managed_orders),
                tracked_count=tracked_count,
            )
            self.log.info(
                f"Managed quote cancel triggered strategy_id={self._external_strategy_id} "
                f"reason={reason} force={force} cache_count={len(managed_orders)} tracked_count={tracked_count}",
            )
            if force and reason in {"on_stop", "quote_fail_circuit_breaker"}:
                tracked_ids.clear()
            elif not managed_orders:
                tracked_ids.clear()

        def _best_bid_ask(self, instrument_id: InstrumentId) -> tuple[Decimal, Decimal] | None:
            book = self._books.get(instrument_id)
            if book is None:
                return None
            bid = book.best_bid_price()
            ask = book.best_ask_price()
            if bid is None or ask is None:
                return None
            return bid.as_decimal(), ask.as_decimal()

        def _best_mid(self, instrument_id: InstrumentId) -> Decimal | None:
            bbo = self._best_bid_ask(instrument_id)
            if bbo is None:
                return None
            bid, ask = bbo
            return (bid + ask) / Decimal("2")

        def _book_spread(self, instrument_id: InstrumentId) -> Decimal | None:
            bbo = self._best_bid_ask(instrument_id)
            if bbo is None:
                return None
            bid, ask = bbo
            return ask - bid

        def _recompute_and_publish_fv(self) -> None:
            maker_mid = self._best_mid(self.config.maker_instrument_id)
            reference_mid = self._best_mid(self.config.reference_instrument_id)
            if maker_mid is None and reference_mid is None:
                return

            if maker_mid is not None and reference_mid is not None:
                self._last_fv = (maker_mid + reference_mid) / Decimal("2")
            else:
                self._last_fv = maker_mid or reference_mid

            now_ns = int(self.clock.timestamp_ns())
            payload = {
                "strategy_id": self._external_strategy_id,
                "fv": str(self._last_fv),
                "maker_mid": str(maker_mid) if maker_mid is not None else None,
                "reference_mid": str(reference_mid) if reference_mid is not None else None,
                "ts_event": now_ns,
                "ts_ms": now_ns // 1_000_000,
            }
            self._publish_json(TOPIC_FV, [payload])
            self._last_fv_snapshot_ts_ns = now_ns

        def _publish_market_bbo(
            self,
            *,
            instrument_id: InstrumentId,
            bid: Any,
            ask: Any,
            ts_ns: int,
        ) -> None:
            instrument_text = str(instrument_id)
            instrument = self._instruments.get(instrument_id)
            if instrument is None:
                instrument = self.cache.instrument(instrument_id)

            exchange = _stringify_identifier(getattr(instrument_id, "venue", None)).lower()
            symbol = _stringify_identifier(getattr(instrument, "raw_symbol", ""))
            if not symbol:
                symbol = instrument_text.split(".", maxsplit=1)[0]

            base = _stringify_identifier(getattr(instrument, "base_currency", None)).upper()
            quote = _stringify_identifier(getattr(instrument, "quote_currency", None)).upper()
            if not base or not quote:
                parsed_base, parsed_quote = _normalize_contract_symbol(symbol)
                base = base or parsed_base
                quote = quote or parsed_quote

            symbol_pair = f"{base}/{quote}" if base and quote else symbol
            fv_coin = (
                f"{base.lower()}/{quote.lower()}"
                if base and quote
                else symbol.replace("-", "/").replace("_", "/").lower()
            )

            payload = {
                "strategy_id": self._external_strategy_id,
                "instrument_id": instrument_text,
                "exchange": exchange,
                "base": base,
                "quote": quote,
                "symbol": symbol_pair,
                "fv_coin": fv_coin,
                "bid": str(bid),
                "ask": str(ask),
                "ts_event": ts_ns,
                "ts_ms": ts_ns // 1_000_000,
            }
            self._publish_json(TOPIC_MARKET_BBO, payload)

        def _publish_state(self, state: str) -> None:
            now_ns = int(self.clock.timestamp_ns())
            self._last_state_ns = now_ns
            managed_orders = self._managed_orders()
            payload: dict[str, Any] = {
                "strategy_id": self._external_strategy_id,
                "state": state,
                "bot_on": self._effective_bot_on(),
                "managed_orders": max(
                    len(managed_orders),
                    len(getattr(self, "_managed_client_order_ids", set())),
                ),
                "ts_event": now_ns,
                "ts_ms": now_ns // 1_000_000,
            }
            if self._last_pricing_debug:
                payload["pricing_debug"] = self._last_pricing_debug
            self._publish_json(
                TOPIC_STATE,
                payload,
            )

        def _publish_event(self, name: str, **payload: Any) -> None:
            now_ns = int(self.clock.timestamp_ns())
            data: dict[str, Any] = {
                "strategy_id": self._external_strategy_id,
                "event": name,
                "ts_event": now_ns,
                "ts_ms": now_ns // 1_000_000,
            }
            data.update(payload)
            self._publish_json(TOPIC_EVENT, data)

        def _publish_alert(self, message: str, level: str = "warning") -> None:
            now_ns = int(self.clock.timestamp_ns())
            self._publish_json(
                TOPIC_ALERT,
                {
                    "strategy_id": self._external_strategy_id,
                    "level": level,
                    "message": message,
                    "ts_event": now_ns,
                    "ts_ms": now_ns // 1_000_000,
                },
            )

        def _publish_balances(self) -> None:
            now_ns = int(self.clock.timestamp_ns())
            self._last_balances_ns = now_ns
            payload: dict[str, Any] = {
                "strategy_id": self._external_strategy_id,
                "accounts": [],
                "positions": [],
            }
            try:
                accounts = list(self.cache.accounts())
            except Exception:
                accounts = []
            if not accounts and hasattr(self, "cache"):
                for account in getattr(self.cache, "accounts", lambda: [])():
                    if account is not None:
                        accounts.append(account)

            if not accounts and hasattr(self, "portfolio"):
                try:
                    maker_venue = getattr(self.config.maker_instrument_id, "venue", None)
                    account = self.portfolio.account(venue=maker_venue) if maker_venue is not None else None
                except Exception:
                    account = None
                if account is not None:
                    accounts.append(account)

            for account in accounts:
                payload["accounts"].append(_serialize_account_payload(account))

            positions: list[Any] = []
            try:
                positions.extend(
                    self.cache.positions_open(
                        instrument_id=self.config.maker_instrument_id,
                    ),
                )
            except Exception:
                pass
            if not positions and hasattr(self, "cache"):
                try:
                    positions.extend(self.cache.positions_open())
                except Exception:
                    pass

            for position in positions:
                payload["positions"].append(_serialize_position_payload(position))

            payload["ts_event"] = now_ns
            payload["ts_ms"] = now_ns // 1_000_000
            self._publish_json(TOPIC_BALANCES, payload)

        def _publish_json(self, topic: str, payload: dict[str, Any]) -> None:
            payload_json = _to_json_safe(payload)
            if FluxBusPayload is None:
                self.msgbus.publish(topic=topic, msg=payload_json)
                return

            self.msgbus.publish(
                topic=topic,
                msg=FluxBusPayload(topic=topic, payload=payload_json),
            )


else:
    class MakerV3SingleLegQuoterConfig:  # pragma: no cover - fallback for pure-math tests
        pass


    class MakerV3SingleLegQuoter:  # pragma: no cover - fallback for pure-math tests
        def __init__(self, *_args: Any, **_kwargs: Any) -> None:
            raise ModuleNotFoundError(
                "NautilusTrader runtime modules are unavailable in this environment",
            ) from _NAUTILUS_IMPORT_ERROR
