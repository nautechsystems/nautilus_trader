#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import os
import sys
import time
import tomllib
from pathlib import Path
from typing import Any
from urllib.parse import quote

import redis

REPO_ROOT = Path(__file__).resolve().parents[2]
FLUX_ROOT = REPO_ROOT / "systems/flux"
for entry in (str(REPO_ROOT), str(FLUX_ROOT)):
    if entry not in sys.path:
        sys.path.insert(0, entry)

from flux.api._payloads_common import decode_text
from flux.api._payloads_common import extract_stream_rows
from flux.common.keys import FluxRedisKeys
from ops.scripts.makerv3_markouts import _default_config_path
from ops.scripts.makerv3_markouts import _parse_strategy_args
from ops.scripts.makerv3_markouts import _read_profile_strategy_ids


def _repo_root() -> Path:
    return REPO_ROOT


def _load_config(path: Path) -> dict[str, Any]:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def _build_redis_url(
    *,
    config_path: Path,
    password: str | None,
) -> str:
    payload = _load_config(config_path)
    redis_config = payload.get("redis", {})
    host = str(redis_config.get("host") or "127.0.0.1")
    port = int(redis_config.get("port") or 6379)
    db = int(redis_config.get("db") or 0)
    username = str(redis_config.get("username") or "")
    configured_password = str(redis_config.get("password") or "")
    effective_password = configured_password
    if password is not None:
        effective_password = password
    scheme = "rediss" if bool(redis_config.get("ssl")) else "redis"
    auth = ""
    if username or effective_password:
        auth = f"{quote(username, safe='')}:{quote(effective_password, safe='')}@"
    return f"{scheme}://{auth}{host}:{port}/{db}"


def _resolve_password(args: argparse.Namespace) -> str | None:
    if args.redis_password is not None:
        return args.redis_password
    if args.redis_password_env:
        return os.environ.get(args.redis_password_env)
    return os.environ.get("TOKENMM_REDIS_PASSWORD")


def _resolve_strategy_ids(args: argparse.Namespace) -> list[str]:
    strategy_ids = _parse_strategy_args(args.strategy or [])
    if strategy_ids:
        return strategy_ids
    profile = args.profile or "tokenmm"
    config_path = Path(args.config) if args.config else _default_config_path(profile)
    return _read_profile_strategy_ids(profile=profile, config_path=config_path)


def _resolve_window(args: argparse.Namespace) -> tuple[int, int]:
    end_ms = int(args.end_ms) if args.end_ms is not None else int(time.time() * 1000)
    if args.start_ms is not None:
        start_ms = int(args.start_ms)
    else:
        hours = float(args.window_hours)
        start_ms = end_ms - int(hours * 60 * 60 * 1000)
    if start_ms >= end_ms:
        raise ValueError("window start must be earlier than window end")
    return start_ms, end_ms


def _load_fv_rows_for_window(
    *,
    redis_client: redis.Redis,
    strategy_id: str,
    start_ms: int,
    end_ms: int,
    page_size: int,
) -> list[dict[str, Any]]:
    keys = FluxRedisKeys(strategy_id=strategy_id)
    max_id = f"{end_ms}-999999"
    collected: list[dict[str, Any]] = []

    while True:
        page = redis_client.xrevrange(
            keys.fv_stream(),
            max=max_id,
            min="-",
            count=page_size,
        )
        if not page:
            break

        page_rows = extract_stream_rows(page)
        stop_after_page = False
        for (entry_id, _fields), row in zip(page, page_rows):
            ts_ms = _safe_int(row.get("ts_ms"))
            if ts_ms is None:
                continue
            if ts_ms > end_ms:
                continue
            if ts_ms < start_ms:
                stop_after_page = True
                continue
            record = {"strategy_id": strategy_id, "entry_id": decode_text(entry_id)}
            record.update(row)
            collected.append(record)

        if stop_after_page or len(page) < page_size:
            break

        oldest_entry_id = decode_text(page[-1][0]).strip()
        if not oldest_entry_id:
            break
        max_id = f"({oldest_entry_id}"

    collected.sort(key=lambda row: (_safe_int(row.get("ts_ms")) or 0, str(row.get("strategy_id") or "")))
    return collected


def _safe_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        text = str(value).strip()
        return int(text) if text else None


def _write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    preferred = ["strategy_id", "entry_id", "ts_ms", "fv", "maker_mid", "reference_mid"]
    seen = set(preferred)
    extra = sorted({key for row in rows for key in row.keys() if key not in seen})
    fieldnames = preferred + extra
    with path.open("w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow({key: row.get(key) for key in fieldnames})


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Freeze retained TokenMM FV stream rows to CSV for notebook demos.",
    )
    parser.add_argument(
        "--strategy",
        action="append",
        default=[],
        help="Flux strategy id to export. Repeat or pass a comma-separated list.",
    )
    parser.add_argument(
        "--profile",
        default="tokenmm",
        help="Profile name used to resolve strategy ids from the deploy TOML when --strategy is omitted.",
    )
    parser.add_argument(
        "--config",
        help="Optional TOML config path used for --profile strategy resolution and Redis defaults.",
    )
    parser.add_argument(
        "--redis-url",
        help="Optional Redis URL override. If omitted, derive it from the deploy TOML.",
    )
    parser.add_argument(
        "--redis-password",
        help="Optional Redis password override. Useful when the deploy TOML omits credentials.",
    )
    parser.add_argument(
        "--redis-password-env",
        help="Environment variable containing the Redis password override.",
    )
    parser.add_argument(
        "--start-ms",
        type=int,
        help="Window start timestamp in epoch milliseconds.",
    )
    parser.add_argument(
        "--end-ms",
        type=int,
        help="Window end timestamp in epoch milliseconds. Defaults to now.",
    )
    parser.add_argument(
        "--window-hours",
        type=float,
        default=24.0,
        help="Fallback trailing window size in hours when --start-ms is omitted.",
    )
    parser.add_argument(
        "--page-size",
        type=int,
        default=1000,
        help="Redis stream page size for xrevrange pagination.",
    )
    parser.add_argument(
        "--output",
        default=str(_repo_root() / "research/tokenmm/data/tokenmm_fv_extract.csv"),
        help="CSV path to write. Existing files will be overwritten.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)

    strategy_ids = _resolve_strategy_ids(args)
    if not strategy_ids:
        parser.error("no strategy ids resolved; pass --strategy or configure the profile")

    start_ms, end_ms = _resolve_window(args)
    config_path = Path(args.config) if args.config else _default_config_path(args.profile or "tokenmm")
    password = _resolve_password(args)
    redis_url = args.redis_url or _build_redis_url(config_path=config_path, password=password)

    redis_client = redis.Redis.from_url(redis_url, decode_responses=False)

    rows: list[dict[str, Any]] = []
    for strategy_id in strategy_ids:
        rows.extend(
            _load_fv_rows_for_window(
                redis_client=redis_client,
                strategy_id=strategy_id,
                start_ms=start_ms,
                end_ms=end_ms,
                page_size=max(1, int(args.page_size)),
            ),
        )

    output_path = Path(args.output)
    _write_csv(output_path, rows)

    print(
        f"wrote_rows={len(rows)} strategies={len(strategy_ids)} "
        f"window_start_ms={start_ms} window_end_ms={end_ms} output={output_path}",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
