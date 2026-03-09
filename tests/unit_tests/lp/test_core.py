from __future__ import annotations

import json
from decimal import Decimal

from lp.config import LpHedgerConfig
from lp.hedgers.core import LpHedger
from lp.hedgers.core import RuntimeState


class FakePriceHelper:
    def get_price_token1_per_token0(self, pool_address: str) -> Decimal:
        assert pool_address == "0xpool"
        return Decimal(95000)


class FakePerpClient:
    def __init__(
        self,
        *,
        fail_position_symbol: str | None = None,
        raise_on_order: bool = False,
    ) -> None:
        self.orders = []
        self.positions = {
            "ETHUSDT": Decimal("-0.5"),
            "PLUMEUSDT": Decimal(-40000),
        }
        self.marks = {
            "ETHUSDT": Decimal(2000),
            "PLUMEUSDT": Decimal("0.02"),
        }
        self.fail_position_symbol = fail_position_symbol
        self.raise_on_order = raise_on_order

    def get_position_size(self, symbol: str) -> Decimal:
        if symbol == self.fail_position_symbol:
            raise RuntimeError(f"position fetch failed for {symbol}")
        return self.positions.get(symbol, Decimal(0))

    def get_mark_price(self, symbol: str) -> Decimal:
        return self.marks[symbol]

    def create_market_order(self, order) -> bool:
        if self.raise_on_order:
            raise RuntimeError("order submission failed")
        self.orders.append(order)
        return True


class FakeRedis:
    def __init__(self) -> None:
        self.values: dict[str, str] = {}
        self.events: dict[str, list[str]] = {}
        self.fail_state_writes = False

    def get(self, key: str):
        return self.values.get(key)

    def set(self, key: str, value: str) -> None:
        if self.fail_state_writes and key.endswith(":state"):
            raise RuntimeError("state write failed")
        self.values[key] = value

    def lpush(self, key: str, value: str) -> int:
        bucket = self.events.setdefault(key, [])
        bucket.insert(0, value)
        return len(bucket)

    def ltrim(self, key: str, start: int, end: int) -> None:
        bucket = self.events.setdefault(key, [])
        if end < 0:
            bucket[:] = bucket[start:]
        else:
            bucket[:] = bucket[start : end + 1]


def build_test_hedger(
    *,
    target_net_token0: Decimal = Decimal(0),
    target_net_token1: Decimal = Decimal(0),
    max_slippage_bps: Decimal = Decimal(30),
    price_helper: FakePriceHelper | None = None,
    bybit_client: FakePerpClient | None = None,
    redis_client: FakeRedis | None = None,
) -> LpHedger:
    config = LpHedgerConfig(
        hedger_id="eth_plume_lp",
        label="ETH/PLUME LP Band1",
        job_id="service-eth-plume-lp-hedger",
        state_key="eth_plume_lp_hedger",
        lp_mode="onchain",
        chain="plume",
        amm="rooster_v3",
        pool_address="0xpool",
        token0_symbol="WETH",
        token1_symbol="WPLUME",
        token0_decimals=18,
        token1_decimals=18,
        initial_token0=Decimal("1.6085"),
        initial_token1=Decimal(169377),
        price_lower=Decimal(85000),
        price_upper=Decimal(111000),
        target_net_token0=target_net_token0,
        target_net_token1=target_net_token1,
        perp_symbol_token0="ETHUSDT",
        perp_symbol_token1="PLUMEUSDT",
        order_qty_step_token0=Decimal("0.001"),
        order_qty_step_token1=Decimal(1),
        max_slippage_bps=max_slippage_bps,
        price_move_pct=Decimal("2.0"),
        token0_exposure_usd_threshold=Decimal(1000),
        token1_exposure_usd_threshold=Decimal(1000),
        min_order_qty_token0=Decimal("0.01"),
        min_order_qty_token1=Decimal(10),
        poll_interval_sec=3,
        hedge_token0=True,
        hedge_token1=True,
        bybit_api_key="",
        bybit_api_secret="",
    )
    return LpHedger(
        config=config,
        price_helper=price_helper or FakePriceHelper(),
        bybit_client=bybit_client or FakePerpClient(),
        redis_client=redis_client or FakeRedis(),
    )


