from __future__ import annotations

import argparse
import logging
import tomllib
from pathlib import Path

from nautilus_trader.persistence.shipper.config import build_telemetry_shipper_config
from nautilus_trader.persistence.shipper.postgres import TelemetryPostgresSink
from nautilus_trader.persistence.shipper.service import SQLiteToPostgresTelemetryShipper


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


def main() -> None:
    logging.basicConfig(level=logging.INFO)
    args = _parse_args()
    config = _load_shipper_config(args.config)
    if not config.enabled:
        raise RuntimeError("Telemetry shipper is disabled in config")

    sink = TelemetryPostgresSink(config.postgres)
    try:
        if args.bootstrap_postgres:
            sink.ensure_schema()
            return

        shipper = SQLiteToPostgresTelemetryShipper(config=config, sink=sink)
        try:
            sink.validate_tables(shipper.configured_table_names())
            if args.once:
                shipper.ship_once()
            else:
                shipper.run_forever()
        finally:
            shipper.close()
    finally:
        sink.close()


if __name__ == "__main__":
    main()
