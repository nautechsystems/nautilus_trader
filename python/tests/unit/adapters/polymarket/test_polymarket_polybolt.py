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
"""
Tests for the Polymarket PolyBolt reference-price surface.
"""

import base64
from decimal import Decimal

from nautilus_trader.adapters.polymarket import PolymarketCryptoTwap
from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig


def test_data_client_config_accepts_polybolt_credentials_without_exposing_them() -> None:
    """
    Test data client config accepts PolyBolt credentials and redacts them.
    """
    api_secret = base64.b64encode(b"fixture-secret").decode()
    config = PolymarketDataClientConfig(
        api_key="fixture-key",
        api_secret=api_secret,
        passphrase="fixture-passphrase",
        base_url_reference_prices="ws://localhost/polybolt",
    )

    assert config.base_url_reference_prices == "ws://localhost/polybolt"
    assert "fixture-passphrase" not in repr(config)
    assert api_secret not in repr(config)


def test_crypto_twap_value_round_trips_as_exact_decimal() -> None:
    """
    Test crypto TWAP value is exposed as an exact decimal string.
    """
    twap = PolymarketCryptoTwap(
        "btcusd",
        "chainlink",
        60,
        "84291.577380840025358336",
        1791355667000,
        1791355667111,
        1791355667000000000,
        1791355667148000000,
    )

    assert Decimal(twap.value) == Decimal("84291.577380840025358336")
    assert twap.symbol == "btcusd"
    assert twap.source == "chainlink"
    assert twap.window_seconds == 60
    assert twap.observation_timestamp_ms == 1791355667000
    assert twap.message_timestamp_ms == 1791355667111
    assert twap.ts_event == 1791355667000000000
    assert twap.ts_init == 1791355667148000000
