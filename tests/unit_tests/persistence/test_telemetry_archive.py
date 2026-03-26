from __future__ import annotations

import pytest

from nautilus_trader.persistence.shipper.config import build_telemetry_shipper_config


def test_build_shipper_config_rejects_s3_athena_sink_without_bucket() -> None:
    with pytest.raises(ValueError, match="archive_s3_bucket"):
        build_telemetry_shipper_config(
            {
                "enabled": True,
                "source_profile": "tokenmm",
                "durable_sink": "s3_athena",
                "orders_db_path": "/tmp/orders.sqlite",
                "state_db_path": "/tmp/shipper_state.sqlite",
            },
            env={},
        )


def test_build_shipper_config_rejects_quote_cycle_path_when_raw_quote_cycles_disabled() -> None:
    with pytest.raises(ValueError, match="quote_cycles_db_path"):
        build_telemetry_shipper_config(
            {
                "enabled": True,
                "source_profile": "tokenmm",
                "durable_sink": "postgres",
                "raw_quote_cycles_enabled": False,
                "orders_db_path": "/tmp/orders.sqlite",
                "quote_cycles_db_path": "/tmp/quote_cycles.sqlite",
                "state_db_path": "/tmp/shipper_state.sqlite",
            },
            env={
                "NAUTILUS_TELEMETRY_PG_HOST": "localhost",
                "NAUTILUS_TELEMETRY_PG_DATABASE": "nautilus_telemetry",
                "NAUTILUS_TELEMETRY_PG_SCHEMA": "telemetry",
                "NAUTILUS_TELEMETRY_PG_USERNAME": "nautilus",
                "NAUTILUS_TELEMETRY_PG_PASSWORD": "pass",
            },
        )
