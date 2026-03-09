from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.common.portfolio_inventory import decode_component
from nautilus_trader.flux.common.portfolio_inventory import encode_portfolio_inventory
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_STATE
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _config(**overrides) -> MakerV4StrategyConfig:
    base = {
        "maker_instrument_id": InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        "reference_instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
        "order_qty": Decimal("1"),
        "external_strategy_id": "aapl_tradexyz_makerv4",
        "strategy_id": "aapl_tradexyz_makerv4",
    }
    base.update(overrides)
    return MakerV4StrategyConfig(**base)


class _FakeRedis:
    def __init__(self, values: dict[str, bytes | str] | None = None) -> None:
        self._values: dict[str, bytes] = {}
        for key, value in dict(values or {}).items():
            self.set(key, value)

    def get(self, key: str) -> bytes | None:
        return self._values.get(key)

    def set(self, key: str, value: str | bytes) -> bool:
        self._values[key] = value.encode() if isinstance(value, str) else value
        return True


def _identity_instrument(*, raw_symbol: str) -> SimpleNamespace:
    return SimpleNamespace(
        raw_symbol=raw_symbol,
        base_currency=SimpleNamespace(code="AAPL"),
        quote_currency=SimpleNamespace(code="USD"),
        settlement_currency=SimpleNamespace(code="USD"),
        multiplier=Decimal("1"),
        is_inverse=False,
        make_qty=lambda value: Decimal(str(value)),
        make_price=lambda value: Decimal(str(value)),
        calculate_base_exposure_qty=lambda qty, _price=None: Decimal(str(qty)),
    )


def _portfolio_inventory_key() -> str:
    return FluxRedisKeys.portfolio_inventory(
        portfolio_id="equities",
        base_currency="AAPL",
        namespace="flux",
        schema_version="v1",
    )


def _component_key() -> str:
    return FluxRedisKeys.portfolio_inventory_component(
        strategy_id="aapl_tradexyz_makerv4",
        portfolio_id="equities",
        base_currency="AAPL",
        namespace="flux",
        schema_version="v1",
    )


def _inventory_strategy(*, redis_client: _FakeRedis) -> tuple[MakerV4Strategy, list[tuple[str, dict[str, object]]]]:
    strategy = MakerV4Strategy(config=_config())
    strategy.register(
        trader_id=TestIdStubs.trader_id(),
        portfolio=TestComponentStubs.portfolio(),
        msgbus=TestComponentStubs.msgbus(),
        cache=TestComponentStubs.cache(),
        clock=TestComponentStubs.clock(),
    )
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    instruments = {
        maker_id: _identity_instrument(raw_symbol="AAPL/USD"),
        ref_id: _identity_instrument(raw_symbol="AAPL"),
    }
    own_position = SimpleNamespace(
        instrument_id=maker_id,
        signed_qty=Decimal("12"),
        strategy_id=strategy.id,
    )

    def _positions_open(*args, **kwargs):
        strategy_filter = kwargs.get("strategy_id")
        if strategy_filter is None and len(args) >= 3:
            strategy_filter = args[2]
        positions = [own_position]
        if strategy_filter is None:
            return positions
        return [
            position
            for position in positions
            if str(getattr(position, "strategy_id", "")) == str(strategy_filter)
        ]

    strategy._instruments = instruments
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: instruments.get(instrument_id),
        positions_open=_positions_open,
        accounts=lambda: [],
    )
    strategy.configure_portfolio_inventory_feed(
        redis_client=redis_client,
        portfolio_id="equities",
        namespace="flux",
        schema_version="v1",
        stale_after_ms=3_000,
        allow_partial_global_risk=True,
    )
    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    return strategy, published


