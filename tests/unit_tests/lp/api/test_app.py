from __future__ import annotations

import json
from pathlib import Path

from lp.api import create_lp_api_app


class FakeRedis:
    def __init__(self) -> None:
        self.values: dict[str, str] = {}
        self.lists: dict[str, list[str]] = {}

    def delete(self, key: str) -> int:
        removed = 0
        if key in self.values:
            del self.values[key]
            removed += 1
        if key in self.lists:
            del self.lists[key]
            removed += 1
        return removed

    def get(self, key: str):
        return self.values.get(key)

    def lpush(self, key: str, value: str) -> int:
        bucket = self.lists.setdefault(key, [])
        bucket.insert(0, value)
        return len(bucket)

    def lrange(self, key: str, start: int, end: int) -> list[str]:
        bucket = self.lists.get(key, [])
        if end < 0:
            return bucket[start:]
        return bucket[start : end + 1]

    def ltrim(self, key: str, start: int, end: int) -> None:
        bucket = self.lists.get(key, [])
        if end < 0:
            self.lists[key] = bucket[start:]
        else:
            self.lists[key] = bucket[start : end + 1]

    def set(self, key: str, value: str) -> None:
        self.values[key] = value


class FakePulse:
    def __init__(self) -> None:
        self.actions: list[tuple[str, str]] = []
        self.status_by_job: dict[str, str] = {}

    def control_job(self, job_id: str, action: str) -> str:
        self.actions.append((job_id, action))
        status = "stopped" if action == "stop" else "running"
        self.status_by_job[job_id] = status
        return status

    def get_job_status(self, job_id: str) -> str:
        return self.status_by_job.get(job_id, "unknown")


def write_ini(tmp_path: Path, content: str, *, name: str) -> Path:
    path = tmp_path / name
    path.write_text(content.strip(), encoding="utf-8")
    return path


def write_band1_config(tmp_path: Path) -> Path:
    return write_ini(
        tmp_path,
        """
        [identity]
        id = eth_plume_lp
        label = ETH/PLUME LP Band1
        state_key = eth_plume_lp_hedger
        job_id = service-eth-plume-lp-hedger

        [lp_pool]
        mode = onchain
        pool_address = 0xpool
        token0_symbol = WETH
        token1_symbol = WPLUME
        token0_decimals = 18
        token1_decimals = 18
        initial_eth = 1.6085
        initial_plume = 169377
        price_lower = 85000
        price_upper = 111000

        [target]
        target_net_eth = 0
        target_net_plume = 0

        [bybit]
        eth_symbol = ETHUSDT
        plume_symbol = PLUMEUSDT
        eth_qty_step = 0.001
        plume_qty_step = 1
        max_slippage_bps = 30
        api_key = test_key_1234
        api_secret = test_secret

        [rebalance]
        poll_interval_sec = 3
        price_move_pct = 2.0
        eth_exposure_usd_threshold = 1000
        plume_exposure_usd_threshold = 1200
        min_order_qty_eth = 0.01
        min_order_qty_plume = 10

        [hedge]
        hedge_token0 = 1
        hedge_token1 = 1
        """,
        name="eth_plume_lp_hedger.ini",
    )


def build_app(tmp_path: Path, monkeypatch):
    redis_client = FakeRedis()
    pulse = FakePulse()
    config_path = write_band1_config(tmp_path)
    monkeypatch.setenv("ETH_PLUME_LP_HEDGER_CONFIG", str(config_path))
    app = create_lp_api_app(
        redis_client=redis_client,
        get_job_status=pulse.get_job_status,
        control_job=pulse.control_job,
    )
    return app, redis_client, pulse, config_path


def test_list_hedger_instances_returns_registry_metadata(tmp_path: Path, monkeypatch) -> None:
    app, _, _, _ = build_app(tmp_path, monkeypatch)

    with app.test_client() as client:
        response = client.get("/api/v1/hedgers/instances")
        body = response.get_json()

    assert response.status_code == 200
    assert body["ok"] is True
    assert body["data"][0]["id"] == "eth_plume_lp"
    assert body["data"][0]["label"] == "ETH/PLUME LP Band1"
    assert body["data"][0]["config_env_var"] == "ETH_PLUME_LP_HEDGER_CONFIG"


def test_list_hedger_instances_only_exposes_active_band1_and_band2_by_default(
    tmp_path: Path,
    monkeypatch,
) -> None:
    app, _, _, _ = build_app(tmp_path, monkeypatch)

    with app.test_client() as client:
        response = client.get("/api/v1/hedgers/instances")
        body = response.get_json()

    assert response.status_code == 200
    assert [item["id"] for item in body["data"]] == [
        "eth_plume_lp",
        "eth_plume_lp_band2",
    ]


