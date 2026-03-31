from __future__ import annotations

import os
from typing import Literal

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import PositiveFloat
from nautilus_trader.common.config import PositiveInt


SslMode = Literal["disable", "allow", "prefer", "require", "verify-ca", "verify-full"]
DurableSink = Literal["postgres", "s3_athena"]


class TelemetryPostgresConfig(NautilusConfig, kw_only=True, frozen=True):
    host: str
    port: PositiveInt = 5432
    database: str
    schema: str = "telemetry"
    username: str
    password: str
    sslmode: SslMode = "require"
    connect_timeout_secs: PositiveFloat = 5.0
    application_name: str = "nautilus-telemetry-shipper"

    def __post_init__(self) -> None:
        if not self.host.strip():
            raise ValueError("`host` must be non-empty")
        if not self.database.strip():
            raise ValueError("`database` must be non-empty")
        if not self.schema.strip():
            raise ValueError("`schema` must be non-empty")
        if not self.username.strip():
            raise ValueError("`username` must be non-empty")

    @classmethod
    def from_env(
        cls,
        *,
        env: dict[str, str] | None = None,
        prefix: str = "NAUTILUS_TELEMETRY_PG_",
    ) -> TelemetryPostgresConfig:
        values = os.environ if env is None else env
        return cls(
            host=_required_text(values, f"{prefix}HOST"),
            port=int(values.get(f"{prefix}PORT", "5432")),
            database=_required_text(values, f"{prefix}DATABASE"),
            schema=values.get(f"{prefix}SCHEMA", "telemetry"),
            username=_required_text(values, f"{prefix}USERNAME"),
            password=_required_text(values, f"{prefix}PASSWORD"),
            sslmode=values.get(f"{prefix}SSLMODE", "require"),
            connect_timeout_secs=float(values.get(f"{prefix}CONNECT_TIMEOUT_SECS", "5.0")),
            application_name=values.get(
                f"{prefix}APPLICATION_NAME",
                "nautilus-telemetry-shipper",
            ),
        )


class TelemetryShipperConfig(NautilusConfig, kw_only=True, frozen=True):
    enabled: bool = False
    enable_local_persistence: bool = False
    source_profile: str
    durable_sink: DurableSink = "postgres"
    archive_s3_bucket: str | None = None
    archive_s3_prefix: str = "nautilus/telemetry"
    athena_database: str = "nautilus_telemetry"
    athena_workgroup: str = "primary"
    raw_quote_cycles_enabled: bool = True
    raw_quote_cycle_local_hours: PositiveInt = 48
    raw_quote_cycle_s3_days: PositiveInt = 7
    core_history_s3_days: PositiveInt = 365
    structured_local_cap_gb: PositiveInt = 8
    quote_cycle_local_cap_gb: PositiveInt = 12
    balance_snapshots_db_path: str | None = None
    fills_db_path: str | None = None
    orders_db_path: str | None = None
    quote_cycles_db_path: str | None = None
    portfolio_inventory_db_path: str | None = None
    state_db_path: str
    poll_interval_ms: PositiveInt = 1_000
    max_batch_size: PositiveInt = 1_000
    prune_retention_hours: PositiveInt = 168
    postgres: TelemetryPostgresConfig | None = None

    def __post_init__(self) -> None:
        if not self.source_profile.strip():
            raise ValueError("`source_profile` must be non-empty")
        if not self.state_db_path.strip():
            raise ValueError("`state_db_path` must be non-empty")
        if (
            self.balance_snapshots_db_path is None
            and self.fills_db_path is None
            and self.orders_db_path is None
            and self.quote_cycles_db_path is None
            and self.portfolio_inventory_db_path is None
        ):
            raise ValueError("At least one source DB path must be configured")
        if self.durable_sink == "postgres" and self.postgres is None:
            raise ValueError("`postgres` config is required when `durable_sink` is `postgres`")
        if self.durable_sink == "s3_athena" and not (self.archive_s3_bucket or "").strip():
            raise ValueError(
                "`archive_s3_bucket` is required when `durable_sink` is `s3_athena`",
            )
        if not self.raw_quote_cycles_enabled and self.quote_cycles_db_path is not None:
            raise ValueError(
                "`quote_cycles_db_path` must be omitted when `raw_quote_cycles_enabled` is false",
            )


def build_telemetry_shipper_config(
    payload: dict[str, object],
    *,
    env: dict[str, str] | None = None,
) -> TelemetryShipperConfig:
    values = os.environ if env is None else env
    durable_sink = str(payload.get("durable_sink", "postgres")).strip() or "postgres"
    return TelemetryShipperConfig(
        enabled=bool(payload.get("enabled", False)),
        enable_local_persistence=bool(payload.get("enable_local_persistence", False)),
        source_profile=str(payload.get("source_profile", "")).strip(),
        durable_sink=durable_sink,
        archive_s3_bucket=_optional_text(
            payload.get("archive_s3_bucket") or values.get("TOKENMM_TELEMETRY_ARCHIVE_S3_BUCKET"),
        ),
        archive_s3_prefix=str(
            payload.get(
                "archive_s3_prefix",
                values.get("TOKENMM_TELEMETRY_ARCHIVE_S3_PREFIX", "nautilus/telemetry"),
            ),
        ).strip(),
        athena_database=str(
            payload.get(
                "athena_database",
                values.get("TOKENMM_TELEMETRY_ATHENA_DATABASE", "nautilus_telemetry"),
            ),
        ).strip(),
        athena_workgroup=str(
            payload.get(
                "athena_workgroup",
                values.get("TOKENMM_TELEMETRY_ATHENA_WORKGROUP", "primary"),
            ),
        ).strip(),
        raw_quote_cycles_enabled=bool(payload.get("raw_quote_cycles_enabled", True)),
        raw_quote_cycle_local_hours=int(payload.get("raw_quote_cycle_local_hours", 48)),
        raw_quote_cycle_s3_days=int(payload.get("raw_quote_cycle_s3_days", 7)),
        core_history_s3_days=int(payload.get("core_history_s3_days", 365)),
        structured_local_cap_gb=int(payload.get("structured_local_cap_gb", 8)),
        quote_cycle_local_cap_gb=int(payload.get("quote_cycle_local_cap_gb", 12)),
        balance_snapshots_db_path=_optional_text(payload.get("balance_snapshots_db_path")),
        fills_db_path=_optional_text(payload.get("fills_db_path")),
        orders_db_path=_optional_text(payload.get("orders_db_path")),
        quote_cycles_db_path=_optional_text(payload.get("quote_cycles_db_path")),
        portfolio_inventory_db_path=_optional_text(payload.get("portfolio_inventory_db_path")),
        state_db_path=str(payload.get("state_db_path", "")).strip(),
        poll_interval_ms=int(payload.get("poll_interval_ms", 1_000)),
        max_batch_size=int(payload.get("max_batch_size", 1_000)),
        prune_retention_hours=int(payload.get("prune_retention_hours", 168)),
        postgres=(
            TelemetryPostgresConfig.from_env(env=values)
            if durable_sink == "postgres"
            else None
        ),
    )


def _required_text(values: dict[str, str], key: str) -> str:
    value = values.get(key, "").strip()
    if not value:
        raise ValueError(f"Missing required environment variable `{key}`")
    return value


def _optional_text(value: object) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None
