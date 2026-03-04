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

from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.config import FluxRedisConfig
from nautilus_trader.flux.common.config import FluxVenuesConfig


class TestFluxConfig:
    def test_creates_flux_config_with_required_sections(self) -> None:
        # Arrange
        identity = FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id="maker_v3_01",
            strategy_instance_id="maker_v3_01_a",
            trader_id="TRADER-001",
            external_strategy_id="bybit_binance_maker_v3",
        )
        redis = FluxRedisConfig(
            host="127.0.0.1",
            port=6379,
            db=0,
        )
        venues = FluxVenuesConfig(
            execution_venue="BYBIT",
            reference_venue="BINANCE",
            execution_symbol="PLUMEUSDT",
            reference_symbol="PLUMEUSDT",
        )

        # Act
        config = FluxConfig(
            mode="paper",
            confirm_live=False,
            identity=identity,
            redis=redis,
            venues=venues,
        )

        # Assert
        assert config.mode == "paper"
        assert config.confirm_live is False
        assert config.identity.strategy_id == "maker_v3_01"

    def test_live_mode_requires_explicit_confirmation(self) -> None:
        with pytest.raises(ValueError, match="confirm_live"):
            FluxConfig(
                mode="live",
                confirm_live=False,
                identity=self._identity(),
                redis=self._redis(),
                venues=self._venues(),
            )

    def test_live_mode_allows_explicit_confirmation(self) -> None:
        config = FluxConfig(
            mode="live",
            confirm_live=True,
            identity=self._identity(),
            redis=self._redis(),
            venues=self._venues(),
        )

        assert config.mode == "live"

    def test_rejects_invalid_mode(self) -> None:
        with pytest.raises(ValueError, match="mode"):
            FluxConfig(
                mode="production",
                confirm_live=False,
                identity=self._identity(),
                redis=self._redis(),
                venues=self._venues(),
            )

    @pytest.mark.parametrize(
        "field_name",
        [
            "strategy_id",
            "strategy_instance_id",
            "trader_id",
            "external_strategy_id",
        ],
    )
    def test_rejects_unsafe_identifier_parts(self, field_name: str) -> None:
        kwargs = {
            "namespace": "flux",
            "schema_version": "v1",
            "strategy_id": "maker_v3_01",
            "strategy_instance_id": "maker_v3_01_a",
            "trader_id": "TRADER-001",
            "external_strategy_id": "external_01",
        }
        kwargs[field_name] = "unsafe:value"

        with pytest.raises(ValueError, match=field_name):
            FluxIdentityConfig(**kwargs)

    def test_requires_explicit_confirm_live_field(self) -> None:
        with pytest.raises(TypeError):
            FluxConfig(
                mode="paper",
                identity=self._identity(),
                redis=self._redis(),
                venues=self._venues(),
            )

    @staticmethod
    def _identity() -> FluxIdentityConfig:
        return FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id="maker_v3_01",
            strategy_instance_id="maker_v3_01_a",
            trader_id="TRADER-001",
            external_strategy_id="external_01",
        )

    @staticmethod
    def _redis() -> FluxRedisConfig:
        return FluxRedisConfig(host="127.0.0.1", port=6379, db=0)

    @staticmethod
    def _venues() -> FluxVenuesConfig:
        return FluxVenuesConfig(
            execution_venue="BYBIT",
            reference_venue="BINANCE",
            execution_symbol="PLUMEUSDT",
            reference_symbol="PLUMEUSDT",
        )