def test_status_endpoint_returns_chainsaw_payload_shape(tmp_path: Path, monkeypatch) -> None:
    app, redis_client, pulse, _ = build_app(tmp_path, monkeypatch)
    pulse.status_by_job["service-eth-plume-lp-hedger"] = "running"
    redis_client.set(
        "eth_plume_lp_hedger:state",
        json.dumps(
            {
                "last_hedge_price": "95000",
                "last_net_eth": "0.10",
                "last_net_plume": "-2500",
            },
        ),
    )
    redis_client.set(
        "eth_plume_lp_hedger:snapshot",
        json.dumps(
            {
                "timestamp": 1700000000,
                "lp_eth": "1.0",
                "lp_plume": "100000",
                "perp_eth": "-0.9",
                "perp_plume": "-102500",
                "net_eth": "0.1",
                "net_plume": "-2500",
                "target_net_eth": "0",
                "target_net_plume": "0",
                "last_hedge_price": "95000",
                "initial_eth_effective": "1.6085",
                "initial_plume_effective": "169377",
                "price_lower_effective": "85000",
                "price_upper_effective": "111000",
                "eth_exposure_usd_threshold_effective": "1000",
                "plume_exposure_usd_threshold_effective": "1200",
                "price_move_pct_effective": "2.0",
            },
        ),
    )
    redis_client.set("eth_plume_lp_hedger:mode", json.dumps({"enabled": True, "dry_run": True}))
    redis_client.lpush("eth_plume_lp_hedger:events", json.dumps({"timestamp": 1700000001, "side": "buy", "qty": "1"}))

    with app.test_client() as client:
        response = client.get("/api/v1/hedgers/eth_plume_lp")
        body = response.get_json()

    assert response.status_code == 200
    payload = body["data"]
    assert payload["snapshot"]["lp_eth"] == "1.0"
    assert payload["geometry_effective"]["price_lower"] == "85000"
    assert payload["threshold_effective"]["price_move_pct"] == "2.0"
    assert payload["hedger_enabled"] is True
    assert payload["dry_run"] is True
    assert payload["job_status"] == "running"


def test_job_endpoint_controls_requested_hedger_job(tmp_path: Path, monkeypatch) -> None:
    app, _, pulse, _ = build_app(tmp_path, monkeypatch)

    with app.test_client() as client:
        response = client.post("/api/v1/hedgers/eth_plume_lp/job", json={"action": "restart"})
        body = response.get_json()

    assert response.status_code == 200
    assert pulse.actions == [("service-eth-plume-lp-hedger", "restart")]
    assert body["data"]["job_status"] == "running"


def test_config_get_and_patch_round_trip(tmp_path: Path, monkeypatch) -> None:
    app, _, pulse, config_path = build_app(tmp_path, monkeypatch)

    with app.test_client() as client:
        response = client.get("/api/v1/hedgers/eth_plume_lp/config")
        body = response.get_json()
        assert response.status_code == 200
        assert body["data"]["lp_pool"]["token0_symbol"] == "WETH"

        patch_response = client.patch(
            "/api/v1/hedgers/eth_plume_lp/config",
            json={
                "label": "ETH/PLUME LP Band1 Updated",
                "hedge": {"hedge_token0": True, "hedge_token1": False},
                "bybit": {"perp_symbol_token0": "ETHUSDT", "perp_symbol_token1": ""},
            },
        )
        patch_body = patch_response.get_json()

    assert patch_response.status_code == 200
    assert patch_body["data"]["label"] == "ETH/PLUME LP Band1 Updated"
    assert patch_body["data"]["hedge"]["hedge_token1"] is False
    assert pulse.actions[-1] == ("service-eth-plume-lp-hedger", "restart")
    assert "ETH/PLUME LP Band1 Updated" in config_path.read_text(encoding="utf-8")


def test_geometry_and_threshold_override_routes_round_trip(tmp_path: Path, monkeypatch) -> None:
    app, redis_client, _, _ = build_app(tmp_path, monkeypatch)

    with app.test_client() as client:
        geometry_response = client.post(
            "/api/v1/hedgers/eth_plume_lp/geometry-overrides",
            json={"initial_eth": "2", "initial_plume": "200000", "price_lower": "80000", "price_upper": "120000"},
        )
        threshold_response = client.post(
            "/api/v1/hedgers/eth_plume_lp/threshold-overrides",
            json={"price_move_pct": "5", "eth_exposure_usd_threshold": "1100", "plume_exposure_usd_threshold": "1300"},
        )
        geometry_body = geometry_response.get_json()
        threshold_body = threshold_response.get_json()
        assert json.loads(redis_client.get("eth_plume_lp_hedger:geometry_overrides")) == {
            "initial_eth": "2",
            "initial_plume": "200000",
            "price_lower": "80000",
            "price_upper": "120000",
        }
        assert json.loads(redis_client.get("eth_plume_lp_hedger:threshold_overrides")) == {
            "price_move_pct": "5",
            "eth_exposure_usd_threshold": "1100",
            "plume_exposure_usd_threshold": "1300",
        }
        cleared_geometry = client.delete("/api/v1/hedgers/eth_plume_lp/geometry-overrides")

    assert geometry_response.status_code == 200
    assert threshold_response.status_code == 200
    assert cleared_geometry.status_code == 200
    assert geometry_body["data"]["geometry_effective"]["price_upper"] == "120000"
    assert threshold_body["data"]["threshold_effective"]["price_move_pct"] == "5"
    assert redis_client.get("eth_plume_lp_hedger:geometry_overrides") is None


def test_enabled_and_clear_events_routes_use_selected_hedger_id(tmp_path: Path, monkeypatch) -> None:
    app, redis_client, _, _ = build_app(tmp_path, monkeypatch)
    redis_client.lpush("eth_plume_lp_hedger:events", json.dumps({"timestamp": 1, "side": "buy", "qty": "1"}))
    redis_client.lpush("eth_plume_lp_hedger:events", json.dumps({"timestamp": 2, "side": "sell", "qty": "2"}))

    with app.test_client() as client:
        enabled_response = client.post("/api/v1/hedgers/eth_plume_lp/enabled", json={"enabled": True})
        clear_response = client.post("/api/v1/hedgers/eth_plume_lp/events/clear")

    assert enabled_response.status_code == 200
    assert enabled_response.get_json()["data"]["hedger_enabled"] is True
    assert json.loads(redis_client.get("eth_plume_lp_hedger:mode"))["enabled"] is True
    assert clear_response.status_code == 200
    assert clear_response.get_json()["data"]["cleared"] == 2
    assert redis_client.lrange("eth_plume_lp_hedger:events", 0, 10) == []
