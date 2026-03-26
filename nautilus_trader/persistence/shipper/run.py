from __future__ import annotations

import argparse
import logging
import tomllib
from pathlib import Path

from nautilus_trader.persistence.shipper.config import build_telemetry_shipper_config
from nautilus_trader.persistence.shipper.postgres import TelemetryPostgresSink
from nautilus_trader.persistence.shipper.service import SQLiteToPostgresTelemetryShipper

STARTUP_FAILURE_EXIT_CODE = 78


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Ship local SQLite telemetry into Postgres.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--bootstrap-postgres", action="store_true")
    return parser.parse_args()


def _load_shipper_config(path: Path):
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    payload = data.get("telemetry_shipper")
    if not isinstance(payload, dict):
        raise ValueError(f"`[telemetry_shipper]` table missing from {path}")
    return build_telemetry_shipper_config(payload)


def _bootstrap_runtime(
    args: argparse.Namespace,
) -> tuple[TelemetryPostgresSink, SQLiteToPostgresTelemetryShipper | None]:
    config = _load_shipper_config(args.config)
    if not config.enabled:
        raise RuntimeError("Telemetry shipper is disabled in config")
    if config.durable_sink != "postgres":
        raise RuntimeError(
            f"Telemetry shipper runtime does not yet support durable sink `{config.durable_sink}`",
        )
    if config.postgres is None:
        raise RuntimeError("Telemetry shipper runtime requires postgres configuration")

    sink = TelemetryPostgresSink(config.postgres)
    try:
        if args.bootstrap_postgres:
            sink.ensure_schema()
            return sink, None

        shipper = SQLiteToPostgresTelemetryShipper(config=config, sink=sink)
        sink.validate_tables(shipper.configured_table_names())
        return sink, shipper
    except Exception:
        sink.close()
        raise


def main() -> int:
    logging.basicConfig(level=logging.INFO)
    args = _parse_args()
    try:
        sink, shipper = _bootstrap_runtime(args)
    except KeyboardInterrupt:  # pragma: no cover
        raise
    except Exception:
        logging.getLogger("nautilus.telemetry.shipper").exception(
            "Telemetry shipper startup failed",
        )
        raise SystemExit(STARTUP_FAILURE_EXIT_CODE)
    if shipper is None:
        sink.close()
        return 0
    try:
        if args.once:
            shipper.ship_once()
        else:
            shipper.run_forever()
    finally:
        shipper.close()
        sink.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
