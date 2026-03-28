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

"""Configuration classes for the Rithmic adapter."""

from __future__ import annotations

import os
from enum import Enum

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


def _normalize_profile_token(profile: str) -> str:
    normalized = "".join(char.upper() if char.isalnum() else "_" for char in profile.strip())
    normalized = "_".join(part for part in normalized.split("_") if part)
    if not normalized:
        raise ValueError("Rithmic env profile cannot be empty")
    return normalized


def _candidate_env_keys(
    key: str,
    profile: str | None = None,
) -> list[str]:
    candidates: list[str] = []

    if profile:
        candidates.append(f"RITHMIC_{_normalize_profile_token(profile)}_{key}")
    candidates.append(f"RITHMIC_{key}")
    return candidates


def _optional_env(
    key: str,
    profile: str | None = None,
) -> str | None:
    for candidate in _candidate_env_keys(key, profile):
        value = os.environ.get(candidate)
        if value:
            return value
    return None


def _required_env(
    key: str,
    profile: str | None = None,
) -> str:
    value = _optional_env(key, profile)
    if value:
        return value
    missing_key = _candidate_env_keys(key, profile)[0]
    raise ValueError(f"{missing_key} environment variable not set")


def _optional_int_env(
    key: str,
    profile: str | None = None,
) -> int | None:
    value = _optional_env(key, profile)
    if value is None:
        return None
    return int(value)


class RithmicEnvironment(Enum):
    """Rithmic connection environment."""

    DEMO = "demo"
    LIVE = "live"
    TEST = "test"

    @classmethod
    def from_str(cls, value: str) -> RithmicEnvironment:
        """Parse environment from string."""
        value_lower = value.lower()
        if value_lower in ("demo", "paper"):
            return cls.DEMO
        elif value_lower in ("live", "prod", "production"):
            return cls.LIVE
        elif value_lower == "test":
            return cls.TEST
        else:
            raise ValueError(f"Invalid environment: {value}")


def to_binding_environment(environment):
    """Convert a Python config enum into the PyO3 RithmicEnv type."""
    from nautilus_trader.core import nautilus_pyo3

    binding_env = getattr(nautilus_pyo3.rithmic, "RithmicEnv", None)
    if binding_env is None:
        raise RuntimeError("RithmicEnv binding is not available")

    if isinstance(environment, binding_env):
        return environment

    name = getattr(environment, "name", None)
    if name is None:
        raise TypeError(f"Cannot convert Rithmic environment {environment!r}")

    return getattr(binding_env, name)


class RithmicDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for Rithmic data clients.

    Parameters
    ----------
    environment : RithmicEnvironment
        The Rithmic environment (Demo, Live, Test).
    username : str
        Rithmic username.
    password : str
        Rithmic password.
    system_name : str
        System name for Rithmic connection.
    app_name : str, default built-in fallback
        Application name.
    app_version : str, default "1.0"
        Application version.
    fcm_id : str, optional
        FCM ID (Futures Commission Merchant).
    ib_id : str, optional
        IB ID (Introducing Broker).
    server : str, optional
        Named primary Rithmic server route (for example, ``Chicago``). Defaults
        to ``Chicago`` for demo/live and ``Test`` for test when omitted.
    alt_server : str, optional
        Named alternate Rithmic server endpoint.
    enable_history : bool, default True
        Whether the client should connect the Rithmic history plant. Disable
        this for live-only streaming sessions that do not request bars.
    """

    environment: RithmicEnvironment = RithmicEnvironment.DEMO
    username: str = ""
    password: str = ""
    system_name: str = ""
    app_name: str = "fufo:fund-forge"
    app_version: str = "1.0"
    fcm_id: str | None = None
    ib_id: str | None = None
    server: str | None = None
    alt_server: str | None = None
    enable_history: bool = True

    @classmethod
    def from_env(cls, profile: str | None = None) -> RithmicDataClientConfig:
        """
        Create configuration from environment variables.

        Environment Variables
        ---------------------
        RITHMIC_ENV : str
            Environment (demo, live, test). Default: demo
        RITHMIC_USERNAME : str
            Rithmic username (required).
        RITHMIC_PASSWORD : str
            Rithmic password (required).
        RITHMIC_SYSTEM_NAME : str
            System name (required).
        RITHMIC_APP_NAME : str
            Application name. Default: built-in fallback
        RITHMIC_APP_VERSION : str
            Application version. Default: 1.0
        RITHMIC_FCM_ID : str
            FCM ID (optional).
        RITHMIC_IB_ID : str
            IB ID (optional).
        RITHMIC_SERVER : str
            Named primary server. Defaults to Chicago for demo/live and Test
            for test when omitted.
        RITHMIC_ALT_SERVER : str
            Named alternate server (optional).
        RITHMIC_{PROFILE}_* : str
            Profile-scoped overrides for any of the variables above. When
            `profile` is provided, these names are checked before the flat
            `RITHMIC_*` variables.
        """
        env_str = _optional_env("ENV", profile) or "demo"
        environment = RithmicEnvironment.from_str(env_str)

        return cls(
            environment=environment,
            username=_required_env("USERNAME", profile),
            password=_required_env("PASSWORD", profile),
            system_name=_required_env("SYSTEM_NAME", profile),
            app_name=_optional_env("APP_NAME", profile) or "fufo:fund-forge",
            app_version=_optional_env("APP_VERSION", profile) or "1.0",
            fcm_id=_optional_env("FCM_ID", profile),
            ib_id=_optional_env("IB_ID", profile),
            server=_optional_env("SERVER", profile),
            alt_server=_optional_env("ALT_SERVER", profile),
        )


class RithmicExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for Rithmic execution clients.

    Parameters
    ----------
    environment : RithmicEnvironment
        The Rithmic environment (Demo, Live, Test).
    username : str
        Rithmic username.
    password : str
        Rithmic password.
    system_name : str
        System name for Rithmic connection.
    account_id : str
        Trading account ID.
    app_name : str, default built-in fallback
        Application name.
    app_version : str, default "1.0"
        Application version.
    fcm_id : str, optional
        FCM ID (Futures Commission Merchant).
    ib_id : str, optional
        IB ID (Introducing Broker).
    server : str, optional
        Named primary Rithmic server route (for example, ``Chicago``). Defaults
        to ``Chicago`` for demo/live and ``Test`` for test when omitted.
    alt_server : str, optional
        Named alternate Rithmic server endpoint.
    execution_replay_lookback_secs : int, default 86400
        Replay window used when reconnecting without a prior local execution
        timestamp to anchor recovery.
    native_bracket_state_path : str, optional
        Override for the local file used to persist native bracket child-ID
        mappings across process restarts.
    """

    environment: RithmicEnvironment = RithmicEnvironment.DEMO
    username: str = ""
    password: str = ""
    system_name: str = ""
    account_id: str = ""
    app_name: str = "fufo:fund-forge"
    app_version: str = "1.0"
    fcm_id: str | None = None
    ib_id: str | None = None
    server: str | None = None
    alt_server: str | None = None
    execution_replay_lookback_secs: int = 86_400
    native_bracket_state_path: str | None = None

    @classmethod
    def from_env(cls, profile: str | None = None) -> RithmicExecClientConfig:
        """
        Create configuration from environment variables.

        Environment Variables
        ---------------------
        RITHMIC_ENV : str
            Environment (demo, live, test). Default: demo
        RITHMIC_USERNAME : str
            Rithmic username (required).
        RITHMIC_PASSWORD : str
            Rithmic password (required).
        RITHMIC_SYSTEM_NAME : str
            System name (required).
        RITHMIC_ACCOUNT_ID : str
            Trading account ID (required).
        RITHMIC_APP_NAME : str
            Application name. Default: built-in fallback
        RITHMIC_APP_VERSION : str
            Application version. Default: 1.0
        RITHMIC_FCM_ID : str
            FCM ID (optional).
        RITHMIC_IB_ID : str
            IB ID (optional).
        RITHMIC_SERVER : str
            Named primary server. Defaults to Chicago for demo/live and Test
            for test when omitted.
        RITHMIC_ALT_SERVER : str
            Named alternate server (optional).
        RITHMIC_EXECUTION_REPLAY_LOOKBACK_SECS : str
            Replay window in seconds used when reconnecting without any prior
            local execution timestamp. Default: 86400
        RITHMIC_NATIVE_BRACKET_STATE_PATH : str
            Optional override for the file used to persist native bracket
            child-ID mappings across process restarts.
        RITHMIC_{PROFILE}_* : str
            Profile-scoped overrides for any of the variables above. When
            `profile` is provided, these names are checked before the flat
            `RITHMIC_*` variables.
        """
        env_str = _optional_env("ENV", profile) or "demo"
        environment = RithmicEnvironment.from_str(env_str)

        return cls(
            environment=environment,
            username=_required_env("USERNAME", profile),
            password=_required_env("PASSWORD", profile),
            system_name=_required_env("SYSTEM_NAME", profile),
            account_id=_required_env("ACCOUNT_ID", profile),
            app_name=_optional_env("APP_NAME", profile) or "fufo:fund-forge",
            app_version=_optional_env("APP_VERSION", profile) or "1.0",
            fcm_id=_optional_env("FCM_ID", profile),
            ib_id=_optional_env("IB_ID", profile),
            server=_optional_env("SERVER", profile),
            alt_server=_optional_env("ALT_SERVER", profile),
            execution_replay_lookback_secs=(
                _optional_int_env("EXECUTION_REPLAY_LOOKBACK_SECS", profile) or 86_400
            ),
            native_bracket_state_path=_optional_env("NATIVE_BRACKET_STATE_PATH", profile),
        )
