from __future__ import annotations

import inspect
from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.flux.strategies import EquitiesTakerStrategy as EquitiesTakerStrategyFromRoot
from nautilus_trader.flux.strategies import (
    EquitiesTakerStrategyConfig as EquitiesTakerStrategyConfigFromRoot,
)
from nautilus_trader.flux.strategies.equities_taker import EquitiesTakerStrategy
from nautilus_trader.flux.strategies.equities_taker import EquitiesTakerStrategyConfig
from nautilus_trader.flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from nautilus_trader.flux.strategies.makerv4.wire import MakerFill
from nautilus_trader.flux.strategies.registry import get_strategy_identity
from nautilus_trader.flux.strategies.registry import get_strategy_spec
from nautilus_trader.flux.strategies.registry import resolve_strategy_spec_for_strategy_id
from nautilus_trader.model.identifiers import InstrumentId


_OVERNIGHT_TS_MS = 1_742_176_800_000


def _config(**overrides) -> EquitiesTakerStrategyConfig:
    base = {
        "maker_instrument_id": InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        "reference_instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
        "order_qty": Decimal("1"),
        "external_strategy_id": "aapl_tradexyz_taker",
        "strategy_id": "aapl_tradexyz_taker",
        "outside_rth_hedge_enabled": True,
    }
    base.update(overrides)
    return EquitiesTakerStrategyConfig(**base)


def _quote(*, bid: str = "190.00", ask: str = "190.04", age_ms: int = 25) -> IbkrQuoteSnapshot:
    return IbkrQuoteSnapshot(
        instrument_id="AAPL.NASDAQ",
        bid=Decimal(bid),
        ask=Decimal(ask),
        age_ms=age_ms,
        ts_ms=1_000,
    )


def _fill(
    *,
    fill_id: str = "fill-overnight-policy",
    side: str = "BUY",
    qty: str = "2",
    px: str = "190.00",
    ts_ms: int = _OVERNIGHT_TS_MS,
) -> MakerFill:
    return MakerFill(
        fill_id=fill_id,
        side=side,
        qty=Decimal(qty),
        price=Decimal(px),
        ts_ms=ts_ms,
    )


def _instrument(*, raw_symbol: str, multiplier: str = "1") -> SimpleNamespace:
    return SimpleNamespace(
        raw_symbol=raw_symbol,
        price_precision=2,
        price_increment=Decimal("0.01"),
        base_currency=SimpleNamespace(code="AAPL"),
        quote_currency=SimpleNamespace(code="USD"),
        settlement_currency=SimpleNamespace(code="USD"),
        multiplier=Decimal(multiplier),
        is_inverse=False,
        make_qty=lambda value: Decimal(str(value)),
        make_price=lambda value: Decimal(str(value)),
        calculate_base_exposure_qty=lambda qty, _price=None: Decimal(str(qty)),
    )


def _quote_tick(*, instrument_id, bid: str, ask: str, ts_event: int):
    return SimpleNamespace(
        instrument_id=instrument_id,
        bid_price=SimpleNamespace(as_decimal=lambda: Decimal(bid)),
        ask_price=SimpleNamespace(as_decimal=lambda: Decimal(ask)),
        ts_event=ts_event,
    )


def _fill_event(
    *,
    instrument_id,
    fill_id: str = "fill-1",
    client_order_id: str = "order-1",
    side: str = "BUY",
    qty: str = "1",
    px: str = "190.00",
    ts_event: int,
):
    return SimpleNamespace(
        instrument_id=instrument_id,
        trade_id=fill_id,
        client_order_id=client_order_id,
        order_side=side,
        last_qty=Decimal(qty),
        last_px=Decimal(px),
        ts_event=ts_event,
    )


def _install_limit_order_factory(strategy: EquitiesTakerStrategy, monkeypatch) -> list[SimpleNamespace]:
    created: list[SimpleNamespace] = []

    def _limit(**kwargs):
        order = SimpleNamespace(
            client_order_id=f"order-{len(created) + 1}",
            instrument_id=kwargs["instrument_id"],
            side=kwargs["order_side"],
            quantity=kwargs["quantity"],
            price=kwargs["price"],
            post_only=kwargs.get("post_only"),
            time_in_force=kwargs.get("time_in_force"),
            reduce_only=kwargs.get("reduce_only"),
            tags=kwargs.get("tags"),
        )
        created.append(order)
        return order

    monkeypatch.setattr(
        type(strategy),
        "order_factory",
        property(lambda _self: SimpleNamespace(limit=_limit)),
    )
    return created


