from __future__ import annotations

from nautilus_trader.flux.api import ContractCatalogEntry
from nautilus_trader.flux.api import StrategyMetadata
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.config import FluxRedisConfig
from nautilus_trader.flux.common.config import FluxVenuesConfig
from nautilus_trader.flux.common.keys import FluxRedisKeys


class _FakeRedisPipeline:
    def __init__(self, redis_client: _FakeRedis) -> None:
        self._redis = redis_client
        self._commands: list[tuple[str, str, int | None]] = []

    def get(self, key: str) -> _FakeRedisPipeline:
        self._commands.append(("get", key, None))
        return self

    def xrevrange(
        self,
        key: str,
        max: str = "+",
        min: str = "-",
        count: int | None = None,
    ) -> _FakeRedisPipeline:
        _ = max, min
        self._commands.append(("xrevrange", key, count))
        return self

    def exists(self, key: str) -> _FakeRedisPipeline:
        self._commands.append(("exists", key, None))
        return self

    def execute(self) -> list[object]:
        out: list[object] = []
        for op, key, count in self._commands:
            if op == "get":
                out.append(None)
            elif op == "xrevrange":
                _ = count
                out.append([])
            elif op == "exists":
                out.append(1 if key in self._redis.hashes else 0)
        return out


class _FakeRedis:
    def __init__(self) -> None:
        self.hashes: dict[str, dict[str, bytes]] = {}

    def ping(self) -> bool:
        return True

    def hmget(self, key: str, fields: list[str]) -> list[bytes | None]:
        mapping = self.hashes.get(key, {})
        return [mapping.get(field) for field in fields]

    def hkeys(self, key: str) -> list[str]:
        return list(self.hashes.get(key, {}).keys())

    def hset(self, key: str, mapping: dict[str, str]) -> int:
        target = self.hashes.setdefault(key, {})
        for field, value in mapping.items():
            target[field] = value.encode("utf-8")
        return len(mapping)

    def publish(self, channel: str, message: str) -> int:
        _ = channel, message
        return 1

    def get(self, key: str):
        _ = key
        return None

    def xrevrange(self, key: str, max: str = "+", min: str = "-", count: int | None = None):
        _ = key, max, min, count
        return []

    def pipeline(self, transaction: bool = False) -> _FakeRedisPipeline:
        _ = transaction
        return _FakeRedisPipeline(self)


def _build_app(redis_client: _FakeRedis, strategy_id: str) -> tuple[FluxConfig, object]:
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(
            host="127.0.0.1",
            port=6380,
            db=0,
        ),
        venues=FluxVenuesConfig(
            execution_venue="venue_a",
            reference_venue="venue_b",
            execution_symbol="ABC/USDT",
            reference_symbol="ABC/USDT",
        ),
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),),
        strategy_metadata=StrategyMetadata(
            strategy_class="maker_v3",
            strategy_groups="tokenmm",
            base_asset="ABC",
            quote_asset="USDT",
        ),
        params_schema={
            "qty": {"type": "number"},
            "bot_on": {"type": "boolean"},
        },
        params_defaults={
            "qty": 1.0,
            "bot_on": False,
        },
    )
    return flux_config, app


def test_api_params_read_returns_explicit_error_when_params_store_invalid() -> None:
    redis_client = _FakeRedis()
    _, app = _build_app(redis_client, strategy_id="bad")
    keys = FluxRedisKeys(strategy_id="bad")
    redis_client.hashes[keys.params_hash_key()] = {
        "qty": b"2.0",
        "oops": b"bad",
    }

    with app.test_client() as client:  # type: ignore[attr-defined]
        response = client.get("/api/v1/params?strategy=bad")
        body = response.get_json()

    assert response.status_code == 500
    assert body["ok"] is False
    assert body["api_version"] == "v1"
    assert body["error"]["code"] == "params_store_invalid"
    assert body["error"]["details"]["strategy_id"] == "bad"


def test_api_signals_read_returns_explicit_error_when_params_store_invalid() -> None:
    redis_client = _FakeRedis()
    _, app = _build_app(redis_client, strategy_id="s1")
    keys = FluxRedisKeys(strategy_id="s1")
    redis_client.hashes[keys.params_hash_key()] = {
        "qty": b"2.0",
        "oops": b"bad",
    }

    with app.test_client() as client:  # type: ignore[attr-defined]
        response = client.get("/api/v1/signals?strategy=s1")
        body = response.get_json()

    assert response.status_code == 500
    assert body["ok"] is False
    assert body["api_version"] == "v1"
    assert body["error"]["code"] == "params_store_invalid"
    assert body["error"]["details"]["strategy_id"] == "s1"
