from __future__ import annotations

import os
from typing import Literal

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import PositiveFloat
from nautilus_trader.common.config import PositiveInt


SslMode = Literal["disable", "allow", "prefer", "require", "verify-ca", "verify-full"]


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
    balance_snapshots_db_path: str | None = None
    fills_db_path: str | None = None
    orders_db_path: str | None = None
    quote_cycles_db_path: str | None = None
    portfolio_inventory_db_path: str | None = None
    state_db_path: str
    poll_interval_ms: PositiveInt = 1_000
    max_batch_size: PositiveInt = 1_000
    prune_retention_hours: PositiveInt = 168
    postgres: TelemetryPostgresConfig

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


def build_telemetry_shipper_config(
    payload: dict[str, object],
    *,
    env: dict[str, str] | None = None,
) -> TelemetryShipperConfig:
    return TelemetryShipperConfig(
        enabled=bool(payload.get("enabled", False)),
        enable_local_persistence=bool(payload.get("enable_local_persistence", False)),
        source_profile=str(payload.get("source_profile", "")).strip(),
        balance_snapshots_db_path=_optional_text(payload.get("balance_snapshots_db_path")),
        fills_db_path=_optional_text(payload.get("fills_db_path")),
        orders_db_path=_optional_text(payload.get("orders_db_path")),
        quote_cycles_db_path=_optional_text(payload.get("quote_cycles_db_path")),
        portfolio_inventory_db_path=_optional_text(payload.get("portfolio_inventory_db_path")),
        state_db_path=str(payload.get("state_db_path", "")).strip(),
        poll_interval_ms=int(payload.get("poll_interval_ms", 1_000)),
        max_batch_size=int(payload.get("max_batch_size", 1_000)),
        prune_retention_hours=int(payload.get("prune_retention_hours", 168)),
        postgres=TelemetryPostgresConfig.from_env(env=env),
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