def _configure_strategy_for_quoting(strategy: EquitiesTakerStrategy) -> tuple[InstrumentId, InstrumentId]:
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._runtime_params.update({"bot_on": True})
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda *_args, **_kwargs: None
    return maker_id, ref_id


def _enum_name(value: object) -> str:
    name = getattr(value, "name", None)
    if isinstance(name, str) and name:
        return name.upper()
    return str(value).strip().upper()


def test_canonical_strategy_exports_match_root_surface() -> None:
    assert EquitiesTakerStrategyFromRoot is EquitiesTakerStrategy
    assert EquitiesTakerStrategyConfigFromRoot is EquitiesTakerStrategyConfig


def test_registry_exports_equities_taker_spec_and_suffix_resolution() -> None:
    identity = get_strategy_identity("equities_taker")
    spec = get_strategy_spec("equities_taker")
    resolved = resolve_strategy_spec_for_strategy_id("aapl_tradexyz_taker")

    assert identity.strategy_id == "equities_taker"
    assert identity.strategy_family == "equities_taker"
    assert identity.strategy_version == "v1"
    assert identity.param_set == "equities_taker"
    assert identity.profile_key == "equities_taker"
    assert spec.strategy_cls is EquitiesTakerStrategy
    assert spec.config_cls is EquitiesTakerStrategyConfig
    assert resolved is spec


def test_equities_taker_config_omits_local_inventory_and_maker_quote_fields() -> None:
    parameters = inspect.signature(EquitiesTakerStrategyConfig).parameters

    for removed_name in (
        "des_qty_local",
        "max_qty_local",
        "max_skew_bps_local",
        "linear_offset_bps",
        "bid_edge1",
        "ask_edge1",
        "place_edge1",
        "distance1",
        "n_orders1",
        "bid_edge2",
        "ask_edge2",
        "place_edge2",
        "distance2",
        "n_orders2",
        "bid_edge3",
        "ask_edge3",
        "place_edge3",
        "distance3",
        "n_orders3",
    ):
        assert removed_name not in parameters

    with pytest.raises((TypeError, ValueError), match="bid_edge1"):
        EquitiesTakerStrategyConfig(
            maker_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            order_qty=Decimal("1"),
            strategy_id="aapl_tradexyz_taker",
            external_strategy_id="aapl_tradexyz_taker",
            bid_edge1=5.0,
        )


def test_equities_taker_forces_taker_mode_and_preserves_overnight_hedge_policy() -> None:
    strategy = EquitiesTakerStrategy(config=_config())

    assert strategy._execution_mode() == "take_take"
    strategy._runtime_params["execution_mode"] = "maker_hedge"
    assert strategy._execution_mode() == "take_take"

    order = strategy.record_maker_fill(
        fill=_fill(),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )

    assert order is not None
    assert order.route == "SMART"
    assert order.time_in_force == "DAY"
    assert order.outside_rth is True
    assert order.include_overnight is True
    assert order.cancel_after_ms == 5_000


def test_equities_taker_submits_aggressive_hl_order_and_hedges_on_fill(monkeypatch) -> None:
    strategy = EquitiesTakerStrategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
        }
    )
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="189.18",
            ask="189.20",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_001_000_000,
        )
    )

    assert [str(order.instrument_id) for order in submitted] == [str(maker_id)]
    assert _enum_name(submitted[0].side) == "BUY"
    assert submitted[0].post_only is False
    assert strategy._pending_hedge is None

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-1",
            client_order_id="order-1",
            side="BUY",
            qty="1",
            px="189.20",
            ts_event=2_002_000_000,
        )
    )

    assert len(submitted) == 2
    assert str(submitted[1].instrument_id) == str(ref_id)
    assert _enum_name(submitted[1].side) == "SELL"
    assert strategy._pending_hedge is not None
    assert strategy._pending_hedge.order_id == "order-2"


