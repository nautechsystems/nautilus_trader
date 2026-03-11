#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from bisect import bisect_left
from collections import defaultdict
from collections.abc import Iterable
from collections.abc import Mapping
from decimal import Decimal
from pathlib import Path
from typing import Any

from flux.persistence.markouts.common import decimal_text as _decimal_text
from flux.persistence.markouts.common import markout_bps as _markout_bps
from flux.persistence.markouts.common import signed_markout
from flux.persistence.markouts.common import to_decimal as _to_decimal
from flux.persistence.markouts.common import to_optional_int as _to_int


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _default_config_path(profile: str) -> Path:
    normalized = _normalize_profile_name(profile)
    if normalized == "tokenmm":
        return _repo_root() / "deploy/tokenmm/tokenmm.live.toml"
    return _repo_root() / f"deploy/{normalized}/{normalized}.live.toml"


def _normalize_profile_name(profile: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", profile.strip().lower()).strip("_")


def _decode_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def _parse_horizons(raw: str) -> tuple[int, ...]:
    seen: set[int] = set()
    horizons: list[int] = []
    for item in raw.split(","):
        text = item.strip()
        if not text:
            continue
        horizon_s = int(text)
        if horizon_s <= 0:
            raise ValueError(f"horizon must be positive, got {horizon_s}")
        if horizon_s in seen:
            continue
        seen.add(horizon_s)
        horizons.append(horizon_s)
    if not horizons:
        raise ValueError("at least one horizon is required")
    return tuple(horizons)


def _parse_strategy_args(values: Iterable[str]) -> list[str]:
    seen: set[str] = set()
    strategy_ids: list[str] = []
    for raw_value in values:
        for part in raw_value.split(","):
            strategy_id = part.strip()
            if not strategy_id or strategy_id in seen:
                continue
            seen.add(strategy_id)
            strategy_ids.append(strategy_id)
    return strategy_ids


def _read_profile_strategy_ids(*, profile: str, config_path: Path) -> list[str]:
    if not config_path.is_file():
        return []
    with config_path.open("rb") as fh:
        payload = tomllib.load(fh)
    field_name = f"{_normalize_profile_name(profile)}_strategy_ids"
    raw_ids = payload.get("api", {}).get(field_name) or []
    return _parse_strategy_args(str(item) for item in raw_ids)


def _first_text(mapping: Mapping[str, Any], *keys: str) -> str:
    for key in keys:
        text = _decode_text(mapping.get(key)).strip()
        if text:
            return text
    return ""


def _fill_identity(row: Mapping[str, Any]) -> str:
    return _first_text(row, "row_id", "event_id", "trade_id", "entry_id")


def _trade_identity(row: Mapping[str, Any]) -> str:
    return _first_text(row, "trade_id") or _fill_identity(row)


def _missing_future_fv_row(
    *,
    strategy_id: str,
    trade_id: str,
    fill_id: str,
    horizon_s: int,
    fill_side: str,
    fill_px: Decimal,
    fill_qty: Decimal,
    fill_ts_ms: int,
) -> dict[str, Any]:
    return {
        "strategy_id": strategy_id,
        "trade_id": trade_id,
        "fill_id": fill_id,
        "horizon_s": int(horizon_s),
        "fill_side": fill_side,
        "fill_px": fill_px,
        "fill_qty": fill_qty,
        "fill_ts_ms": fill_ts_ms,
        "benchmark_px": None,
        "benchmark_ts_ms": None,
        "markout_abs": None,
        "markout_bps": None,
        "status": "missing_future_fv",
    }


def _build_fv_index(
    fv_rows: Iterable[Mapping[str, Any]],
) -> dict[str, tuple[list[int], list[dict[str, Any]]]]:
    grouped_rows: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in fv_rows:
        strategy_id = _first_text(row, "strategy_id")
        ts_ms = _to_int(row.get("ts_ms"))
        fv = _to_decimal(row.get("fv"))
        if not strategy_id or ts_ms is None or fv is None:
            continue
        grouped_rows[strategy_id].append(
            {
                "strategy_id": strategy_id,
                "ts_ms": ts_ms,
                "fv": fv,
            },
        )

    indexed_rows: dict[str, tuple[list[int], list[dict[str, Any]]]] = {}
    for strategy_id, rows in grouped_rows.items():
        ordered_rows = sorted(rows, key=lambda item: item["ts_ms"])
        indexed_rows[strategy_id] = (
            [int(item["ts_ms"]) for item in ordered_rows],
            ordered_rows,
        )
    return indexed_rows


def _oldest_target_ts_ms(
    trade_rows: Iterable[Mapping[str, Any]],
    horizons_s: Iterable[int] | None,
) -> int | None:
    if horizons_s is None:
        return None
    ordered_horizons = tuple(int(horizon_s) for horizon_s in horizons_s)
    if not ordered_horizons:
        return None
    min_horizon_ms = min(ordered_horizons) * 1_000
    oldest_target_ts_ms: int | None = None
    for trade_row in trade_rows:
        fill_ts_ms = _to_int(trade_row.get("ts_ms"))
        if fill_ts_ms is None:
            continue
        target_ts_ms = fill_ts_ms + min_horizon_ms
        if oldest_target_ts_ms is None or target_ts_ms < oldest_target_ts_ms:
            oldest_target_ts_ms = target_ts_ms
    return oldest_target_ts_ms


def compute_markout_rows(
    *,
    trade_rows: Iterable[Mapping[str, Any]],
    fv_rows: Iterable[Mapping[str, Any]],
    horizons_s: Iterable[int],
) -> list[dict[str, Any]]:
    ordered_horizons = tuple(int(horizon_s) for horizon_s in horizons_s)
    fv_index = _build_fv_index(fv_rows)
    output_rows: list[dict[str, Any]] = []

    for trade_row in trade_rows:
        strategy_id = _first_text(trade_row, "strategy_id")
        trade_id = _trade_identity(trade_row)
        fill_id = _fill_identity(trade_row)
        fill_side = _first_text(trade_row, "side", "order_side").upper()
        fill_px = _to_decimal(trade_row.get("price") or trade_row.get("fill_px"))
        fill_qty = _to_decimal(
            trade_row.get("qty") or trade_row.get("size") or trade_row.get("fill_qty")
        )
        fill_ts_ms = _to_int(trade_row.get("ts_ms"))
        if (
            not strategy_id
            or not trade_id
            or not fill_id
            or not fill_side
            or fill_px is None
            or fill_qty is None
            or fill_ts_ms is None
        ):
            continue

        fv_timestamps, strategy_fv_rows = fv_index.get(strategy_id, ([], []))
        earliest_fv_ts_ms = fv_timestamps[0] if fv_timestamps else None
        for horizon_s in ordered_horizons:
            target_ts_ms = fill_ts_ms + (int(horizon_s) * 1_000)
            position = bisect_left(fv_timestamps, target_ts_ms)
            if position >= len(strategy_fv_rows) or (
                position == 0 and earliest_fv_ts_ms is not None and earliest_fv_ts_ms > target_ts_ms
            ):
                output_rows.append(
                    _missing_future_fv_row(
                        strategy_id=strategy_id,
                        trade_id=trade_id,
                        fill_id=fill_id,
                        horizon_s=int(horizon_s),
                        fill_side=fill_side,
                        fill_px=fill_px,
                        fill_qty=fill_qty,
                        fill_ts_ms=fill_ts_ms,
                    ),
                )
                continue

            benchmark_row = strategy_fv_rows[position]
            benchmark_px = benchmark_row["fv"]
            markout_abs = signed_markout(fill_side, fill_px, benchmark_px)
            output_rows.append(
                {
                    "strategy_id": strategy_id,
                    "trade_id": trade_id,
                    "fill_id": fill_id,
                    "horizon_s": int(horizon_s),
                    "fill_side": fill_side,
                    "fill_px": fill_px,
                    "fill_qty": fill_qty,
                    "fill_ts_ms": fill_ts_ms,
                    "benchmark_px": benchmark_px,
                    "benchmark_ts_ms": int(benchmark_row["ts_ms"]),
                    "markout_abs": markout_abs,
                    "markout_bps": _markout_bps(markout_abs, fill_px),
                    "status": "ok",
                },
            )

    return output_rows


def summarize_markout_rows(rows: Iterable[Mapping[str, Any]]) -> list[dict[str, Any]]:
    grouped_rows: dict[int, list[Mapping[str, Any]]] = defaultdict(list)
    for row in rows:
        horizon_s = _to_int(row.get("horizon_s"))
        if horizon_s is None:
            continue
        grouped_rows[horizon_s].append(row)

    summary_rows: list[dict[str, Any]] = []
    for horizon_s in sorted(grouped_rows):
        ok_rows = [
            row
            for row in grouped_rows[horizon_s]
            if row.get("status") == "ok"
            and isinstance(row.get("markout_abs"), Decimal)
            and isinstance(row.get("markout_bps"), Decimal)
        ]
        if not ok_rows:
            summary_rows.append(
                {
                    "horizon_s": horizon_s,
                    "count": 0,
                    "avg_markout_abs": None,
                    "avg_markout_bps": None,
                },
            )
            continue

        count = len(ok_rows)
        total_markout_abs = sum((row["markout_abs"] for row in ok_rows), Decimal(0))
        total_markout_bps = sum((row["markout_bps"] for row in ok_rows), Decimal(0))
        summary_rows.append(
            {
                "horizon_s": horizon_s,
                "count": count,
                "avg_markout_abs": total_markout_abs / count,
                "avg_markout_bps": total_markout_bps / count,
            },
        )
    return summary_rows


def load_stream_rows(
    redis_client: Any,
    strategy_id: str,
    *,
    limit: int = 5_000,
    horizons_s: Iterable[int] | None = None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    from flux.api._payloads_common import extract_stream_rows
    from flux.common.keys import FluxRedisKeys

    keys = FluxRedisKeys(strategy_id=strategy_id)
    fetch_count = max(1, int(limit))
    trade_entries = redis_client.xrevrange(keys.trades_stream(), count=fetch_count)
    trade_rows = list(reversed(extract_stream_rows(trade_entries)))
    oldest_target_ts_ms = _oldest_target_ts_ms(trade_rows, horizons_s)

    fv_entries: list[Any] = []
    fv_max = "+"
    while True:
        page = redis_client.xrevrange(
            keys.fv_stream(),
            max=fv_max,
            min="-",
            count=fetch_count,
        )
        if not page:
            break
        fv_entries.extend(page)
        fv_rows = extract_stream_rows(page)
        oldest_page_ts_ms = min(
            (ts_ms for row in fv_rows if (ts_ms := _to_int(row.get("ts_ms"))) is not None),
            default=None,
        )
        if oldest_target_ts_ms is None:
            break
        if oldest_page_ts_ms is not None and oldest_page_ts_ms <= oldest_target_ts_ms:
            break
        if len(page) < fetch_count:
            break
        oldest_entry_id = _decode_text(page[-1][0]).strip()
        if not oldest_entry_id:
            break
        fv_max = f"({oldest_entry_id}"

    fv_rows = list(reversed(extract_stream_rows(fv_entries)))
    return trade_rows, fv_rows


def _json_ready(value: Any) -> Any:
    if isinstance(value, Decimal):
        return _decimal_text(value)
    if isinstance(value, Mapping):
        return {str(key): _json_ready(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_json_ready(item) for item in value]
    if isinstance(value, tuple):
        return [_json_ready(item) for item in value]
    return value


def _resolve_strategy_ids(args: argparse.Namespace) -> list[str]:
    strategy_ids = _parse_strategy_args(args.strategy or [])
    if strategy_ids:
        return strategy_ids
    if not args.profile:
        raise ValueError("provide at least one --strategy or a --profile")

    config_path = Path(args.config) if args.config else _default_config_path(args.profile)
    profile_strategy_ids = _read_profile_strategy_ids(profile=args.profile, config_path=config_path)
    if not profile_strategy_ids:
        raise ValueError(
            f"no strategy ids found for profile={args.profile!r} in {config_path}",
        )
    return profile_strategy_ids


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Compute MakerV3 30s/60s/120s markouts from Redis trade and FV streams.",
    )
    parser.add_argument(
        "--strategy",
        action="append",
        default=[],
        help="Flux strategy id to report. Repeat or pass a comma-separated list.",
    )
    parser.add_argument(
        "--profile",
        help="Profile name that maps to <profile>_strategy_ids in a deploy TOML file.",
    )
    parser.add_argument(
        "--config",
        help="Optional TOML config path used for --profile strategy resolution.",
    )
    parser.add_argument(
        "--redis-url",
        default="redis://127.0.0.1:6379/0",
        help="Redis connection URL for the live Flux host.",
    )
    parser.add_argument(
        "--horizons",
        default="30,60,120",
        help="Comma-separated markout horizons in seconds.",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=5_000,
        help="Max trade rows to fetch per strategy; FV rows are paged backward as needed to cover the trade horizons.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit raw report payload as JSON instead of text.",
    )
    return parser


def _build_report(args: argparse.Namespace) -> dict[str, Any]:
    try:
        horizons = _parse_horizons(args.horizons)
    except ValueError as e:
        raise ValueError(f"invalid --horizons value: {e}") from e

    strategy_ids = _resolve_strategy_ids(args)

    import redis

    redis_client = redis.Redis.from_url(args.redis_url, decode_responses=False)
    reports: list[dict[str, Any]] = []
    for strategy_id in strategy_ids:
        trade_rows, fv_rows = load_stream_rows(
            redis_client,
            strategy_id=strategy_id,
            limit=args.limit,
            horizons_s=horizons,
        )
        markout_rows = compute_markout_rows(
            trade_rows=trade_rows,
            fv_rows=fv_rows,
            horizons_s=horizons,
        )
        reports.append(
            {
                "strategy_id": strategy_id,
                "trade_count": len(trade_rows),
                "fv_count": len(fv_rows),
                "markout_rows": markout_rows,
                "summary": summarize_markout_rows(markout_rows),
            },
        )

    return {
        "benchmark": "fv_market_mid",
        "horizons_s": list(horizons),
        "strategies": reports,
    }


def _render_text_report(report: Mapping[str, Any]) -> str:
    lines = [
        f"benchmark={report.get('benchmark')}",
        f"horizons_s={','.join(str(value) for value in report.get('horizons_s', []))}",
    ]
    for strategy_report in report.get("strategies", []):
        if not isinstance(strategy_report, Mapping):
            continue
        strategy_id = _decode_text(strategy_report.get("strategy_id")).strip()
        lines.append("")
        lines.append(f"[{strategy_id}]")
        lines.append(
            f"trade_count={strategy_report.get('trade_count', 0)} fv_count={strategy_report.get('fv_count', 0)}",
        )
        for row in strategy_report.get("summary", []):
            if not isinstance(row, Mapping):
                continue
            lines.append(
                "  horizon_s={horizon_s} count={count} avg_markout_abs={avg_markout_abs} avg_markout_bps={avg_markout_bps}".format(
                    horizon_s=row.get("horizon_s"),
                    count=row.get("count"),
                    avg_markout_abs=_decimal_text(row.get("avg_markout_abs"))
                    if isinstance(row.get("avg_markout_abs"), Decimal)
                    else "None",
                    avg_markout_bps=_decimal_text(row.get("avg_markout_bps"))
                    if isinstance(row.get("avg_markout_bps"), Decimal)
                    else "None",
                ),
            )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)
    try:
        report = _build_report(args)
    except ValueError as e:
        parser.error(str(e))
    if args.json:
        print(json.dumps(_json_ready(report), indent=2, sort_keys=False))
    else:
        print(_render_text_report(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
