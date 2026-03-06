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
            strategy_instance_id="maker_v3_01",
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

    def test_rejects_unsupported_schema_version(self) -> None:
        with pytest.raises(ValueError, match="schema_version"):
            FluxIdentityConfig(
                namespace="flux",
                schema_version="v2",
                strategy_id="maker_v3_01",
                strategy_instance_id="maker_v3_01",
                trader_id="TRADER-001",
                external_strategy_id="external_01",
            )

    def test_rejects_strategy_instance_id_different_from_strategy_id(self) -> None:
        with pytest.raises(ValueError, match="strategy_instance_id"):
            FluxIdentityConfig(
                namespace="flux",
                schema_version="v1",
                strategy_id="maker_v3_01",
                strategy_instance_id="maker_v3_01_a",
                trader_id="TRADER-001",
                external_strategy_id="external_01",
            )

    @pytest.mark.parametrize(
        "field_name",
        [
            "namespace",
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
            "strategy_instance_id": "maker_v3_01",
            "trader_id": "TRADER-001",
            "external_strategy_id": "external_01",
        }
        kwargs[field_name] = "unsafe:value"

        with pytest.raises(ValueError, match=field_name):
            FluxIdentityConfig(**kwargs)

    @pytest.mark.parametrize(
        ("field_name", "field_value"),
        [
            ("execution_venue", ""),
            ("execution_venue", "BYBIT:PERP"),
            ("reference_venue", "BINANCE TEST"),
            ("execution_symbol", "PLUME USDT"),
            ("reference_symbol", "PLUME:USDT"),
        ],
    )
    def test_rejects_invalid_venue_and_symbol_fields(
        self,
        field_name: str,
        field_value: str,
    ) -> None:
        kwargs = {
            "execution_venue": "BYBIT",
            "reference_venue": "BINANCE",
            "execution_symbol": "PLUMEUSDT",
            "reference_symbol": "PLUMEUSDT",
        }
        kwargs[field_name] = field_value

        with pytest.raises(ValueError, match=field_name):
            FluxVenuesConfig(**kwargs)

    def test_requires_explicit_confirm_live_field(self) -> None:
        with pytest.raises(TypeError):
            FluxConfig(
                mode="paper",
                identity=self._identity(),
                redis=self._redis(),
                venues=self._venues(),
            )

    def test_rejects_invalid_redis_host(self) -> None:
        with pytest.raises(ValueError, match="host"):
            FluxRedisConfig(host="   ", port=6379, db=0)

    @pytest.mark.parametrize("port", [0, -1, 65536, 1.5, True])
    def test_rejects_invalid_redis_port(self, port) -> None:
        with pytest.raises((TypeError, ValueError), match="port"):
            FluxRedisConfig(host="127.0.0.1", port=port, db=0)

    @pytest.mark.parametrize("db", [-1, 1.2, True])
    def test_rejects_invalid_redis_db(self, db) -> None:
        with pytest.raises((TypeError, ValueError), match="db"):
            FluxRedisConfig(host="127.0.0.1", port=6379, db=db)

    @pytest.mark.parametrize(
        ("field_name", "field_value"),
        [
            ("connect_timeout_secs", 0.0),
            ("connect_timeout_secs", -1.0),
            ("connect_timeout_secs", float("nan")),
            ("connect_timeout_secs", float("inf")),
            ("connect_timeout_secs", True),
            ("read_timeout_secs", 0.0),
            ("read_timeout_secs", -1.0),
            ("read_timeout_secs", float("nan")),
            ("read_timeout_secs", float("inf")),
            ("read_timeout_secs", True),
        ],
    )
    def test_rejects_invalid_redis_timeouts(self, field_name: str, field_value) -> None:
        kwargs = {
            "host": "127.0.0.1",
            "port": 6379,
            "db": 0,
            "connect_timeout_secs": 5.0,
            "read_timeout_secs": 5.0,
        }
        kwargs[field_name] = field_value

        with pytest.raises((TypeError, ValueError), match=field_name):
            FluxRedisConfig(**kwargs)

    @staticmethod
    def _identity() -> FluxIdentityConfig:
        return FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id="maker_v3_01",
            strategy_instance_id="maker_v3_01",
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