def test_equities_taker_quote_tick_uses_family_owned_dispatch(monkeypatch) -> None:
    strategy = EquitiesTakerStrategy(config=_config())
    maker_id, _ref_id = _configure_strategy_for_quoting(strategy)
    calls: list[str] = []

    monkeypatch.setattr(
        strategy,
        "_reconcile_closed_take_take_orders_from_cache",
        lambda **_kwargs: calls.append("legacy_reconcile"),
    )
    monkeypatch.setattr(
        strategy,
        "_reconcile_closed_taker_orders_from_cache",
        lambda **_kwargs: calls.append("family_reconcile"),
        raising=False,
    )
    monkeypatch.setattr(
        strategy,
        "_refresh_taker_orders",
        lambda **_kwargs: calls.append("family_refresh"),
        raising=False,
    )
    monkeypatch.setattr(
        strategy,
        "_retry_hedge_backlog",
        lambda **_kwargs: calls.append("retry"),
    )
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="189.18",
            ask="189.20",
            ts_event=2_000_000_000,
        )
    )

    assert "family_reconcile" in calls
    assert "family_refresh" in calls
    assert "legacy_reconcile" not in calls


def test_equities_taker_fill_dispatch_uses_family_owned_handler(monkeypatch) -> None:
    strategy = EquitiesTakerStrategy(config=_config())
    maker_id, _ref_id = _configure_strategy_for_quoting(strategy)
    calls: list[str] = []

    monkeypatch.setattr(strategy, "_event_has_market_exit_tag", lambda _event: False)
    monkeypatch.setattr(
        strategy,
        "_managed_maker_state_for_client_order_id",
        lambda _client_order_id: SimpleNamespace(post_only=False),
    )
    monkeypatch.setattr(strategy, "_apply_maker_fill_to_managed_order", lambda _event: None)
    monkeypatch.setattr(strategy, "_cache_order_is_closed", lambda _client_order_id: False)
    monkeypatch.setattr(strategy, "_reconcile_managed_maker_order", lambda _event: None)
    monkeypatch.setattr(
        strategy,
        "_handle_take_take_fill_event",
        lambda _event, *, now_ns: calls.append("legacy_fill"),
    )
    monkeypatch.setattr(
        strategy,
        "_handle_taker_fill_event",
        lambda _event, *, now_ns: calls.append("family_fill"),
        raising=False,
    )
    monkeypatch.setattr(
        strategy,
        "_handle_maker_fill_event",
        lambda _event, *, now_ns: calls.append("maker_fill"),
    )
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-owned-1",
            client_order_id="order-1",
            side="BUY",
            qty="1",
            px="189.20",
            ts_event=2_002_000_000,
        )
    )

    assert "family_fill" in calls
    assert "legacy_fill" not in calls
    assert "maker_fill" not in calls


def test_equities_taker_stale_reference_quote_creates_recoverable_hedge_backlog(
    monkeypatch,
) -> None:
    strategy = EquitiesTakerStrategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)

    class _FakeClock:
        def __init__(self) -> None:
            self.now = 2_000_000_000

        def timestamp_ns(self) -> int:
            return self.now

    fake_clock = _FakeClock()
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
        }
    )
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="189.18",
            ask="189.20",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_001_000_000,
        )
    )

    assert [str(order.instrument_id) for order in submitted] == [str(maker_id)]

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-stale-1",
            client_order_id="order-1",
            side="BUY",
            qty="1",
            px="189.20",
            ts_event=4_500_000_000,
        )
    )

    assert len(submitted) == 1
    assert strategy._pending_hedge is None
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "stale_quote"
    assert strategy._hedge_backlog is not None
    assert strategy.snapshot_state()["hedge_backlog"]["blocked_reason"] == "stale_quote"

    fake_clock.now = 4_501_000_000
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="190.01",
            ask="190.05",
            ts_event=4_501_000_000,
        )
    )

    assert len(submitted) == 2
    hedge_order = submitted[1]
    assert str(hedge_order.instrument_id) == str(ref_id)
    assert _enum_name(hedge_order.side) == "SELL"
    assert hedge_order.quantity == Decimal("1")