def seed_enabled_state(redis_client: FakeRedis, *, last_hedge_price: str) -> None:
    redis_client.set(
        "eth_plume_lp_hedger:mode",
        json.dumps({"enabled": True, "dry_run": False}),
    )
    redis_client.set(
        "eth_plume_lp_hedger:state",
        json.dumps(
            {
                "last_hedge_price": last_hedge_price,
                "last_net_eth": "0",
                "last_net_plume": "0",
                "last_net_token0": "0",
                "last_net_token1": "0",
            },
        ),
    )


def test_snapshot_preserves_chainsaw_field_names_and_token_aliases() -> None:
    hedger = build_test_hedger()

    snapshot = hedger.build_snapshot()

    assert snapshot["lp_eth"] == snapshot["lp_token0"]
    assert snapshot["lp_plume"] == snapshot["lp_token1"]
    assert snapshot["perp_eth"] == snapshot["perp_token0"]
    assert snapshot["perp_plume"] == snapshot["perp_token1"]
    assert snapshot["net_eth"] == snapshot["net_token0"]
    assert snapshot["net_plume"] == snapshot["net_token1"]
    assert snapshot["target_net_eth"] == snapshot["target_net_token0"]
    assert snapshot["target_net_plume"] == snapshot["target_net_token1"]


def test_target_values_are_respected_instead_of_overwritten() -> None:
    hedger = build_test_hedger(
        target_net_token0=Decimal(5),
        target_net_token1=Decimal(10),
    )
    redis_client = hedger._redis
    redis_client.set(
        "eth_plume_lp_hedger:geometry_overrides",
        json.dumps(
            {
                "initial_eth": "10",
                "initial_plume": "20",
                "price_lower": "80000",
                "price_upper": "120000",
            },
        ),
    )

    snapshot = hedger.build_snapshot()

    assert hedger.target_net_token0 == Decimal(5)
    assert hedger.target_net_token1 == Decimal(10)
    assert snapshot["target_net_token0"] == "5"
    assert snapshot["target_net_token1"] == "10"


def test_max_slippage_bps_is_applied_or_rejected_explicitly() -> None:
    hedger = build_test_hedger(max_slippage_bps=Decimal(30))

    order = hedger.build_market_order(
        symbol="ETHUSDT",
        incremental_change=Decimal("1.2344"),
        qty_step=Decimal("0.001"),
    )

    assert order is not None
    assert order.side == "buy"
    assert order.qty == Decimal("1.234")
    assert order.max_slippage_bps == Decimal(30)


def test_tick_persists_initial_state_without_trade() -> None:
    hedger = build_test_hedger()

    snapshot = hedger.tick()

    state_raw = hedger._redis.get("eth_plume_lp_hedger:state")
    assert hedger._bybit.orders == []
    assert state_raw is not None
    state = json.loads(state_raw)
    assert state["last_hedge_price"] == snapshot["last_hedge_price"]
    assert state["last_net_token0"] == snapshot["last_net_token0"]
    assert state["last_net_token1"] == snapshot["last_net_token1"]


def test_invalid_geometry_overrides_fall_back_to_base_values() -> None:
    hedger = build_test_hedger()
    hedger._redis.set(
        "eth_plume_lp_hedger:geometry_overrides",
        json.dumps({"price_lower": "120000", "price_upper": "80000"}),
    )

    snapshot = hedger.build_snapshot()

    assert snapshot["price_lower_effective"] == snapshot["price_lower_base"]
    assert snapshot["price_upper_effective"] == snapshot["price_upper_base"]


def test_invalid_threshold_overrides_are_ignored() -> None:
    hedger = build_test_hedger()
    hedger._redis.set(
        "eth_plume_lp_hedger:threshold_overrides",
        json.dumps(
            {
                "price_move_pct": "not-a-number",
                "eth_exposure_usd_threshold": "-1",
                "plume_exposure_usd_threshold": "oops",
            },
        ),
    )

    snapshot = hedger.build_snapshot()

    assert snapshot["price_move_pct_effective"] == snapshot["price_move_pct_base"]
    assert (
        snapshot["eth_exposure_usd_threshold_effective"]
        == snapshot["eth_exposure_usd_threshold_base"]
    )
    assert (
        snapshot["plume_exposure_usd_threshold_effective"]
        == snapshot["plume_exposure_usd_threshold_base"]
    )


def test_bybit_fetch_failure_preserves_existing_state() -> None:
    redis_client = FakeRedis()
    seed_enabled_state(redis_client, last_hedge_price="100000")
    hedger = build_test_hedger(
        bybit_client=FakePerpClient(fail_position_symbol="ETHUSDT"),
        redis_client=redis_client,
    )

    hedger.tick()

    assert redis_client.get("eth_plume_lp_hedger:state") == json.dumps(
        {
            "last_hedge_price": "100000",
            "last_net_eth": "0",
            "last_net_plume": "0",
            "last_net_token0": "0",
            "last_net_token1": "0",
        },
    )
    assert hedger._bybit.orders == []


