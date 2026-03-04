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

import pytest

from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.keys import FluxRedisKeys


class TestFluxRedisKeys:
    def test_builds_strategy_scoped_keys(self):
        # Arrange
        keys = FluxRedisKeys(strategy_id="maker_v3_01")

        # Act, Assert
        assert keys.state() == "flux:v1:state:maker_v3_01"
        assert keys.events() == "flux:v1:events:maker_v3_01"
        assert keys.trades_stream() == "flux:v1:trades:stream:maker_v3_01"
        assert keys.alerts() == "flux:v1:alerts:maker_v3_01"
        assert keys.params_hash_key() == "flux:v1:params:maker_v3_01"

    def test_builds_namespace_scoped_keys_from_identity(self) -> None:
        identity = FluxIdentityConfig(
            namespace="tokenmm",
            schema_version="v1",
            strategy_id="maker_v3_01",
            strategy_instance_id="maker_v3_01_a",
            trader_id="TRADER-001",
            external_strategy_id="external_01",
        )

        keys = FluxRedisKeys.from_identity(identity)

        assert keys.prefix == "tokenmm:v1"
        assert keys.state() == "tokenmm:v1:state:maker_v3_01"
        assert keys.inbound_stream(environment="paper", topic="market_bbo") == (
            "tokenmm:v1:in:stream:paper:maker_v3_01:market_bbo"
        )

    def test_builds_inbound_stream_key(self):
        # Arrange
        keys = FluxRedisKeys(strategy_id="maker_v3_01")

        # Act
        result = keys.inbound_stream(environment="paper", topic="market_bbo")

        # Assert
        assert result == "flux:v1:in:stream:paper:maker_v3_01:market_bbo"

    def test_builds_params_keys_and_channels_with_explicit_semantics(self):
        # Arrange
        keys = FluxRedisKeys(strategy_id="maker_v3_01")

        # Act, Assert
        assert keys.params_hash_key() == "flux:v1:params:maker_v3_01"
        assert keys.params_pubsub_channel() == "flux:v1:params:maker_v3_01"
        # Explicitly assert current protocol semantics: key and channel share the same address.
        assert keys.params_hash_key() == keys.params_pubsub_channel()

        # Backward-compatible aliases remain equivalent.
        assert keys.params_hash() == keys.params_hash_key()
        assert keys.params_channel() == keys.params_pubsub_channel()

        assert FluxRedisKeys.global_params_channel() == "flux:v1:params:global"

    @pytest.mark.parametrize(
        "strategy_id",
        [
            "",
            "contains space",
            "contains:colon",
        ],
    )
    def test_rejects_unsafe_strategy_identifier(self, strategy_id: str) -> None:
        with pytest.raises(ValueError, match="strategy_id"):
            FluxRedisKeys(strategy_id=strategy_id)

    @pytest.mark.parametrize(
        "schema_version",
        [
            "v2",
            "",
            "v1:bad",
        ],
    )
    def test_rejects_unsupported_or_unsafe_schema_version(self, schema_version: str) -> None:
        with pytest.raises(ValueError, match="schema_version"):
            FluxRedisKeys(
                strategy_id="maker_v3_01",
                namespace="flux",
                schema_version=schema_version,
            )

    @pytest.mark.parametrize(
        ("environment", "topic"),
        [
            ("", "market_bbo"),
            ("paper", ""),
            ("live", "topic:bad"),
            ("test net", "market_bbo"),
        ],
    )
    def test_rejects_unsafe_inbound_parts(self, environment: str, topic: str) -> None:
        keys = FluxRedisKeys(strategy_id="maker_v3_01")

        with pytest.raises(ValueError):
            keys.inbound_stream(environment=environment, topic=topic)
