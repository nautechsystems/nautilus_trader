"""Tests for Rithmic configuration."""

import os
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.rithmic.config import (
    RithmicDataClientConfig,
    RithmicEnvironment,
    RithmicExecClientConfig,
)


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
        assert config.app_name == "NautilusTrader"
        assert config.app_version == "1.0"
        assert config.fcm_id is None
        assert config.ib_id is None

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
        )
        assert config.app_name == "MyApp"
        assert config.app_version == "2.0"
        assert config.fcm_id == "FCM001"
        assert config.ib_id == "IB001"

    def test_from_env(self):
        env_vars = {
            "RITHMIC_ENV": "demo",
            "RITHMIC_USERNAME": "env_user",
            "RITHMIC_PASSWORD": "env_pass",
            "RITHMIC_SYSTEM_NAME": "env_system",
        }
        with patch.dict(os.environ, env_vars, clear=False):
            config = RithmicDataClientConfig.from_env()
            assert config.environment == RithmicEnvironment.DEMO
            assert config.username == "env_user"
            assert config.password == "env_pass"
            assert config.system_name == "env_system"

    def test_from_env_profile(self):
        env_vars = {
            "RITHMIC_APEX_ENV": "live",
            "RITHMIC_APEX_USERNAME": "profile_user",
            "RITHMIC_APEX_PASSWORD": "profile_pass",
            "RITHMIC_APEX_SYSTEM_NAME": "Apex",
            "RITHMIC_APEX_APP_NAME": "ProfileApp",
        }
        with patch.dict(os.environ, env_vars, clear=True):
            config = RithmicDataClientConfig.from_env("Apex")
            assert config.environment == RithmicEnvironment.LIVE
            assert config.username == "profile_user"
            assert config.password == "profile_pass"
            assert config.system_name == "Apex"
            assert config.app_name == "ProfileApp"

    def test_from_env_missing_username(self):
        env_vars = {
            "RITHMIC_PASSWORD": "pass",
            "RITHMIC_SYSTEM_NAME": "system",
        }
        with patch.dict(os.environ, env_vars, clear=True):
            with pytest.raises(ValueError, match="RITHMIC_USERNAME"):
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
        assert config.execution_replay_lookback_secs == 86_400
        assert config.native_bracket_state_path is None

    def test_from_env(self):
        env_vars = {
            "RITHMIC_ENV": "demo",
            "RITHMIC_USERNAME": "env_user",
            "RITHMIC_PASSWORD": "env_pass",
            "RITHMIC_SYSTEM_NAME": "env_system",
            "RITHMIC_ACCOUNT_ID": "ENV_ACCOUNT",
            "RITHMIC_EXECUTION_REPLAY_LOOKBACK_SECS": "7200",
            "RITHMIC_NATIVE_BRACKET_STATE_PATH": "/tmp/rithmic-native-brackets.json",
        }
        with patch.dict(os.environ, env_vars, clear=False):
            config = RithmicExecClientConfig.from_env()
            assert config.account_id == "ENV_ACCOUNT"
            assert config.execution_replay_lookback_secs == 7200
            assert config.native_bracket_state_path == "/tmp/rithmic-native-brackets.json"

    def test_from_env_profile(self):
        env_vars = {
            "RITHMIC_APEX_USERNAME": "profile_user",
            "RITHMIC_APEX_PASSWORD": "profile_pass",
            "RITHMIC_APEX_SYSTEM_NAME": "Apex",
            "RITHMIC_APEX_ACCOUNT_ID": "PROFILE_ACCOUNT",
        }
        with patch.dict(os.environ, env_vars, clear=True):
            config = RithmicExecClientConfig.from_env("Apex")
            assert config.username == "profile_user"
            assert config.account_id == "PROFILE_ACCOUNT"

    def test_from_env_missing_account_id(self):
        env_vars = {
            "RITHMIC_USERNAME": "user",
            "RITHMIC_PASSWORD": "pass",
            "RITHMIC_SYSTEM_NAME": "system",
        }
        with patch.dict(os.environ, env_vars, clear=True):
            with pytest.raises(ValueError, match="RITHMIC_ACCOUNT_ID"):
                RithmicExecClientConfig.from_env()