def test_order_submission_failure_preserves_existing_state_and_events() -> None:
    redis_client = FakeRedis()
    seed_enabled_state(redis_client, last_hedge_price="95000")
    hedger = build_test_hedger(
        bybit_client=FakePerpClient(raise_on_order=True),
        redis_client=redis_client,
    )

    hedger.tick()

    assert redis_client.get("eth_plume_lp_hedger:state") == json.dumps(
        {
            "last_hedge_price": "95000",
            "last_net_eth": "0",
            "last_net_plume": "0",
            "last_net_token0": "0",
            "last_net_token1": "0",
        },
    )
    assert redis_client.events == {}


def test_post_hedge_state_write_failure_is_contained() -> None:
    redis_client = FakeRedis()
    seed_enabled_state(redis_client, last_hedge_price="95000")
    redis_client.fail_state_writes = True
    hedger = build_test_hedger(redis_client=redis_client)

    snapshot = hedger.tick()

    assert snapshot["last_hedge_price"] == "95000"
    assert [order.symbol for order in hedger._bybit.orders] == ["ETHUSDT", "PLUMEUSDT"]
    assert len(redis_client.events["eth_plume_lp_hedger:events"]) == 2
    assert redis_client.get("eth_plume_lp_hedger:state") == json.dumps(
        {
            "last_hedge_price": "95000",
            "last_net_eth": "0",
            "last_net_plume": "0",
            "last_net_token0": "0",
            "last_net_token1": "0",
        },
    )


def test_usd_thresholds_are_diagnostic_only_without_price_trigger() -> None:
    redis_client = FakeRedis()
    seed_enabled_state(redis_client, last_hedge_price="100000")
    hedger = build_test_hedger(redis_client=redis_client)

    hedger.tick()

    assert hedger._bybit.orders == []
    assert redis_client.events == {}


def test_tick_can_hedge_both_assets_when_both_triggers_fire() -> None:
    redis_client = FakeRedis()
    seed_enabled_state(redis_client, last_hedge_price="95000")
    hedger = build_test_hedger(redis_client=redis_client)

    hedger.tick()

    assert [order.symbol for order in hedger._bybit.orders] == ["ETHUSDT", "PLUMEUSDT"]
    assert len(redis_client.events["eth_plume_lp_hedger:events"]) == 2


def test_token1_min_notional_blocks_small_hedges(monkeypatch) -> None:
    hedger = build_test_hedger()
    state = RuntimeState(
        price=Decimal(100000),
        price_move_pct=Decimal(5),
        price_source="perp_cross",
        pool_price=Decimal(95000),
        pool_price_source="rooster",
        lp_token0=Decimal(0),
        lp_token1=Decimal(10),
        perp_token0=Decimal(0),
        perp_token1=Decimal(0),
        net_token0=Decimal(0),
        net_token1=Decimal(10),
        target_net_token0=Decimal(0),
        target_net_token1=Decimal(0),
        token0_error=Decimal(0),
        token1_error=Decimal(10),
        token0_mark=Decimal(2000),
        token1_mark=Decimal("0.5"),
        token0_usd_error=Decimal(0),
        token1_usd_error=Decimal(5),
        last_hedge_price=Decimal(95000),
        last_net_token0=Decimal(0),
        last_net_token1=Decimal(0),
        base_geometry=hedger._base_geometry,
        effective_geometry=hedger._base_geometry,
        base_token0_threshold=hedger.config.token0_exposure_usd_threshold,
        base_token1_threshold=hedger.config.token1_exposure_usd_threshold,
        effective_token0_threshold=hedger.config.token0_exposure_usd_threshold,
        effective_token1_threshold=hedger.config.token1_exposure_usd_threshold,
        base_price_move_pct=hedger.config.price_move_pct,
        effective_price_move_pct=hedger.config.price_move_pct,
        hedger_enabled=True,
        token0_price_triggered=False,
        token1_price_triggered=True,
        token0_usd_triggered=False,
        token1_usd_triggered=False,
    )
    monkeypatch.setattr(hedger, "_collect_runtime_state", lambda persist_initial_state=False: state)

    hedger.tick()

    assert hedger._bybit.orders == []
