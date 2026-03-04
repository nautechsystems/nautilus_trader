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

import re
from typing import Final

from nautilus_trader.common.config import NautilusConfig


FLUX_SCHEMA_VERSION: Final[str] = "v1"

_IDENTIFIER_SAFE_PATTERN: Final[re.Pattern[str]] = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
_SYMBOL_SAFE_PATTERN: Final[re.Pattern[str]] = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]*$")
_ALLOWED_MODES: Final[frozenset[str]] = frozenset({"paper", "testnet", "live"})


def validate_identifier_part(value: str, field_name: str) -> str:
    """
    Validate that a value is safe for use as a Redis key identifier part.
    """
    if not isinstance(value, str) or not value:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    if _IDENTIFIER_SAFE_PATTERN.fullmatch(value) is None:
        raise ValueError(
            f"`{field_name}` was not identifier-safe: {value!r}. "
            "Allowed characters are letters, digits, '.', '_' and '-'.",
        )
    return value


def validate_symbol_part(value: str, field_name: str) -> str:
    """
    Validate that a symbol-like value is safe for use in Redis key segments.
    """
    if not isinstance(value, str) or not value:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    if _SYMBOL_SAFE_PATTERN.fullmatch(value) is None:
        raise ValueError(
            f"`{field_name}` was not identifier-safe: {value!r}. "
            "Allowed characters are letters, digits, '.', '_', '-' and '/'.",
        )
    return value


class FluxIdentityConfig(NautilusConfig, frozen=True):
    """
    Identity fields for a Flux deployment.
    """

    namespace: str
    schema_version: str
    strategy_id: str
    strategy_instance_id: str
    trader_id: str
    external_strategy_id: str

    def __post_init__(self) -> None:
        validate_identifier_part(self.namespace, "namespace")
        validate_identifier_part(self.schema_version, "schema_version")
        validate_identifier_part(self.strategy_id, "strategy_id")
        validate_identifier_part(self.strategy_instance_id, "strategy_instance_id")
        validate_identifier_part(self.trader_id, "trader_id")
        validate_identifier_part(self.external_strategy_id, "external_strategy_id")


class FluxRedisConfig(NautilusConfig, frozen=True):
    """
    Redis connection configuration for Flux components.
    """

    host: str
    port: int
    db: int
    username: str | None = None
    password: str | None = None
    connect_timeout_secs: float = 5.0
    read_timeout_secs: float = 5.0

    def __post_init__(self) -> None:
        if not isinstance(self.host, str) or not self.host.strip():
            raise ValueError("`host` must be a non-empty string")
        if self.port <= 0 or self.port > 65535:
            raise ValueError("`port` must be between 1 and 65535")
        if self.db < 0:
            raise ValueError("`db` must be >= 0")
        if self.connect_timeout_secs <= 0.0:
            raise ValueError("`connect_timeout_secs` must be > 0")
        if self.read_timeout_secs <= 0.0:
            raise ValueError("`read_timeout_secs` must be > 0")


class FluxVenuesConfig(NautilusConfig, frozen=True):
    """
    Venue and symbol routing configuration for Flux components.
    """

    execution_venue: str
    reference_venue: str
    execution_symbol: str
    reference_symbol: str

    def __post_init__(self) -> None:
        validate_identifier_part(self.execution_venue, "execution_venue")
        validate_identifier_part(self.reference_venue, "reference_venue")
        validate_symbol_part(self.execution_symbol, "execution_symbol")
        validate_symbol_part(self.reference_symbol, "reference_symbol")


class FluxConfig(NautilusConfig, frozen=True):
    """
    Top-level Flux configuration.
    """

    mode: str
    confirm_live: bool
    identity: FluxIdentityConfig
    redis: FluxRedisConfig
    venues: FluxVenuesConfig

    def __post_init__(self) -> None:
        if self.mode not in _ALLOWED_MODES:
            raise ValueError(
                f"`mode` was invalid: {self.mode!r}. "
                f"Expected one of {sorted(_ALLOWED_MODES)}.",
            )
        if self.mode == "live" and not self.confirm_live:
            raise ValueError("`confirm_live` must be True when `mode='live'`")
