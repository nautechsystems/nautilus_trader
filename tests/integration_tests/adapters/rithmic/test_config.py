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

"""Tests for Rithmic configuration."""

import os
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import RithmicEnvironment
from nautilus_trader.adapters.rithmic.config import RithmicExecClientConfig


class TestRithmicEnvironment:
    """Tests for RithmicEnvironment."""

    def test_from_str_demo(self):
        assert RithmicEnvironment.from_str("demo") == RithmicEnvironment.DEMO
        assert RithmicEnvironment.from_str("DEMO") == RithmicEnvironment.DEMO
        assert RithmicEnvironment.from_str("paper") == RithmicEnvironment.DEMO

    def test_from_str_live(self):
        assert RithmicEnvironment.from_str("live") == RithmicEnvironment.LIVE
        assert RithmicEnvironment.from_str("LIVE") == RithmicEnvironment.LIVE
        assert RithmicEnvironment.from_str("prod") == RithmicEnvironment.LIVE
        assert RithmicEnvironment.from_str("production") == RithmicEnvironment.LIVE

    def test_from_str_test(self):
        assert RithmicEnvironment.from_str("test") == RithmicEnvironment.TEST
        assert RithmicEnvironment.from_str("TEST") == RithmicEnvironment.TEST

    def test_from_str_invalid(self):
        with pytest.raises(ValueError):
            RithmicEnvironment.from_str("invalid")


class TestRithmicDataClientConfig:
    """Tests for RithmicDataClientConfig."""

    def test_create_config(self):
        config = RithmicDataClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="test_user",
            password="test_pass",
            system_name="test_system",
        )
        assert config.environment == RithmicEnvironment.DEMO
        assert config.username == "test_user"
        assert config.password == "test_pass"
        assert config.system_name == "test_system"
        assert config.app_name == "fufo:fund-forge"
        assert config.app_version == "1.0"
        assert config.fcm_id is None
        assert config.ib_id is None
        assert config.server is None
        assert config.alt_server is None
        assert config.enable_history is True

    def test_create_config_with_optional_fields(self):
        config = RithmicDataClientConfig(
            environment=RithmicEnvironment.LIVE,
            username="user",
            password="pass",
            system_name="system",
            app_name="MyApp",
            app_version="2.0",
            fcm_id="FCM001",
            ib_id="IB001",
            server="Chicago",
            alt_server="Sydney",
            enable_history=False,
        )
        assert config.app_name == "MyApp"
        assert config.app_version == "2.0"
        assert config.fcm_id == "FCM001"
        assert config.ib_id == "IB001"
        assert config.server == "Chicago"
        assert config.alt_server == "Sydney"
        assert config.enable_history is False

    def test_from_env(self):
        env_vars = {
            "RITHMIC_ENV": "demo",
            "RITHMIC_USERNAME": "env_user",
            "RITHMIC_PASSWORD": "env_pass",
            "RITHMIC_SYSTEM_NAME": "env_system",
            "RITHMIC_SERVER": "Chicago",
            "RITHMIC_ALT_SERVER": "Sydney",
        }
        with patch.dict(os.environ, env_vars, clear=False):
            config = RithmicDataClientConfig.from_env()
            assert config.environment == RithmicEnvironment.DEMO
            assert config.username == "env_user"
            assert config.password == "env_pass"
            assert config.system_name == "env_system"
            assert config.server == "Chicago"
            assert config.alt_server == "Sydney"
            assert config.enable_history is True

    def test_from_env_profile(self):
        env_vars = {
            "RITHMIC_APEX_ENV": "live",
            "RITHMIC_APEX_USERNAME": "profile_user",
            "RITHMIC_APEX_PASSWORD": "profile_pass",
            "RITHMIC_APEX_SYSTEM_NAME": "Apex",
            "RITHMIC_APEX_APP_NAME": "ProfileApp",
            "RITHMIC_APEX_SERVER": "Frankfurt",
        }
        with patch.dict(os.environ, env_vars, clear=True):
            config = RithmicDataClientConfig.from_env("Apex")
            assert config.environment == RithmicEnvironment.LIVE
            assert config.username == "profile_user"
            assert config.password == "profile_pass"
            assert config.system_name == "Apex"
            assert config.app_name == "ProfileApp"
            assert config.server == "Frankfurt"

    def test_from_env_missing_username(self):
        env_vars = {
            "RITHMIC_PASSWORD": "pass",
            "RITHMIC_SYSTEM_NAME": "system",
        }
        with (
            patch.dict(os.environ, env_vars, clear=True),
            pytest.raises(
                ValueError,
                match="RITHMIC_USERNAME",
            ),
        ):
            RithmicDataClientConfig.from_env()


class TestRithmicExecClientConfig:
    """Tests for RithmicExecClientConfig."""

    def test_create_config(self):
        config = RithmicExecClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="test_user",
            password="test_pass",
            system_name="test_system",
            account_id="ACCOUNT123",
        )
        assert config.environment == RithmicEnvironment.DEMO
        assert config.account_id == "ACCOUNT123"
        assert config.server is None
        assert config.alt_server is None
        assert config.execution_replay_lookback_secs == 86_400
        assert config.native_bracket_state_path is None

    def test_from_env(self, tmp_path):
        state_path = tmp_path / "rithmic-native-brackets.json"
        env_vars = {
            "RITHMIC_ENV": "demo",
            "RITHMIC_USERNAME": "env_user",
            "RITHMIC_PASSWORD": "env_pass",
            "RITHMIC_SYSTEM_NAME": "env_system",
            "RITHMIC_ACCOUNT_ID": "ENV_ACCOUNT",
            "RITHMIC_SERVER": "Chicago",
            "RITHMIC_ALT_SERVER": "Sydney",
            "RITHMIC_EXECUTION_REPLAY_LOOKBACK_SECS": "7200",
            "RITHMIC_NATIVE_BRACKET_STATE_PATH": str(state_path),
        }
        with patch.dict(os.environ, env_vars, clear=False):
            config = RithmicExecClientConfig.from_env()
            assert config.account_id == "ENV_ACCOUNT"
            assert config.server == "Chicago"
            assert config.alt_server == "Sydney"
            assert config.execution_replay_lookback_secs == 7200
            assert config.native_bracket_state_path == str(state_path)

    def test_from_env_profile(self):
        env_vars = {
            "RITHMIC_APEX_USERNAME": "profile_user",
            "RITHMIC_APEX_PASSWORD": "profile_pass",
            "RITHMIC_APEX_SYSTEM_NAME": "Apex",
            "RITHMIC_APEX_ACCOUNT_ID": "PROFILE_ACCOUNT",
            "RITHMIC_APEX_SERVER": "Frankfurt",
        }
        with patch.dict(os.environ, env_vars, clear=True):
            config = RithmicExecClientConfig.from_env("Apex")
            assert config.username == "profile_user"
            assert config.account_id == "PROFILE_ACCOUNT"
            assert config.server == "Frankfurt"

    def test_from_env_missing_account_id(self):
        env_vars = {
            "RITHMIC_USERNAME": "user",
            "RITHMIC_PASSWORD": "pass",
            "RITHMIC_SYSTEM_NAME": "system",
        }
        with (
            patch.dict(os.environ, env_vars, clear=True),
            pytest.raises(
                ValueError,
                match="RITHMIC_ACCOUNT_ID",
            ),
        ):
            RithmicExecClientConfig.from_env()
