# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
"""Unit tests for Coinbase adapter configuration."""

import pytest

from nautilus_trader.adapters.coinbase.config import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase.config import CoinbaseExecClientConfig
from nautilus_trader.adapters.coinbase.constants import COINBASE_VENUE


class TestCoinbaseDataClientConfig:
    """Test CoinbaseDataClientConfig."""

    def test_default_config(self):
        """Test default configuration values."""
        config = CoinbaseDataClientConfig()
        
        assert config.venue == COINBASE_VENUE
        assert config.api_key is None
        assert config.api_secret is None
        assert config.base_url_http is None
        assert config.base_url_ws is None
        assert config.http_timeout_secs == 60
        assert config.update_instruments_interval_mins is None

    def test_custom_config(self):
        """Test custom configuration values."""
        config = CoinbaseDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            base_url_http="https://test.coinbase.com",
            base_url_ws="wss://test.coinbase.com",
            http_timeout_secs=30,
            update_instruments_interval_mins=60,
        )
        
        assert config.api_key == "test_key"
        assert config.api_secret == "test_secret"
        assert config.base_url_http == "https://test.coinbase.com"
        assert config.base_url_ws == "wss://test.coinbase.com"
        assert config.http_timeout_secs == 30
        assert config.update_instruments_interval_mins == 60


class TestCoinbaseExecClientConfig:
    """Test CoinbaseExecClientConfig."""

    def test_default_config(self):
        """Test default configuration values."""
        config = CoinbaseExecClientConfig()
        
        assert config.venue == COINBASE_VENUE
        assert config.api_key is None
        assert config.api_secret is None
        assert config.base_url_http is None
        assert config.base_url_ws is None
        assert config.http_timeout_secs == 60
        assert config.update_instruments_interval_mins is None

    def test_custom_config(self):
        """Test custom configuration values."""
        config = CoinbaseExecClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            base_url_http="https://test.coinbase.com",
            base_url_ws="wss://test.coinbase.com",
            http_timeout_secs=30,
            update_instruments_interval_mins=60,
        )
        
        assert config.api_key == "test_key"
        assert config.api_secret == "test_secret"
        assert config.base_url_http == "https://test.coinbase.com"
        assert config.base_url_ws == "wss://test.coinbase.com"
        assert config.http_timeout_secs == 30
        assert config.update_instruments_interval_mins == 60