def test_makerv4_state_snapshot_publishes_shared_inventory_contract_fields() -> None:
    redis_client = _FakeRedis(
        {
            _portfolio_inventory_key(): encode_portfolio_inventory(
                {
                    "portfolio_id": "equities",
                    "base_currency": "AAPL",
                    "global_qty_base": "37",
                    "global_qty": "37",
                    "global_qty_base_complete": False,
                    "global_qty_complete": False,
                    "aggregation_mode": "partial",
                    "ts_ms": 2_000,
                    "stale_after_ms": 3_000,
                },
            ),
        },
    )
    strategy, published = _inventory_strategy(redis_client=redis_client)

    strategy._publish_state_snapshot(now_ns=2_500_000_000)

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert payload["position_qty_venue"] == "12"
    assert payload["position_qty_base"] == "12"
    assert payload["local_qty_base"] == "12"
    assert payload["local_qty"] == "12"
    assert payload["global_qty_base"] == "37"
    assert payload["global_qty"] == "37"
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["aggregation_mode"] == "partial"
    assert payload["qty_conversion_status"] == "identity"
    assert payload["qty_conversion_source"] == "generic:multiplier=1"
    assert payload["pricing_debug"]["skew"]["local_position_qty_venue"] == "12"
    assert payload["pricing_debug"]["skew"]["global_inventory_qty_base"] == "37"
    assert payload["pricing_debug"]["skew"]["global_inventory_aggregation_mode"] == "partial"


def test_makerv4_state_snapshot_filters_local_positions_to_strategy_id() -> None:
    redis_client = _FakeRedis()
    strategy, published = _inventory_strategy(redis_client=redis_client)
    maker_id = strategy.config.maker_instrument_id
    own_position = SimpleNamespace(
        instrument_id=maker_id,
        signed_qty=Decimal("12"),
        strategy_id=strategy.id,
    )
    foreign_position = SimpleNamespace(
        instrument_id=maker_id,
        signed_qty=Decimal("5"),
        strategy_id="other_strategy",
    )

    def _positions_open(*args, **kwargs):
        strategy_filter = kwargs.get("strategy_id")
        if strategy_filter is None and len(args) >= 3:
            strategy_filter = args[2]
        positions = [own_position, foreign_position]
        if strategy_filter is None:
            return positions
        return [
            position
            for position in positions
            if str(getattr(position, "strategy_id", "")) == str(strategy_filter)
        ]

    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=_positions_open,
        accounts=lambda: [],
    )

    strategy._publish_state_snapshot(now_ns=2_500_000_000)

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert payload["position_qty_venue"] == "12"
    assert payload["local_qty_base"] == "12"
    component = decode_component(redis_client.get(_component_key()))
    assert component is not None
    assert component.local_qty_base == Decimal("12")


def test_makerv4_state_snapshot_omits_partial_global_qty_when_feed_disallows_it() -> None:
    redis_client = _FakeRedis(
        {
            _portfolio_inventory_key(): encode_portfolio_inventory(
                {
                    "portfolio_id": "equities",
                    "base_currency": "AAPL",
                    "global_qty_base": "37",
                    "global_qty": "37",
                    "global_qty_base_complete": False,
                    "global_qty_complete": False,
                    "aggregation_mode": "partial",
                    "ts_ms": 2_000,
                    "stale_after_ms": 3_000,
                },
            ),
        },
    )
    strategy, published = _inventory_strategy(redis_client=redis_client)
    strategy.configure_portfolio_inventory_feed(
        redis_client=redis_client,
        portfolio_id="equities",
        namespace="flux",
        schema_version="v1",
        stale_after_ms=3_000,
        allow_partial_global_risk=False,
    )

    strategy._publish_state_snapshot(now_ns=2_500_000_000)

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert "global_qty_base" not in payload
    assert "global_qty" not in payload
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["aggregation_mode"] == "partial"
    assert "global_inventory_qty_base" not in payload["pricing_debug"]["skew"]


def test_makerv4_state_snapshot_publishes_portfolio_inventory_component() -> None:
    redis_client = _FakeRedis()
    strategy, _published = _inventory_strategy(redis_client=redis_client)

    strategy._publish_state_snapshot(now_ns=2_500_000_000)

    component = decode_component(redis_client.get(_component_key()))
    assert component is not None
    assert component.local_qty_base == Decimal("12")
    assert component.local_position_qty_venue == Decimal("12")
    assert component.local_position_qty_base == Decimal("12")
    assert component.qty_conversion_status == "identity"
    assert component.qty_conversion_source == "generic:multiplier=1"


def test_makerv4_on_stop_publishes_terminal_inventory_component() -> None:
    redis_client = _FakeRedis()
    strategy, published = _inventory_strategy(redis_client=redis_client)
    strategy.unsubscribe_quote_ticks = lambda *args, **kwargs: None

    strategy.on_stop()

    component = decode_component(redis_client.get(_component_key()))
    assert component is not None
    assert component.state == "on_stop"
    assert component.is_fresh(now_ms_value=component.ts_ms) is False
    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    assert state_payloads[-1]["state"] == "on_stop"
